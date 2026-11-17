//! Einen getippten Reviewer („Leo“, „Anna Schmidt“, „a.schmidt@firma.de“, „leo-mxyz“)
//! einem Mitglied der GitHub-Organisation zuordnen.
//!
//! Warum: Wer das Deck einrichtet, kennt die Namen der Kolleginnen und Kollegen, aber
//! selten ihre GitHub-Logins; die weichen oft vom Namen und von der Mailadresse ab.
//! Verglichen wird ohne Groß/klein und ohne Akzente (ä = a = ae, ß = ss), in Stufen von
//! streng nach locker. Die erste Stufe mit Treffern entscheidet: ein Treffer = eindeutig,
//! mehrere = zur Wahl. So gewinnt „Leo“ (Vorname genau) gegen „Leon“ (nur Anfang).

/// Ein Mitglied der Organisation. `emails`: Adressen, die GitHub uns zeigt (öffentliche
/// Profiladresse, bestätigte Adressen der Firmendomäne), klein geschrieben.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Member {
    pub login: String,
    pub name: String,
    pub emails: Vec<String>,
}

/// Ergebnis für eine Eingabe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    Unique(Member),
    Ambiguous(Vec<Member>),
    None,
}

/// Klein, ohne Akzente, Umlaute als Grundbuchstabe. `ae`/`oe`/`ue` werden ebenfalls zu
/// `a`/`o`/`u`, damit „Mueller“, „Müller“ und „Muller“ gleich aussehen. Das gilt für
/// beide Seiten des Vergleichs, darum schadet es Namen wie „Samuel“ nicht.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'æ' => "ae",
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
            'œ' => "oe",
            'ü' | 'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' => "u",
            'ß' => "ss",
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ı' => "i",
            'ç' | 'ć' | 'č' => "c",
            'ñ' | 'ń' | 'ň' => "n",
            'š' | 'ś' | 'ş' => "s",
            'ž' | 'ź' | 'ż' => "z",
            'ý' | 'ÿ' => "y",
            'ł' => "l",
            'ř' => "r",
            'ť' => "t",
            'ď' => "d",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(plain);
    }
    out.replace("ae", "a").replace("oe", "o").replace("ue", "u")
}

/// Wörter eines Texts nach [`fold`]; alles außer Buchstaben und Ziffern trennt.
pub fn words(s: &str) -> Vec<String> {
    fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Alle Wörter, unter denen ein Mitglied gefunden wird: Namensteile, Teile des Logins,
/// der Login selbst und die üblichen Firmen-Muster einer Mailadresse (vornachname,
/// vnachname), damit `aschmidt@…` auch ohne Punkt passt.
fn member_words(m: &Member) -> Vec<String> {
    let name = words(&m.name);
    let mut all = name.clone();
    all.extend(words(&m.login));
    all.push(fold(&m.login));
    if let (Some(first), Some(last)) = (name.first(), name.last())
        && name.len() > 1
    {
        all.push(format!("{first}{last}"));
        if let Some(initial) = first.chars().next() {
            all.push(format!("{initial}{last}"));
        }
    }
    all
}

/// Treffer einer Stufe: einer, mehrere oder keiner (dann weiter zur nächsten).
fn tier<'a>(members: &'a [Member], hit: impl Fn(&Member) -> bool) -> Option<Match> {
    let found: Vec<&'a Member> = members.iter().filter(|m| hit(m)).collect();
    match found.as_slice() {
        [] => None,
        [one] => Some(Match::Unique((*one).clone())),
        many => Some(Match::Ambiguous(many.iter().map(|m| (*m).clone()).collect())),
    }
}

/// Jedes Wort der Anfrage findet sich unter den Wörtern des Mitglieds: genau (`exact`)
/// oder als Anfang.
fn words_hit(query: &[String], m: &Member, exact: bool) -> bool {
    if query.is_empty() {
        return false;
    }
    let own = member_words(m);
    query.iter().all(|q| own.iter().any(|w| if exact { w == q } else { w.starts_with(q.as_str()) }))
}

/// Ordnet eine Eingabe zu. Stufen: Login genau, Mailadresse genau, ganzer Name genau,
/// alle Wörter genau (Vorname, Nachname, Mail-Teil), alle Wörter als Anfang. Bei einer
/// Mailadresse ohne bekannte Adresse zählt ihr Teil vor dem `@` wie ein Name
/// (`a.schmidt` = „a“ und „schmidt“, `brandt` = Nachname).
pub fn resolve(members: &[Member], input: &str) -> Match {
    let raw = input.trim().trim_start_matches('@');
    if raw.is_empty() {
        return Match::None;
    }
    if let Some(m) = tier(members, |m| m.login.eq_ignore_ascii_case(raw)) {
        return m;
    }
    let (query_text, email) = match raw.split_once('@') {
        Some((local, domain)) if !local.is_empty() && domain.contains('.') => (local, true),
        _ => (raw, false),
    };
    if email {
        let lower = raw.to_lowercase();
        if let Some(m) = tier(members, |m| m.emails.iter().any(|e| e.eq_ignore_ascii_case(&lower))) {
            return m;
        }
        if let Some(m) = tier(members, |m| m.login.eq_ignore_ascii_case(query_text)) {
            return m;
        }
    }
    let query = words(query_text);
    let whole = query.join(" ");
    let stages: [&dyn Fn(&Member) -> bool; 3] = [
        &|m| !m.name.is_empty() && words(&m.name).join(" ") == whole,
        &|m| words_hit(&query, m, true),
        &|m| words_hit(&query, m, false),
    ];
    stages.iter().find_map(|hit| tier(members, hit)).unwrap_or(Match::None)
}

/// Taugt der Text als GitHub-Login? Buchstaben, Ziffern, Bindestrich, nicht vorn, höchstens
/// 39 Zeichen; ein `[bot]` am Ende ist erlaubt.
pub fn valid_login(s: &str) -> bool {
    let core = s.strip_suffix("[bot]").unwrap_or(s);
    !core.is_empty()
        && core.len() <= 39
        && !core.starts_with('-')
        && core.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
