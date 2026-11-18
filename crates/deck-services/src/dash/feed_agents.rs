//! Hintergrund-Agenten im Feed-Dienst: fällige Aufträge starten (`bg_tasks::due_for`,
//! `schedule`), laufende überwachen (fertig, Laufzeitgrenze), Ergebnis als Bericht
//! übernehmen, Antwort-Entwürfe einlesen. Auch der Knopf „Claude soll umsetzen“ startet
//! über [`start_run`].
//!
//! Gestartet wird nie, während eine Agenten-Session des Decks im selben worktree
//! arbeitet, und nie mehr als `MAX_RUNNING` zugleich, einer je PR.

use std::path::{Path, PathBuf};

use deck_core::dash::bg_tasks::{self, report_from_stream, Due, Hold, TaskKind, MAX_CI_TRIES, MAX_RUNNING, TIME_LIMIT_S};
use deck_core::dash::feed_state::{Did, Draft, Run, RunStatus};
use deck_core::dash::pr_extra::Comment;
use deck_core::dash::prs::{PrInfo, PrSnapshot};

use super::agents::ensure_worktree;
use super::feed_round::activity;
use super::feed_service::{lock, Inner, Shared};
use crate::fsutil::random_u64;

fn norm(p: &str) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// Arbeitet eine Session des Decks im worktree dieses PRs (bekannter Ordner oder
/// `wt\<nr>-<kürzel>` nach Konvention)?
fn busy(i: &Inner, pr: &PrInfo, dir: Option<&Path>) -> bool {
    let folder = format!("\\wt\\{}", bg_tasks::worktree_folder(pr).to_lowercase());
    let known: Vec<String> = dir.into_iter().chain(i.pr_dirs.get(&pr.key()).map(PathBuf::as_path)).map(|d| norm(&d.to_string_lossy())).collect();
    i.in_use.iter().map(|u| norm(u)).any(|u| {
        u.ends_with(&folder) || u.contains(&format!("{folder}\\")) || known.iter().any(|k| u == *k || u.starts_with(&format!("{k}\\")))
    })
}

fn safe(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Fällige automatische Aufträge starten (nur bei eingeschalteter Automatik).
pub(crate) fn start_due(sh: &Shared, snap: &PrSnapshot, now: f64) {
    let exhausted: Vec<Due>;
    let start = {
        let mut i = lock(&sh.inner);
        if !i.st.automation || snap.fetched_at <= 0.0 {
            i.waiting.clear();
            return;
        }
        let due: Vec<Due> = snap.open.iter().filter_map(|p| bg_tasks::due_for(p, &i.st, i.causes.get(&p.key()).copied().flatten())).collect();
        exhausted = due.iter().filter(|d| d.exhausted).cloned().collect();
        // Erledigt sich ein Grund (PR zu, nichts mehr fällig), darf derselbe Grund später
        // wieder einmal ins Log.
        i.not_started.retain(|k, _| due.iter().any(|d| d.pr == *k));
        let (start, wait) = bg_tasks::schedule(due, &i.st, |key| snap.open.iter().find(|p| p.key() == key).is_some_and(|p| busy(&i, p, None)));
        i.waiting = wait;
        start
    };
    give_up(sh, snap, &exhausted, now);
    for d in start {
        let Some(pr) = snap.open.iter().find(|p| p.key() == d.pr) else { continue };
        match start_run(sh, snap, pr, d.task, None, Some(&d), now) {
            Ok(_) => {
                lock(&sh.inner).not_started.remove(&d.pr);
            }
            Err(e) => {
                let changed = lock(&sh.inner).not_started.insert(d.pr.clone(), e.clone()).as_deref() != Some(e.as_str());
                if changed {
                    sh.log.note(&format!("{}: Agent nicht gestartet: {e}", pr.label()));
                }
            }
        }
    }
}

/// Rote CI, für die [`MAX_CI_TRIES`] Agenten verbraucht sind: einmal je Commit melden
/// und den Commit als bearbeitet merken, damit nichts mehr startet.
fn give_up(sh: &Shared, snap: &PrSnapshot, due: &[Due], now: f64) {
    for d in due.iter().filter(|d| d.exhausted) {
        let label = snap.open.iter().find(|p| p.key() == d.pr).map(PrInfo::label).unwrap_or_default();
        let mut i = lock(&sh.inner);
        i.st.handled_mut(&d.pr).ci_oid = d.oid.clone();
        i.st.record(activity(snap, &d.pr, Did::CiGaveUp, "", now));
        i.dirty = true;
        drop(i);
        sh.log.note(&format!("{label}: rote CI nach {MAX_CI_TRIES} Agenten, kein weiterer Versuch bis grün"));
    }
}

/// Einen Hintergrund-Agenten für `pr` starten. `target`: der Kommentar eines Menschen
/// (nur `HumanReply`), `due`: der automatische Auftrag (wird beim Start als bearbeitet
/// markiert). Gibt die Lauf-Id zurück.
pub(crate) fn start_run(sh: &Shared, snap: &PrSnapshot, pr: &PrInfo, task: TaskKind, target: Option<(&Comment, &str)>, due: Option<&Due>, now: f64) -> Result<String, String> {
    let key = pr.key();
    if pr.draft {
        return Err("Draft: geparkt, kein Agent".into());
    }
    let check = |i: &Inner, dir: Option<&Path>| -> Result<(), String> {
        if i.st.running().any(|r| r.pr == key) {
            return Err("für diesen PR läuft schon ein Agent".into());
        }
        if i.st.running().count() >= MAX_RUNNING {
            return Err(format!("schon {MAX_RUNNING} Agenten unterwegs"));
        }
        if busy(i, pr, dir) {
            return Err("eine Session des Decks arbeitet im worktree".into());
        }
        Ok(())
    };
    let repos_root = {
        let i = lock(&sh.inner);
        check(&i, None)?;
        i.cfg.repos_root.clone()
    };
    let dir = match ensure_worktree(sh.deps.git.as_ref(), &repos_root, &pr.repo, &pr.head_ref, &bg_tasks::worktree_folder(pr)) {
        Ok(d) => d,
        Err(e) => {
            // Ein automatischer Auftrag gilt trotzdem als bearbeitet: sonst versuchte das
            // Deck es alle 15 s erneut. Im Feed steht, warum nichts lief.
            if let Some(d) = due {
                let mut i = lock(&sh.inner);
                mark(&mut i, &key, d);
                i.st.record(activity(snap, &key, Did::AgentFailed(task, e.clone()), "", now));
                i.dirty = true;
            }
            return Err(e);
        }
    };
    let id = format!("{:016x}", random_u64())[..10].to_string();
    let draft_path = if task == TaskKind::HumanReply { sh.drafts_dir().join(format!("{}.md", safe(&key))) } else { PathBuf::new() };
    let _ = std::fs::create_dir_all(sh.drafts_dir());
    if task == TaskKind::HumanReply {
        let _ = std::fs::remove_file(&draft_path);
    }
    let prompt = bg_tasks::prompt(task, pr, target.map(|t| t.0), &draft_path.to_string_lossy().replace('\\', "/"));
    {
        let mut i = lock(&sh.inner);
        if let Err(e) = check(&i, Some(&dir)) {
            // Den worktree merken: die nächste Runde erkennt die Session dann schon in
            // `schedule` und versucht nicht alle 15 s denselben Start.
            i.pr_dirs.insert(key.clone(), dir.clone());
            if due.is_some() {
                i.waiting.retain(|(w, _)| w.pr != key);
                i.waiting.push((due.cloned().expect("due"), Hold::SessionActive));
            }
            return Err(e);
        }
        i.pr_dirs.insert(key.clone(), dir.clone());
        if let Some(d) = due {
            mark(&mut i, &key, d);
        }
        i.st.runs.push(Run {
            id: id.clone(),
            pr: key.clone(),
            label: pr.label(),
            url: pr.url.clone(),
            title: pr.title.clone(),
            task,
            started: now,
            ended: 0.0,
            status: RunStatus::Running,
            report: String::new(),
            dir: dir.to_string_lossy().to_string(),
            comment_id: target.map(|t| t.0.id.clone()).unwrap_or_default(),
            thread_id: target.map(|t| t.1.to_string()).unwrap_or_default(),
            draft_path: draft_path.to_string_lossy().to_string(),
        });
        i.st.record(activity(snap, &key, Did::AgentStarted(task), &id, now));
        i.dirty = true;
    }
    let out = sh.runs_dir().join(format!("{id}.jsonl"));
    match sh.deps.runner.start(&dir, &prompt, &out, &sh.drafts_dir()) {
        Ok(proc) => {
            sh.log.note(&format!("Agent {id} gestartet: {} ({task:?})", pr.label()));
            lock(&sh.inner).procs.insert(id.clone(), proc);
            sh.save();
            sh.notify();
            Ok(id)
        }
        Err(e) => {
            finish(sh, snap, &id, RunStatus::Failed, e.clone(), now);
            Err(e)
        }
    }
}

/// Einen automatischen Auftrag als bearbeitet merken.
fn mark(i: &mut Inner, key: &str, d: &Due) {
    let h = i.st.handled_mut(key);
    match d.task {
        TaskKind::BotFindings => h.bot = d.mark,
        TaskKind::CiRed => {
            h.ci_oid = d.oid.clone();
            h.ci_tries += 1;
        }
        TaskKind::Conflict => h.conflict = d.mark,
        TaskKind::HumanReply => {}
    }
}

/// Einen Lauf abschließen und in die Zeitleiste schreiben.
pub(crate) fn finish(sh: &Shared, snap: &PrSnapshot, id: &str, status: RunStatus, report: String, now: f64) {
    let mut i = lock(&sh.inner);
    i.procs.remove(id);
    let Some(run) = i.st.runs.iter_mut().find(|r| r.id == id && r.status == RunStatus::Running) else { return };
    run.status = status;
    run.ended = now;
    run.report = report.clone();
    let (key, task, started, draft_path, comment_id, thread_id) =
        (run.pr.clone(), run.task, run.started, run.draft_path.clone(), run.comment_id.clone(), run.thread_id.clone());
    let did = match status {
        RunStatus::Done => Did::AgentDone(task),
        RunStatus::Aborted => Did::AgentAborted(task),
        RunStatus::TimedOut => Did::AgentTimedOut(task),
        _ => Did::AgentFailed(task, report.lines().next().unwrap_or("").chars().take(120).collect()),
    };
    sh.log.note(&format!("Agent {id} ({task:?}) {status:?} nach {:.0} s", now - started));
    i.st.record(activity(snap, &key, did, id, now));
    if task == TaskKind::HumanReply && status == RunStatus::Done {
        let text = std::fs::read_to_string(&draft_path).unwrap_or_default();
        if !text.trim().is_empty() {
            i.st.drafts.insert(key.clone(), Draft { comment_id, thread_id, path: draft_path, text, at: now, ..Draft::default() });
            i.st.record(activity(snap, &key, Did::DraftReady, id, now));
        }
    }
    i.dirty = true;
}

/// Laufende Agenten: fertig geworden? Laufzeit überschritten?
pub(crate) fn watch(sh: &Shared, snap: &PrSnapshot, now: f64) {
    let mut ended: Vec<(String, Option<i32>)> = Vec::new();
    {
        let mut i = lock(&sh.inner);
        let running: Vec<(String, f64)> = i.st.running().map(|r| (r.id.clone(), r.started)).collect();
        for (id, started) in running {
            let Some(proc) = i.procs.get_mut(&id) else { continue };
            if let Some(code) = proc.poll() {
                ended.push((id, Some(code)));
            } else if now - started > TIME_LIMIT_S {
                proc.kill();
                ended.push((id, None));
            }
        }
    }
    for (id, code) in ended {
        let out = sh.runs_dir().join(format!("{id}.jsonl"));
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let (status, report) = match (code, report_from_stream(&text)) {
            (None, _) => (RunStatus::TimedOut, "Laufzeitgrenze erreicht, beendet".to_string()),
            (Some(0), Some((false, r))) => (RunStatus::Done, r),
            (Some(c), Some((_, r))) => (RunStatus::Failed, if r.is_empty() { format!("Exit {c}") } else { r }),
            (Some(c), None) => {
                let err = std::fs::read_to_string(out.with_extension("err")).unwrap_or_default();
                let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect::<String>();
                (RunStatus::Failed, format!("Exit {c}, kein Ergebnis. {first}").trim().to_string())
            }
        };
        finish(sh, snap, &id, status, report, now);
    }
}

/// Entwürfe aus ihren Dateien nachlesen („Ändern“ bearbeitet die Datei im Editor).
pub(crate) fn refresh_drafts(sh: &Shared) {
    let paths: Vec<(String, String)> = lock(&sh.inner).st.drafts.iter().map(|(k, d)| (k.clone(), d.path.clone())).collect();
    for (key, path) in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut i = lock(&sh.inner);
        if let Some(d) = i.st.drafts.get_mut(&key)
            && d.text != text
        {
            d.text = text;
            i.dirty = true;
        }
    }
}
