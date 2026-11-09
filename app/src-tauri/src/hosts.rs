//! Zwei Hosts, eine Schnittstelle (wie `deck/ui/hosts.py`): VS Code über den Broker
//! und die Extension, Windows Terminal über [`WtHost`]. Das Deck fragt und sendet an
//! genau einer Stelle; der Verteiler gibt jede Frage an den Host weiter, der den
//! Buchstaben trägt.
//!
//! VS Code steckt hinter dem Trait [`VsCodeHost`]: echt ist es der TCP-Broker, in
//! Tests eine Attrappe ohne Socket.

use std::collections::BTreeSet;

use deck_core::protocol::Command;

use crate::wt_host::WtHost;

/// Was das Deck von der VS-Code-Seite braucht (die Methoden von `net/broker.py`).
pub trait VsCodeHost: Send + Sync {
    fn connected(&self, win: &str) -> bool;
    /// Remote-Name (`ssh-remote`, `wsl` …) oder `None` für lokal bzw. unbekannt.
    fn remote(&self, win: &str) -> Option<String>;
    fn terminals(&self, win: &str) -> Vec<String>;
    fn send(&self, win: &str, cmd: &Command) -> bool;
    fn assign(&self, workspace: &str, win: &str) -> bool;
    fn forget(&self, win: &str) -> bool;
    /// In VS Code fokussierte Panes seit dem letzten Aufruf (Puffer wird geleert).
    fn drain_seen(&self) -> BTreeSet<String>;
    fn workspaces(&self) -> Vec<String>;
    fn workspace_slots(&self, workspace: &str) -> Vec<String>;
}

/// Der Verteiler.
pub struct Hosts {
    pub vscode: std::sync::Arc<dyn VsCodeHost>,
    pub wt: WtHost,
}

impl Hosts {
    pub fn is_wt(&self, win: &str) -> bool {
        !win.is_empty() && self.wt.has(win)
    }

    pub fn connected(&mut self, win: &str) -> bool {
        if self.wt.has(win) { self.wt.connected(win) } else { self.vscode.connected(win) }
    }

    pub fn remote(&self, win: &str) -> Option<String> {
        if self.wt.has(win) { None } else { self.vscode.remote(win) }
    }

    pub fn terminals(&mut self, win: &str) -> Vec<String> {
        if self.wt.has(win) { self.wt.terminals(win) } else { self.vscode.terminals(win) }
    }

    /// Fensterlage als Reihen samt Spalten des Monitors (nur Windows Terminal; VS Code
    /// verrät sie nicht).
    pub fn screen_rows(&mut self, win: &str) -> (Vec<Vec<String>>, u32) {
        if self.wt.has(win) { self.wt.screen_rows(win) } else { (Vec::new(), 0) }
    }

    /// Kommando an das Fenster `win`. `true` = an einen verbundenen Host übergeben.
    pub fn send(&mut self, win: &str, cmd: &Command) -> bool {
        if self.wt.has(win) { self.wt.send(win, cmd) } else { self.vscode.send(win, cmd) }
    }

    /// Ein WT-Buchstabe wird nie an ein VS-Code-Fenster vergeben: sonst landeten die
    /// Kommandos einer Kachel bei zwei Hosts zugleich.
    pub fn assign(&self, workspace: &str, win: &str) -> bool {
        !self.wt.has(win) && self.vscode.assign(workspace, win)
    }

    pub fn forget(&mut self, win: &str) -> bool {
        if self.wt.has(win) { self.wt.forget(win) } else { self.vscode.forget(win) }
    }

    pub fn drain_seen(&mut self) -> BTreeSet<String> {
        let mut out = self.vscode.drain_seen();
        out.extend(self.wt.drain_seen());
        out
    }

    // Nur VS Code: Auto-Binden neu verbundener Extensions.
    pub fn workspaces(&self) -> Vec<String> {
        self.vscode.workspaces()
    }

    pub fn workspace_slots(&self, workspace: &str) -> Vec<String> {
        self.vscode.workspace_slots(workspace)
    }
}
