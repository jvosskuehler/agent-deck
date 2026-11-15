//! Abgestürzte Agenten fortsetzen: die App-Seite von `deck_core::crash_resume`.
//!
//! Je Takt: tote Pane-Shells vom Host abholen, beurteilen, eine Zeile ins Protokoll,
//! Fortsetzungen in die Warteschlange. Dann die Warteschlange antreiben: zur fälligen
//! Zeit das Fenster mit `claude --resume` öffnen und warten, bis seine Shell sich meldet.

use deck_core::agent_flags::flags;
use deck_core::claude::settings::MODEL_CHOICES;
use deck_core::crash_resume::{Pending, Step, Verdict, continue_prompt, judge, log_line, status_text};

use crate::applog::note;
use crate::deck::Deck;

impl Deck {
    /// Tote Agenten beurteilen und Fortsetzungen einplanen. Nach `read_files`: die
    /// Zustandsdatei des Toten liegt dann noch vor.
    pub fn handle_ended(&mut self, now: f64) {
        let ended = self.wt.drain_ended();
        if ended.is_empty() {
            return;
        }
        let idle = self.desktop.input_idle_s();
        let mut batch = Vec::new();
        for e in ended {
            let st = self.states.get(&e.slot);
            let open = self.wt.has(&Deck::letter(&e.slot));
            let v = judge(&e, st, idle, open, &mut self.resume_memo, now);
            if let Verdict::Resume(plan) = &v {
                self.resumes.push(Pending::new(plan.clone()));
            }
            batch.push((e, status_text(st), v));
        }
        if let Some(line) = log_line(&batch, idle) {
            note(line);
        }
    }

    /// Die Warteschlange der Fortsetzungen einen Schritt weiter.
    pub fn drive_resumes(&mut self, now: f64) {
        if self.resumes.is_empty() {
            return;
        }
        let alive: Vec<String> = self.wt.alive_slots().into_keys().collect();
        let mut keep = Vec::new();
        for mut p in std::mem::take(&mut self.resumes) {
            match p.step(now, |s| alive.iter().any(|a| a == s)) {
                Step::Wait => keep.push(p),
                Step::Launch => {
                    let name = self.launch_resume(&p, now);
                    p.launched_as(name, now);
                    keep.push(p);
                }
                Step::Confirmed(name) => note(format!("{name} fortgesetzt (Session {}, vorher {})", short(&p.plan.session_id), p.plan.slot)),
                Step::GiveUp => note(format!(
                    "{} nicht fortgesetzt: nach {} Versuchen kein Fenster (Session {})",
                    p.plan.slot,
                    p.tries,
                    short(&p.plan.session_id)
                )),
            }
        }
        self.resumes = keep;
    }

    /// Das Fenster öffnen. Modell wie beim ＋, Effort und Modus wie zuletzt gemeldet.
    fn launch_resume(&mut self, p: &Pending, now: f64) -> Option<String> {
        let plan = &p.plan;
        let win = Deck::letter(&plan.slot);
        let model = self.setting_str("model").unwrap_or_else(|| MODEL_CHOICES[0].1.to_string());
        let effort = plan.effort.clone().or_else(|| self.chosen_effort().map(String::from));
        let args = flags(Some(&plan.session_id), Some(&model), effort.as_deref(), plan.mode.as_deref());
        let prompt = plan.nudge.then(|| continue_prompt(self.lang));
        let name = self.wt.resume_agent(&win, &plan.slot, plan.cwd.as_deref(), &args, prompt);
        if name.is_some() {
            self.resume_memo.record(&plan.session_id, now);
        } else {
            note(format!("{}: Fortsetzen nicht gestartet (Block {win} nicht verbunden oder Flags unbrauchbar)", plan.slot));
        }
        name
    }

    /// Slots, die für ein Fortsetzen frei gehalten werden: der worktree-Sweep soll ihre
    /// worktrees in der Wartezeit nicht für verwaist halten.
    pub fn held_for_resume(&self) -> Vec<String> {
        self.resumes.iter().flat_map(Pending::holds).collect()
    }
}

fn short(sid: &str) -> &str {
    sid.get(..8).unwrap_or(sid)
}
