//! Fenster einsammeln und kacheln (Python `test_domain_arrange.py`), Windows Terminal als
//! Host (`test_domain_wt.py`) und der Fokus-Rahmen (`test_domain_focus_frame.py`).

use deck_core::arrange as ar;
use deck_core::focus_frame as ff;
use deck_core::wt::{self, Step};
use serde_json::json;

const CODE: &str = r"C:\Users\jemand\AppData\Local\Programs\Microsoft VS Code\Code.exe";
const FIREFOX: &str = r"C:\Program Files\Mozilla Firefox\firefox.exe";
const VOLL_HD: ar::Area = (0, 0, 1920, 1040);

#[test]
fn the_process_decides_not_the_title() {
    assert!(!ar::is_editor_window(FIREFOX, "Getting Started - Visual Studio Code - Mozilla Firefox"));
    assert!(ar::is_editor_window(CODE, "nur-mein-repo"));
    assert!(ar::is_editor_window(r"D:\vscode\CODE.EXE", ""));
    assert!(ar::is_editor_window(r"C:\VS Code Insiders\Code - Insiders.exe", ""));
    assert!(ar::is_editor_window(r"C:\VSCodium\codium.exe", ""));
}

#[test]
fn without_process_path_the_title_decides() {
    assert!(ar::is_editor_window("", "settings.json - agent-deck - Visual Studio Code"));
    assert!(!ar::is_editor_window("", "Orpheus Client"));
    assert!(!ar::is_editor_window("", ""));
}

#[test]
fn a_dialog_is_not_maximized() {
    assert!(ar::is_maximizable(0x15C7_0000));
    assert!(!ar::is_maximizable(0x94C8_0AC4));
}

#[test]
fn the_restore_rect_is_centered_clamped_and_never_a_dot() {
    assert_eq!(ar::normal_rect(1000, 600, VOLL_HD), ((1920 - 1000) / 2, (1040 - 600) / 2, 1000, 600));
    assert_eq!(ar::normal_rect(3800, 2000, VOLL_HD), (0, 0, 1920, 1040));
    let (_, _, w, h) = ar::normal_rect(0, 0, VOLL_HD);
    assert_eq!((w, h), (ar::MIN_W, ar::MIN_H));
    let (_, _, w, h) = ar::normal_rect(0, 0, (0, 0, 320, 240));
    assert_eq!((w, h), (320, 240));
    let area = (-2560, -300, 0, 1140);
    for (sw, sh) in [(800, 600), (5000, 5000), (0, 0)] {
        let (x, y, w, h) = ar::normal_rect(sw, sh, area);
        assert!(area.0 <= x && x + w <= area.2 && area.1 <= y && y + h <= area.3);
    }
}

#[test]
fn tiles_share_the_area_in_as_few_rows_as_possible_without_gaps() {
    assert_eq!(ar::tile_grid(3, (0, 0, 1200, 800), 4), [(0, 0, 400, 800), (400, 0, 400, 800), (800, 0, 400, 800)]);
    let five = ar::tile_grid(5, (0, 0, 1200, 800), 4);
    assert!(five.iter().all(|(_, _, w, h)| (*w, *h) == (400, 400)));
    assert_eq!(five.iter().map(|(x, y, _, _)| (*x, *y)).collect::<Vec<_>>(), [(0, 0), (400, 0), (800, 0), (0, 400), (400, 400)]);
    let area = (3840, 0, 3840 + 2561, 1397);
    let k = ar::tile_grid(3, area, 4);
    for pair in k.windows(2) {
        assert_eq!(pair[0].0 + pair[0].2, pair[1].0);
    }
    assert_eq!(k[2].0 + k[2].2, area.2);
    assert_eq!(ar::tile_grid(1, (10, 20, 110, 220), 4), [(10, 20, 100, 200)]);
    assert!(ar::tile_grid(0, (0, 0, 100, 100), 4).is_empty());
}

#[test]
fn columns_depend_on_width_and_scaling() {
    assert_eq!(ar::max_cols_for(3840, 1.5), 4);
    assert_eq!(ar::max_cols_for(2560, 1.0), 4);
    assert_eq!(ar::max_cols_for(1920, 1.0), 3);
    assert_eq!(ar::max_cols_for(1920, 1.25), 2);
    assert_eq!(ar::max_cols_for(800, 2.0), 1);
    assert_eq!(ar::max_cols_for(10_000, 1.0), ar::MAX_COLS);
}

#[test]
fn slot_names_follow_the_extension() {
    assert_eq!(wt::next_slot("A", &["A1", "A3", "B7"]), "A4");
    assert_eq!(wt::next_slot::<&str>("A", &[]), "A1");
    assert_eq!(wt::next_slot("a", &["A2"]), "A3");
    assert_eq!(wt::wanted_or_next("A", Some("A2"), &["A1"]), "A2");
    assert_eq!(wt::wanted_or_next("A", Some("A1"), &["A1"]), "A2");
    assert_eq!(wt::wanted_or_next::<&str>("A", Some("B2"), &[]), "A1");
    assert_eq!(wt::wanted_or_next("A", None, &["A1", "A2"]), "A3");
    assert_eq!(wt::slots_of("A", &["A10", "A2", "B1", "x", "A"]), ["A2", "A10"]);
}

#[test]
fn the_flag_line_is_split_like_a_shell_would() {
    assert_eq!(wt::claude_argv(None, Some(r#"--resume "abc" --model "opus[1m]""#)).unwrap(), ["--resume", "abc", "--model", "opus[1m]"]);
    assert_eq!(wt::claude_argv(Some("opus"), None).unwrap(), ["--model", "opus"]);
    assert!(wt::claude_argv(None, None).unwrap().is_empty());
    assert_eq!(wt::claude_argv(Some("opus"), Some("  ")).unwrap(), ["--model", "opus"]);
    assert_eq!(wt::shlex_split(r#"a"b c"d 'x\y' "q\"z" "" e\ f"#).unwrap(), ["ab cd", r"x\y", "q\"z", "", "e f"]);
    assert!(wt::shlex_split("\"offen").is_err());
}

#[test]
fn the_launch_spec_survives_wt_and_powershell() {
    let spec = wt::launch_spec("A1", r"C:\Users\Voßkühler\state", &["--model".into(), "opus[1m]".into()]);
    assert!(!spec.contains(';') && !spec.contains('"') && !spec.contains(' '));
    assert_eq!(wt::decode_spec(&spec), Some(json!({"slot": "A1", "state": r"C:\Users\Voßkühler\state", "args": ["--model", "opus[1m]"]})));
}

#[test]
fn every_agent_gets_its_own_window() {
    for slot in ["A1", "A2"] {
        let argv = wt::wt_argv("wt.exe", slot, r"C:\r", "pwsh.exe", "s.ps1", "SPEC", Some(wt::PROFILE));
        assert_eq!(argv[..4], ["wt.exe", "-w", &format!("deck-{slot}"), "new-tab"]);
        assert!(!argv.iter().any(|a| a == "split-pane"));
        let after = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--title"), slot);
        assert_eq!(after("-d"), r"C:\r");
        assert_eq!(after("-p"), wt::PROFILE);
        assert_eq!(argv[argv.len() - 2..], ["-Spec", "SPEC"]);
    }
    assert!(!wt::wt_argv("wt.exe", "A1", "C:\\", "pwsh.exe", "s", "S", None).iter().any(|a| a == "-p"));
}

#[test]
fn the_window_title_is_the_slot_of_the_active_pane() {
    assert_eq!(wt::slot_from_title(Some("A3")).as_deref(), Some("A3"));
    assert_eq!(wt::slot_from_title(Some(" b12 ")).as_deref(), Some("B12"));
    assert_eq!(wt::slot_from_title(Some("✳ Claude Code")), None);
    assert_eq!(wt::slot_from_title(Some("")), None);
    assert_eq!(wt::slot_from_title(None), None);
}

#[test]
fn a_pid_file_is_believed_only_when_complete_and_start_times_may_differ_slightly() {
    assert_eq!(wt::parse_pidfile(&json!({"pid": 5, "created": 7, "slot": "A1"})), Some((5, 7)));
    assert_eq!(wt::parse_pidfile(&json!({"pid": 0, "created": 7})), None);
    assert_eq!(wt::parse_pidfile(&json!({"pid": 5})), None);
    assert_eq!(wt::parse_pidfile(&json!({"pid": "x", "created": 1})), None);
    assert_eq!(wt::parse_pidfile(&json!(null)), None);
    let t = 133_000_000_000_000_000_i64;
    assert!(wt::same_start(t, t + 5_000_000));
    assert!(!wt::same_start(t, t + 20_000_000));
}

#[test]
fn a_long_prompt_arrives_as_paste_with_its_own_enter_and_keys_repeat_with_pauses() {
    assert_eq!(wt::send_steps("hi", true, true), [
        Step::Text("\x1b[200~hi\x1b[201~".into()), Step::Sleep(wt::SUBMIT_ENTER_DELAY_S), Step::Text("\r".into())]);
    assert_eq!(wt::send_steps("/model opus", true, false), [Step::Text("/model opus\r".into())]);
    assert_eq!(wt::send_steps("x", false, false), [Step::Text("x".into())]);
    assert_eq!(wt::key_steps("shift-tab", 2), [
        Step::Text("\x1b[Z".into()), Step::Sleep(wt::KEY_REPEAT_DELAY_S), Step::Text("\x1b[Z".into())]);
    assert_eq!(wt::key_steps("esc", 1), [Step::Text("\x1b".into())]);
    assert_eq!(wt::key_steps("q", 0), [Step::Text("q".into())]);
}

fn piece(p: &[ff::Piece], side: &str) -> Option<ff::Rect> {
    p.iter().find(|x| x.side == side).map(|x| x.rect)
}

#[test]
fn four_strips_enclose_the_window_without_gap_or_overlap() {
    let p = ff::pieces((100, 50, 500, 650), 30, 10);
    assert_eq!(piece(&p, "top"), Some((70, 20, 460, 40)));
    assert_eq!(piece(&p, "bottom"), Some((70, 640, 460, 40)));
    assert_eq!(piece(&p, "left"), Some((70, 60, 40, 580)));
    assert_eq!(piece(&p, "right"), Some((490, 60, 40, 580)));
    assert_eq!(p.iter().map(|x| x.rect.2 * x.rect.3).sum::<i32>(), 460 * 660 - 380 * 580);
    assert_eq!(ff::pieces((0, 0, 300, 20), 30, 10).len(), 2, "zu niedrig: keine Seitenstreifen");
}

#[test]
fn the_glow_is_cut_at_the_monitor_edge() {
    assert_eq!(ff::clip((-30, 20, 100, 40), Some((0, 0, 1920, 1080))), Some(((0, 20, 70, 40), (30, 0, 100, 40))));
    assert_eq!(ff::clip((2000, 0, 50, 50), Some((0, 0, 1920, 1080))), None);
    assert_eq!(ff::clip((5, 5, 10, 10), None), Some(((5, 5, 10, 10), (0, 0, 10, 10))));
}

#[test]
fn depth_is_positive_inside_negative_outside_and_round_at_corners() {
    assert_eq!(ff::depth(0, 50, 100, 100), 0.0);
    assert_eq!(ff::depth(3, 50, 100, 100), 3.0);
    assert_eq!(ff::depth(-2, 50, 100, 100), -2.0);
    assert!((ff::depth(-3, -4, 100, 100) + 5.0).abs() < 1e-9);
}

#[test]
fn the_line_is_opaque_and_the_glow_fades_outwards_but_stays_narrow_inside() {
    let (core, glows) = ((0.0, 3.0), [(5.0, 0.8), (14.0, 0.3)]);
    assert_eq!(ff::profile(1.0, core, &glows, false).0, 1.0);
    assert_eq!(ff::profile(-1.0, core, &glows, false).0, 0.0);
    let (near, far) = (ff::profile(-2.0, core, &glows, false).1, ff::profile(-20.0, core, &glows, false).1);
    assert!(near > far && far > 0.0);
    assert!(ff::profile(-200.0, core, &glows, false).1 < 0.001);
    assert!(ff::profile(20.0, core, &glows, false).1 < ff::profile(-17.0, core, &glows, false).1);
}

#[test]
fn a_new_design_is_never_the_one_just_shown() {
    let mut seed: u64 = 4;
    let mut rnd = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    let mut last: Option<&str> = None;
    for _ in 0..50 {
        let d = ff::pick(&ff::DESIGNS, last, &mut rnd);
        assert!(ff::DESIGNS.contains(&d) && Some(d) != last);
        last = Some(d);
    }
    assert_eq!(ff::pick(&["glut"], Some("glut"), &mut |_| 0), "glut");
}

fn named(wins: &[(&str, ar::Rect)]) -> Vec<(String, ar::Rect)> {
    wins.iter().map(|(n, r)| (n.to_string(), *r)).collect()
}

#[test]
fn screen_rows_follow_the_windows_top_to_bottom_left_to_right() {
    // Zweimal drei, gemeldet in wilder Reihenfolge.
    let wins = named(&[
        ("A6", (2560, 720, 1280, 720)),
        ("A1", (0, 0, 1280, 720)),
        ("A5", (1280, 720, 1280, 720)),
        ("A3", (2560, 0, 1280, 720)),
        ("A4", (0, 720, 1280, 720)),
        ("A2", (1280, 0, 1280, 720)),
    ]);
    assert_eq!(ar::screen_rows(&wins), [vec!["A1", "A2", "A3"], vec!["A4", "A5", "A6"]]);
}

#[test]
fn screen_rows_match_the_tile_grid_for_seven_windows() {
    let grid = ar::tile_grid(7, (0, 0, 3840, 2100), 4);
    let names = ["G", "F", "E", "D", "C", "B", "A"];
    let wins: Vec<(String, ar::Rect)> = names.iter().zip(grid).map(|(n, r)| (n.to_string(), r)).collect();
    assert_eq!(ar::screen_rows(&wins), [vec!["G", "F", "E", "D"], vec!["C", "B", "A"]]);
}

#[test]
fn slightly_offset_windows_stay_in_one_row_and_a_lower_one_opens_the_next() {
    let wins = named(&[
        ("rechts", (2000, 30, 1000, 900)),
        ("links", (0, 0, 1000, 900)),
        ("unten", (500, 800, 1000, 900)),
    ]);
    assert_eq!(ar::screen_rows(&wins), [vec!["links", "rechts"], vec!["unten"]]);
}

#[test]
fn screen_rows_of_nothing_and_of_a_degenerate_window() {
    assert!(ar::screen_rows(&[]).is_empty());
    let wins = named(&[("a", (0, 0, 0, 0)), ("b", (10, 0, 0, 0))]);
    assert_eq!(ar::screen_rows(&wins).concat(), ["a", "b"]);
}
