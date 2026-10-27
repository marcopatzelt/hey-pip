//! Web lookup behind a "Good to know" tip: Muse Spark with web search checks the CURRENT official rule and Pip shows
//! the page it comes from. OWNER: rules builder.
//!
//! Privacy by construction: the query is a FIXED generic text per tip id from `TOPICS` (no name, number, date,
//! place, insurer or diagnosis can enter it; tests check every row against the demo profile). The Rust rule stays
//! the ground truth: the web note is one extra sentence after the tip ("The website … says …, please check there"),
//! and when its amount or period does not match the rule Pip says both and asks the user to check.
//! Only pages on `ALLOWED_DOMAINS` count as a source; no such page, no web note. One answer per topic is kept for
//! LOOKUP_CACHE_DAYS in memory events (`web_lookup`), so the second ask costs no call and no token.
//! Wording: no advice ("you must", "sue", "lawyer", "claim now"), no verdicts; an answer with such words is dropped.

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use super::tips::{self, TIP_REPEAT_DAYS, Tip};
use crate::i18n::{Lang, tf};
use crate::memory::Memory;
use crate::meta::{MODEL_SPARK, MetaClient, SearchOutput, WebNote};

/// Event kind of a cached answer (text = JSON `CachedNote`).
pub const EV_WEB_LOOKUP: &str = "web_lookup";
/// A cached answer is used for this many days (the same rhythm as a tip's repeat).
pub const LOOKUP_CACHE_DAYS: i64 = TIP_REPEAT_DAYS;
/// How many recent events the cache check looks at.
const EVENTS_SCAN: u32 = 300;
/// The web text Pip repeats is at most this long (two sentences).
const MAX_TEXT: usize = 320;
/// A page title longer than this is cut at a word.
const MAX_TITLE: usize = 60;

/// Official sites whose pages count as a source (a host equal to one of them or below it). An insurer's or
/// airline's own site from memory never: the query and the source list know nothing about the user.
pub const ALLOWED_DOMAINS: &[&str] = &[
    "europa.eu",
    "eur-lex.europa.eu",
    "ec.europa.eu",
    "gesetze-im-internet.de",
    "verbraucherzentrale.de",
    "bmj.de",
    "bundesregierung.de",
    "bund.de",
    "bundesgesundheitsministerium.de",
    "gkv-spitzenverband.de",
    "bmbf.de",
    "studierendenwerke.de",
    "hrk.de",
    // The German states' own portals (state law, e.g. university rules).
    "nrw.de",
    "bayern.de",
    "berlin.de",
    "hamburg.de",
    "bremen.de",
    "hessen.de",
    "niedersachsen.de",
    "sachsen.de",
    "sachsen-anhalt.de",
    "thueringen.de",
    "brandenburg.de",
    "saarland.de",
    "rlp.de",
    "schleswig-holstein.de",
    "service-bw.de",
];

/// Words that make a web answer unusable (advice or verdicts). Matched as whole words ("issue" is not "sue").
const BANNED: &[&str] = &[
    "you must",
    "sue",
    "claim now",
    "lawyer",
    "attorney",
    "scam",
    "guaranteed",
    "du musst",
    "klagen",
    "verklagen",
    "klage",
    "anwalt",
    "betrug",
    "garantiert",
];

/// An answer that talks about the search instead of the rule ("the results show no period", "I found") is not a
/// sentence Pip can repeat to a person; such an answer is dropped and the tip keeps the built-in rule only.
const META_TALK: &[&str] = &[
    "the results",
    "search results",
    "in the results",
    "i found",
    "i could not",
    "i couldn't",
    "i cannot",
    "i can't",
    "die ergebnisse",
    "den ergebnissen",
    "suchergebnis",
    "ich habe",
    "ich konnte",
];

/// Words before a dot that do not end a sentence.
const ABBREVIATIONS: &[&str] = &["art", "no", "nr", "abs", "vgl", "z.b", "e.g", "i.e", "ec", "eg", "bzw", "ca", "para"];

/// One topic Pip may look up: the tip it belongs to, the fixed query, and what the Rust rule says.
#[derive(Debug, Clone, Copy)]
pub struct Topic {
    pub tip_id: &'static str,
    /// Where the rule holds (ISO country), None = EU-wide.
    pub region: Option<&'static str>,
    /// The fixed query (generic topic + official site hints). Digits only from `query_digits`.
    pub query: &'static str,
    /// The only digit groups the query may contain (the regulation's own number).
    pub query_digits: &'static [&'static str],
    /// What the Rust rule says, short (the "rule I know" in `tip_web_differs`).
    pub rule_en: &'static str,
    pub rule_de: &'static str,
    /// Who the user asks when web and rule disagree.
    pub org_en: &'static str,
    pub org_de: &'static str,
    /// The web answer agrees with the rule when it contains one of these (lowercase); empty = never compared.
    pub agrees: &'static [&'static str],
    /// When the model cites no page, a search result on an allowed domain whose address or title holds one of these
    /// (lowercase) is the source: the page of the law the rule names, never just any official page.
    pub hints: &'static [&'static str],
    pub source: &'static str,
    pub url: &'static str,
}

/// The topics. Rows without a Rust tip yet (withdrawal, deposit, re-registration) keep their query and source here
/// so the eval can check them; they are looked up only when a tip with that id exists.
pub const TOPICS: &[Topic] = &[
    Topic {
        tip_id: "eu261_delay",
        region: None,
        query: "Regulation (EC) No 261/2004 air passenger rights compensation amounts site:europa.eu",
        query_digits: &["261", "2004"],
        rule_en: "EUR 250, 400 or 600 by distance under EU rule 261/2004",
        rule_de: "250, 400 oder 600 Euro je nach Strecke nach der EU-Regel 261/2004",
        org_en: "the airline",
        org_de: "der Fluggesellschaft",
        agrees: &["250", "400", "600"],
        hints: &["32004r0261", "261/2004", "reg/2004/261", "passenger-rights", "passenger rights", "air-passenger"],
        source: super::flight::EU261_SOURCE,
        url: super::flight::EU261_URL,
    },
    Topic {
        tip_id: "au_gap",
        region: Some("DE"),
        query: "Social Code Book V Section 46 sickness benefit gesetze-im-internet englisch_sgb_5",
        query_digits: &["46", "5"],
        rule_en: "the next sick note usually has to be issued by the next working day after the last sick day",
        rule_de: "die nächste Krankschreibung muss in der Regel spätestens am nächsten Werktag nach dem letzten \
                  Krankheitstag ausgestellt sein",
        org_en: "your health insurer",
        org_de: "deiner Krankenkasse",
        agrees: &["working day", "werktag", "business day"],
        hints: &["sgb_5", "sgb v", "krankengeld", "sickness benefit", "sick pay"],
        source: "§ 46 Satz 2 SGB V",
        url: "https://www.gesetze-im-internet.de/sgb_5/__46.html",
    },
    Topic {
        tip_id: "widerspruch_month",
        region: Some("DE"),
        query: "Code of Administrative Court Procedure Section 70 objection period gesetze-im-internet englisch_vwgo",
        query_digits: &["70"],
        rule_en: "one month from the day the letter arrived",
        rule_de: "einen Monat ab dem Tag, an dem der Brief kam",
        org_en: "the office that sent the letter",
        org_de: "der Stelle, die den Brief geschickt hat",
        agrees: &[
            "one month",
            "1 month",
            "a month",
            "einen monat",
            "ein monat",
            "1 monat",
            "eines monats",
            "monatsfrist",
        ],
        hints: &["vwgo", "sgg", "widerspruch", "objection"],
        source: "§ 84 SGG / § 70 VwGO",
        url: "https://www.gesetze-im-internet.de/sgg/__84.html",
    },
    Topic {
        tip_id: "eu_withdrawal_14d",
        region: None,
        query: "Directive 2011/83/EU consumer rights right of withdrawal period site:europa.eu",
        query_digits: &["2011", "83"],
        rule_en: "14 days from delivery for most online purchases",
        rule_de: "14 Tage ab Lieferung bei den meisten Online-Käufen",
        org_en: "the shop",
        org_de: "dem Shop",
        agrees: &["14", "fourteen", "vierzehn"],
        hints: &["2011l0083", "2011/83", "consumer-rights", "consumer rights", "withdrawal"],
        source: "Art. 9 Richtlinie 2011/83/EU, § 355 BGB",
        url: "https://eur-lex.europa.eu/eli/dir/2011/83/oj",
    },
    Topic {
        tip_id: "deposit_return",
        region: Some("DE"),
        query: "German Civil Code Section 551 rental security deposit gesetze-im-internet englisch_bgb",
        query_digits: &["551"],
        rule_en: "the deposit is at most three months' rent, and the landlord may keep it a reasonable time to check",
        rule_de: "die Kaution ist höchstens drei Monatsmieten, und der Vermieter darf sie eine angemessene Zeit zum \
                  Prüfen behalten",
        org_en: "your landlord",
        org_de: "deinem Vermieter",
        agrees: &[],
        hints: &["bgb", "mietkaution", "kaution", "deposit"],
        source: "§ 551 BGB",
        url: "https://www.gesetze-im-internet.de/bgb/__551.html",
    },
    Topic {
        tip_id: "uni_reregistration",
        region: Some("DE"),
        query: "Hochschulrahmengesetz HRG gesetze-im-internet.de/hrg Exmatrikulation Rückmeldung",
        query_digits: &[],
        rule_en: "the university sets the re-registration date; missing it can end the enrolment",
        rule_de: "die Hochschule legt die Frist für die Rückmeldung fest; wer sie verpasst, kann exmatrikuliert werden",
        org_en: "your university",
        org_de: "deiner Hochschule",
        agrees: &[],
        hints: &["hrg", "exmatri", "rückmeld", "rueckmeld", "re-registration", "enrol"],
        source: "§ 43 HRG / Hochschulgesetz des Landes",
        url: "https://www.gesetze-im-internet.de/hrg/",
    },
];

/// What happened on one lookup (the log line and the eval count these).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A web search answered with an official page.
    Hit,
    /// The answer of the last LOOKUP_CACHE_DAYS days was used (no call).
    Cache,
    /// No search (no topic, switched off) or no official page in the answer.
    None,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Hit => "hit",
            Outcome::Cache => "cache",
            Outcome::None => "none",
        }
    }
}

/// One lookup: what happened, the note (Hit/Cache), and the query that was (or would be) sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Lookup {
    pub outcome: Outcome,
    pub note: Option<WebNote>,
    pub query: Option<&'static str>,
}

/// The cached answer in an event's text.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedNote {
    tip_id: String,
    text: String,
    url: String,
    title: String,
    at: NaiveDate,
    /// The answer's language ("en"/"de"); the same topic is asked again for the other language.
    #[serde(default = "lang_en")]
    lang: String,
}

fn lang_en() -> String {
    "en".into()
}

pub fn topic(tip_id: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.tip_id == tip_id)
}

/// The fixed query for a tip id in `region` (None = any). Nothing else ever goes into a search.
pub fn query_for(tip_id: &str, region: Option<&str>) -> Option<&'static str> {
    let t = topic(tip_id)?;
    let fits = match (t.region, region) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => true,
    };
    fits.then_some(t.query)
}

/// Is `url` a page of an allowed official site (https or http, host equal to or below an allowed domain)?
pub fn allowed_domain(url: &str) -> bool {
    let Some(host) = host_of(url) else { return false };
    ALLOWED_DOMAINS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map(|(scheme, r)| (scheme.to_ascii_lowercase(), r))?;
    if rest.0 != "https" && rest.0 != "http" {
        return None;
    }
    let host = rest.1.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?.to_lowercase();
    let host = host.trim_end_matches('.').to_string();
    (!host.is_empty()).then_some(host)
}

/// The web note of a search answer for `tip_id`, or None: the model's `url_citation`s on allowed domains; when it
/// cited no page at all, the first allowed search result whose address or title names the law (`Topic::hints`).
/// The text is cleaned (no links, at most two sentences) and free of advice.
pub fn note_from(tip_id: &str, answer: &SearchOutput, today: NaiveDate) -> Option<WebNote> {
    let text = clean_text(&answer.text)?;
    let mut citations: Vec<(String, String)> = Vec::new();
    for c in answer.citations.iter().filter(|c| allowed_domain(&c.url)) {
        if !citations.iter().any(|(u, _)| u == &c.url) {
            citations.push((c.url.clone(), c.title.clone()));
        }
    }
    if answer.citations.is_empty() {
        let hints = topic(tip_id).map(|t| t.hints).unwrap_or_default();
        let names_the_law = |(u, t): &&(String, String)| {
            let hay = format!("{} {}", u.to_lowercase(), t.to_lowercase());
            hints.iter().any(|h| hay.contains(h))
        };
        citations.extend(answer.results.iter().filter(|(u, _)| allowed_domain(u)).find(names_the_law).cloned());
    }
    if citations.is_empty() {
        return None;
    }
    Some(WebNote { text, citations, checked_on: today })
}

/// Links and citation marks out, two sentences at most, None when empty or when it gives advice.
fn clean_text(raw: &str) -> Option<String> {
    let mut s = raw.replace('\n', " ");
    // Citation marks of the search ("【123†L4-L9】", also cut off at the end) go.
    while let Some(open) = s.find('【') {
        let close = s[open..].find('】').map(|i| open + i + '】'.len_utf8()).unwrap_or(s.len());
        s.replace_range(open..close, "");
    }
    // [title](url) -> title
    while let Some(open) = s.find('[') {
        let Some(mid) = s[open..].find("](").map(|i| open + i) else { break };
        let Some(close) = s[mid..].find(')').map(|i| mid + i) else { break };
        let title = s[open + 1..mid].to_string();
        s.replace_range(open..=close, &title);
    }
    // "(https://…)", "(eur-lex.europa.eu)" and bare links
    let mut words: Vec<String> = Vec::new();
    for w in s.split_whitespace() {
        let bare = w.trim_matches(|c: char| "()[],;.".contains(c));
        let link = bare.starts_with("http") || ALLOWED_DOMAINS.iter().any(|d| bare.ends_with(d));
        if !link {
            words.push(w.to_string());
        } else if w.ends_with('.')
            && let Some(prev) = words.last_mut()
        {
            // "… 261/2004 (eur-lex.europa.eu)." keeps its full stop.
            prev.push('.');
        }
    }
    let s = words.join(" ").replace(" ()", "").replace("  ", " ").replace(" .", ".").replace(" ,", ",");
    let low = s.to_lowercase();
    if BANNED.iter().chain(META_TALK).any(|b| super::guard::contains_phrase(&low, b)) {
        return None;
    }
    let mut out = String::new();
    let mut sentences = 0;
    for (i, c) in s.char_indices() {
        out.push(c);
        let next = s[i + c.len_utf8()..].chars().next();
        let word = s[..i].rsplit(' ').next().unwrap_or("").to_lowercase();
        let abbreviation = word.chars().count() <= 1 || ABBREVIATIONS.contains(&word.as_str());
        if c == '.' && !abbreviation && next.is_none_or(char::is_whitespace) {
            sentences += 1;
            if sentences == 2 {
                break;
            }
        }
    }
    let out = out.trim().to_string();
    if out.chars().count() < 20 {
        return None;
    }
    if out.chars().count() > MAX_TEXT {
        let cut: String = out.chars().take(MAX_TEXT).collect();
        let cut = cut.rsplit_once(' ').map(|(a, _)| a).unwrap_or(&cut);
        return Some(format!("{}…", cut.trim_end_matches([',', ';'])));
    }
    Some(out)
}

/// The cached note for `tip_id` in `lang` from the last LOOKUP_CACHE_DAYS days (newest first).
pub fn cached(mem: &Memory, tip_id: &str, lang: Lang, now: DateTime<Local>) -> Result<Option<WebNote>> {
    let since = now - Duration::days(LOOKUP_CACHE_DAYS);
    for e in mem.recent_events(EVENTS_SCAN)? {
        if e.kind != EV_WEB_LOOKUP || e.at < since {
            continue;
        }
        let Ok(c) = serde_json::from_str::<CachedNote>(&e.text) else { continue };
        if c.tip_id == tip_id && c.lang == lang.code() && allowed_domain(&c.url) {
            return Ok(Some(WebNote { text: c.text, citations: vec![(c.url, c.title)], checked_on: c.at }));
        }
    }
    Ok(None)
}

/// Keep `note` for LOOKUP_CACHE_DAYS (event `web_lookup`, its first citation).
pub fn remember(mem: &Memory, tip_id: &str, lang: Lang, note: &WebNote, now: DateTime<Local>) -> Result<()> {
    let Some((url, title)) = note.citations.first() else { return Ok(()) };
    let c = CachedNote {
        tip_id: tip_id.to_string(),
        text: note.text.clone(),
        url: url.clone(),
        title: title.clone(),
        at: note.checked_on,
        lang: lang.code().to_string(),
    };
    mem.add_event_at(now, EV_WEB_LOOKUP, &serde_json::to_string(&c)?)
}

/// Look up the topic of `tip_id`: the cache first, else one web search with the fixed query, answered in `lang`.
/// Logs `web lookup: <tip_id> <hit|cache|none>`.
pub async fn lookup(
    client: &MetaClient,
    mem: &Memory,
    tip_id: &str,
    region: Option<&str>,
    lang: Lang,
    now: DateTime<Local>,
) -> Result<Lookup> {
    let query = query_for(tip_id, region);
    let result = match query {
        None => Lookup { outcome: Outcome::None, note: None, query },
        Some(q) => match cached(mem, tip_id, lang, now)? {
            Some(note) => Lookup { outcome: Outcome::Cache, note: Some(note), query },
            None => {
                let language = match lang {
                    Lang::En => "English",
                    Lang::De => "German",
                };
                let answer = client.web_search(MODEL_SPARK, q, language).await?;
                match note_from(tip_id, &answer, now.date_naive()) {
                    Some(note) => {
                        remember(mem, tip_id, lang, &note, now)?;
                        Lookup { outcome: Outcome::Hit, note: Some(note), query }
                    }
                    None => Lookup { outcome: Outcome::None, note: None, query },
                }
            }
        },
    };
    tracing::info!("web lookup: {tip_id} {}", result.outcome.as_str());
    Ok(result)
}

/// The web note for a tip (its region from the tips table). Errors (no internet, API) only lose the note.
pub async fn for_tip(
    client: &MetaClient,
    mem: &Memory,
    tip: &Tip,
    lang: Lang,
    now: DateTime<Local>,
) -> Option<WebNote> {
    let region = tips::TIPS.iter().find(|r| r.id == tip.id).and_then(|r| r.region);
    match lookup(client, mem, tip.id, region, lang, now).await {
        Ok(l) => l.note,
        Err(e) => {
            tracing::warn!(error = %e, "web lookup: {} none (failed)", tip.id);
            None
        }
    }
}

/// Does the web text miss every amount or period the Rust rule names? (Topics without `agrees` never differ.)
pub fn differs(tip_id: &str, web_text: &str) -> bool {
    let Some(t) = topic(tip_id) else { return false };
    let low = web_text.to_lowercase();
    !t.agrees.is_empty() && !t.agrees.iter().any(|a| low.contains(a))
}

/// The one sentence Pip adds after the tip: "The website … says: … (checked …). Please check there." or, when the
/// web does not match the rule, "The rule I know says …; the website … says …. Please check with …".
pub fn web_sentence(tip_id: &str, note: &WebNote, lang: Lang) -> Option<String> {
    let t = topic(tip_id)?;
    let (url, title) = note.citations.first()?;
    let title = short_title(title, url);
    let text = note.text.trim().trim_end_matches(['.', '…']).to_string();
    let date = match lang {
        Lang::En => note.checked_on.format("%-d %b %Y").to_string(),
        Lang::De => note.checked_on.format("%d.%m.%Y").to_string(),
    };
    Some(if differs(tip_id, &note.text) {
        let (rule, org) = match lang {
            Lang::En => (t.rule_en, t.org_en),
            Lang::De => (t.rule_de, t.org_de),
        };
        tf("tip_web_differs", lang, &[("rule", rule), ("title", &title), ("web", &text), ("org", org)])
    } else {
        tf("tip_web_note", lang, &[("title", &title), ("text", &text), ("date", &date)])
    })
}

/// Add the web note to a letter tip: the note and one sentence after the tip's text.
pub fn attach(tip: &mut Tip, note: WebNote, lang: Lang) {
    if let Some(s) = web_sentence(tip.id, &note, lang) {
        tip.text = format!("{} {s}", tip.text.trim_end());
        tip.web = Some(note);
    }
}

/// A page title for the sentence: quoted, cut at a word; the host when the title is empty.
fn short_title(title: &str, url: &str) -> String {
    let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = if t.is_empty() { host_of(url).unwrap_or_default() } else { t };
    let t = if t.chars().count() > MAX_TITLE {
        let cut: String = t.chars().take(MAX_TITLE).collect();
        format!("{}…", cut.rsplit_once(' ').map(|(a, _)| a).unwrap_or(&cut))
    } else {
        t
    };
    format!("\"{t}\"")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::meta::UrlCitation;

    fn now() -> DateTime<Local> {
        super::super::local_at(NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(), 12, 0)
    }

    #[test]
    fn every_tip_has_a_topic_and_every_query_is_generic() {
        for rule in tips::TIPS {
            assert!(query_for(rule.id, rule.region).is_some(), "{} has no topic", rule.id);
        }
        assert_eq!(TOPICS.len(), 6);
        // The demo profile: none of its values may be in any query (privacy by construction).
        let mem = std::sync::Arc::new(Memory::open_in_memory().unwrap());
        let clock: std::sync::Arc<dyn crate::clock::Clock> = std::sync::Arc::new(crate::clock::FixedClock::new(now()));
        let rules = super::super::RuleEngine::new(mem.clone(), clock);
        let letters = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters");
        assert!(crate::memory::seed_demo_profile(&mem, &rules, &letters, now()).unwrap());
        let values: Vec<String> = mem
            .profile_all()
            .unwrap()
            .into_iter()
            .map(|(_, v)| v.trim().to_lowercase())
            .filter(|v| v.chars().count() >= 3)
            .collect();
        assert!(values.len() >= 5, "the demo profile is seeded: {values:?}");
        for t in TOPICS {
            let q = t.query.to_lowercase();
            for v in &values {
                assert!(!q.contains(v.as_str()), "{}: query holds the profile value {v:?}", t.tip_id);
            }
            // Digits only from the regulation's own number.
            let digits: Vec<&str> = q.split(|c: char| !c.is_ascii_digit()).filter(|d| !d.is_empty()).collect();
            for d in digits {
                assert!(t.query_digits.contains(&d), "{}: digits {d} in the query", t.tip_id);
            }
            for word in ["erika", "beispiel", "evergreen", "@", "de02", "bx"] {
                assert!(!q.contains(word), "{}: {word}", t.tip_id);
            }
            assert!(t.url.starts_with("https://") && !t.source.is_empty(), "{}", t.tip_id);
            for text in [t.rule_en, t.rule_de, t.org_en, t.org_de] {
                let low = text.to_lowercase();
                assert!(!BANNED.iter().any(|b| super::super::guard::contains_phrase(&low, b)), "{}: {text}", t.tip_id);
            }
        }
        assert_eq!(query_for("au_gap", Some("FR")), None, "a German rule is not looked up for France");
        assert!(query_for("eu261_delay", Some("FR")).is_some());
        assert_eq!(query_for("no_such_tip", None), None);
    }

    #[test]
    fn only_official_domains_count() {
        assert!(allowed_domain("https://eur-lex.europa.eu/eli/reg/2004/261/oj"));
        assert!(allowed_domain("https://www.gesetze-im-internet.de/sgb_5/__46.html"));
        assert!(allowed_domain("https://www.Gesetze-im-Internet.de/x"));
        assert!(allowed_domain("https://www.verbraucherzentrale.de/wissen"));
        assert!(allowed_domain("https://europa.eu/youreurope/citizens/travel/"));
        assert!(!allowed_domain("https://www.flightright.com/your-rights/eu-regulation"));
        assert!(!allowed_domain("https://europa.eu.evil.example/x"));
        assert!(!allowed_domain("https://noteuropa.eu/x"));
        assert!(!allowed_domain("ftp://europa.eu/x"));
        assert!(!allowed_domain("europa.eu"));
    }

    fn answer(text: &str, cited: &[&str], results: &[&str]) -> SearchOutput {
        SearchOutput {
            text: text.into(),
            citations: cited
                .iter()
                .map(|u| UrlCitation { url: (*u).into(), title: "Page".into(), start: 0, end: 10 })
                .collect(),
            results: results.iter().map(|u| ((*u).into(), "Result".into())).collect(),
            incomplete: false,
        }
    }

    #[test]
    fn notes_keep_official_citations_only() {
        let today = now().date_naive();
        let eur = "https://eur-lex.europa.eu/eli/reg/2004/261/oj";
        let blog = "https://blog.example/eu261";
        let text = "The rule is Regulation (EC) No 261/2004, Article 7. It gives EUR 250, 400 or 600 by distance.";
        let n = note_from("eu261_delay", &answer(text, &[blog, eur], &[]), today).unwrap();
        assert_eq!(n.citations, vec![(eur.to_string(), "Page".to_string())]);
        assert_eq!(n.checked_on, today);
        assert!(note_from("eu261_delay", &answer(text, &[blog], &[blog, eur]), today).is_none(), "cited only a blog");
        // No annotation: the first official result that names the law.
        let proposal = "https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=celex:52013PC0130";
        let n = note_from("eu261_delay", &answer(text, &[], &[blog, proposal, eur]), today).unwrap();
        assert_eq!(n.citations[0].0, eur, "the regulation's own page, not the first official one");
        assert!(note_from("eu261_delay", &answer(text, &[], &[blog, proposal]), today).is_none());
        let sgb = "https://www.gesetze-im-internet.de/englisch_sgb_5/englisch_sgb_5.pdf";
        assert!(note_from("au_gap", &answer(text, &[], &[sgb]), today).is_some());
        assert!(note_from("widerspruch_month", &answer(text, &[], &[sgb]), today).is_none(), "another law");
        assert!(note_from("eu261_delay", &answer("", &[eur], &[]), today).is_none());
        let advice = "Under rule 261/2004 you must claim now. A lawyer helps.";
        assert!(note_from("eu261_delay", &answer(advice, &[eur], &[]), today).is_none(), "advice is dropped");
        let issue = "The issue of a new sick note follows Section 46 SGB V. The next working day counts.";
        assert!(note_from("au_gap", &answer(issue, &[eur], &[]), today).is_some(), "\"issue\" is not \"sue\"");
        // The model talking about its search (seen live, Oct 27) is nothing Pip can repeat to a person.
        let meta = "The law is Section 46 SGB V. The results show no current amount or period.";
        assert!(note_from("au_gap", &answer(meta, &[eur], &[]), today).is_none(), "meta talk is dropped");
        assert!(
            note_from("au_gap", &answer("Ich habe nichts gefunden. Paragraf 46 SGB V.", &[eur], &[]), today).is_none()
        );
    }

    #[test]
    fn text_is_cleaned_to_two_sentences() {
        let raw = "The rule is Regulation (EC) No 261/2004 ([eur-lex.europa.eu](https://eur-lex.europa.eu/x)). \
                   It gives EUR 250, 400 or 600. A third sentence goes.";
        assert_eq!(clean_text(raw).unwrap(), "The rule is Regulation (EC) No 261/2004. It gives EUR 250, 400 or 600.");
        assert_eq!(
            clean_text("Art. 7 sets 2.5 hours. Period one month. More.").unwrap(),
            "Art. 7 sets 2.5 hours. Period one month."
        );
    }

    #[test]
    fn sentences_say_the_source_or_both_rules() {
        let note = WebNote {
            text: "Regulation 261/2004 sets EUR 250, 400 or 600 by distance.".into(),
            citations: vec![("https://eur-lex.europa.eu/x".into(), "EUR-Lex – Regulation 261/2004".into())],
            checked_on: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
        };
        let s = web_sentence("eu261_delay", &note, Lang::En).unwrap();
        assert_eq!(
            s,
            "The website \"EUR-Lex – Regulation 261/2004\" says: Regulation 261/2004 sets EUR 250, 400 or 600 by \
             distance (checked 27 Oct 2026). Please check there."
        );
        let other = WebNote { text: "Section 47 SGB XIV pays sick pay for health damage.".into(), ..note.clone() };
        let s = web_sentence("au_gap", &other, Lang::En).unwrap();
        assert!(s.starts_with("The rule I know says the next sick note"), "{s}");
        assert!(s.contains("Section 47") && s.ends_with("Please check with your health insurer."), "{s}");
        let de = web_sentence("au_gap", &other, Lang::De).unwrap();
        assert!(de.ends_with("Bitte frag bei deiner Krankenkasse nach."), "{de}");
        assert!(!differs("deposit_return", "anything"), "no amount to compare");
        assert!(!differs("widerspruch_month", "The objection period is one month after notification."));
        for lang in [Lang::En, Lang::De] {
            let low = web_sentence("eu261_delay", &note, lang).unwrap().to_lowercase();
            assert!(!BANNED.iter().any(|b| super::super::guard::contains_phrase(&low, b)), "{low}");
        }
    }

    #[test]
    fn attach_adds_one_sentence_and_keeps_the_rule() {
        let mut tip = Tip {
            id: "eu261_delay",
            text: "Good to know: you may be owed EUR 400 from Beispiel Air. Please check.".into(),
            source: super::super::flight::EU261_SOURCE,
            url: super::super::flight::EU261_URL,
            deadline_id: None,
            web: None,
        };
        let note = WebNote {
            text: "Regulation 261/2004 sets EUR 250, 400 or 600 by distance.".into(),
            citations: vec![("https://eur-lex.europa.eu/x".into(), "EUR-Lex".into())],
            checked_on: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
        };
        attach(&mut tip, note.clone(), Lang::En);
        assert!(tip.text.starts_with("Good to know: you may be owed EUR 400 from Beispiel Air. Please check. "));
        assert!(tip.text.ends_with("(checked 27 Oct 2026). Please check there."), "{}", tip.text);
        assert_eq!(tip.text.matches("The website").count(), 1);
        assert_eq!(tip.web, Some(note));
    }

    fn search_body(text: &str) -> serde_json::Value {
        json!({
            "id": "resp_ws", "status": "completed",
            "output": [
                {"type":"web_search_call","results":[{"url":"https://eur-lex.europa.eu/eli/reg/2004/261/oj","title":"Regulation 261/2004"}]},
                {"type":"message","content":[{"type":"output_text","text": text,
                  "annotations":[{"type":"url_citation","url":"https://eur-lex.europa.eu/eli/reg/2004/261/oj",
                    "title":"Regulation 261/2004","start_index":0,"end_index":20}]}]}
            ],
            "usage": {"input_tokens": 3800, "output_tokens": 590}
        })
    }

    #[tokio::test]
    async fn the_second_lookup_comes_from_the_cache_with_no_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
                "Regulation 261/2004 Article 7 sets EUR 250, 400 or 600. The amount depends on the distance.",
            )))
            .expect(1)
            .mount(&server)
            .await;
        let client = MetaClient::new(server.uri(), "k");
        let mem = Memory::open_in_memory().unwrap();
        let first = lookup(&client, &mem, "eu261_delay", None, Lang::En, now()).await.unwrap();
        assert_eq!(first.outcome, Outcome::Hit);
        assert_eq!(first.query, query_for("eu261_delay", None));
        let tokens = client.usage_total();
        let second = lookup(&client, &mem, "eu261_delay", None, Lang::En, now() + Duration::days(3)).await.unwrap();
        assert_eq!(second.outcome, Outcome::Cache);
        assert_eq!(second.note, first.note);
        assert_eq!(client.usage_total(), tokens, "the cache costs no token");
        // The request carried only the fixed query.
        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
        assert_eq!(body["input"][0]["content"][0]["text"], query_for("eu261_delay", None).unwrap());
        assert_eq!(body["tools"][0]["type"], "web_search");
    }

    #[tokio::test]
    async fn an_old_cache_entry_asks_again_and_off_topic_asks_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
                "Regulation 261/2004 sets EUR 250, 400 or 600. It depends on the distance.",
            )))
            .expect(2)
            .mount(&server)
            .await;
        let client = MetaClient::new(server.uri(), "k");
        let mem = Memory::open_in_memory().unwrap();
        assert_eq!(lookup(&client, &mem, "eu261_delay", None, Lang::En, now()).await.unwrap().outcome, Outcome::Hit);
        let later = now() + Duration::days(LOOKUP_CACHE_DAYS + 1);
        assert_eq!(lookup(&client, &mem, "eu261_delay", None, Lang::En, later).await.unwrap().outcome, Outcome::Hit);
        let off = lookup(&client, &mem, "no_such_tip", None, Lang::En, later).await.unwrap();
        assert_eq!((off.outcome, off.query), (Outcome::None, None));
    }

    #[tokio::test]
    async fn a_blog_only_answer_gives_no_note_and_no_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "r", "status": "completed",
                "output": [{"type":"message","content":[{"type":"output_text","text":"EUR 600 for long flights, per a blog.",
                  "annotations":[{"type":"url_citation","url":"https://blog.example/x","title":"Blog"}]}]}],
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })))
            .mount(&server)
            .await;
        let client = MetaClient::new(server.uri(), "k");
        let mem = Memory::open_in_memory().unwrap();
        let l = lookup(&client, &mem, "eu261_delay", None, Lang::En, now()).await.unwrap();
        assert_eq!((l.outcome, l.note), (Outcome::None, None));
        assert!(cached(&mem, "eu261_delay", Lang::En, now()).unwrap().is_none());
    }

    /// Live: the six topics against the Meta API (about 6 web searches). `cargo test -p pip-core --lib
    /// live_lookups -- --ignored --nocapture`. Each topic needs an official citation; the second ask is the cache.
    #[tokio::test]
    #[ignore = "live Meta API, costs about 2 cents"]
    async fn live_lookups() {
        let client = MetaClient::from_env().expect("MODEL_API_KEY (.env)");
        let mem = Memory::open_in_memory().unwrap();
        let mut ok = 0;
        for t in TOPICS {
            let before = client.usage_total();
            let l = lookup(&client, &mem, t.tip_id, t.region, Lang::En, Local::now()).await.unwrap();
            let used = client.usage_total();
            let tokens = used.input_tokens + used.output_tokens - before.input_tokens - before.output_tokens;
            match &l.note {
                Some(n) => {
                    let number = n.text.chars().any(|c| c.is_ascii_digit());
                    ok += usize::from(number && n.citations.iter().all(|(u, _)| allowed_domain(u)));
                    println!(
                        "{} {} tokens={tokens} number={number} differs={} url={} | {}",
                        t.tip_id,
                        l.outcome.as_str(),
                        differs(t.tip_id, &n.text),
                        n.citations[0].0,
                        n.text
                    );
                }
                None => println!("{} none tokens={tokens}", t.tip_id),
            }
        }
        let again = lookup(&client, &mem, "eu261_delay", None, Lang::En, Local::now()).await.unwrap();
        println!("second eu261_delay: {}", again.outcome.as_str());
        println!("topics with a number and an official citation: {ok}/{}", TOPICS.len());
    }
}
