//! Rote CI, deren Ursache bekannt ist: im Feed „erklärt“ statt Problem, und für die
//! Automatik die Frage, ob sie trotzdem als grün zählt.
//!
//! Zwei bekannte Ursachen (gemessen 17.11.2026):
//!
//! - **Code Security aus:** CodeQL, Lizenzprüfung und Image-Scan scheitern mit „Code
//!   Security must be enabled for this repository“ bzw. die Abhängigkeitsprüfung mit
//!   „Dependency graph“ und HTTP 403. Das steht nur im Job-Log; das Log holt der Dienst
//!   per `gh` (`deck-services/src/dash/feed_service.rs`, Trait `GhApi::job_log`), hier
//!   wird nur sein Text geprüft ([`log_explains`]). Ein Sammel-Job, der so heißt wie sein
//!   Workflow („CI“ in „CI“), scheitert dann mit; er zählt mit, wenn ein anderer Job
//!   desselben Workflows erklärt ist. Zählt für die Automatik als grün.
//! - **Review-Gate:** der einzige rote Check ist `claude / Review` auf einem PR, der die
//!   Claude-Workflows oder das Review-Gate selbst ändert; claude-code-action prüft solche
//!   PRs bewusst nicht. Braucht eine menschliche Freigabe, zählt also NICHT als grün.

use std::collections::BTreeMap;

use super::pr_extra::Check;

/// Warum die CI eines PRs rot ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiCause {
    /// Unbekannt: ein echtes Problem.
    Unknown,
    /// Nur Code Security (CodeQL, Lizenzen, Upload) aus.
    CodeSecurity,
    /// Nur `claude / Review` auf einem PR, der das Review-Gate ändert.
    ReviewGate,
}

impl CiCause {
    /// Zählt die rote CI für die Automatik als grün?
    pub fn counts_as_green(self) -> bool {
        self == CiCause::CodeSecurity
    }
}

/// Lohnt das Log dieses Checks? Nur die Jobs, die an Code Security hängen; alle anderen
/// Logs werden nie geholt (Kosten, und ein Testlauf-Log beweist nichts).
pub fn log_needed(c: &Check) -> bool {
    let n = format!("{} {}", c.name, c.workflow).to_lowercase();
    c.failed && c.job_id > 0 && ["codeql", "licen", "upload", "scan", "dependency", "sarif", "trivy", "image"].iter().any(|k| n.contains(k))
}

/// Stand der Regel für Log-Urteile; wer sie ändert, zählt hoch, dann werden gemerkte
/// Urteile verworfen und neu geholt (`FeedState::verdict_rule`).
pub const VERDICT_RULE: u32 = 3;

/// Erklärt dieses Log den Fehlschlag mit abgeschalteter Code Security?
///
/// C5: Ein Image-Scan (Trivy), der nur beim Hochladen des Berichts scheitert, ist nur
/// erklärt, wenn der Prüfschritt selbst sauber ist: der Schritt „… verdict“ (Trivy mit
/// `exit-code: 1` ab der Schwelle) endet mit `outcome=success`. Ohne diesen Nachweis
/// bleibt es ein Problem.
pub fn log_explains(log: &str) -> bool {
    let lower = log.to_lowercase();
    let security_off = lower.contains("code security must be enabled") || (lower.contains("dependency graph") && lower.contains("403"));
    security_off && (!is_scan(&lower) || scan_verdict_clean(log))
}

fn is_scan(lower: &str) -> bool {
    ["trivy", "grype", "image-scan"].iter().any(|k| lower.contains(k))
}

/// Ist jeder Prüfschritt („… verdict“) sauber? Gelaufen heißt `outcome=success`. Ein
/// übersprungener zählt nur, wenn auch der Schritt direkt davor (sein Bericht) übersprungen
/// wurde: dann ist der ganze Block abgeschaltet (z. B. Lizenzprüfung aus). Mindestens ein
/// Prüfschritt muss gelaufen und sauber sein.
fn scan_verdict_clean(log: &str) -> bool {
    let outcome_of = |id: &str| {
        let end = format!("##[end-action id={id};outcome=");
        log.lines().find_map(|l| l.split(&end).nth(1).map(|o| o.split(';').next().unwrap_or("").to_string()))
    };
    let mut prev = String::new();
    let mut ran_clean = false;
    for line in log.lines() {
        if let Some(rest) = line.split("##[end-action id=").nth(1)
            && let Some(o) = rest.split(";outcome=").nth(1)
        {
            prev = o.split(';').next().unwrap_or("").to_string();
        }
        let Some(rest) = line.split("##[start-action display=").nth(1) else { continue };
        let Some((display, id)) = rest.split_once(";id=") else { continue };
        if !display.to_lowercase().contains("verdict") {
            continue;
        }
        match outcome_of(id.trim_end_matches(']').trim()).as_deref() {
            Some("success") => ran_clean = true,
            Some("skipped") if prev == "skipped" => {}
            _ => return false,
        }
    }
    ran_clean
}

/// Ein Sammeljob, der nur scheitert, weil ein anderer Job rot ist („CI“, „Jenkins“,
/// „Alle Jobs grün“ oder so benannt wie sein Workflow). Sein Log beweist nichts.
fn is_summary(c: &Check) -> bool {
    let n = c.name.trim().to_lowercase();
    (!c.workflow.is_empty() && n == c.workflow.trim().to_lowercase())
        || ["ci", "jenkins", "checks", "gate", "summary", "required"].contains(&n.as_str())
        || n.contains("alle jobs")
        || n.contains("all jobs")
        || n == "all green"
}

/// Ändert der PR die Claude-Workflows oder das Review-Gate?
pub fn touches_review_gate(files: &[String]) -> bool {
    files.iter().any(|f| {
        let f = f.to_lowercase();
        f.starts_with(".github/workflows/claude") || f.starts_with(".github/actions/claude") || f.contains("review-gate")
    })
}

fn is_claude_review(c: &Check) -> bool {
    let n = c.name.to_lowercase();
    n == "claude / review" || (n.ends_with("review") && c.workflow.to_lowercase().contains("claude"))
}

/// Die Ursache der roten CI; `None`, wenn kein Check rot ist. `verdicts`: Job-Id → das
/// Log erklärt den Fehlschlag (fehlt ein Eintrag, ist das Log noch nicht geholt, und die
/// Ursache bleibt unbekannt, bis es da ist).
pub fn cause(checks: &[Check], files: &[String], verdicts: &BTreeMap<u64, bool>) -> Option<CiCause> {
    // `merge-order` ist keine CI, sondern die Reihenfolge (`merge_order`): eigene Regel.
    let failed: Vec<&Check> = checks.iter().filter(|c| c.failed && c.name != super::merge_order::MERGE_ORDER).collect();
    if failed.is_empty() {
        return None;
    }
    if failed.len() == 1 && is_claude_review(failed[0]) && touches_review_gate(files) {
        return Some(CiCause::ReviewGate);
    }
    let explained = |c: &Check| log_needed(c) && verdicts.get(&c.job_id) == Some(&true);
    // Sammeljobs zählen nur mit, wenn ihre roten Vorgänger alle erklärt sind (und es
    // mindestens einen gibt): dann liegt auch ihr Rot an Code Security.
    let any_explained = failed.iter().any(|c| explained(c));
    let all = failed.iter().all(|c| explained(c) || is_summary(c));
    Some(if any_explained && all { CiCause::CodeSecurity } else { CiCause::Unknown })
}

/// Ist die CI durchgelaufen und nichts rot? Ohne Checks: nein (nichts gesehen).
/// `merge-order` zählt nicht mit, es ist keine CI.
pub fn all_green(checks: &[Check]) -> bool {
    let ci: Vec<&Check> = checks.iter().filter(|c| c.name != super::merge_order::MERGE_ORDER).collect();
    !ci.is_empty() && ci.iter().all(|c| !c.running && !c.failed)
}

/// Wann die CI zuletzt fertig wurde (für die Zeitleiste); 0 = unbekannt.
pub fn finished_at(checks: &[Check]) -> f64 {
    if checks.iter().any(|c| c.running) {
        return 0.0;
    }
    checks.iter().map(|c| c.done_at).fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chk(name: &str, workflow: &str, id: u64, failed: bool) -> Check {
        Check { name: name.into(), workflow: workflow.into(), job_id: id, failed, done_at: 100.0, ..Check::default() }
    }

    #[test]
    fn code_security_off_explains_codeql_scan_and_the_summary_job_of_the_same_workflow() {
        let checks = [
            chk("codeql / Analyze (actions)", "CodeQL", 1, true),
            chk("Image und Scan", "CI", 2, true),
            chk("CI", "CI", 3, true),
            chk("Build", "CI", 4, false),
        ];
        let v = BTreeMap::from([(1, true), (2, true)]);
        assert_eq!(cause(&checks, &[], &v), Some(CiCause::CodeSecurity));
        assert!(CiCause::CodeSecurity.counts_as_green());
        assert!(!log_needed(&checks[2]), "das Log des Sammel-Jobs wird nie geholt");
    }

    #[test]
    fn a_missing_or_negative_log_verdict_keeps_the_cause_unknown() {
        let checks = [chk("codeql / Analyze (actions)", "CodeQL", 1, true)];
        assert_eq!(cause(&checks, &[], &BTreeMap::new()), Some(CiCause::Unknown));
        assert_eq!(cause(&checks, &[], &BTreeMap::from([(1, false)])), Some(CiCause::Unknown));
    }

    #[test]
    fn a_failing_test_job_next_to_code_security_is_still_a_real_problem() {
        let checks = [chk("codeql / Analyze (actions)", "CodeQL", 1, true), chk("Test", "CI", 2, true), chk("CI", "CI", 3, true)];
        assert_eq!(cause(&checks, &[], &BTreeMap::from([(1, true)])), Some(CiCause::Unknown));
    }

    #[test]
    fn summary_jobs_like_jenkins_count_when_their_red_predecessors_are_explained() {
        let checks = [chk("Image und Scan", "CI", 2, true), chk("Jenkins", "Other", 5, true), chk("Alle Jobs grün", "Gate", 6, true)];
        assert_eq!(cause(&checks, &[], &BTreeMap::from([(2, true)])), Some(CiCause::CodeSecurity));
        assert_eq!(cause(&checks, &[], &BTreeMap::from([(2, false)])), Some(CiCause::Unknown));
    }

    #[test]
    fn an_image_scan_failing_only_at_upload_is_explained_only_with_a_clean_verdict() {
        let upload = "##[start-action display=Upload Trivy SARIF;id=up]
##[error]Please verify: Code Security must be enabled for this repository
##[end-action id=up;outcome=failure;conclusion=failure]
";
        let verdict = |o: &str| format!("##[start-action display=Trivy verdict;id=x.trivy_2]
  exit-code: 1
##[end-action id=x.trivy_2;outcome={o};conclusion={o}]
");
        let log = |v: &str| format!("Run aquasecurity/trivy-action
{upload}{v}");
        assert!(log_explains(&log(&verdict("success"))));
        assert!(!log_explains(&log(&verdict("failure"))), "Befunde über der Schwelle sind ein Problem");
        assert!(!log_explains(&log("")), "ohne Nachweis des Prüfschritts kein „erklärt“");
        assert!(log_explains("codeql: Code Security must be enabled"), "CodeQL braucht keinen Prüfschritt");
        let off = "##[start-action display=Licence report;id=lr]
##[end-action id=lr;outcome=skipped;conclusion=skipped]
##[start-action display=Licence verdict;id=lv]
##[end-action id=lv;outcome=skipped;conclusion=skipped]
";
        assert!(log_explains(&log(&format!("{}{off}", verdict("success")))), "abgeschalteter Lizenzblock stört nicht");
        let ran = "##[start-action display=Licence report;id=lr]
##[end-action id=lr;outcome=success;conclusion=success]
##[start-action display=Licence verdict;id=lv]
##[end-action id=lv;outcome=skipped;conclusion=skipped]
";
        assert!(!log_explains(&log(&format!("{}{ran}", verdict("success")))), "Bericht gelaufen, Urteil übersprungen: Problem");
    }

    #[test]
    fn the_summary_job_alone_is_not_explained() {
        let checks = [chk("CI", "CI", 3, true)];
        assert_eq!(cause(&checks, &[], &BTreeMap::from([(3, true)])), Some(CiCause::Unknown));
    }

    #[test]
    fn claude_review_red_on_a_pr_that_changes_the_gate_needs_a_human_and_is_not_green() {
        let checks = [chk("claude / Review", "Claude Code Review", 9, true), chk("Lint", "CI", 1, false)];
        let gate = vec![".github/workflows/claude-code-review.yml".to_string()];
        assert_eq!(cause(&checks, &gate, &BTreeMap::new()), Some(CiCause::ReviewGate));
        assert!(!CiCause::ReviewGate.counts_as_green());
        let other = vec!["src/main.rs".to_string()];
        assert_eq!(cause(&checks, &other, &BTreeMap::new()), Some(CiCause::Unknown), "ohne Gate-Änderung unbekannt");
        assert!(touches_review_gate(&[".github/actions/claude-review-gate/gate.sh".into()]));
    }

    #[test]
    fn green_ci_has_no_cause_and_logs_are_recognised() {
        assert_eq!(cause(&[chk("Build", "CI", 1, false)], &[], &BTreeMap::new()), None);
        assert!(log_explains("##[error]Please verify ... Code Security must be enabled for this repository"));
        assert!(log_explains("Dependency graph is not enabled (HTTP 403)"));
        assert!(!log_explains("##[error]Process completed with exit code 1."));
    }

    /// Probelauf gegen ein echtes Job-Log (nur lesend):
    /// `CI_LOG=<datei> cargo test -p deck-core ci_cause::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_log_verdict() {
        let path = std::env::var("CI_LOG").expect("CI_LOG");
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        println!("erklärt: {}", log_explains(&text));
    }

    #[test]
    fn ci_counts_as_finished_only_when_nothing_runs() {
        let mut c = chk("Build", "CI", 1, false);
        assert_eq!(finished_at(std::slice::from_ref(&c)), 100.0);
        c.running = true;
        assert_eq!(finished_at(&[c]), 0.0);
    }

    #[test]
    fn all_green_needs_finished_checks_without_red_and_ignores_merge_order() {
        assert!(all_green(&[chk("build", "CI", 1, false)]));
        assert!(!all_green(&[]), "nichts gesehen ist nicht grün");
        assert!(!all_green(&[chk("build", "CI", 1, true)]));
        assert!(!all_green(&[Check { running: true, ..chk("build", "CI", 1, false) }]));
        let order = Check { name: super::super::merge_order::MERGE_ORDER.into(), failed: true, ..Check::default() };
        assert!(all_green(&[chk("build", "CI", 1, false), order]));
    }
}
