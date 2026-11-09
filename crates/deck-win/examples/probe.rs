//! Lesefunktionen einmal gegen das echte System, ohne etwas zu verändern:
//! Monitore, Vordergrundfenster, Windows-Terminal-Fenster, Prozesse, Hotkey-Parser,
//! und wie lange ein Bild des Fokus-Rahmens auf der CPU braucht (nur gerechnet, nicht
//! gezeigt).
//!
//! ```powershell
//! cargo run -p deck-win --example probe --release
//! ```

use std::time::Instant;

use deck_win::frame_fx::Design;
use deck_win::frame_gpu::GpuPipeline;
use deck_win::frame_layout::FrameLayout;
use deck_win::frame_render::render_parallel;
use deck_win::{Rect, WT_WINDOW_CLASS, arrange, dpi, focus, hotkey, monitor, procs};

fn main() {
    // Sicherheitsgurt: dieses Beispiel liest nur.
    deck_win::forbid_effects();
    println!(
        "Per-Monitor-V2 angemeldet: {}",
        dpi::enable_per_monitor_v2()
    );
    println!("System-Skalierung: {:.2}", dpi::system_factor());

    println!("\n== Monitore ==");
    for m in monitor::monitors() {
        println!(
            "{} {:?} Arbeitsfläche {:?} Skalierung {:.2} {} Hz{}",
            m.device,
            m.rect,
            m.work,
            m.scale,
            m.refresh_hz.map_or("?".into(), |h| h.to_string()),
            if m.primary { " (Hauptanzeige)" } else { "" }
        );
    }
    println!("Hauptarbeitsfläche: {:?}", monitor::primary_work_area());

    println!("\n== Vordergrund ==");
    match focus::foreground() {
        Some(w) => {
            println!(
                "{:?} \"{}\" Klasse {} PID {}",
                w.hwnd, w.title, w.class, w.pid
            );
            println!("  Platzierung: {:?}", arrange::placement(w.hwnd));
            println!("  sichtbarer Rand: {:?}", arrange::visible_rect(w.hwnd));
            println!("  Greifrand: {:?}", arrange::frame_insets(w.hwnd));
            println!("  Exe: {:?}", arrange::exe_of(w.hwnd));
            println!("  Skalierung: {:.2}", dpi::factor_for_window(w.hwnd));
        }
        None => println!("kein aktives Fenster"),
    }

    println!("\n== Windows-Terminal-Fenster ==");
    let wt = arrange::class_windows(WT_WINDOW_CLASS);
    for w in &wt {
        println!("{:?} \"{}\" minimiert={}", w.hwnd, w.title, w.minimized);
    }
    if let Some(first) = wt.first() {
        println!(
            "wiedergefunden über Klasse+Titel: {:?}",
            focus::find_by_class_and_title(WT_WINDOW_CLASS, &first.title)
        );
    }
    println!(
        "VS-Code-Fenster: {:?}",
        focus::list_titles(&["Visual Studio Code"])
    );
    println!(
        "sichtbare Fenster mit Titel: {}",
        arrange::visible_windows().len()
    );

    println!("\n== Prozesse ==");
    let t = Instant::now();
    let list = procs::processes();
    println!(
        "Liste ({:.1} ms): {:?} Einträge",
        t.elapsed().as_secs_f64() * 1000.0,
        list.as_ref().map(Vec::len)
    );
    let me = std::process::id();
    if let Some(p) = list.as_ref().and_then(|l| l.iter().find(|p| p.pid == me)) {
        println!(
            "ich: {:?}, gestartet {:.0} (Unix)",
            p,
            p.start.map_or(0.0, procs::filetime_to_unix)
        );
    }
    println!(
        "Code.exe läuft: {:?} ({:?} Prozesse)",
        procs::is_running("Code.exe"),
        procs::count_exe("Code.exe")
    );
    println!(
        "WindowsTerminal.exe läuft: {:?}",
        procs::is_running("WindowsTerminal.exe")
    );

    println!("\n== Hotkey-Parser ==");
    for c in ["ctrl+<", "ctrl+alt+space", "win+f12", "k"] {
        println!("{c:>16} -> {:?}", hotkey::parse(c));
    }

    println!("\n== Fokus-Rahmen: Rechenzeit je Bild (nur gerechnet) ==");
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    for (w, h, sc) in [(1280, 800, 1.0), (1700, 1000, 1.5), (2560, 1400, 1.5)] {
        let window = Rect::from_xywh(200, 150, w, h);
        let layout = FrameLayout::new(window, Rect::new(0, 0, 6000, 4000), sc).expect("Lage");
        let strips = layout.strips();
        let px: i64 = strips
            .iter()
            .map(|s| i64::from(s.region.width()) * i64::from(s.region.height()))
            .sum();
        for d in Design::ALL {
            let mut worst = 0.0f64;
            let mut total = 0.0f64;
            let frames = 30;
            for f in 0..frames {
                let p = layout.params(d, 42, f64::from(f) / 30.0);
                let t = Instant::now();
                for s in &strips {
                    let mut buf = vec![0u8; (s.region.width() * s.region.height() * 4) as usize];
                    render_parallel(&p, s.region, &mut buf, threads);
                }
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                worst = worst.max(ms);
                total += ms;
            }
            println!(
                "{w}x{h} @{sc}: {px} px, {:<12} Mittel {:5.1} ms, max {:5.1} ms ({threads} Thr.)",
                d.name(),
                total / f64::from(frames),
                worst
            );
        }
    }

    // Die GPU-Rechenstrecke nur anlegen (Shader übersetzen), nichts zeigen.
    match GpuPipeline::new() {
        Ok(_) => println!("\nGPU-Rechenstrecke: Gerät und Shader ok"),
        Err(e) => println!("\nGPU-Rechenstrecke: {e}"),
    }
}
