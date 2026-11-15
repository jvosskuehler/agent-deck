//! Agenten, deren Fenster verschwindet, ohne dass das Deck es geschlossen hat: erkennen,
//! ins Protokoll schreiben und die Session von selbst fortsetzen.
//!
//! Anlass (15.11.2026, 04:28): Der Microsoft Store hat PowerShell aktualisiert und dabei
//! jede laufende `pwsh.exe` beendet. Die Agenten liefen in pwsh, alle Fenster waren auf
//! einmal weg, und im Log des Decks stand nichts dazu.
//!
//! Die Shell im Pane läuft mit `-NoExit` (`wt_agent.ps1`): endet claude, bleibt sie
//! stehen. Stirbt die Shell doch, hat entweder jemand das Fenster geschlossen, oder sie
//! wurde von außen beendet (Update, Absturz von Windows Terminal). Wer ein Fenster
//! schließt, hat eben geklickt oder getippt. Liegt die letzte Eingabe am Rechner länger
//! als [`USER_ACTIVE_S`] zurück, war es niemand: dann gilt es als Absturz und wird
//! fortgesetzt. Das ist bewusst die vorsichtige Seite: ein Absturz, während man am
//! Rechner sitzt, bleibt liegen (man sieht ihn ja), ein absichtlich geschlossenes
//! Fenster kommt nie ungefragt zurück.
//!
//! Fortgesetzt wird mit `claude --resume <id>`. Lief der Agent gerade (denkt, arbeitet,
//! wartet auf eine Freigabe), bekommt er dazu einen kurzen Anstoß als ersten Prompt
//! ([`continue_prompt`]), sonst geht nur das Fenster wieder auf. Hier steht nur die
//! Rechnung; Starten und Warten macht die App (`app/src-tauri/src/crash_resume.rs`).

use std::collections::HashMap;

use crate::agent_flags::valid_session;
use crate::field::text;
use crate::i18n::Lang;
use crate::prompt_cache::WORKING;
use crate::slot_state::SlotState;

/// Letzte Eingabe jünger als das: jemand sitzt am Rechner und hat das Fenster wohl
/// selbst geschlossen.
pub const USER_ACTIVE_S: f64 = 15.0;
/// So lange nach dem Tod warten, bevor das Fenster neu aufgeht. Ein Update ersetzt
/// gerade noch die Programmdateien (bei PowerShell am 15.11. eine Sekunde nach dem
/// Beenden), und wer doch am Rechner war, sieht kein Fenster zurückspringen.
pub const RESUME_DELAY_S: f64 = 20.0;
/// So lange darf das neue Fenster brauchen, bis seine Shell sich meldet.
pub const LAUNCH_WAIT_S: f64 = 60.0;
/// Startversuche je Fortsetzen (der zweite nach [`RETRY_DELAY_S`]).
pub const MAX_TRIES: u32 = 2;
pub const RETRY_DELAY_S: f64 = 60.0;
/// Höchstens so oft wird dieselbe Session in [`MEMO_WINDOW_S`] fortgesetzt. Stirbt sie
/// danach wieder, stimmt etwas Grundsätzliches nicht; ein Kreislauf aus Starten und
/// Sterben hilft niemandem.
pub const MAX_RESUMES: usize = 2;
pub const MEMO_WINDOW_S: f64 = 30.0 * 60.0;

/// Eine Pane-Shell, die seit dem letzten Blick tot ist.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    pub slot: String,
    /// Das Deck hat sie selbst beendet (Schließen auf der Kachel).
    pub by_deck: bool,
    /// Beim ersten Blick nach dem Start des Decks schon tot: wann und warum, weiß niemand.
    pub at_start: bool,
}

/// Was fortgesetzt wird.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub slot: String,
    pub session_id: String,
    /// Arbeitsordner der Session (aus den Hooks); `--resume` sucht die Session im
    /// Projekt dieses Ordners.
    pub cwd: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// Er lief gerade: mit Anstoß fortsetzen.
    pub nudge: bool,
    /// Frühester Start (Unix-Sekunden).
    pub due: f64,
}

/// Urteil über einen toten Agenten.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    ByDeck,
    AtStart,
    /// Eingabe vor so vielen Sekunden: vermutlich selbst geschlossen.
    UserActive(f64),
    /// Sein Block ist im Deck nicht mehr verbunden.
    NoBlock,
    NoSession,
    /// Diese Session wurde eben schon [`MAX_RESUMES`]-mal fortgesetzt.
    TooOften,
    Resume(Plan),
}

/// Wann welche Session zuletzt fortgesetzt wurde.
#[derive(Debug, Clone, Default)]
pub struct ResumeMemo {
    starts: HashMap<String, Vec<f64>>,
}

impl ResumeMemo {
    pub fn record(&mut self, session_id: &str, now: f64) {
        self.starts.entry(session_id.to_string()).or_default().push(now);
    }

    /// Fortsetzungen dieser Session im Fenster [`MEMO_WINDOW_S`]; Älteres fliegt raus.
    pub fn recent(&mut self, session_id: &str, now: f64) -> usize {
        self.starts.retain(|_, v| {
            v.retain(|t| now - t < MEMO_WINDOW_S);
            !v.is_empty()
        });
        self.starts.get(session_id).map_or(0, Vec::len)
    }
}

fn status_of(st: Option<&SlotState>) -> Option<&str> {
    text(st?.get("status"))
}

/// Das Urteil. `st` ist die letzte Zustandsdatei des Slots (sie bleibt nach dem Tod
/// liegen, bis ein neuer Agent den Namen übernimmt), `idle_s` die Zeit seit der letzten
/// Eingabe am Rechner.
pub fn judge(e: &Ended, st: Option<&SlotState>, idle_s: f64, block_open: bool, memo: &mut ResumeMemo, now: f64) -> Verdict {
    if e.by_deck {
        return Verdict::ByDeck;
    }
    if e.at_start {
        return Verdict::AtStart;
    }
    if idle_s < USER_ACTIVE_S {
        return Verdict::UserActive(idle_s);
    }
    if !block_open {
        return Verdict::NoBlock;
    }
    let Some(sid) = st.and_then(|s| text(s.get("session_id"))).filter(|s| valid_session(s)) else {
        return Verdict::NoSession;
    };
    if memo.recent(sid, now) >= MAX_RESUMES {
        return Verdict::TooOften;
    }
    let field = |k: &str| st.and_then(|s| text(s.get(k))).map(String::from);
    let status = status_of(st).unwrap_or("");
    Verdict::Resume(Plan {
        slot: e.slot.clone(),
        session_id: sid.trim().to_string(),
        cwd: field("cwd"),
        effort: field("effort"),
        mode: field("mode"),
        nudge: WORKING.contains(&status) || status == "waiting",
        due: now + RESUME_DELAY_S,
    })
}

/// Erster Prompt nach dem Fortsetzen, wenn der Agent mitten in der Arbeit war. Ohne
/// Gedankenstriche und Anführungszeichen: er geht als Argument durch PowerShell.
pub fn continue_prompt(lang: Lang) -> &'static str {
    lang.l(
        "Dein Fenster wurde unerwartet beendet (Absturz oder Update), das Agent Deck hat die Session fortgesetzt. Prüf kurz, wo du stehengeblieben bist, und mach dann weiter.",
        "Your window was closed unexpectedly (crash or update), Agent Deck resumed the session. Check briefly where you left off, then carry on.",
    )
}

fn short_id(sid: &str) -> &str {
    sid.get(..8).unwrap_or(sid)
}

fn verdict_text(v: &Verdict) -> String {
    match v {
        Verdict::ByDeck => "vom Deck geschlossen".into(),
        Verdict::AtStart => "beim Start schon tot, nicht fortgesetzt".into(),
        Verdict::UserActive(_) => "vermutlich selbst geschlossen, nicht fortgesetzt".into(),
        Verdict::NoBlock => "Block nicht verbunden, nicht fortgesetzt".into(),
        Verdict::NoSession => "ohne Session-ID, nicht fortgesetzt".into(),
        Verdict::TooOften => format!("in {:.0} min schon {MAX_RESUMES}-mal fortgesetzt, jetzt nicht mehr", MEMO_WINDOW_S / 60.0),
        Verdict::Resume(p) => format!(
            "wird in {RESUME_DELAY_S:.0} s fortgesetzt{} (Session {})",
            if p.nudge { " mit Anstoß" } else { "" },
            short_id(&p.session_id)
        ),
    }
}

/// Eine Protokollzeile für alle Agenten, die im selben Takt verschwunden sind. Vom Deck
/// geschlossene zählen nicht: das war eine Aktion, kein Ereignis. `None` = nichts zu sagen.
pub fn log_line(batch: &[(Ended, Option<String>, Verdict)], idle_s: f64) -> Option<String> {
    let items: Vec<String> = batch
        .iter()
        .filter(|(_, _, v)| *v != Verdict::ByDeck)
        .map(|(e, status, v)| format!("{} ({}): {}", e.slot, status.as_deref().unwrap_or("ohne Status"), verdict_text(v)))
        .collect();
    if items.is_empty() {
        return None;
    }
    let at_start = batch.iter().all(|(e, _, _)| e.at_start);
    let head = match (items.len(), at_start) {
        (1, true) => "Agent beim Start ohne Fenster vorgefunden".to_string(),
        (n, true) => format!("{n} Agenten beim Start ohne Fenster vorgefunden"),
        (1, false) => format!("Agent ohne das Deck beendet (letzte Eingabe vor {idle_s:.0} s)"),
        (n, false) => format!("{n} Agenten gleichzeitig ohne das Deck beendet (letzte Eingabe vor {idle_s:.0} s)"),
    };
    Some(format!("{head}: {}", items.join("; ")))
}

/// Status aus der Zustandsdatei, für [`log_line`].
pub fn status_text(st: Option<&SlotState>) -> Option<String> {
    status_of(st).map(String::from)
}

/// Ein Fortsetzen, das noch läuft: warten, starten, auf die Shell warten, nochmal.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub plan: Plan,
    /// Unter diesem Slot-Namen zuletzt gestartet, und wann.
    pub launched: Option<(String, f64)>,
    pub tries: u32,
}

/// Was die App im Takt mit einem [`Pending`] tut.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Wait,
    /// Jetzt starten (danach [`Pending::launched_as`] setzen).
    Launch,
    /// Die Shell des neuen Fensters lebt: erledigt.
    Confirmed(String),
    /// Auch der letzte Versuch kam nicht an.
    GiveUp,
}

impl Pending {
    pub fn new(plan: Plan) -> Pending {
        Pending { plan, launched: None, tries: 0 }
    }

    /// Der nächste Schritt. `alive` sagt, ob ein Slot-Name gerade eine lebende Shell hat.
    pub fn step(&mut self, now: f64, alive: impl Fn(&str) -> bool) -> Step {
        match &self.launched {
            Some((name, _)) if alive(name) => Step::Confirmed(name.clone()),
            None if now >= self.plan.due => Step::Launch,
            None => Step::Wait,
            Some((_, t)) if now - t < LAUNCH_WAIT_S => Step::Wait,
            Some(_) if self.tries < MAX_TRIES => {
                self.launched = None;
                self.plan.due = now + RETRY_DELAY_S;
                Step::Wait
            }
            Some(_) => Step::GiveUp,
        }
    }

    /// Nach [`Step::Launch`]: gestartet unter `name` (`None` = der Start wurde gar nicht
    /// angenommen; das zählt als Versuch, der nächste kommt nach [`RETRY_DELAY_S`]).
    pub fn launched_as(&mut self, name: Option<String>, now: f64) {
        self.tries += 1;
        self.launched = Some((name.unwrap_or_default(), now));
    }

    /// Der Slot, der gerade frei gehalten wird (für den worktree-Sweep).
    pub fn holds(&self) -> Vec<String> {
        let mut out = vec![self.plan.slot.clone()];
        if let Some((n, _)) = &self.launched
            && !n.is_empty()
            && *n != self.plan.slot
        {
            out.push(n.clone());
        }
        out
    }
}
