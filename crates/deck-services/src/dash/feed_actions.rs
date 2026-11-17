//! Die Knöpfe des PR-Feeds und des Kommentar-Fensters (`dash_action`, docs/IPC.md):
//! Halteliste, Not-Aus, „Später“, Kommentar-Fenster schließen, „Claude soll umsetzen“,
//! Agent abbrechen, Entwurf absenden oder bearbeiten.
//!
//! Läuft im Hintergrund-Thread der App (gh und claude brauchen Sekunden), nie unter der
//! Sperre des Decks. Jede Aktion prüft selbst, ob sie zum aktuellen Stand passt.

use std::path::PathBuf;

use deck_core::dash::bg_tasks::TaskKind;
use deck_core::dash::feed::pending;
use deck_core::dash::feed_state::{Did, RunStatus};

use super::feed_agents::{finish, start_run};
use super::feed_round::activity;
use super::feed_service::{lock, FeedService};

/// Was die App nach einer Aktion tun soll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedReply {
    /// Nichts weiter.
    Quiet,
    /// Kurz im Fenster zeigen.
    Say(String),
    /// Diese Datei im Editor öffnen (Entwurf ändern).
    Open(PathBuf),
}

impl FeedService {
    /// Gehört `command` zum Feed?
    pub fn handles(command: &str) -> bool {
        let verb = command.split_once(':').map_or(command, |(v, _)| v);
        matches!(verb, "hold" | "release" | "automation" | "later" | "popupClose" | "delegate" | "agentAbort" | "draftSend" | "draftEdit")
    }

    /// Einen Knopf ausführen. `Err` = kurzer Grund für die Anzeige.
    pub fn action(&self, command: &str) -> Result<FeedReply, String> {
        let (verb, arg) = command.split_once(':').ok_or("unbekannte Aktion")?;
        let sh = &self.sh;
        let now = (sh.deps.now)();
        let snap = (sh.deps.prs)();
        let open_pr = |key: &str| snap.open.iter().find(|p| p.key() == key).cloned();
        let newest = |key: &str| open_pr(key).and_then(|p| pending(&p, &snap.viewer).last().map(|x| x.comment.at)).unwrap_or(now);
        let reply = match verb {
            "hold" | "release" => {
                let mut i = lock(&sh.inner);
                if verb == "hold" { i.st.hold.insert(arg.to_string()) } else { i.st.hold.remove(arg) };
                i.dirty = true;
                sh.log.note(&format!("Halteliste: {} {verb}", arg.rsplit('/').next().unwrap_or(arg)));
                FeedReply::Quiet
            }
            "automation" => {
                let on = arg == "on";
                let mut i = lock(&sh.inner);
                i.st.automation = on;
                i.dirty = true;
                sh.log.note(if on { "Automatik an" } else { "Automatik aus (Not-Aus)" });
                FeedReply::Quiet
            }
            "later" => {
                let at = newest(arg);
                let mut i = lock(&sh.inner);
                i.st.snoozed.insert(arg.to_string(), at);
                let seen = i.st.notified.entry(arg.to_string()).or_insert(at);
                *seen = seen.max(at);
                i.dirty = true;
                FeedReply::Quiet
            }
            "popupClose" => {
                let at = newest(arg);
                let mut i = lock(&sh.inner);
                let seen = i.st.notified.entry(arg.to_string()).or_insert(at);
                *seen = seen.max(at);
                i.dirty = true;
                FeedReply::Quiet
            }
            "delegate" => {
                let pr = open_pr(arg).ok_or("PR nicht mehr offen")?;
                let list = pending(&pr, &snap.viewer);
                let target = list.last().ok_or("kein offener Kommentar")?;
                let id = start_run(sh, &snap, &pr, TaskKind::HumanReply, Some((&target.comment, &target.thread)), None, now)?;
                let mut i = lock(&sh.inner);
                let seen = i.st.notified.entry(arg.to_string()).or_insert(target.comment.at);
                *seen = seen.max(target.comment.at);
                i.dirty = true;
                FeedReply::Say(format!("Claude setzt um ({id})"))
            }
            "agentAbort" => {
                let known = lock(&sh.inner).procs.get_mut(arg).map(|p| p.kill()).is_some();
                if !known {
                    return Err("kein laufender Agent mit dieser Id".into());
                }
                finish(sh, &snap, arg, RunStatus::Aborted, "abgebrochen".into(), now);
                FeedReply::Quiet
            }
            "draftEdit" => {
                let path = lock(&sh.inner).st.drafts.get(arg).map(|d| PathBuf::from(&d.path)).ok_or("kein Entwurf")?;
                FeedReply::Open(path)
            }
            "draftSend" => send_draft(self, arg, now)?,
            _ => return Err("unbekannte Aktion".into()),
        };
        sh.save();
        sh.notify();
        Ok(reply)
    }
}

/// Den Entwurf senden: in den Thread (und ihn auflösen) bzw. als PR-Kommentar.
fn send_draft(svc: &FeedService, key: &str, now: f64) -> Result<FeedReply, String> {
    let sh = &svc.sh;
    let snap = (sh.deps.prs)();
    let url = snap.open.iter().find(|p| p.key() == key).map(|p| p.url.clone()).ok_or("PR nicht mehr offen")?;
    let draft = {
        let mut i = lock(&sh.inner);
        let d = i.st.drafts.get_mut(key).ok_or("kein Entwurf")?;
        if d.sending {
            return Err("wird schon gesendet".into());
        }
        // Frisch aus der Datei: „Ändern“ hat sie vielleicht eben bearbeitet.
        if let Ok(text) = std::fs::read_to_string(&d.path) {
            d.text = text;
        }
        if d.text.trim().is_empty() {
            return Err("Entwurf ist leer".into());
        }
        d.sending = true;
        d.error.clear();
        d.clone()
    };
    sh.notify();
    let body = draft.text.trim();
    let sent = if draft.thread_id.is_empty() {
        sh.deps.write.comment(&url, body)
    } else {
        sh.deps.write.reply_thread(&draft.thread_id, body).and_then(|()| sh.deps.write.resolve_thread(&draft.thread_id))
    };
    let mut i = lock(&sh.inner);
    i.dirty = true;
    match sent {
        Ok(()) => {
            i.st.drafts.remove(key);
            i.st.record(activity(&snap, key, Did::ReplySent, "", now));
            sh.log.note(&format!("Antwort gesendet: {}", key.rsplit('/').next().unwrap_or(key)));
            let _ = std::fs::remove_file(&draft.path);
            Ok(FeedReply::Say("Antwort gesendet".into()))
        }
        Err(e) => {
            if let Some(d) = i.st.drafts.get_mut(key) {
                d.sending = false;
                d.error = e.clone();
            }
            i.st.record(activity(&snap, key, Did::ReplyFailed(e.clone()), "", now));
            sh.log.note(&format!("Antwort nicht gesendet: {e}"));
            Err(e)
        }
    }
}
