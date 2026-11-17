//! Das Info-Fenster in der App (`deck_core::dash`): Datenquellen starten, je Takt die
//! Ansicht bauen ([`DashView`]); dazu die Befehle `get_dash`, `open_dash_url` und
//! `dash_action` (Approve, Titan-Lauf beenden, Worktrees entfernen, Fenster zu).
//!
//! Die Dienste laufen nur, solange das Info-Fenster eingeschaltet ist (Einstellung
//! „Info-Fenster in der freien Fensterkachel“): gh, ssh und git sollen nicht im
//! Hintergrund arbeiten, wenn niemand hinsieht. Tests bauen [`Dash::inert`] ohne Dienst.

use std::sync::{Arc, Mutex};

use deck_core::dash::board::{self, DashView};
use deck_core::dash::{self as dash_core};
use deck_core::i18n::Lang;
use deck_services::dash::cleanup::CleanupService;
use deck_services::dash::costs::CostService;
use deck_services::dash::github::GithubService;
use deck_services::dash::titan::TitanService;
use deck_services::logfile::{LogFile, Rotate};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::app_paths::Paths;
use crate::applog::note;
use crate::services::Wake;
use crate::state::{lock, shared};

/// So oft (s) bekommt der Aufräum-Dienst die Ordner der offenen Agenten.
const IN_USE_EVERY_S: f64 = 5.0;
/// So lange (s) steht das Ergebnis einer Aktion im Fenster.
const NOTICE_S: f64 = 20.0;

/// Die Datenquellen. Jede hat ihren eigenen Thread, der erst mit `start` anläuft.
struct Sources {
    github: GithubService,
    costs: CostService,
    titan: TitanService,
    cleanup: CleanupService,
    wake: Wake,
}

impl Sources {
    fn system(paths: &Paths, wake: Wake) -> Sources {
        let log = |name: &str| LogFile::new(paths.state_dir.join(name), 200_000, Rotate::Rename);
        Sources {
            github: GithubService::system(&paths.state_dir),
            costs: CostService::new(paths.projects_dir.clone(), Some(log("dash-costs.log"))),
            titan: TitanService::system(log("dash-titan.log")),
            cleanup: CleanupService::system(&paths.state_dir),
            wake,
        }
    }

    fn start(&self) {
        self.github.start(self.wake.clone());
        let w = self.wake.clone();
        self.costs.start(move || w());
        self.titan.start(self.wake.clone());
        self.cleanup.start(self.wake.clone());
    }

    fn stop(&self) {
        self.github.stop();
        self.costs.stop();
        self.titan.stop();
        self.cleanup.stop();
    }
}

/// Was die Ansicht über den Takt wissen muss.
pub struct DashInput<'a> {
    pub now: f64,
    pub lang: Lang,
    pub enabled: bool,
    /// Ordner der offenen Agenten (cwd, gemeldete worktrees): die räumt niemand weg.
    pub in_use: Vec<String>,
    pub jira_prefix: &'a str,
}

pub struct Dash {
    /// Geteilt, damit eine Aktion (gh, ssh: Sekunden) ohne Deck-Sperre laufen kann.
    sources: Option<Arc<Sources>>,
    running: bool,
    /// Stand der gemergten PRs, den der Aufräum-Dienst zuletzt bekam.
    merged_at: f64,
    in_use_at: f64,
    /// Ergebnis der letzten Aktion und bis wann es steht.
    notice: Mutex<Option<(String, f64)>>,
}

impl Dash {
    /// Ohne Dienste (Tests).
    pub fn inert() -> Dash {
        Dash { sources: None, running: false, merged_at: 0.0, in_use_at: f64::NEG_INFINITY, notice: Mutex::new(None) }
    }

    /// Echt: Dienste angelegt (laufen erst mit dem ersten eingeschalteten Takt an).
    pub fn system(paths: &Paths, wake: Wake) -> Dash {
        Dash { sources: Some(Arc::new(Sources::system(paths, wake))), ..Dash::inert() }
    }

    /// Die Ansicht dieses Takts; `None`, wenn abgeschaltet.
    pub fn view(&mut self, input: &DashInput) -> Option<DashView> {
        self.switch(input.enabled);
        if !input.enabled {
            return None;
        }
        let notice = {
            let mut n = lock(&self.notice);
            if n.as_ref().is_some_and(|(_, until)| *until < input.now) {
                *n = None;
            }
            n.as_ref().map(|(t, _)| t.clone())
        };
        let Some(src) = &self.sources else {
            let empty = deck_core::dash::prs::PrSnapshot::default();
            return Some(DashView {
                costs: None,
                prs: board::prs(&empty, input.now, input.lang),
                reviews: Vec::new(),
                titan: None,
                cleanup: None,
                notice,
            });
        };
        let prs = src.github.snapshot();
        feed_cleanup(src, &prs, input, &mut self.merged_at, &mut self.in_use_at);
        src.costs.set_prefix(input.jira_prefix);
        Some(DashView {
            costs: src.costs.view(),
            prs: board::prs(&prs, input.now, input.lang),
            reviews: board::reviews(&prs, input.now, input.lang),
            titan: board::titan(&src.titan.snapshot(), input.lang),
            cleanup: board::cleanup(&src.cleanup.snapshot(), input.lang),
            notice,
        })
    }

    /// Dienste an- bzw. abschalten, wenn der Schalter wechselt.
    fn switch(&mut self, on: bool) {
        let Some(src) = &self.sources else { return };
        if on && !self.running {
            src.start();
            note("Info-Fenster: Datenquellen gestartet");
        } else if !on && self.running {
            src.stop();
            note("Info-Fenster: abgeschaltet, Datenquellen angehalten");
        }
        self.running = on;
    }

    fn say(&self, text: String, now: f64) {
        *lock(&self.notice) = Some((text, now + NOTICE_S));
    }
}

/// Der Aufräum-Dienst braucht die gemergten PRs (nur bei neuem Stand) und die Ordner der
/// offenen Agenten (alle paar Sekunden).
fn feed_cleanup(src: &Sources, prs: &dash_core::prs::PrSnapshot, input: &DashInput, merged_at: &mut f64, in_use_at: &mut f64) {
    if prs.fetched_at > *merged_at && prs.error.is_none() {
        *merged_at = prs.fetched_at;
        src.cleanup.set_merged(prs.merged.clone());
    }
    if input.now - *in_use_at >= IN_USE_EVERY_S {
        *in_use_at = input.now;
        src.cleanup.set_in_use(input.in_use.clone());
    }
}

/// Startwert für das Info-Fenster (danach `deck://dash`).
#[tauri::command]
pub fn get_dash(app: AppHandle) -> Option<DashView> {
    lock(&shared(&app).last_dash).clone()
}

/// Klick auf eine Zeile oder Kachel: Adresse im Browser.
#[tauri::command]
pub async fn open_dash_url(app: AppHandle, url: String) -> Result<(), String> {
    if !dash_core::allowed_url(&url) {
        // Die Adresse kommt aus einer fremden Antwort; nur die Länge ins Protokoll.
        note(format!("Info-Fenster: Adresse abgelehnt ({} Zeichen)", url.len()));
        return Err("Adresse nicht erlaubt".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Knopf im Info-Fenster. Die Oberfläche hat schon nach einer Bestätigung gefragt; die
/// Dienste prüfen trotzdem selbst, ob die Aktion erlaubt ist (nur bekannte Review-Adressen,
/// nur verwaiste Läufe, nur entfernbare Worktrees). gh und ssh laufen im Hintergrund.
#[tauri::command]
pub async fn dash_action(app: AppHandle, command: String) -> Result<(), String> {
    if command == "closePopup" {
        crate::dash_window::close_popup(&app);
        return Ok(());
    }
    let work = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_action(&work, &command)).await.map_err(|e| e.to_string())?
}

fn run_action(app: &AppHandle, command: &str) -> Result<(), String> {
    let s = shared(app);
    let (src, lang) = {
        let d = lock(&s.deck);
        (d.dash.sources.clone(), d.lang)
    };
    let src = src.ok_or("Info-Fenster ohne Dienste")?;
    let l = |de: &'static str, en: &'static str| lang.l(de, en);
    let outcome: Result<String, String> = match command.split_once(':') {
        None if command == "cleanupWorktrees" => {
            let started = src.cleanup.remove_all();
            note(format!("Info-Fenster: Aufräumen {}", if started { "gestartet" } else { "läuft schon" }));
            crate::poll::nudge(app);
            return Ok(());
        }
        Some(("approve", url)) => {
            src.github.approve(url).map(|()| l("Approve gesendet", "Approved").to_string()).map_err(|e| format!("{} {e}", l("Approve gescheitert:", "Approve failed:")))
        }
        Some(("titanKill", name)) => src
            .titan
            .kill_orphan(name)
            .map(|n| format!("{} {name} {} ({n} {})", l("Lauf", "Run"), l("beendet", "stopped"), l("Prozesse", "processes")))
            .map_err(|e| format!("{} {e}", l("Nicht beendet:", "Not stopped:"))),
        None if command == "titanKillAll" => {
            let names: Vec<String> = src.titan.snapshot().runs.into_iter().filter(|r| r.orphan).map(|r| r.name).collect();
            let done = names.iter().filter(|n| src.titan.kill_orphan(n).is_ok()).count();
            Ok(format!("{done} {} {} {}", l("von", "of"), names.len(), l("Läufen beendet", "runs stopped")))
        }
        _ => return Err(format!("unbekannte Aktion: {command}")),
    };
    lock(&s.deck).dash.say(outcome.clone().unwrap_or_else(|e| e), crate::now());
    crate::poll::nudge(app);
    outcome.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_off_there_is_no_view() {
        let mut d = Dash::inert();
        let input = DashInput { now: crate::now(), lang: Lang::German, enabled: false, in_use: Vec::new(), jira_prefix: "ORPH" };
        assert!(d.view(&input).is_none());
    }

    /// Probelauf gegen die echten Quellen (gh, ssh, git, Transcripts; nur lesend, nichts
    /// wird entfernt oder beendet): schreibt die Ansicht als JSON nach `DASH_DUMP`, damit der
    /// Browser-Mock sie zeigen kann (`?dash=real`).
    /// `DASH_DUMP=app/dash-real.json cargo test -p agent-deck dash::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_dash_dump() {
        let paths = Paths::from_env();
        let src = Sources::system(&paths, Arc::new(|| {}));
        let now = crate::now();
        src.github.fetch_once();
        src.costs.set_prefix("ORPH");
        src.costs.scan_now(now);
        src.titan.poll_once();
        src.cleanup.set_merged(src.github.snapshot().merged);
        src.cleanup.scan_now();
        let prs = src.github.snapshot();
        let view = DashView {
            costs: src.costs.view(),
            prs: board::prs(&prs, now, Lang::German),
            reviews: board::reviews(&prs, now, Lang::German),
            titan: board::titan(&src.titan.snapshot(), Lang::German),
            cleanup: board::cleanup(&src.cleanup.snapshot(), Lang::German),
            notice: None,
        };
        let json = serde_json::to_string_pretty(&view).unwrap();
        match std::env::var("DASH_DUMP") {
            Ok(path) => std::fs::write(path, json).unwrap(),
            Err(_) => println!("{json}"),
        }
    }

    #[test]
    fn a_notice_shows_for_a_while_then_goes() {
        let mut d = Dash::inert();
        let now = 1000.0;
        d.say("Approve gesendet".into(), now);
        let input = |now| DashInput { now, lang: Lang::German, enabled: true, in_use: Vec::new(), jira_prefix: "ORPH" };
        assert_eq!(d.view(&input(now + 1.0)).unwrap().notice.as_deref(), Some("Approve gesendet"));
        assert_eq!(d.view(&input(now + NOTICE_S + 1.0)).unwrap().notice, None);
    }
}
