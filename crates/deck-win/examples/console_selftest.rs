//! Beweist, dass `deck-console` Tasten in eine fremde Konsole bringt, ohne irgendein
//! anderes Programm anzufassen: das Beispiel startet sich selbst ein zweites Mal als
//! unsichtbaren Kindprozess mit eigener Konsole, schickt ihm über den Wegwerfprozess
//! eine Zeile und prüft, ob das Kind genau diese Zeile gelesen hat.
//!
//! ```powershell
//! cargo build -p deck-win --bins --examples
//! target\debug\examples\console_selftest.exe
//! ```

use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use deck_win::console_input::{Step, helper_path, send_via_helper};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const LINE: &str = "Hallo Konsole 42";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--child") {
        // Kind: eine Zeile aus dem Eingabepuffer der EIGENEN Konsole lesen.
        let mut line = String::new();
        let conin = std::fs::File::open("CONIN$").expect("CONIN$");
        BufReader::new(conin).read_line(&mut line).expect("Zeile");
        std::fs::write(&args[2], line.trim_end()).expect("Ergebnisdatei");
        return ExitCode::SUCCESS;
    }

    let Some(helper) = helper_path() else {
        eprintln!("deck-console.exe fehlt: erst `cargo build -p deck-win --bins --examples`");
        return ExitCode::FAILURE;
    };
    let out =
        std::env::temp_dir().join(format!("deck-console-selftest-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut child = Command::new(std::env::current_exe().expect("eigene Exe"))
        .arg("--child")
        .arg(&out)
        .creation_flags(CREATE_NO_WINDOW) // eigene, unsichtbare Konsole
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Kind startet");
    std::thread::sleep(Duration::from_millis(400));

    let steps = [
        Step::Text(LINE.into()),
        Step::Sleep(Duration::from_millis(100)),
        Step::Text("\r".into()),
    ];
    let sent = send_via_helper(&helper, child.id(), &steps);
    println!("deck-console: {sent:?}");

    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill(); // nur das eigene Kind
    }
    let got = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    println!("Kind las: {got:?}");
    if got == LINE && sent.is_ok() {
        println!("ok: Zeile kam an");
        ExitCode::SUCCESS
    } else {
        println!("FEHLER: erwartet {LINE:?}");
        ExitCode::FAILURE
    }
}
