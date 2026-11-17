//! Das Info-Fenster in der App (`deck_core::dash`): Datenquellen starten, je Takt die
//! Ansicht bauen ([`DashView`]); dazu die Befehle `get_dash`, `open_dash_url` und
//! `dash_action` (Approve, Review aus- und einblenden, Titan-Lauf beenden, Worktrees
//! entfernen, Fenster zu).
//!
//! Die Dienste laufen nur, solange das Info-Fenster eingeschaltet ist (Einstellung
//! „Info-Fenster in der freien Fensterkachel“): gh, ssh und git sollen nicht im
//! Hintergrund arbeiten, wenn niemand hinsieht. Titan (ssh) hat einen eigenen Schalter
//! (`titan`, neue Installationen: aus); aus heißt: kein ssh. Tests bauen [`Dash::inert`]
//! ohne Dienst.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use deck_core::dash::board::{self, DashView};
use deck_core::dash::{self as dash_core};
use deck_core::i18n::Lang;
use deck_services::dash::cleanup::CleanupService;
use deck_services::dash::costs::CostService;
use deck_services::dash::feed_service::FeedService;
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
/// Ausgeblendete Review-Anfragen (JSON-Liste der PR-Adressen) im App-Ordner.
const HIDDEN_FILE: &str = "dash-hidden-reviews.json";

/// Die Datenquellen. Jede hat ihren eigenen Thread, der erst mit `start` anläuft.
struct Sources {
    github: Arc<GithubService>,
    costs: CostService,
    titan: TitanService,
    cleanup: Arc<CleanupService>,
    /// PR-Feed und Automatik (`pr_feed.rs`); läuft nach jedem Abruf der PRs eine Runde.
    feed: Arc<FeedService>,
    wake: Wake,
}

impl Sources {
    fn system(paths: &Paths, wake: Wake) -> Sources {
        let log = |name: &str| LogFile::new(paths.state_dir.join(name), 200_000, Rotate::Rename);
        let github = Arc::new(GithubService::system(&paths.state_dir));
        let prs = github.clone();
        Sources {
            feed: Arc::new(FeedService::system(&paths.state_dir, &paths.app_dir, Arc::new(move || prs.snapshot()))),
            github,
            costs: CostService::new(paths.projects_dir.clone(), Some(log("dash-costs.log"))),
            titan: TitanService::system(log("dash-titan.log")),
            cleanup: Arc::new(CleanupService::system(&paths.state_dir)),
            wake,
        }
    }

    fn start(&self) {
        // Nach jedem Abruf der PRs gleich eine Runde des Feeds (mergen, Agenten, Jira).
        let (wake, feed) = (self.wake.clone(), self.feed.clone());
        self.github.start(Arc::new(move || {
            wake();
            feed.kick();
        }));
        self.feed.start(self.wake.clone());
        let w = self.wake.clone();
        self.costs.start(move || w());
        self.cleanup.start(self.wake.clone());
    }

    fn stop(&self) {
        self.github.stop();
        self.feed.stop();
        self.costs.stop();
        self.cleanup.stop();
    }
}

/// Was die Ansicht über den Takt wissen muss.
pub struct DashInput<'a> {
    pub now: f64,
    pub lang: Lang,
    pub enabled: bool,
    /// Titan-Kachel und ssh-Abfrage (`deck_core::dash::titan_enabled`).
    pub titan: bool,
    /// Repos-Ordner, nur wenn er taugt (`deck_settings::usable_repos_root`): dort sucht das
    /// Aufräumen nach `wt`.
    pub repos_root: Option<std::path::PathBuf>,
    /// Ordner der offenen Agenten (cwd, gemeldete worktrees): die räumt niemand weg.
    pub in_use: Vec<String>,
    pub jira_prefix: &'a str,
    /// `pr_reviewer` und `github_org` aus den Einstellungen.
    pub feed: crate::pr_feed::FeedSettings,
}

pub struct Dash {
    /// Geteilt, damit eine Aktion (gh, ssh: Sekunden) ohne Deck-Sperre laufen kann.
    sources: Option<Arc<Sources>>,
    running: bool,
    titan_running: bool,
    /// Stand der gemergten PRs, den der Aufräum-Dienst zuletzt bekam.
    merged_at: f64,
    in_use_at: f64,
    /// Ergebnis der letzten Aktion und bis wann es steht.
    notice: Mutex<Option<(String, f64)>>,
    /// Ausgeblendete Review-Anfragen (PR-Adressen) und wo sie liegen; ohne Pfad (Tests)
    /// nur im Speicher.
    hidden: Vec<String>,
    hidden_path: Option<PathBuf>,
}

impl Dash {
    /// Ohne Dienste (Tests).
    pub fn inert() -> Dash {
        Dash {
            sources: None,
            running: false,
            titan_running: false,
            merged_at: 0.0,
            in_use_at: f64::NEG_INFINITY,
            notice: Mutex::new(None),
            hidden: Vec::new(),
            hidden_path: None,
        }
    }

    /// Echt: Dienste angelegt (laufen erst mit dem ersten eingeschalteten Takt an).
    pub fn system(paths: &Paths, wake: Wake) -> Dash {
        let hidden_path = paths.app_dir.join(HIDDEN_FILE);
        Dash { sources: Some(Arc::new(Sources::system(paths, wake))), hidden: load_hidden(&hidden_path), hidden_path: Some(hidden_path), ..Dash::inert() }
    }

    /// Die Ansicht dieses Takts; `None`, wenn abgeschaltet.
    pub fn view(&mut self, input: &DashInput) -> Option<DashView> {
        // Präfix und Ordner vor dem Start: der erste Lauf soll schon mit ihnen lesen.
        if let Some(src) = &self.sources {
            src.costs.set_prefix(input.jira_prefix);
            src.cleanup.set_repos_root(input.repos_root.as_deref());
        }
        self.switch(input.enabled, input.enabled && input.titan);
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
                hidden_reviews: Vec::new(),
                titan: None,
                cleanup: None,
                notice,
                feed: None,
                popup: None,
            });
        };
        let prs = src.github.snapshot();
        feed_cleanup(src, &prs, input, &mut self.merged_at, &mut self.in_use_at);
        let (feed, popup) = crate::pr_feed::tick(&src.feed, &src.cleanup, &input.feed, input.jira_prefix, &input.in_use, input.now, input.lang);
        if board::prune_hidden(&mut self.hidden, &prs) {
            self.save_hidden("vergessen (Anfrage erledigt)");
        }
        let (reviews, hidden_reviews) = board::split_reviews(board::reviews(&prs, input.now, input.lang), &self.hidden);
        Some(DashView {
            costs: src.costs.view(),
            prs: board::prs(&prs, input.now, input.lang),
            reviews,
            hidden_reviews,
            titan: if input.titan { board::titan(&src.titan.snapshot(), input.lang) } else { None },
            cleanup: board::cleanup(&src.cleanup.snapshot(), input.lang),
            notice,
            feed: Some(feed),
            popup,
        })
    }

    /// Dienste an- bzw. abschalten, wenn ein Schalter wechselt.
    fn switch(&mut self, on: bool, titan: bool) {
        let Some(src) = &self.sources else { return };
        if on && !self.running {
            src.start();
            note("Info-Fenster: Datenquellen gestartet");
        } else if !on && self.running {
            src.stop();
            note("Info-Fenster: abgeschaltet, Datenquellen angehalten");
        }
        if titan && !self.titan_running {
            src.titan.start(src.wake.clone());
            note("Info-Fenster: Titan-Abfrage gestartet");
        } else if !titan && self.titan_running {
            src.titan.stop();
            note("Info-Fenster: Titan-Abfrage angehalten");
        }
        self.running = on;
        self.titan_running = titan;
    }

    fn say(&self, text: String, now: f64) {
        *lock(&self.notice) = Some((text, now + NOTICE_S));
    }

    /// Eine Review-Anfrage aus- (`hide`) oder wieder einblenden. `false` = nichts geändert.
    fn set_hidden(&mut self, url: &str, hide: bool) -> bool {
        let known = self.hidden.iter().any(|u| u == url);
        if hide == known {
            return false;
        }
        if hide {
            self.hidden.push(url.to_string());
        } else {
            self.hidden.retain(|u| u != url);
        }
        self.save_hidden(if hide { "ausgeblendet" } else { "eingeblendet" });
        true
    }

    fn save_hidden(&self, what: &str) {
        let Some(path) = &self.hidden_path else { return };
        let json = serde_json::to_vec(&self.hidden).unwrap_or_default();
        match deck_core::paths::atomic_write(path, &json) {
            // Nur die Anzahl: die Adressen nennen fremde Repos.
            Ok(()) => note(format!("Info-Fenster: Review {what}, {} ausgeblendet", self.hidden.len())),
            Err(e) => note(format!("Info-Fenster: ausgeblendete Reviews nicht gespeichert ({what}): {}", e.kind())),
        }
    }
}

/// Ausgeblendete Review-Adressen lesen; fehlt die Datei oder ist kaputt: keine.
fn load_hidden(path: &std::path::Path) -> Vec<String> {
    deck_core::paths::load_json(path)
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        .unwrap_or_default()
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
    // Aus- und Einblenden ist nur eine Liste im Deck: kein gh, gleich hier.
    let toggle = match command.split_once(':') {
        Some(("hideReview", url)) => Some((url, true)),
        Some(("showReview", url)) => Some((url, false)),
        _ => None,
    };
    if let Some((url, hide)) = toggle {
        if !dash_core::allowed_url(url) {
            return Err("Adresse nicht erlaubt".into());
        }
        lock(&shared(&app).deck).dash.set_hidden(url, hide);
        crate::poll::nudge(&app);
        return Ok(());
    }
    let work = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_action(&work, &command)).await.map_err(|e| e.to_string())?
}

fn run_action(app: &AppHandle, command: &str) -> Result<(), String> {
    let s = shared(app);
    let (src, lang, titan) = {
        let d = lock(&s.deck);
        (d.dash.sources.clone(), d.lang, d.dash.titan_running)
    };
    let src = src.ok_or("Info-Fenster ohne Dienste")?;
    if command.starts_with("titanKill") && !titan {
        // Titan ist abgeschaltet: kein ssh, auch nicht auf Knopfdruck.
        return Err("Titan ist abgeschaltet".into());
    }
    let l = |de: &'static str, en: &'static str| lang.l(de, en);
    if FeedService::handles(command) {
        let outcome = crate::pr_feed::action(app, &src.feed, &src.github, command);
        let text = match &outcome {
            Ok(t) => t.clone(),
            Err(e) => format!("{} {e}", l("Nicht ausgeführt:", "Not done:")),
        };
        if !text.is_empty() {
            lock(&s.deck).dash.say(text, crate::now());
        }
        crate::poll::nudge(app);
        return outcome.map(drop);
    }
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
#[path = "dash_tests.rs"]
mod tests;
