//! Der Broker: schlanker TCP-Server, über den das Deck mit den VS-Code-Extensions
//! spricht (`deck/net/broker.py`). Newline-getrenntes JSON, Vokabular in
//! `deck_core::protocol`, Gegenstelle `extension/broker.js` (Leitung) und
//! `extension/commands.js` (Kommandos).
//!
//! - Jede Extension verbindet sich und meldet sich mit `hello` (Workspace, Ordner,
//!   Remote, evtl. Buchstabe und Slots), danach mit `terminals` bei jeder Änderung.
//! - Das Deck weist ihr per `assign` ihren Fenster-Buchstaben zu und adressiert sie
//!   danach über ihn.
//! - `seen`: ein Pane wurde in VS Code fokussiert; das Deck holt die Slots per
//!   [`VsCodeHost::drain_seen`] ab.
//!
//! **Kein `SO_REUSEADDR`**: Windows erlaubt damit zwei Listener auf demselben Port, und
//! ein zweites Deck bekäme still die Hälfte der Extensions. `std::net::TcpListener`
//! setzt es unter Windows nicht. Ist der Port belegt, bleibt der Broker aus (wie in
//! Python) und das Deck zeigt seine VS-Code-Blöcke als getrennt.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deck_core::protocol::{Command, TYPE_HELLO, TYPE_SEEN, TYPE_TERMINALS};
use serde_json::Value;

use crate::applog::note;
use crate::hosts::VsCodeHost;

struct Client {
    id: u64,
    writer: TcpStream,
    workspace: Option<String>,
    folder: Option<String>,
    remote: Option<String>,
    window: Option<String>,
    slots: Vec<String>,
}

#[derive(Default)]
struct Inner {
    clients: Vec<Client>,
    seen: BTreeSet<String>,
}

pub struct Broker {
    inner: Arc<Mutex<Inner>>,
    stop: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
    port: u16,
    listening: AtomicBool,
}

fn text(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
}

impl Broker {
    /// Broker ohne Socket: Ausgangszustand von [`Broker::start`], bleibt so, wenn der Port belegt ist.
    pub fn offline() -> Broker {
        Broker {
            inner: Arc::default(),
            stop: Arc::new(AtomicBool::new(true)),
            next_id: Arc::default(),
            port: 0,
            listening: AtomicBool::new(false),
        }
    }

    /// Auf `127.0.0.1:port` lauschen. Belegt → Broker bleibt aus (`listening` false).
    pub fn start(host: &str, port: u16) -> Arc<Broker> {
        let b = Arc::new(Broker { port, ..Broker::offline() });
        b.stop.store(false, Ordering::SeqCst);
        match TcpListener::bind((host, port)) {
            Ok(l) => {
                b.listening.store(true, Ordering::SeqCst);
                note(format!("Broker lauscht auf {host}:{port}"));
                let me = b.clone();
                std::thread::Builder::new().name("broker".into()).spawn(move || me.serve(l)).ok();
            }
            Err(e) => note(format!("Broker: {host}:{port} belegt ({e}), VS-Code-Blöcke bleiben getrennt")),
        }
        b
    }

    pub fn listening(&self) -> bool {
        self.listening.load(Ordering::SeqCst)
    }

    /// Port freigeben (beim Beenden). Der Accept-Loop wird mit einer eigenen Verbindung
    /// geweckt, damit er das Stopp-Signal sieht.
    pub fn stop(&self) {
        if self.stop.swap(true, Ordering::SeqCst) || !self.listening() {
            return;
        }
        let _ = TcpStream::connect_timeout(&([127, 0, 0, 1], self.port).into(), Duration::from_millis(200));
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for c in g.clients.drain(..) {
            let _ = c.writer.shutdown(Shutdown::Both);
        }
    }

    fn serve(self: Arc<Self>, listener: TcpListener) {
        for conn in listener.incoming() {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = conn else { continue };
            let me = self.clone();
            std::thread::Builder::new().name("broker-client".into()).spawn(move || me.handle(stream)).ok();
        }
        self.listening.store(false, Ordering::SeqCst);
    }

    fn handle(&self, stream: TcpStream) {
        let Ok(writer) = stream.try_clone() else { return };
        // Eine Extension, die nicht liest, darf das Deck nicht anhalten.
        let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.lock().clients.push(Client {
            id,
            writer,
            workspace: None,
            folder: None,
            remote: None,
            window: None,
            slots: Vec::new(),
        });
        let reader = BufReader::new(stream);
        for line in reader.split(b'\n') {
            let Ok(line) = line else { break };
            if let Ok(msg) = serde_json::from_slice::<Value>(&line) {
                self.on_message(id, &msg);
            }
        }
        self.lock().clients.retain(|c| c.id != id);
    }

    fn on_message(&self, id: u64, msg: &Value) {
        let mut g = self.lock();
        match msg.get("type").and_then(Value::as_str) {
            Some(TYPE_HELLO | TYPE_TERMINALS) => {
                let Some(c) = g.clients.iter_mut().find(|c| c.id == id) else { return };
                if let Some(ws) = text(msg.get("workspace")) {
                    c.workspace = Some(ws);
                }
                if let Some(f) = text(msg.get("folder")) {
                    c.folder = Some(f);
                }
                if msg.get("remote").is_some() {
                    c.remote = text(msg.get("remote"));
                }
                if msg.get("window").is_some() {
                    // Explizit null (nach unassign) MUSS den Buchstaben löschen; echte
                    // Bindungen setzt das Deck jede Runde neu.
                    c.window = text(msg.get("window"));
                }
                if let Some(Value::Array(a)) = msg.get("slots") {
                    c.slots = a.iter().filter_map(|s| s.as_str().map(String::from)).collect();
                }
            }
            Some(TYPE_SEEN) => {
                if let Some(slot) = text(msg.get("slot")) {
                    g.seen.insert(slot);
                }
            }
            _ => {}
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(c: &mut Client, v: &Value) -> bool {
        let mut line = v.to_string();
        line.push('\n');
        c.writer.write_all(line.as_bytes()).is_ok()
    }
}

impl VsCodeHost for Broker {
    fn connected(&self, win: &str) -> bool {
        self.lock().clients.iter().any(|c| c.window.as_deref() == Some(win))
    }

    fn remote(&self, win: &str) -> Option<String> {
        self.lock().clients.iter().find(|c| c.window.as_deref() == Some(win)).and_then(|c| c.remote.clone())
    }

    fn terminals(&self, win: &str) -> Vec<String> {
        self.lock().clients.iter().find(|c| c.window.as_deref() == Some(win)).map(|c| c.slots.clone()).unwrap_or_default()
    }

    fn send(&self, win: &str, cmd: &Command) -> bool {
        let mut g = self.lock();
        match g.clients.iter_mut().find(|c| c.window.as_deref() == Some(win)) {
            Some(c) => Self::write(c, &cmd.to_value()),
            None => false,
        }
    }

    fn assign(&self, workspace: &str, win: &str) -> bool {
        let mut g = self.lock();
        let ws = workspace.to_lowercase();
        let Some(c) = g.clients.iter_mut().find(|c| c.workspace.as_deref().is_some_and(|w| w.to_lowercase() == ws)) else {
            return false;
        };
        c.window = Some(win.to_string());
        Self::write(c, &Command::Assign { window: win.to_string() }.to_value())
    }

    fn forget(&self, win: &str) -> bool {
        let mut g = self.lock();
        let Some(c) = g.clients.iter_mut().find(|c| c.window.as_deref() == Some(win)) else { return false };
        Self::write(c, &Command::Unassign.to_value());
        c.window = None;
        true
    }

    fn drain_seen(&self) -> BTreeSet<String> {
        std::mem::take(&mut self.lock().seen)
    }

    fn workspaces(&self) -> Vec<String> {
        self.lock().clients.iter().filter_map(|c| c.workspace.clone()).collect()
    }

    fn workspace_slots(&self, workspace: &str) -> Vec<String> {
        let ws = workspace.to_lowercase();
        let g = self.lock();
        g.clients
            .iter()
            .find(|c| c.workspace.as_deref().is_some_and(|w| w.to_lowercase() == ws))
            .map(|c| c.slots.clone())
            .unwrap_or_default()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::Instant;

    fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(5) {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn free_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn an_extension_says_hello_gets_its_letter_and_receives_commands() {
        let port = free_port();
        let b = Broker::start("127.0.0.1", port);
        assert!(b.listening());
        let mut ext = TcpStream::connect(("127.0.0.1", port)).unwrap();
        ext.write_all(b"{\"type\":\"hello\",\"workspace\":\"Agent-Deck\",\"folder\":\"c:/x\",\"remote\":null,\"slots\":[\"C1\"]}\n").unwrap();
        assert!(wait_for(|| b.workspaces() == vec!["Agent-Deck".to_string()]));
        assert_eq!(b.workspace_slots("agent-deck"), vec!["C1"]);
        assert!(!b.connected("C"));

        assert!(b.assign("agent-deck", "C"));
        assert!(b.connected("C"));
        assert!(b.send("C", &Command::FocusPane { slot: "C1".into() }));
        let mut got = String::new();
        let mut buf = [0u8; 256];
        ext.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        while got.matches('\n').count() < 2 {
            let n = ext.read(&mut buf).unwrap();
            got.push_str(std::str::from_utf8(&buf[..n]).unwrap());
        }
        let lines: Vec<Value> = got.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["cmd"], "assign");
        assert_eq!(lines[1], serde_json::json!({"cmd": "focusPane", "slot": "C1"}));

        ext.write_all(b"{\"type\":\"seen\",\"slot\":\"C1\"}\n{\"type\":\"terminals\",\"window\":\"C\",\"slots\":[\"C1\",\"C2\"]}\n").unwrap();
        assert!(wait_for(|| b.terminals("C").len() == 2));
        assert_eq!(b.drain_seen().into_iter().collect::<Vec<_>>(), vec!["C1"]);
        assert!(b.drain_seen().is_empty(), "der Puffer leert sich");

        drop(ext);
        assert!(wait_for(|| !b.connected("C")), "Verbindung weg = Fenster getrennt");
        b.stop();
    }

    #[test]
    fn an_explicit_null_window_clears_the_letter() {
        let port = free_port();
        let b = Broker::start("127.0.0.1", port);
        let mut ext = TcpStream::connect(("127.0.0.1", port)).unwrap();
        ext.write_all(b"{\"type\":\"hello\",\"workspace\":\"w\",\"window\":\"B\",\"remote\":\"ssh-remote\"}\n").unwrap();
        assert!(wait_for(|| b.connected("B")));
        assert_eq!(b.remote("B").as_deref(), Some("ssh-remote"));
        ext.write_all(b"kaputt\n{\"type\":\"terminals\",\"window\":null}\n").unwrap();
        assert!(wait_for(|| !b.connected("B")));
        b.stop();
    }

    #[test]
    fn a_taken_port_leaves_the_broker_off_instead_of_sharing_it() {
        let taken = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let b = Broker::start("127.0.0.1", taken.local_addr().unwrap().port());
        assert!(!b.listening());
        assert!(!b.send("A", &Command::Reload));
    }
}
