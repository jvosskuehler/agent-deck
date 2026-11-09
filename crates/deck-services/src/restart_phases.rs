//! Die Phasen des Neustarts (Teil von `restart.rs`, eigene Datei wegen der Länge).

use super::{
    DEAD_TIMEOUT, FORCE_KILL_S, HELLO_TIMEOUT, KILL_SETTLE, LAUNCH_GAP, Phase, Restarter, SAVE_SETTLE, SHUT_SETTLE,
    SLOT_SETTLE, SLOT_TIMEOUT, SOFT_CLOSE_S, TICK,
};
use deck_core::restart_plan::counts;

impl Restarter {
    // ── herunterfahren ──────────────────────────────────────────────────────

    pub(super) fn do_save(&mut self) {
        for w in self.letters() {
            self.bridge.save_all(&w);
        }
        let t = self.l("Editoren gespeichert.".into(), "Saved editors.".into());
        self.say(&t, None);
        self.enter(Phase::Kill, SAVE_SETTLE);
    }

    /// JEDES Agent-Terminal einzeln schließen, vor den Fenstern (Regel 4).
    pub(super) fn do_kill(&mut self) {
        let slots: Vec<String> = self.plan.windows.iter().flat_map(|w| w.slots.iter().map(|s| s.slot.clone())).collect();
        let n = slots.iter().filter(|s| self.bridge.close_agent(s)).count();
        let t = self.l(format!("{n} Chat(s) geschlossen."), format!("Closed {n} chat(s)."));
        self.say(&t, None);
        self.enter(Phase::Shut, KILL_SETTLE);
    }

    pub(super) fn do_shut(&mut self) {
        for w in self.letters() {
            self.bridge.close_window(&w);
        }
        let t = self.l("Fenster werden geschlossen …".into(), "Closing windows …".into());
        self.say(&t, None);
        self.esc = 0;
        self.enter(Phase::Wait, SHUT_SETTLE);
    }

    /// Nachsehen, bis kein Code-Prozess mehr läuft, und in drei Stufen nachhelfen (Regel
    /// 3). „Beendet“ ist eine Zusage, keine Hoffnung: die Extension erreicht nur die
    /// Fenster, die das Deck kennt, offen halten den Prozess aber alle. Erst zusehen, ab
    /// `SOFT_CLOSE_S` JEDES verbliebene Fenster per WM_CLOSE fragen, ab `FORCE_KILL_S` die
    /// Notbremse, nach `DEAD_TIMEOUT` aufgeben statt in einen halb lebenden VS Code
    /// hineinzustarten.
    pub(super) fn do_wait(&mut self) {
        let name = self
            .exe
            .as_deref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Code.exe".to_string(), |n| n.to_string_lossy().into_owned());
        let Some(n) = self.host.count_exe(&name) else {
            // „Ich weiß es nicht“ ist keine Entwarnung.
            let t = self.l("Prozessliste nicht lesbar, Neustart abgebrochen.".into(), "Cannot read the process list, restart aborted.".into());
            self.fail(&t);
            return;
        };
        if n == 0 {
            let t = self.l("VS Code ist vollständig beendet (kein Prozess mehr).".into(), "VS Code has fully exited (no process left).".into());
            self.say(&t, None);
            self.todo = self.plan.windows.clone();
            self.enter(Phase::Launch, 0.0);
            return;
        }
        let dt = self.host.now() - self.t0;
        if dt >= FORCE_KILL_S && self.esc < 2 {
            self.esc = 2;
            self.forced = true;
            let ok = self.host.kill_editors(&name);
            let (de_tail, en_tail) = if ok { ("", "") } else { (" (Werkzeug meldete einen Fehler)", " (the tool reported an error)") };
            let t = self.l(
                format!("Noch {n} Prozesse offen, Notbremse: {name} samt Kindprozessen beendet{de_tail}."),
                format!("{n} process(es) left, emergency stop: terminated {name} and its child processes{en_tail}."),
            );
            self.say(&t, Some(false));
            self.at = self.host.now() + 2.0;
            return;
        }
        if dt >= SOFT_CLOSE_S && self.esc < 1 {
            self.esc = 1;
            let k = self.host.close_editor_windows();
            let t = self.l(
                format!("Noch {n} Prozesse offen, {k} weitere(s) VS-Code-Fenster zum Schließen aufgefordert."),
                format!("{n} process(es) left, asked {k} more VS Code window(s) to close."),
            );
            self.say(&t, None);
            self.at = self.host.now() + 1.0;
            return;
        }
        if dt > DEAD_TIMEOUT {
            let t = self.l(
                format!(
                    "Es laufen noch {n} VS-Code-Prozesse, auch nach Schließen und Notbremse. Wahrscheinlich \
                     hängt ein „Speichern?“-Dialog. Abgebrochen; der Plan bleibt gespeichert, derselbe Knopf setzt fort."
                ),
                format!(
                    "{n} VS Code process(es) are still running, even after closing and the emergency stop. Most \
                     likely a \"Save?\" dialog is waiting. Aborted; the plan is kept, the same button will resume."
                ),
            );
            self.fail(&t);
            return;
        }
        self.at = self.host.now() + TICK;
    }

    // ── hochfahren ──────────────────────────────────────────────────────────

    /// Fenster der Reihe nach öffnen, mit Abstand.
    pub(super) fn do_launch(&mut self) {
        if self.todo.is_empty() {
            self.pending = self
                .plan
                .windows
                .iter()
                .filter(|w| !self.lost.contains(&w.window))
                .map(|w| (w.window.clone(), w.slots.clone()))
                .collect();
            self.enter(Phase::Hello, 0.0);
            return;
        }
        let w = self.todo.remove(0);
        let exe = self.exe.clone().unwrap_or_default();
        if self.host.launch(&exe, &w.folder) {
            let name = if w.workspace.is_empty() { w.folder.clone() } else { w.workspace.clone() };
            let t = self.l(format!("Öffne {name} …"), format!("Opening {name} …"));
            self.say(&t, None);
        } else {
            self.lost.push(w.window.clone());
        }
        self.at = self.host.now() + LAUNCH_GAP;
    }

    /// Warten, bis die Extensions verbunden UND gebunden sind: erst dann ist ein Fenster
    /// adressierbar.
    pub(super) fn do_hello(&mut self) {
        let waiting: Vec<String> = self.pending.keys().filter(|w| !self.bridge.connected(w)).cloned().collect();
        if waiting.is_empty() {
            let t = self.l("Fenster sind zurück.".into(), "Windows are back.".into());
            self.say(&t, None);
            self.enter(Phase::Resume, SLOT_SETTLE);
            return;
        }
        if self.host.now() - self.t0 > HELLO_TIMEOUT {
            for w in &waiting {
                self.lost.push(w.clone());
                self.pending.remove(w);
            }
            let list = waiting.join(", ");
            let t = self.l(format!("Nicht zurückgemeldet: {list}."), format!("Did not report back: {list}."));
            self.say(&t, Some(false));
            self.enter(Phase::Resume, 0.0);
            return;
        }
        self.at = self.host.now() + TICK;
    }

    /// Chats anlegen: je Fenster streng der Reihe nach (jeder entsteht als Split des
    /// vorigen), Fenster parallel.
    pub(super) fn do_resume(&mut self) {
        let now = self.host.now();
        let wins: Vec<String> = self.pending.keys().cloned().collect();
        for win in wins {
            if self.inflight.contains_key(&win) {
                self.check_inflight(&win, now);
            } else if now >= self.ready_at.get(&win).copied().unwrap_or(0.0) {
                self.send_next(&win, now);
            }
        }
        if self.pending.is_empty() && self.inflight.is_empty() {
            self.finish();
            return;
        }
        self.at = now + TICK;
    }

    /// Ist der angeforderte Chat aufgetaucht? Gezählt wird an der ANZAHL der Terminals,
    /// nicht am Namen: der Wunschname kann belegt sein, dann vergibt die Extension einen
    /// anderen, der Chat ist aber da.
    fn check_inflight(&mut self, win: &str, now: f64) {
        let Some(&(before, deadline)) = self.inflight.get(win) else { return };
        if self.bridge.terminals(win).len() > before {
            self.inflight.remove(win);
            self.made += 1;
            self.ready_at.insert(win.to_string(), now + SLOT_SETTLE);
        } else if now > deadline {
            // Nicht das ganze Fenster aufgeben: der nächste Chat darin kann klappen.
            self.inflight.remove(win);
            let t = self.l(format!("{win}: ein Chat kam nicht zurück."), format!("{win}: one chat did not come back."));
            self.say(&t, Some(false));
            self.ready_at.insert(win.to_string(), now);
        } else {
            return;
        }
        if self.pending.get(win).is_none_or(Vec::is_empty) {
            self.pending.remove(win);
        }
    }

    fn send_next(&mut self, win: &str, now: f64) {
        let Some(entry) = self.pending.get_mut(win).filter(|q| !q.is_empty()).map(|q| q.remove(0)) else {
            self.pending.remove(win);
            return;
        };
        let before = self.bridge.terminals(win).len();
        if !self.bridge.create_agent(win, &entry.slot, &entry.args, &entry.cwd) {
            let t = self.l(format!("{win}: Verbindung weg, Rest übersprungen."), format!("{win}: connection lost, skipped the rest."));
            self.say(&t, Some(false));
            self.pending.remove(win);
            return;
        }
        self.inflight.insert(win.to_string(), (before, now + SLOT_TIMEOUT));
    }

    /// Bilanz. Nur ein Lauf ohne verlorene Fenster räumt den Plan weg; sonst bleibt er,
    /// damit derselbe Knopf fortsetzen kann.
    fn finish(&mut self) {
        let (wins, keep, _) = counts(&self.plan);
        let made = self.made;
        if !self.lost.is_empty() {
            self.phase = Phase::Failed;
            let (n, list) = (self.lost.len(), self.lost.join(", "));
            let t = self.l(
                format!("{made} Chat(s) wiederhergestellt, aber {n} Fenster fehlen ({list}). Plan bleibt gespeichert."),
                format!("Restored {made} chat(s), but {n} window(s) are missing ({list}). The plan is kept."),
            );
            self.say(&t, Some(false));
            return;
        }
        self.store.clear();
        self.phase = Phase::Done;
        let mut t = self.l(
            format!("Fertig: {wins} Fenster, {made} Chat(s), davon {keep} wiederaufgenommen."),
            format!("Done: {wins} window(s), {made} chat(s), {keep} resumed."),
        );
        if self.forced {
            // Nicht verschweigen: in einem Fenster, das das Deck nicht kannte, kann beim
            // harten Beenden etwas verloren gegangen sein.
            t += &self.l(" VS Code musste hart beendet werden.".into(), " VS Code had to be terminated forcefully.".into());
        }
        self.say(&t, Some(true));
    }
}
