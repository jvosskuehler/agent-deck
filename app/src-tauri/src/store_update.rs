//! Store-Update beim Start (`deck_services::store_update`): eigener Thread, der nach
//! einer Minute einmal nachsieht. Die Agenten zählt das Deck, die Prozessliste kommt
//! aus Win32.

use std::path::PathBuf;

use deck_services::store_update::{Probe, START_DELAY, Winget, run_once};
use deck_win::effects::{Processes, Win32Processes};
use tauri::AppHandle;

use crate::applog::note;
use crate::state::{lock, shared};

struct AppProbe(AppHandle);

impl Probe for AppProbe {
    fn agents_alive(&self) -> usize {
        lock(&shared(&self.0).deck).wt.alive_slots().len()
    }
    fn running_exes(&self) -> Option<Vec<String>> {
        Win32Processes.list().map(|l| l.into_iter().map(|p| p.exe).collect())
    }
}

pub fn start(app: &AppHandle, app_dir: PathBuf) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("store-update".into()).spawn(move || {
        std::thread::sleep(START_DELAY);
        run_once(&Winget, &AppProbe(app), &app_dir, crate::now(), &mut |line| note(line));
    });
}
