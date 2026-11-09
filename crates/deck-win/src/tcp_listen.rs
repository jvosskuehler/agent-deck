//! Wer lauscht auf welchem TCP-Port: `GetExtendedTcpTable` mit
//! `TCP_TABLE_OWNER_PID_LISTENER`, IPv4 und IPv6. Ohne `netstat`, ohne PowerShell und
//! ohne lokalisierte Ausgabe; ein Aufruf kostet unter einer Millisekunde.

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// `(PID, Port)` jedes lauschenden TCP-Sockets, IPv4 und IPv6, ohne Doppelte.
/// `None` = Tabelle nicht lesbar (nie als „kein Port“ deuten).
pub fn listening() -> Option<Vec<(u32, u16)>> {
    let mut out: Vec<(u32, u16)> = Vec::new();
    for (pid, port) in table(AF_INET, row_v4)?.into_iter().chain(table(AF_INET6, row_v6)?) {
        if !out.contains(&(pid, port)) {
            out.push((pid, port));
        }
    }
    out.sort_unstable();
    Some(out)
}

/// Der Port steht in den unteren 16 Bit in Netzwerk-Reihenfolge.
fn port_of(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

fn row_v4(bytes: &[u8]) -> (u32, u16) {
    // SAFETY: Aufrufer liefert genau size_of::<MIB_TCPROW_OWNER_PID>() Bytes; read_unaligned.
    let r: MIB_TCPROW_OWNER_PID = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) };
    (r.dwOwningPid, port_of(r.dwLocalPort))
}

fn row_v6(bytes: &[u8]) -> (u32, u16) {
    // SAFETY: wie row_v4, mit der IPv6-Zeile.
    let r: MIB_TCP6ROW_OWNER_PID = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) };
    (r.dwOwningPid, port_of(r.dwLocalPort))
}

/// Eine Tabelle lesen: erst Größe erfragen, dann füllen (wächst sie dazwischen, nochmal).
fn table(family: u32, row: fn(&[u8]) -> (u32, u16)) -> Option<Vec<(u32, u16)>> {
    let row_size = if family == AF_INET { size_of::<MIB_TCPROW_OWNER_PID>() } else { size_of::<MIB_TCP6ROW_OWNER_PID>() };
    let mut size = 0u32;
    for _ in 0..4 {
        let mut buf = vec![0u8; size as usize + 64];
        let mut len = buf.len() as u32;
        // SAFETY: Puffer und Länge passen zusammen; die Tabelle beginnt mit dwNumEntries.
        let rc = unsafe { GetExtendedTcpTable(Some(buf.as_mut_ptr().cast()), &mut len, false, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        match rc {
            NO_ERROR => {
                let n = u32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
                // Die Zeilen beginnen nach dwNumEntries, ausgerichtet auf 4 Byte.
                let first = 4;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let at = first + i * row_size;
                    let bytes = buf.get(at..at + row_size)?;
                    out.push(row(bytes));
                }
                return Some(out);
            }
            ERROR_INSUFFICIENT_BUFFER => size = len,
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_listener_of_this_process_shows_up_with_its_port_on_v4_and_v6() {
        let me = std::process::id();
        let v4 = TcpListener::bind("127.0.0.1:0").unwrap();
        let p4 = v4.local_addr().unwrap().port();
        let v6 = TcpListener::bind("[::1]:0").ok();
        let all = listening().expect("Tabelle lesbar");
        assert!(all.contains(&(me, p4)), "{p4} fehlt in {all:?}");
        if let Some(l) = v6 {
            let p6 = l.local_addr().unwrap().port();
            assert!(all.contains(&(me, p6)), "IPv6-Port {p6} fehlt");
        }
        drop(v4);
        let after = listening().unwrap();
        assert!(!after.contains(&(me, p4)), "geschlossener Port bleibt nicht stehen");
    }

    #[test]
    fn ports_are_read_in_network_byte_order() {
        assert_eq!(port_of(u32::from(3000u16.to_be())), 3000);
    }
}
