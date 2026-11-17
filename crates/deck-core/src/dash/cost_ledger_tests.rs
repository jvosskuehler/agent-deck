use super::*;
use crate::dash::costs::tests::reply;

/// 2026-11-16 12:00 MEZ.
const NOW: f64 = 1_794_826_800.0;
const MEZ: i64 = 3600;

fn tool(path: &str) -> String {
    format!(r#"{{"command":"cd {path}"}}"#)
}

fn ticket_sum(v: &CostView, range: &str, ticket: &str) -> f64 {
    let r = v.ranges.iter().find(|r| r.key == range).unwrap();
    r.series.iter().filter(|s| s.ticket == ticket).map(|s| s.mix.total()).sum()
}

#[test]
fn each_reply_counts_once_and_goes_to_the_last_worktree_of_its_session() {
    let mut l = CostLedger::default();
    assert!(l.add_line("s1", None, &reply("a", "2026-11-16T09:00:00Z", "claude-opus-5-5", &tool("wt/4258-api")), "ORPH"));
    assert!(!l.add_line("s1", None, &reply("a", "2026-11-16T09:00:01Z", "claude-opus-5-5", ""), "ORPH"), "dieselbe message.id");
    l.add_line("s1", None, &reply("b", "2026-11-16T09:30:00Z", "claude-opus-5-5", &tool("wt/4259-api")), "ORPH");
    l.add_line("s1", None, &reply("c", "2026-11-16T10:00:00Z", "claude-opus-5-5", ""), "ORPH");
    let v = l.view(NOW, MEZ);
    let one = ticket_sum(&v, "d1", "ORPH-4258");
    assert!(one > 0.0);
    assert_eq!(ticket_sum(&v, "d1", "ORPH-4259"), one * 2.0, "b und c: zuletzt 4259");
}

#[test]
fn before_the_first_hit_only_a_single_ticket_session_claims_the_reply() {
    let mut l = CostLedger::default();
    l.add_line("one", None, &reply("a", "2026-11-16T09:00:00Z", "claude-opus-5-5", ""), "ORPH");
    l.add_line("one", None, &reply("b", "2026-11-16T09:10:00Z", "claude-opus-5-5", &tool("wt/4258-api")), "ORPH");
    l.add_line("two", None, &reply("c", "2026-11-16T09:00:00Z", "claude-opus-5-5", ""), "ORPH");
    l.add_line("two", None, &reply("d", "2026-11-16T09:10:00Z", "claude-opus-5-5", &tool("wt/4258-api")), "ORPH");
    l.add_line("two", None, &reply("e", "2026-11-16T09:20:00Z", "claude-opus-5-5", &tool("wt/4259-api")), "ORPH");
    let v = l.view(NOW, MEZ);
    let unit = ticket_sum(&v, "d1", "ORPH-4259");
    assert_eq!(ticket_sum(&v, "d1", "ORPH-4258"), unit * 3.0, "a, b, d");
    assert_eq!(ticket_sum(&v, "d1", ""), unit, "c: Sitzung mit zwei Tickets, vor dem ersten Treffer");
}

#[test]
fn a_subagent_without_own_hit_inherits_its_sessions_ticket_at_that_time() {
    let mut l = CostLedger::default();
    l.add_line("main", None, &reply("a", "2026-11-16T09:00:00Z", "claude-opus-5-5", &tool("wt/4258-api")), "ORPH");
    l.add_line("main", None, &reply("b", "2026-11-16T10:00:00Z", "claude-opus-5-5", &tool("wt/4259-api")), "ORPH");
    l.add_line("main/agent-1", Some("main"), &reply("c", "2026-11-16T09:30:00Z", "claude-opus-5-5", ""), "ORPH");
    let v = l.view(NOW, MEZ);
    assert_eq!(ticket_sum(&v, "d1", "ORPH-4258"), ticket_sum(&v, "d1", "ORPH-4259") * 2.0);
}

#[test]
fn buckets_align_to_local_time_and_the_period_before_is_summed() {
    let mut l = CostLedger::default();
    // 08:00 MEZ heute und 08:00 MEZ vor zwei Tagen (fällt in die 24 Std. davor nicht).
    l.add_line("s", None, &reply("a", "2026-11-16T07:00:00Z", "claude-opus-5-5", ""), "ORPH");
    l.add_line("s", None, &reply("b", "2026-11-15T07:00:00Z", "claude-opus-5-5", ""), "ORPH");
    let v = l.view(NOW, MEZ);
    let d1 = v.ranges.iter().find(|r| r.key == "d1").unwrap();
    assert_eq!(d1.starts.len(), 24);
    assert_eq!(*d1.starts.last().unwrap(), NOW, "letzter Eimer beginnt zur vollen Stunde");
    let w1 = v.ranges.iter().find(|r| r.key == "w1").unwrap();
    assert_eq!((*w1.starts.last().unwrap() + 3600.0).rem_euclid(21_600.0), 0.0, "Viertel-Tage ab 00:00 Ortszeit");
    assert!(d1.prev > 0.0, "b liegt in den 24 Std. davor");
    let m1 = v.ranges.iter().find(|r| r.key == "m1").unwrap();
    assert_eq!(m1.prev, 0.0);
    assert_eq!(v.first_at, Some(NOW - 86_400.0 - 4.0 * 3600.0));
}

#[test]
fn big_tickets_get_a_fixed_color_small_ones_none() {
    let mut l = CostLedger::default();
    for i in 0..40 {
        l.add_line("s1", None, &reply(&format!("a{i}"), "2026-11-16T09:00:00Z", "claude-opus-5-5", &tool("wt/4258-api")), "ORPH");
    }
    l.add_line("s2", None, &reply("b", "2026-11-16T09:00:00Z", "claude-opus-5-5", &tool("wt/4259-api")), "ORPH");
    let v = l.view(NOW, MEZ);
    assert_eq!(v.colored, ["ORPH-4258"], "4259 hat unter 3 %");
}

#[test]
fn old_replies_are_pruned() {
    let mut l = CostLedger::default();
    l.add_line("s", None, &reply("a", "2026-10-01T09:00:00Z", "claude-opus-5-5", ""), "ORPH");
    l.add_line("s", None, &reply("b", "2026-11-16T09:00:00Z", "claude-opus-5-5", ""), "ORPH");
    l.prune(NOW - 31.0 * 86_400.0);
    assert_eq!(l.len(), 1);
}
