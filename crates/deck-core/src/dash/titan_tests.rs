use super::*;

/// Ausgabe wie am 15.11.2026 gemessen (Namen verfremdet).
const SAMPLE: &str = "cores=32\nload1=7.79\nmem_total_kb=390382184\nmem_avail_kb=302761840\ncontainers=10\n\
vm=orph4018-rdp\nvm=orph3192-ts\nrun=4258-w206-3c2272|146476|1\nrun=agent-deck-320ef9\nend=1\n";

fn ok(text: &str) -> TitanSnapshot {
    parse(text, 1000.0).expect("lesbar")
}

#[test]
fn the_status_output_reads_into_numbers_and_names() {
    let s = ok(SAMPLE);
    assert!(s.reachable);
    assert_eq!(s.cores, 32);
    assert!((s.load1 - 7.79).abs() < 1e-9);
    assert!((s.mem_total_gb - 372.3).abs() < 0.1, "{}", s.mem_total_gb);
    assert!((s.mem_avail_gb - 288.7).abs() < 0.1, "{}", s.mem_avail_gb);
    assert_eq!(s.containers, 10);
    assert_eq!(s.vms, ["orph4018-rdp", "orph3192-ts"]);
    let names: Vec<&str> = s.runs.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["4258-w206-3c2272", "agent-deck-320ef9"]);
    assert_eq!((s.runs[0].age_s, s.runs[0].orphan), (Some(146_476), true));
    assert_eq!((s.runs[1].age_s, s.runs[1].orphan), (None, false), "altes Format: nie verwaist");
    assert_eq!(s.last_ok, Some(1000.0));
}

#[test]
fn a_cut_off_or_empty_output_is_an_error_not_a_zero_snapshot() {
    assert!(parse("cores=32\nload1=1.0\n", 1.0).is_err(), "ohne end=1");
    assert!(parse("end=1\n", 1.0).is_err(), "ohne Kerne");
    assert!(parse("", 1.0).is_err());
    let s = ok("cores=4\nrauschen\nfoo=bar\nvm=\nend=1");
    assert!(s.vms.is_empty() && s.runs.is_empty());
}

#[test]
fn a_failed_poll_keeps_the_last_values_and_remembers_when_it_went_down() {
    let first = merge(&TitanSnapshot::default(), Ok(ok(SAMPLE)), 100.0);
    let down = merge(&first, Err("Zeitüberschreitung".into()), 160.0);
    assert!(!down.reachable);
    assert_eq!((down.cores, down.last_ok, down.down_since), (32, Some(100.0), Some(160.0)));
    let still = merge(&down, Err("Exit 255".into()), 220.0);
    assert_eq!(still.down_since, Some(160.0), "Beginn des Ausfalls bleibt");
    let back = merge(&still, Ok(ok(SAMPLE)), 280.0);
    assert!(back.reachable && back.error.is_none() && back.down_since.is_none());
}

#[test]
fn running_runs_shorten_the_poll_interval() {
    assert_eq!(next_delay_s(&ok(SAMPLE)), POLL_BUSY_S);
    assert_eq!(next_delay_s(&ok("cores=32\nend=1")), POLL_IDLE_S);
    let down = merge(&ok(SAMPLE), Err("x".into()), 5.0);
    assert_eq!(next_delay_s(&down), POLL_IDLE_S, "nicht erreichbar: nicht hämmern");
}

#[test]
fn run_folders_lose_only_the_six_digit_hash() {
    assert_eq!(run_label("agent-deck-320ef9"), "agent-deck");
    assert_eq!(run_label("4258-w206-3c2272"), "4258-w206");
    assert_eq!(run_label("my-repo"), "my-repo");
    assert_eq!(run_label("abc-12345g"), "abc-12345g");
    assert_eq!(run_label("-abcdef"), "-abcdef");
}

#[test]
fn the_status_script_ends_with_the_completion_marker_and_skips_the_citrix_lab() {
    assert!(STATUS_SH.ends_with("echo end=1"));
    assert!(STATUS_SH.contains("ctx-*) continue"));
    assert!(!STATUS_SH.contains("du "), "du macht titan status langsam");
}

#[test]
fn only_plain_run_names_reach_the_kill_script() {
    assert!(kill_script("4258-w206-3c2272").is_some_and(|k| k.contains("N='4258-w206-3c2272'")));
    for bad in ["", "x; rm -rf ~", "a b", "../x", ".hidden", "a'b"] {
        assert_eq!(kill_script(bad), None, "{bad}");
    }
}

#[test]
fn the_kill_script_refuses_attached_runs_and_reports_the_count() {
    assert_eq!(parse_kill("killed=13
end=1
"), Ok(13));
    assert!(parse_kill("attached=1
end=1
").is_err());
    assert!(parse_kill("killed=3
").is_err(), "abgeschnitten");
    let k = kill_script("x-abcdef").unwrap();
    assert!(k.contains("sshd*) echo attached=1"), "prüft auf dem Titan noch einmal");
}
