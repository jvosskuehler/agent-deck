//! Datenquelle `github`: die eigenen PRs über `gh api graphql`, alle 3 Min.
//!
//! Ein Aufruf holt alles mit Aliassen (Vertrag in `deck_core::dash::prs`): offene PRs,
//! heute erstellte und heute gemergte (Anzahl, Ortszeit) und die gemergten PRs der
//! letzten 30 Tage in sechs Fenstern zu fünf Tagen. Eine Suche liefert höchstens 100
//! Treffer; am 15.11.2026 waren es 130 in 30 Tagen, davon 107 in den letzten zehn.
//!
//! `GITHUB_TOKEN`/`GH_TOKEN` werden dem Kindprozess entzogen: der Token in der Umgebung
//! des Nutzers ist ungültig, die Keyring-Anmeldung von gh funktioniert.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{Days, NaiveDate};
use deck_core::dash::prs::{self, ALIAS_CREATED_TODAY, ALIAS_MERGED_PREFIX, ALIAS_MERGED_TODAY, ALIAS_OPEN, ALIAS_REVIEWS, PrSnapshot};
use serde_json::Value;

use super::hidden_cmd;
use crate::fsutil::now_unix;
use crate::logfile::{LogFile, Rotate};

/// Takt nach einem erfolgreichen Abruf.
pub const POLL: Duration = Duration::from_secs(180);
/// Takt nach einem gescheiterten.
pub const RETRY: Duration = Duration::from_secs(60);
/// Zeitlimit für gh.
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const LOG_FILE: &str = "dash-github.log";
/// Gemergte PRs: so viele Fenster zu [`WINDOW_DAYS`] Tagen.
const WINDOWS: u64 = 6;
const WINDOW_DAYS: u64 = 5;

pub use super::ticker::Wake;

/// Eine GraphQL-Abfrage an GitHub; `Ok` = Antwort-JSON als Text.
pub trait GhApi: Send + Sync {
    fn graphql(&self, query: &str) -> Result<String, String>;
    /// Einen PR freigeben (`gh pr review <url> --approve`). Attrappen können es nicht.
    fn approve(&self, _url: &str) -> Result<(), String> {
        Err("nicht unterstützt".into())
    }
}

/// Die echte Fassung: `gh api graphql -f query=…`, versteckt, mit Zeitlimit. Nur lesend,
/// darum ohne `guard` (der Probelauf `real_query_runs_against_github` braucht sie).
#[derive(Debug, Clone, Copy)]
pub struct GhCli {
    pub timeout: Duration,
}

impl Default for GhCli {
    fn default() -> GhCli {
        GhCli { timeout: TIMEOUT }
    }
}

impl GhApi for GhCli {
    fn graphql(&self, query: &str) -> Result<String, String> {
        let mut cmd = Command::new("gh");
        cmd.args(["api", "graphql", "-f"]).arg(format!("query={query}"));
        cmd.env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
        let out = hidden_cmd::run(cmd, self.timeout)?;
        // Bei GraphQL-Fehlern endet gh mit 1, druckt die Antwort aber trotzdem; ob sie
        // brauchbar ist, entscheidet der Parser.
        if out.stdout.trim_start().starts_with('{') {
            return Ok(out.stdout);
        }
        let why = hidden_cmd::short_error(&out.stderr);
        Err(if why.is_empty() { format!("gh: Exit {}", out.code) } else { format!("gh: {why}") })
    }

    fn approve(&self, url: &str) -> Result<(), String> {
        // Wirkung nach außen: in Tests nie.
        if cfg!(test) || deck_win::effects_forbidden() {
            return Err("in Tests gesperrt".into());
        }
        let mut cmd = Command::new("gh");
        cmd.args(["pr", "review", url, "--approve"]);
        cmd.env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
        let out = hidden_cmd::run(cmd, self.timeout)?;
        if out.code == 0 {
            return Ok(());
        }
        let why = hidden_cmd::short_error(&out.stderr);
        Err(if why.is_empty() { format!("gh: Exit {}", out.code) } else { format!("gh: {why}") })
    }
}

/// Die Abfrage für den lokalen Tag `today` (Abstand zu UTC `offset_s`).
pub fn build_query(today: NaiveDate, offset_s: i32) -> String {
    let sign = if offset_s < 0 { '-' } else { '+' };
    let off = offset_s.unsigned_abs();
    let since = format!("{today}T00:00:00{sign}{:02}:{:02}", off / 3600, (off % 3600) / 60);
    let day = |n: u64| today.checked_sub_days(Days::new(n)).unwrap_or(today);
    let pr_fields = "number title url repository{nameWithOwner} isDraft createdAt mergeable reviewDecision headRefName \
                     commits(last:1){nodes{commit{statusCheckRollup{state}}}} reviewThreads(first:100){nodes{isResolved}} \
                     reviewRequests(first:10){nodes{requestedReviewer{... on User{login} ... on Team{name}}}} \
                     timelineItems(last:60,itemTypes:[PULL_REQUEST_REVIEW,PULL_REQUEST_COMMIT]){nodes{... on PullRequestReview{state submittedAt} ... on PullRequestCommit{commit{committedDate}}}}";
    let review_fields = "number title url repository{nameWithOwner} createdAt additions deletions commits(last:1){nodes{commit{statusCheckRollup{state}}}}";
    let mut q = format!(
        "query{{ {ALIAS_OPEN}: search(query:\"is:pr author:@me is:open archived:false\", type:ISSUE, first:50){{ nodes{{ ... on PullRequest{{ {pr_fields} }} }} }} \
         {ALIAS_REVIEWS}: search(query:\"is:pr is:open review-requested:@me archived:false\", type:ISSUE, first:30){{ nodes{{ ... on PullRequest{{ {review_fields} }} }} }} \
         {ALIAS_CREATED_TODAY}: search(query:\"is:pr author:@me created:>={since}\", type:ISSUE, first:0){{ issueCount }} \
         {ALIAS_MERGED_TODAY}: search(query:\"is:pr author:@me is:merged merged:>={since}\", type:ISSUE, first:0){{ issueCount }} "
    );
    for i in 0..WINDOWS {
        let newest = i * WINDOW_DAYS;
        let oldest = newest + WINDOW_DAYS - 1;
        // Das jüngste Fenster ist nach vorn offen (Zeitzonen-Rand von GitHub).
        let range = if i == 0 { format!(">={}", day(oldest)) } else { format!("{}..{}", day(oldest), day(newest)) };
        q.push_str(&format!(
            "{ALIAS_MERGED_PREFIX}{i}: search(query:\"is:pr author:@me is:merged merged:{range}\", type:ISSUE, first:100){{ issueCount \
             nodes{{ ... on PullRequest{{ repository{{nameWithOwner}} headRefName headRefOid mergedAt }} }} }} "
        ));
    }
    q.push('}');
    q
}

/// Fenster mit mehr Treffern als geliefert (dort fehlen gemergte PRs fürs Aufräumen).
fn truncated_windows(v: &Value) -> u32 {
    let Some(data) = v.get("data").and_then(Value::as_object) else { return 0 };
    data.iter()
        .filter(|(k, _)| k.starts_with(ALIAS_MERGED_PREFIX) && *k != ALIAS_MERGED_TODAY)
        .filter(|(_, w)| {
            let total = w.get("issueCount").and_then(Value::as_u64).unwrap_or(0);
            let got = w.get("nodes").and_then(Value::as_array).map_or(0, Vec::len) as u64;
            total > got
        })
        .count() as u32
}

#[derive(Default)]
struct Control {
    generation: u64,
    force: bool,
}

/// Für das Protokoll: nur Wechsel schreiben, nicht jeden Abruf.
#[derive(Default)]
struct Health {
    ever_ok: bool,
    fails: u32,
    truncated: u32,
}

struct State {
    api: Arc<dyn GhApi>,
    log: LogFile,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    today: Arc<dyn Fn() -> (NaiveDate, i32) + Send + Sync>,
    snap: Mutex<PrSnapshot>,
    health: Mutex<Health>,
    ctl: Mutex<Control>,
    cv: Condvar,
    wake: Mutex<Option<Wake>>,
}

/// Der Hintergrund-Dienst.
pub struct GithubService {
    state: Arc<State>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn local_today() -> (NaiveDate, i32) {
    let now = chrono::Local::now();
    (now.date_naive(), now.offset().local_minus_utc())
}

impl GithubService {
    /// Mit eigener Anbindung, Uhr und Tag (Tests).
    pub fn new(
        api: Arc<dyn GhApi>,
        log: LogFile,
        now: Arc<dyn Fn() -> f64 + Send + Sync>,
        today: Arc<dyn Fn() -> (NaiveDate, i32) + Send + Sync>,
    ) -> GithubService {
        GithubService {
            state: Arc::new(State {
                api,
                log,
                now,
                today,
                snap: Mutex::new(PrSnapshot::default()),
                health: Mutex::new(Health::default()),
                ctl: Mutex::new(Control::default()),
                cv: Condvar::new(),
                wake: Mutex::new(None),
            }),
            thread: Mutex::new(None),
        }
    }

    /// Die echte Fassung: gh, Ortszeit, Protokoll `state\dash-github.log`.
    pub fn system(state_dir: &Path) -> GithubService {
        let log = LogFile::new(state_dir.join(LOG_FILE), 200_000, Rotate::Rename);
        GithubService::new(Arc::new(GhCli::default()), log, Arc::new(now_unix), Arc::new(local_today))
    }

    /// Thread starten (erster Abruf sofort); ein zweiter Aufruf ersetzt nur den Rückruf.
    pub fn start(&self, wake: Wake) {
        *lock(&self.state.wake) = Some(wake);
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let generation = lock(&self.state.ctl).generation;
        let st = self.state.clone();
        *t = std::thread::Builder::new().name("dash-github".into()).spawn(move || run(&st, generation)).ok();
    }

    /// Anhalten (ein laufender Abruf endet nach höchstens [`TIMEOUT`]).
    pub fn stop(&self) {
        lock(&self.state.ctl).generation += 1;
        self.state.cv.notify_all();
        lock(&self.thread).take();
    }

    /// Sofort neu abrufen, im Hintergrund.
    pub fn refresh(&self) {
        lock(&self.state.ctl).force = true;
        self.state.cv.notify_all();
    }

    pub fn snapshot(&self) -> PrSnapshot {
        lock(&self.state.snap).clone()
    }

    /// Einen PR freigeben, für den ein Review von mir erbeten ist; danach neu abrufen.
    /// Nur Adressen aus der letzten Liste der Review-Anfragen (nie eine beliebige).
    pub fn approve(&self, url: &str) -> Result<(), String> {
        let known = lock(&self.state.snap).review_requests.iter().any(|r| r.url == url);
        if !known {
            return Err("kein offener Review-Auftrag mit dieser Adresse".into());
        }
        let res = self.state.api.approve(url);
        match &res {
            Ok(()) => self.state.log.note(&format!("Approve: {}", url.rsplit_once("/pull/").map_or(url, |(_, n)| n))),
            Err(e) => self.state.log.note(&format!("Approve gescheitert: {e}")),
        }
        self.refresh();
        res
    }

    /// Ein Abruf, synchron (der Thread macht genau das im Takt). `true` = geklappt.
    pub fn fetch_once(&self) -> bool {
        fetch(&self.state)
    }
}

impl Drop for GithubService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn fetch(st: &State) -> bool {
    let (today, offset) = (st.today)();
    let started = Instant::now();
    let result = st.api.graphql(&build_query(today, offset)).and_then(|text| {
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("Antwort kein JSON (Zeile {})", e.line()))?;
        prs::parse(&v).map(|s| (s, truncated_windows(&v))).map_err(|e| hidden_cmd::short_error(&e))
    });
    let ms = started.elapsed().as_millis();
    let mut health = lock(&st.health);
    let mut snap = lock(&st.snap);
    match result {
        Ok((mut fresh, truncated)) => {
            fresh.fetched_at = (st.now)();
            let summary = format!(
                "{} offen, heute {} erstellt / {} gemergt, {} gemergt in 30 Tg., {ms} ms",
                fresh.open.len(),
                fresh.created_today.unwrap_or(0),
                fresh.merged_today.unwrap_or(0),
                fresh.merged.len()
            );
            if health.fails > 0 {
                st.log.note(&format!("Abruf klappt wieder nach {} Fehlversuchen: {summary}", health.fails));
            } else if !health.ever_ok {
                st.log.note(&format!("erster Abruf: {summary}"));
            }
            if truncated != health.truncated {
                st.log.note(&format!("gemergte PRs: {truncated} von {WINDOWS} Zeitfenstern abgeschnitten (je max. 100)"));
            }
            *health = Health { ever_ok: true, fails: 0, truncated };
            *snap = fresh;
            true
        }
        Err(e) => {
            if health.fails == 0 {
                st.log.note(&format!("Abruf gescheitert: {e} ({ms} ms)"));
            }
            health.fails += 1;
            // Alte Daten bleiben, die Karte zeigt sie matt mit dem Fehler.
            snap.error = Some(e);
            false
        }
    }
}

fn run(st: &State, generation: u64) {
    loop {
        // Die Schleife darf nie sterben: ein Panik im Abruf zählt als Fehlschlag.
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetch(st))).unwrap_or(false);
        if let Some(cb) = lock(&st.wake).clone() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb()));
        }
        let delay = if ok { POLL } else { RETRY };
        let mut ctl = lock(&st.ctl);
        if ctl.generation == generation && !ctl.force {
            ctl = st.cv.wait_timeout(ctl, delay).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        if ctl.generation != generation {
            return;
        }
        ctl.force = false;
    }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
