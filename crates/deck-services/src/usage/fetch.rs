//! Ein Abruf des Usage-Endpoints, über die Tokens der Reihe nach.
//!
//! `401`/`403`/`429` → nächstes Token, `5xx` und Netzfehler → Abbruch. Dass auch 429
//! weiterprobiert, ist gemessen: am 2026-08-18 lieferten CLI- und Desktop-Token
//! gleichzeitig 200, während die Anzeige im Backoff stand. Das CLI-Token teilen sich
//! alle laufenden Claude-Code-Sessions und pumpen es leer. Ein 5xx dagegen ist kein
//! Token-Problem; alle durchzuprobieren wäre nur mehr Last auf einem strauchelnden Dienst.

use std::time::Duration;

use serde_json::Value;

use crate::guard;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Wem ein Token gehört (`account.email_address`), siehe `account.rs`.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

/// Die Antwort des Servers, so weit sie hier zählt.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: String,
}

/// Was beim Abruf schiefging, bevor eine Antwort da war. Der Name landet im
/// gemeinsamen Cache (`error`) und wird von Tray und Python-Deck mitgelesen; darum die
/// Namen, die Pythons urllib dort hinterlassen hätte.
#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    /// Netz, DNS, TLS (`URLError`).
    Network,
    /// Zeitüberschreitung (`TimeoutError`).
    Timeout,
    /// Eingriff gesperrt (Tests) oder sonst nicht ausführbar.
    Other(String),
}

impl Transport {
    pub fn name(&self) -> String {
        match self {
            Transport::Network => "URLError".into(),
            Transport::Timeout => "TimeoutError".into(),
            Transport::Other(s) => s.clone(),
        }
    }
}

/// Ein GET mit Bearer-Token an einen der OAuth-Endpoints ([`USAGE_URL`], [`PROFILE_URL`]).
pub trait UsageHttp: Send + Sync {
    fn get(&self, url: &str, token: &str) -> Result<HttpReply, Transport>;
}

/// Ergebnis eines gescheiterten Abrufs über alle Tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// Der Server hat geantwortet, aber nicht mit 200.
    Http { code: u16, retry_after: Option<String> },
    /// Keine Antwort oder keine lesbare (der Name wie in Python, z. B. `URLError`).
    Other(String),
}

/// Die Tokens der Reihe nach; das erste, das 200 liefert, gewinnt. Rückgabe: rohe
/// Antwort und Index des Tokens (für das Protokoll: WESSEN Budget leer war). `note`
/// bekommt jede übersprungene Quelle.
pub fn fetch_usage(
    http: &dyn UsageHttp,
    tokens: &[String],
    note: &mut dyn FnMut(usize, u16),
) -> Result<(Value, usize), FetchError> {
    let mut last = None;
    for (i, tok) in tokens.iter().enumerate() {
        let reply = http.get(USAGE_URL, tok).map_err(|t| FetchError::Other(t.name()))?;
        if reply.status == 200 {
            let data = serde_json::from_str(&reply.body).map_err(|_| FetchError::Other("JSONDecodeError".into()))?;
            return Ok((data, i));
        }
        let err = FetchError::Http { code: reply.status, retry_after: reply.retry_after };
        if matches!(reply.status, 401 | 403 | 429) {
            note(i, reply.status);
            last = Some(err);
            continue;
        }
        return Err(err);
    }
    Err(last.unwrap_or_else(|| FetchError::Other("Kein Token verfuegbar".into())))
}

/// Die echte Fassung über `ureq`: Schannel und Windows-Proxy wie bei Pythons urllib.
pub struct UreqHttp {
    agent: ureq::Agent,
    user_agent: String,
}

impl UreqHttp {
    pub fn new() -> UreqHttp {
        // PlatformVerifier = Zertifikatsspeicher von Windows. Ohne diese Angabe sucht ureq
        // eigene Wurzeln und scheitert mit native-tls an jeder Kette (nachgeprüft).
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .tls_config(tls)
            .build()
            .into();
        UreqHttp { agent, user_agent: "agent-deck-usage/2".into() }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        UreqHttp::new()
    }
}

impl UsageHttp for UreqHttp {
    fn get(&self, url: &str, token: &str) -> Result<HttpReply, Transport> {
        guard::check("OAuth-Endpoint abfragen").map_err(|e| Transport::Other(e.to_string()))?;
        let resp = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("Accept", "application/json")
            .header("User-Agent", &self.user_agent)
            .call();
        let mut resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => return Err(Transport::Timeout),
            Err(_) => return Err(Transport::Network),
        };
        let status = resp.status().as_u16();
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).map(String::from);
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        Ok(HttpReply { status, retry_after, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Antwortet je Token mit einem festen Status; 200 = Nutzdaten.
    pub(crate) struct FakeHttp {
        pub plan: Vec<(&'static str, u16)>,
        pub tried: Mutex<Vec<String>>,
    }

    impl UsageHttp for FakeHttp {
        fn get(&self, url: &str, token: &str) -> Result<HttpReply, Transport> {
            assert_eq!(url, USAGE_URL);
            self.tried.lock().unwrap().push(token.into());
            let status = self.plan.iter().find(|(t, _)| *t == token).map_or(500, |(_, s)| *s);
            Ok(HttpReply { status, retry_after: None, body: format!("{{\"ok\": \"{token}\"}}") })
        }
    }

    fn run(plan: Vec<(&'static str, u16)>) -> (Result<(Value, usize), FetchError>, Vec<String>) {
        let http = FakeHttp { plan, tried: Mutex::new(Vec::new()) };
        let toks: Vec<String> = ["cli", "desktop"].map(String::from).to_vec();
        let r = fetch_usage(&http, &toks, &mut |_, _| {});
        let tried = http.tried.lock().unwrap().clone();
        (r, tried)
    }

    #[test]
    fn a_429_tries_the_second_source() {
        let (r, tried) = run(vec![("cli", 429), ("desktop", 200)]);
        let (data, idx) = r.unwrap();
        assert_eq!((data["ok"].as_str(), idx), (Some("desktop"), 1));
        assert_eq!(tried, ["cli", "desktop"]);
    }

    #[test]
    fn only_when_all_give_429_the_error_comes_through() {
        let (r, tried) = run(vec![("cli", 429), ("desktop", 429)]);
        assert_eq!(r.unwrap_err(), FetchError::Http { code: 429, retry_after: None });
        assert_eq!(tried, ["cli", "desktop"]);
    }

    #[test]
    fn a_401_moves_on_as_before() {
        assert_eq!(run(vec![("cli", 401), ("desktop", 200)]).0.unwrap().1, 1);
    }

    #[test]
    fn a_server_error_stops_at_once() {
        let (r, tried) = run(vec![("cli", 503), ("desktop", 200)]);
        assert!(matches!(r, Err(FetchError::Http { code: 503, .. })));
        assert_eq!(tried, ["cli"]);
    }

    #[test]
    fn the_first_good_token_wins_without_a_second_call() {
        let (r, tried) = run(vec![("cli", 200), ("desktop", 200)]);
        assert_eq!(r.unwrap().1, 0);
        assert_eq!(tried, ["cli"]);
    }

    #[test]
    fn the_real_client_refuses_to_call_out_in_tests() {
        assert!(matches!(UreqHttp::new().get(USAGE_URL, "x"), Err(Transport::Other(_))));
        assert!(matches!(UreqHttp::new().get(PROFILE_URL, "x"), Err(Transport::Other(_))));
    }
}
