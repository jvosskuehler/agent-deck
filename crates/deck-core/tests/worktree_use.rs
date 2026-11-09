//! Der worktree, in dem ein Agent arbeitet, aus den Eingaben seiner Werkzeugaufrufe.
//! Die Zeilen sind gebaut wie echte Claude-Code-Transcripts (JSONL): eine
//! `assistant`-Zeile mit `tool_use`-Blöcken, dazu `cwd` der Session.

use deck_core::claude::refs::Refs;
use deck_core::claude::summary_cache::{cached_refs, ensure_refs};
use deck_core::claude::summarize::enc_cwd;
use deck_core::claude::worktree_use::{active_worktree, worktree_uses};
use deck_core::i18n::Lang;
use deck_core::ref_sources::{RefEvidence, pick_ticket};
use deck_core::view::RefSource;
use serde_json::{Value, json};

const REPOS: &str = r"C:\Users\JorritVoßkühler\Repos";

/// Eine Antwortzeile mit einem Werkzeugaufruf, wie Claude Code sie schreibt.
fn call(cwd: &str, name: &str, input: Value) -> Value {
    json!({
        "type": "assistant",
        "cwd": cwd,
        "sessionId": "s1",
        "isSidechain": false,
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Ich schaue nach."},
                {"type": "tool_use", "id": "toolu_01", "name": name, "input": input}
            ]
        }
    })
}

fn bash(cmd: &str) -> Value {
    call(REPOS, "Bash", json!({"command": cmd, "description": "x"}))
}

fn edit(path: &str) -> Value {
    call(REPOS, "Edit", json!({"file_path": path, "old_string": "a", "new_string": "b"}))
}

fn read(path: &str) -> Value {
    call(REPOS, "Read", json!({"file_path": path}))
}

/// Ein unbeteiligter Aufruf (lässt die Zeit vergehen).
fn other() -> Value {
    call(REPOS, "TodoWrite", json!({"todos": []}))
}

fn found(records: &[Value]) -> Option<(String, u32, u32)> {
    active_worktree(records).map(|w| (w.dir, w.writes, w.reads))
}

#[test]
fn an_edit_with_an_absolute_path_names_the_worktree_and_counts_as_write() {
    let r = [edit(&format!(r"{REPOS}\wt\4116-ui\apps\management-ui\src\page.tsx"))];
    assert_eq!(found(&r), Some((format!(r"{REPOS}\wt\4116-ui"), 1, 0)));
}

#[test]
fn git_dash_c_with_a_relative_path_resolves_against_the_session_cwd() {
    let r = [bash("git -C wt/4062-api status"), bash("git -C wt/4062-api diff --stat"), bash("git -C wt/4062-api log -3")];
    assert_eq!(found(&r), Some((format!(r"{REPOS}\wt\4062-api"), 0, 3)));
    let commit = [bash("git -C wt/4062-api add -A && git -C wt/4062-api commit -m \"Fix trial\"")];
    assert_eq!(found(&commit).map(|f| f.1), Some(1), "Commit zählt als Schreiben");
}

#[test]
fn a_cd_carries_to_the_following_parts_of_the_command() {
    let r = [bash("cd wt/3773-api && git commit -am \"Rechte\" && git push")];
    assert_eq!(found(&r), Some((format!(r"{REPOS}\wt\3773-api"), 1, 0)));
    let ps = [call(REPOS, "PowerShell", json!({"command": "Set-Location wt\\3773-api; git push"}))];
    assert_eq!(found(&ps).map(|f| f.1), Some(1));
}

#[test]
fn a_command_without_path_counts_for_the_worktree_the_session_is_in() {
    let cwd = format!(r"{REPOS}\wt\4105-docs\src");
    let r = [call(&cwd, "Bash", json!({"command": "git commit -m \"Anleitung\""}))];
    assert_eq!(found(&r), Some((format!(r"{REPOS}\wt\4105-docs"), 1, 0)));
}

#[test]
fn git_bash_home_and_legacy_paths_are_understood() {
    let r = [bash("ls /c/Users/JorritVoßkühler/Repos/wt/4009-client/src"), bash("cat ~/Repos/wt/4009-client/package.json"),
        read(&format!(r"{REPOS}\wt\4009-client\README.md"))];
    assert_eq!(found(&r), Some((format!(r"{REPOS}\wt\4009-client"), 0, 3)));
    let legacy = [edit(&format!(r"{REPOS}\wt-deck-tauri\app\src\main.tsx"))];
    assert_eq!(found(&legacy).map(|f| f.0), Some(format!(r"{REPOS}\wt-deck-tauri")));
}

#[test]
fn a_few_reads_are_a_glance_and_no_worktree() {
    let r = [bash("ls wt/4116-ui"), other(), read(&format!(r"{REPOS}\wt\4116-ui\CLAUDE.md"))];
    assert_eq!(found(&r), None);
    assert_eq!(worktree_uses(&r).len(), 1, "gezählt wird er trotzdem");
}

#[test]
fn writes_beat_more_reads_elsewhere() {
    let mut r = vec![edit(&format!(r"{REPOS}\wt\4116-ui\src\a.ts"))];
    for _ in 0..5 {
        r.push(bash("git -C wt/4062-api status"));
    }
    assert_eq!(found(&r).map(|f| f.0), Some(format!(r"{REPOS}\wt\4116-ui")));
}

#[test]
fn recent_work_wins_over_old_work() {
    let mut r: Vec<Value> = (0..6).map(|_| edit(&format!(r"{REPOS}\wt\3192-vm\setup.ps1"))).collect();
    r.extend((0..200).map(|_| other()));
    r.push(edit(&format!(r"{REPOS}\wt\4116-ui\src\a.ts")));
    r.push(bash("cd wt/4116-ui && git push"));
    assert_eq!(found(&r).map(|f| f.0), Some(format!(r"{REPOS}\wt\4116-ui")));
}

#[test]
fn overview_and_cleanup_commands_do_not_count() {
    let round = "for d in x; do :; done; git -C wt/4062-api status; git -C wt/4116-ui status; git -C wt/3773-api status; git -C wt/4105-docs status";
    let r = vec![bash(round), bash(round), bash(round), bash(round)];
    assert_eq!(worktree_uses(&r), vec![], "Statusrunde über alle worktrees");
    let rm = [bash("git -C orpheus-management-ui worktree remove ../wt/4116-ui"), bash("rm -rf wt/4116-ui")];
    assert_eq!(worktree_uses(&rm), vec![]);
}

#[test]
fn tool_results_text_and_lookalikes_do_not_count() {
    let result = json!({"type": "user", "cwd": REPOS, "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "toolu_01", "content": "C:\\Users\\x\\Repos\\wt\\4116-ui\nwt/4062-api"}]}});
    let text = json!({"type": "assistant", "cwd": REPOS, "message": {"content": [{"type": "text", "text": "Arbeite in wt/4116-ui"}]}});
    let look = [bash("wt -w deck-A new-tab"), bash("cat newt/4116-x"), bash("ls wt/*"), read(r"C:\Repos\wt\README.md"),
        bash("node --wt-4116 x"), bash("echo WT-Fenster WT-4116"), call(REPOS, "Agent", json!({"prompt": "Arbeite in wt/4116-ui"}))];
    let mut all = vec![result, text];
    all.extend(look);
    assert_eq!(worktree_uses(&all), vec![]);
}

#[test]
fn the_worktree_source_ranks_after_the_cwd_folder_and_before_the_title() {
    let chat = Refs { ticket: "ORPH-1212".into(), worktree: format!(r"{REPOS}\wt\4116-ui"), worktree_writes: true, ..Refs::default() };
    let ev = RefEvidence { title: Some("ORPH-3192 Test-VM"), chat: Some(&chat), prefix: "ORPH", ..RefEvidence::default() };
    let t = pick_ticket(&ev, Lang::German).unwrap();
    assert_eq!((t.text.as_str(), t.source, t.detected, t.origin.as_str()), ("ORPH-4116", RefSource::Worktree, false, "Worktree"));
    let dirs = vec![format!(r"{REPOS}\wt\4062-api")];
    assert_eq!(pick_ticket(&RefEvidence { dirs: &dirs, ..ev }, Lang::German).unwrap().source, RefSource::Folder);
    let reads = Refs { worktree_writes: false, ..chat.clone() };
    assert!(pick_ticket(&RefEvidence { chat: Some(&reads), ..ev }, Lang::German).unwrap().detected, "nur gelesen: gedimmt");
}

#[test]
fn a_worktree_without_number_takes_the_ticket_from_its_branch() {
    let chat = Refs { worktree: format!(r"{REPOS}\wt-deck-tauri"), worktree_writes: true, ..Refs::default() };
    let ev = RefEvidence { chat: Some(&chat), worktree_branch: Some("feature/ORPH-4116-tauri"), prefix: "ORPH", ..RefEvidence::default() };
    assert_eq!(pick_ticket(&ev, Lang::German).map(|t| (t.text, t.source)), Some(("ORPH-4116".into(), RefSource::Worktree)));
    let ev = RefEvidence { worktree_branch: Some("main"), ..ev };
    assert_eq!(pick_ticket(&ev, Lang::German), None);
}

#[test]
fn the_scan_caches_the_worktree_next_to_the_chat_refs() {
    let projects = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let dir = projects.path().join(enc_cwd(REPOS));
    std::fs::create_dir_all(&dir).unwrap();
    let user = json!({"type": "user", "cwd": REPOS, "message": {"role": "user", "content": "Mach weiter"}});
    let lines = [user, edit(&format!(r"{REPOS}\wt\4116-ui\src\a.ts"))];
    let body: String = lines.iter().map(|l| l.to_string() + "\n").collect();
    std::fs::write(dir.join("s1.jsonl"), body).unwrap();
    let r = ensure_refs(projects.path(), cache.path(), "s1", Some(REPOS), Some("ORPH"));
    assert_eq!((r.worktree.as_str(), r.worktree_writes), (format!(r"{REPOS}\wt\4116-ui").as_str(), true));
    assert_eq!(cached_refs(cache.path(), "s1", Some("ORPH")), r);
}
