//! Die freie Kachel im Fensterraster: dort steht das Info-Fenster.

use deck_core::arrange as ar;

#[test]
fn five_windows_leave_the_tile_bottom_right_free() {
    // 3840 × 2160 bei 150 %: vier passen nebeneinander, fünf liegen als 3 + 2.
    let area = (0, 0, 3840, 2112);
    let grid = ar::tile_grid(5, area, 4);
    let gap = ar::grid_gap(5, area, 4).unwrap();
    assert_eq!(gap, (2560, 1056, 1280, 1056));
    // Die Lücke schließt bündig an die letzte Kachel an und reicht bis zum Rand.
    assert_eq!(grid[4].0 + grid[4].2, gap.0);
    assert_eq!(grid[4].1, gap.1);
    assert_eq!(gap.0 + gap.2, area.2);
}

#[test]
fn several_free_tiles_become_one_area() {
    // Vier Spalten: 9 → 3 Reihen zu 3, keine Lücke; 7 → 4 + 3, eine Lücke.
    let area = (0, 0, 1600, 800);
    assert!(ar::grid_gap(9, area, 4).is_none());
    assert_eq!(ar::grid_gap(7, area, 4), Some((1200, 400, 400, 400)));
    // 10 bei vier Spalten: 3 Reihen zu 4, zwei Kacheln frei, als eine Fläche.
    let g = ar::grid_gap(10, (0, 0, 1600, 900), 4).unwrap();
    assert_eq!(g, (800, 600, 800, 300));
}

#[test]
fn a_full_grid_or_no_window_has_no_gap() {
    let area = (0, 0, 1200, 800);
    assert!(ar::grid_gap(0, area, 4).is_none());
    assert!(ar::grid_gap(1, area, 4).is_none());
    assert!(ar::grid_gap(4, area, 4).is_none());
    assert!(ar::grid_gap(6, area, 4).is_none());
    assert!(ar::grid_gap(4, area, 2).is_none());
    assert_eq!(ar::grid_gap(3, area, 2), Some((600, 400, 600, 400)));
}
