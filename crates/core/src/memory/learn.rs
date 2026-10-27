//! Learning memory: Pip keeps what helps next time, without an extra model call (SPEC §8).
//!
//! - After a run: a playbook per (site, task kind) with the URL, which field label took which profile key, the
//!   attach and send button labels, steps and seconds. Labels and keys only, never the typed values.
//! - After a letter: name, insurer, insurance number (source "letter:<id>"), never over what the user said.
//! - Facts have a source; a lower source never replaces a higher one: user > typed > letter > model.
//! - Passwords, PINs, codes and health details are never kept.

use anyhow::Result;

use super::db::{Memory, PLAYBOOK_MAX_FAILS, Playbook};
use crate::docs::{DocType, DocumentExtract};

/// Profile key: sites (hosts, or the demo fixtures folder) where the user said "Don't ask again" on a send card.
pub const PROFILE_TRUSTED_SEND: &str = "trusted_send_hosts";

/// Profile key: seconds Pip saved the user so far (sum of `by_hand_secs` over finished runs). Hidden from the
/// model (recall) and from "What Pip knows".
pub const PROFILE_SECONDS_SAVED: &str = "seconds_saved_total";

/// What remember_fact answers for a password or a health detail.
pub const SENSITIVE_REFUSAL: &str = "not saved: Pip does not keep passwords or health details";

/// One finished run, as the agent saw it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunSummary {
    /// Site key (host, or the demo fixtures folder). None = no playbook.
    pub site: Option<String>,
    /// "send_au", "send_letter", "look_screen", "text", …
    pub task_kind: String,
    /// The page Pip opened (without query or fragment).
    pub url: Option<String>,
    /// (field label, or the words for the user when there was no label; the full typed value)
    pub typed: Vec<(String, String)>,
    /// Label of the button that opened the file picker.
    pub attach_label: Option<String>,
    /// Label of the button that really sent (never one that was refused).
    pub send_label: Option<String>,
    pub steps: u32,
    pub seconds: u32,
    pub ok: bool,
}

/// What `learn_from_run` changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Learned {
    /// A playbook for this site and task did not exist before.
    pub playbook_new: bool,
    /// A playbook was written (new or updated).
    pub playbook_saved: bool,
    /// The playbook was dropped (failed too often).
    pub playbook_dropped: bool,
    /// Profile keys whose values went into the form.
    pub used_keys: Vec<String>,
    /// The playbook as it was before this run updated it (only when an existing one was saved again).
    pub before: Option<PriorRun>,
    /// What this task takes by hand (`by_hand_secs`); 0 for a failed run or one that typed, attached and sent
    /// nothing.
    pub by_hand_secs: u32,
    /// Seconds saved over all finished runs, this one included (profile `seconds_saved_total`).
    pub saved_total_secs: u64,
}

/// An earlier run on the same site and task: how often, how many steps, how many seconds (the last one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriorRun {
    pub runs: i64,
    pub steps: i64,
    pub seconds: i64,
}

/// Higher wins. "letter:12" counts as "letter".
pub fn source_rank(source: &str) -> u8 {
    match source.split(':').next().unwrap_or("") {
        "user" => 3,
        "typed" => 2,
        "letter" => 1,
        _ => 0,
    }
}

/// Keys Pip never keeps: passwords, PINs, TANs, codes, diagnoses, ICD codes, findings.
pub fn is_sensitive_key(key: &str) -> bool {
    let low = key.to_lowercase();
    if ["password", "passwort", "kennwort", "diagnos", "befund"].iter().any(|w| low.contains(w)) {
        return true;
    }
    let tokens: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    // A postcode or country code is an ordinary fact; any other code is a secret.
    let place = tokens.iter().any(|t| matches!(*t, "zip" | "post" | "postal" | "plz" | "country" | "area"));
    tokens.iter().any(|t| matches!(*t, "pin" | "tan" | "puk" | "otp" | "icd" | "2fa") || (*t == "code" && !place))
}

/// Values that are themselves a secret or a health detail ("Passwort: …", "Diagnose: …"). Narrower than the
/// key check, so a name like "Tan" or "Pin" stays a name.
pub fn is_sensitive_value(value: &str) -> bool {
    let low = value.to_lowercase();
    ["password", "passwort", "kennwort", "diagnos", "befund", "icd-10", "icd10"].iter().any(|w| low.contains(w))
}

/// Profile keys that decide where things go, or who the user is.
pub fn is_protected_key(key: &str) -> bool {
    let key = super::db::profile_key(key);
    [
        "url",
        "formular",
        "kontakt",
        "adress",
        "address",
        "mail",
        "iban",
        "konto",
        "insurer",
        "airline",
        "arbeitgeber",
        "employer",
        "versichert",
        "lang",
        "name",
    ]
    .iter()
    .any(|w| key.contains(w))
}

/// The honest estimate of what the same task takes a non-technical person by hand, in seconds. Fixed numbers,
/// no model call, so the "By hand this takes about …" line is the same every time:
/// - 120 s to find and open the right page (when Pip opened one: `run.url` is Some),
/// - 45 s per typed field (look up the value, type it, check it),
/// - 120 s when a file was attached (find the scan or photo, pick it in the file dialog),
/// - 30 s when something was sent (find the button, check, confirm).
///
/// 0 when nothing was typed, attached or sent (an answer or a look is not counted as saved time).
pub fn by_hand_secs(run: &RunSummary) -> u32 {
    let attach = run.attach_label.is_some();
    let send = run.send_label.is_some();
    if run.typed.is_empty() && !attach && !send {
        return 0;
    }
    let fields = u32::try_from(run.typed.len()).unwrap_or(u32::MAX);
    let mut secs = fields.saturating_mul(45);
    if run.url.is_some() {
        secs = secs.saturating_add(120);
    }
    if attach {
        secs = secs.saturating_add(120);
    }
    if send {
        secs = secs.saturating_add(30);
    }
    secs
}

/// Adds `secs` to the running total in the profile; returns the new total.
fn add_saved_secs(mem: &Memory, secs: u32) -> Result<u64> {
    let old = mem.profile_get(PROFILE_SECONDS_SAVED)?.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    if secs == 0 {
        return Ok(old);
    }
    let total = old.saturating_add(u64::from(secs));
    mem.profile_set(PROFILE_SECONDS_SAVED, &total.to_string())?;
    Ok(total)
}

/// Saves `key = value` from `source` unless the stored value came from a higher source (then false).
/// The same value again only lifts the source when the new one is higher.
pub fn set_fact(mem: &Memory, key: &str, value: &str, source: &str) -> Result<bool> {
    if let Some(old) = mem.profile_meta(key)? {
        if source_rank(&old.source) > source_rank(source) {
            return Ok(false);
        }
        if old.value == value && source_rank(&old.source) == source_rank(source) {
            return Ok(true);
        }
    }
    let confidence = match source_rank(source) {
        3 | 2 => 1.0,
        1 => 0.8,
        _ => 0.6,
    };
    mem.profile_set_with(key, value, source, confidence)?;
    Ok(true)
}

/// After a run. Writes a playbook only when the run went well and something was typed or sent; a failed run
/// counts against an existing playbook (dropped after two failures in a row). Used profile keys are touched.
/// A run that went well adds its `by_hand_secs` to the profile total `seconds_saved_total`.
pub fn learn_from_run(mem: &Memory, run: &RunSummary) -> Result<Learned> {
    let mut learned = Learned::default();
    // Field label -> the profile key whose value Pip typed there (values themselves are not stored).
    let profile = mem.profile_meta_all()?;
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut fields: Vec<(String, String)> = Vec::new();
    for (label, value) in &run.typed {
        let v = norm(value);
        if v.is_empty() || label.trim().is_empty() {
            continue;
        }
        // The fact used most wins when two keys hold the same value ("name" vs "full_name" differ anyway).
        if let Some(f) =
            profile.iter().find(|f| norm(&f.value) == v && !is_sensitive_key(&f.key) && f.key != PROFILE_SECONDS_SAVED)
        {
            fields.retain(|(l, _)| l != label);
            fields.push((label.trim().to_string(), f.key.clone()));
            if !learned.used_keys.contains(&f.key) {
                learned.used_keys.push(f.key.clone());
            }
        }
    }
    if !learned.used_keys.is_empty() {
        mem.profile_touch(&learned.used_keys)?;
    }
    if run.ok {
        learned.by_hand_secs = by_hand_secs(run);
        learned.saved_total_secs = add_saved_secs(mem, learned.by_hand_secs)?;
    }
    let Some(site) = run.site.as_deref().filter(|s| !s.trim().is_empty()) else { return Ok(learned) };
    let kind = if run.task_kind.trim().is_empty() { "text" } else { run.task_kind.as_str() };
    let old = mem.playbook_get(site, kind)?;
    if !run.ok {
        if let Some(mut p) = old {
            p.failed_since_ok += 1;
            if p.failed_since_ok >= PLAYBOOK_MAX_FAILS {
                mem.playbook_delete(site, kind)?;
                learned.playbook_dropped = true;
            } else {
                mem.playbook_upsert(&p)?;
            }
        }
        return Ok(learned);
    }
    if run.typed.is_empty() && run.send_label.is_none() {
        return Ok(learned);
    }
    let url = run.url.as_deref().map(strip_query).unwrap_or_default();
    let p = match old {
        Some(mut p) => {
            learned.before = Some(PriorRun { runs: p.runs, steps: p.steps, seconds: p.seconds });
            p.runs += 1;
            p.failed_since_ok = 0;
            if !url.is_empty() {
                p.url = url;
            }
            if !fields.is_empty() {
                p.fields = fields;
            }
            p.attach_label = run.attach_label.clone().or(p.attach_label);
            p.send_label = run.send_label.clone().or(p.send_label);
            p.steps = i64::from(run.steps);
            p.seconds = i64::from(run.seconds);
            p
        }
        None => {
            learned.playbook_new = true;
            Playbook {
                site: site.to_string(),
                task: kind.to_string(),
                url,
                fields,
                attach_label: run.attach_label.clone(),
                send_label: run.send_label.clone(),
                steps: i64::from(run.steps),
                seconds: i64::from(run.seconds),
                runs: 1,
                failed_since_ok: 0,
                updated_at: chrono::Local::now(),
            }
        }
    };
    mem.playbook_upsert(&p)?;
    learned.playbook_saved = true;
    Ok(learned)
}

/// After a new letter: the person's full name, the insurer and the insurance number (source "letter:<id>").
/// Never replaces what the user said or confirmed. Returns the keys that were saved.
pub fn learn_from_letter(mem: &Memory, doc_id: i64, x: &DocumentExtract) -> Result<Vec<String>> {
    learn_from_letter_text(mem, doc_id, x, "")
}

/// `learn_from_letter`, plus from an airline's notice (`flight`): the airline (`airline`) and, when the letter's
/// text or summary names a claim page (an http(s) link near "claim", "compensation", "refund", "Entschädigung"
/// or "Erstattung"), that page as `airline_claim_form`. Other links are never kept.
pub fn learn_from_letter_text(mem: &Memory, doc_id: i64, x: &DocumentExtract, text: &str) -> Result<Vec<String>> {
    let source = format!("letter:{doc_id}");
    let insurer = crate::docs::pipeline::insurer_from(x);
    let mut facts: Vec<(&str, String)> = Vec::new();
    if let Some(p) = x.person.as_deref().map(str::trim).filter(|p| !p.is_empty() && p.chars().count() <= 80) {
        facts.push(("full_name", p.to_string()));
    } else if let Some(r) = person_name(&x.recipient).filter(|r| Some(r) != insurer.as_ref())
        && mem.profile_get("full_name")?.is_none()
        && mem.profile_get("name")?.is_none_or(|n| n.trim().is_empty())
    {
        // No person on the letter: the recipient is the user when it reads like a person's name ("Erika
        // Beispiel", "Frau Erika Beispiel"), never an office or a firm.
        facts.push(("full_name", r));
    }
    if let Some(name) = &insurer {
        facts.push(("insurer", name.clone()));
    }
    let from_insurer = x.doc_type == DocType::AuBescheinigung || insurer.is_some();
    if from_insurer && let Some(n) = x.reference.as_deref().and_then(insurance_number) {
        facts.push(("versichertennummer", n));
    }
    if let Some(f) = &x.flight {
        let airline = f.airline.trim();
        let airline = if airline.is_empty() { x.sender.trim() } else { airline };
        if !airline.is_empty() && airline.chars().count() <= 60 {
            facts.push(("airline", airline.to_string()));
        }
        let all = format!("{text}\n{}\n{}", x.summary_simple, x.requests.join("\n"));
        if let Some(url) = claim_url(&all) {
            facts.push(("airline_claim_form", url));
        }
    }
    let mut saved = Vec::new();
    for (key, value) in facts {
        // A letter only fills gaps or replaces a guess; a newer letter does not move the insurer elsewhere.
        if let Some(old) = mem.profile_meta(key)?
            && source_rank(&old.source) >= source_rank("letter")
        {
            continue;
        }
        if set_fact(mem, key, &value, &source)? {
            saved.push(key.to_string());
        }
    }
    Ok(saved)
}

/// Words in a recipient that make it an organisation, not a person.
const ORG_WORDS: &[&str] = &[
    "health",
    "kasse",
    "krankenkasse",
    "versicherung",
    "insurance",
    "gmbh",
    "ag",
    "kg",
    "e.v.",
    "ev",
    "amt",
    "stadt",
    "gemeinde",
    "air",
    "airline",
    "airlines",
    "airways",
    "bank",
    "sparkasse",
    "university",
    "universität",
    "hochschule",
    "praxis",
    "dr",
    "ltd",
    "inc",
    "company",
    "service",
    "center",
    "zentrum",
    "team",
    "aok",
    "tk",
    "barmer",
    "dak",
    "office",
    "büro",
    "verwaltung",
    "behörde",
    "jobcenter",
    "agentur",
    "finanzamt",
    "customer",
];

/// "Frau Erika Beispiel" / "Erika Beispiel" -> "Erika Beispiel": two to four capitalised words, no digits, no
/// organisation words. None for "Evergreen Health" or an address line.
fn person_name(recipient: &str) -> Option<String> {
    let line = recipient.lines().next().unwrap_or("").trim();
    let words: Vec<&str> = line
        .split_whitespace()
        .filter(|w| !matches!(w.to_lowercase().trim_end_matches('.'), "frau" | "herr" | "herrn" | "mr" | "mrs" | "ms"))
        .collect();
    let ok = (2..=4).contains(&words.len())
        && line.chars().count() <= 60
        && !line.chars().any(|c| c.is_ascii_digit())
        && words.iter().all(|w| w.chars().next().is_some_and(char::is_uppercase))
        && !words
            .iter()
            .any(|w| ORG_WORDS.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric() && c != '.')));
    ok.then(|| words.join(" "))
}

/// Words that mark a link as the airline's claim page.
const CLAIM_WORDS: &[&str] =
    &["claim", "compensation", "refund", "entschädigung", "entschaedigung", "erstattung", "antrag"];

/// The first http(s) link that is a claim page: the link itself or the words before it in its sentence (at most
/// 120 characters, no other link) name a claim.
/// Without query or fragment; never a file or other scheme.
fn claim_url(text: &str) -> Option<String> {
    let low = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = low[from..].find("http").map(|i| from + i) {
        from = at + 4;
        let rest = &text[at..];
        if !(rest.to_lowercase().starts_with("https://") || rest.to_lowercase().starts_with("http://")) {
            continue;
        }
        let end =
            rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | ')' | ']')).unwrap_or(rest.len());
        let url = strip_query(rest[..end].trim_end_matches(['.', ',', ';', ':']));
        let mut start = at.saturating_sub(120);
        while !low.is_char_boundary(start) {
            start -= 1;
        }
        // Only the words of this sentence before the link, never another link ("claim.html … https://news").
        let mut before = &low[start..at];
        if let Some(i) = before.rfind(['.', '\n']).filter(|i| before[i + 1..].starts_with([' ', '\n'])) {
            before = &before[i + 1..];
        }
        if let Some(i) = before.rfind("://") {
            before = before[i..].split_once(char::is_whitespace).map_or("", |(_, b)| b);
        }
        let near = format!("{before} {}", url.to_lowercase());
        if url.len() > "https://".len() + 3 && CLAIM_WORDS.iter().any(|w| near.contains(w)) {
            return Some(url);
        }
    }
    None
}

/// German health insurance number (one letter + nine digits), spaces removed. None for file numbers.
fn insurance_number(reference: &str) -> Option<String> {
    let n: String = reference.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let mut chars = n.chars();
    let ok = n.chars().count() == 10
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_digit());
    ok.then_some(n)
}

/// Sites where the user ticked "Don't ask again".
pub fn trusted_sites(mem: &Memory) -> Vec<String> {
    let list = mem.profile_get(PROFILE_TRUSTED_SEND).ok().flatten().unwrap_or_default();
    list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// "Forget": Pip asks again before sending on this site. True if it was trusted.
pub fn untrust_site(mem: &Memory, site: &str) -> Result<bool> {
    let list = trusted_sites(mem);
    if !list.iter().any(|s| s == site) {
        return Ok(false);
    }
    let rest: Vec<String> = list.into_iter().filter(|s| s != site).collect();
    if rest.is_empty() {
        mem.profile_delete(PROFILE_TRUSTED_SEND)?;
    } else {
        mem.profile_set(PROFILE_TRUSTED_SEND, &rest.join(","))?;
    }
    Ok(true)
}

/// "https://x.example/form?name=Erika#top" -> "https://x.example/form" (no personal data in a stored link).
pub fn strip_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::{AuInfo, AuKind};
    use chrono::NaiveDate;

    fn mem() -> Memory {
        let m = Memory::open_in_memory().unwrap();
        m.profile_set("full_name", "Erika Beispiel").unwrap();
        m.profile_set("versichertennummer", "G123456789").unwrap();
        m
    }

    fn run(ok: bool) -> RunSummary {
        RunSummary {
            site: Some("www.evergreen-health.example".into()),
            task_kind: "send_au".into(),
            url: Some("https://www.evergreen-health.example/kontakt?name=Erika".into()),
            typed: vec![
                ("Name".into(), "Erika  Beispiel".into()),
                ("Versichertennummer".into(), "G123456789".into()),
                ("Nachricht".into(), "Anbei meine AU".into()),
            ],
            attach_label: Some("Datei auswählen".into()),
            send_label: Some("Absenden".into()),
            steps: 7,
            seconds: 41,
            ok,
        }
    }

    #[test]
    fn playbook_from_a_run_keeps_labels_and_keys_only() {
        let m = mem();
        let l = learn_from_run(&m, &run(true)).unwrap();
        assert!(l.playbook_new && l.playbook_saved);
        assert_eq!(l.used_keys, ["full_name", "versichertennummer"]);
        let p = m.playbook_get("www.evergreen-health.example", "send_au").unwrap().unwrap();
        assert_eq!(p.url, "https://www.evergreen-health.example/kontakt", "no query (personal data) in the link");
        assert_eq!(
            p.fields,
            [("Name".to_string(), "full_name".to_string()), ("Versichertennummer".into(), "versichertennummer".into())]
        );
        assert_eq!((p.steps, p.seconds, p.runs), (7, 41, 1));
        assert_eq!(p.send_label.as_deref(), Some("Absenden"));
        let stored = serde_json::to_string(&p).unwrap();
        assert!(!stored.contains("Erika") && !stored.contains("G123") && !stored.contains("Anbei"), "{stored}");
        assert_eq!(m.profile_meta("full_name").unwrap().unwrap().uses, 1);
    }

    #[test]
    fn runs_count_up_and_failures_drop_the_playbook() {
        let m = mem();
        assert_eq!(learn_from_run(&m, &run(true)).unwrap().before, None);
        let mut second = run(true);
        second.steps = 3;
        let l = learn_from_run(&m, &second).unwrap();
        assert!(!l.playbook_new && l.playbook_saved);
        assert_eq!(l.before, Some(PriorRun { runs: 1, steps: 7, seconds: 41 }), "values before this run");
        let p = m.playbook_get("www.evergreen-health.example", "send_au").unwrap().unwrap();
        assert_eq!((p.runs, p.steps), (2, 3));
        learn_from_run(&m, &run(false)).unwrap();
        assert_eq!(m.playbook_get("www.evergreen-health.example", "send_au").unwrap().unwrap().failed_since_ok, 1);
        // A success resets the count.
        learn_from_run(&m, &run(true)).unwrap();
        assert_eq!(m.playbook_get("www.evergreen-health.example", "send_au").unwrap().unwrap().failed_since_ok, 0);
        learn_from_run(&m, &run(false)).unwrap();
        let l = learn_from_run(&m, &run(false)).unwrap();
        assert!(l.playbook_dropped);
        assert!(m.playbook_get("www.evergreen-health.example", "send_au").unwrap().is_none());
        // A failed first run writes nothing; a run that typed and sent nothing writes nothing.
        assert!(!learn_from_run(&m, &run(false)).unwrap().playbook_saved);
        let empty = RunSummary { site: Some("x.example".into()), ok: true, ..Default::default() };
        assert!(!learn_from_run(&m, &empty).unwrap().playbook_saved);
    }

    fn au_letter() -> DocumentExtract {
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        DocumentExtract {
            doc_type: DocType::AuBescheinigung,
            sender: "Dr. med. Max Beispiel".into(),
            recipient: "Evergreen Health".into(),
            date: Some(d(10, 20)),
            reference: Some("H 987654321".into()),
            person: Some("Erika Muster".into()),
            summary_simple: String::new(),
            requests: vec![],
            deadlines: vec![],
            au: Some(AuInfo { from: d(10, 20), until: d(10, 30), first_or_follow: AuKind::Erst, issued: None }),
            flight: None,
            consequences_if_ignored: None,
            confidence: 0.9,
        }
    }

    #[test]
    fn a_letter_fills_gaps_but_never_overwrites_the_user() {
        let m = mem(); // full_name and versichertennummer came from the user
        let saved = learn_from_letter(&m, 4, &au_letter()).unwrap();
        assert_eq!(saved, ["insurer"]);
        assert_eq!(m.profile_get("full_name").unwrap().as_deref(), Some("Erika Beispiel"));
        assert_eq!(m.profile_meta("insurer").unwrap().unwrap().source, "letter:4");

        let fresh = Memory::open_in_memory().unwrap();
        fresh.profile_set_with("full_name", "E. B.", "model", 0.6).unwrap();
        let saved = learn_from_letter(&fresh, 5, &au_letter()).unwrap();
        assert_eq!(saved, ["full_name", "insurer", "versichertennummer"]);
        assert_eq!(fresh.profile_get("versichertennummer").unwrap().as_deref(), Some("H987654321"));
        // A later letter from another insurer does not move the insurer.
        let mut other = au_letter();
        other.recipient = "AOK Nordwest".into();
        assert!(learn_from_letter(&fresh, 6, &other).unwrap().is_empty());
        assert_eq!(fresh.profile_get("insurer").unwrap().as_deref(), Some("Evergreen Health"));
        // A file number is not an insurance number.
        let mut bescheid = au_letter();
        bescheid.doc_type = DocType::Bescheid;
        bescheid.sender = "Stadt Beispielstadt".into();
        bescheid.reference = Some("50.2-WG-2026-04711".into());
        let m2 = Memory::open_in_memory().unwrap();
        assert_eq!(learn_from_letter(&m2, 7, &bescheid).unwrap(), ["full_name"]);
    }

    #[test]
    fn the_recipient_gives_the_name_when_the_letter_names_no_person() {
        let m = Memory::open_in_memory().unwrap();
        let mut x = au_letter();
        x.person = None;
        x.recipient = "Frau Erika Beispiel".into();
        assert!(learn_from_letter(&m, 7, &x).unwrap().contains(&"full_name".to_string()));
        let f = m.profile_meta("full_name").unwrap().unwrap();
        assert_eq!((f.value.as_str(), f.source.as_str()), ("Erika Beispiel", "letter:7"));
        // An office or a firm is never the user's name; nor does a letter replace a name the user gave.
        let m = Memory::open_in_memory().unwrap();
        x.recipient = "Evergreen Health".into();
        learn_from_letter(&m, 8, &x).unwrap();
        assert_eq!(m.profile_get("full_name").unwrap(), None);
        set_fact(&m, "full_name", "Erika B.", "user").unwrap();
        x.recipient = "Erika Beispiel".into();
        learn_from_letter(&m, 9, &x).unwrap();
        assert_eq!(m.profile_get("full_name").unwrap().as_deref(), Some("Erika B."));
        assert_eq!(person_name("Stadt Beispielstadt Amt für Wohnen"), None);
        assert_eq!(person_name("Lindenweg 7"), None);
        assert_eq!(person_name("Herr Max Mustermann").as_deref(), Some("Max Mustermann"));
    }

    #[test]
    fn an_airline_notice_teaches_the_airline_and_its_claim_page() {
        use crate::docs::extract::FlightInfo;
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let mut x = au_letter();
        x.doc_type = DocType::Brief;
        x.sender = "Beispiel Air Customer Care".into();
        x.reference = Some("BX7Q2K".into());
        x.au = None;
        x.flight = Some(FlightInfo {
            airline: "Beispiel Air".into(),
            flight_no: "BX 2041".into(),
            date: d(10, 3),
            from: "DUS".into(),
            to: "TFS".into(),
            delay_minutes: Some(250),
            cancelled: false,
            notice_days: None,
            booking_ref: Some("BX7Q2K".into()),
        });
        let text = "We are sorry. Our newsletter: https://news.beispiel-air.example/offers. To claim compensation, \
                    visit https://www.beispiel-air.example/claims?ref=BX7Q2K.";
        let m = Memory::open_in_memory().unwrap();
        let saved = learn_from_letter_text(&m, 9, &x, text).unwrap();
        assert_eq!(saved, ["full_name", "airline", "airline_claim_form"]);
        assert_eq!(m.profile_get("airline").unwrap().as_deref(), Some("Beispiel Air"));
        assert_eq!(
            m.profile_get("airline_claim_form").unwrap().as_deref(),
            Some("https://www.beispiel-air.example/claims"),
            "the claim link, without the booking code"
        );
        assert!(m.profile_get("versichertennummer").unwrap().is_none(), "a booking code is no insurance number");
        // Never over what the user said; a link without a claim word or a file link is not kept.
        let m = Memory::open_in_memory().unwrap();
        m.profile_set_with("airline", "Other Air", "user", 1.0).unwrap();
        let saved = learn_from_letter_text(&m, 9, &x, "See file:///C:/claim.html or https://beispiel-air.example/news")
            .unwrap();
        assert_eq!(saved, ["full_name"]);
        assert_eq!(m.profile_get("airline").unwrap().as_deref(), Some("Other Air"));
        assert!(m.profile_get("airline_claim_form").unwrap().is_none());
        assert!(is_protected_key("airline") && is_protected_key("airline_claim_form"));
    }

    #[test]
    fn source_order_and_sensitive_keys() {
        let m = Memory::open_in_memory().unwrap();
        assert!(set_fact(&m, "hobby", "Garten", "model").unwrap());
        assert!(set_fact(&m, "hobby", "Lesen", "letter:1").unwrap());
        assert!(set_fact(&m, "hobby", "Wandern", "user").unwrap());
        assert!(!set_fact(&m, "hobby", "Kochen", "typed").unwrap());
        assert!(!set_fact(&m, "hobby", "Kochen", "model").unwrap());
        assert_eq!(m.profile_get("hobby").unwrap().as_deref(), Some("Wandern"));
        for k in ["password", "bank_pin", "Passwort", "tan", "sms_code", "diagnose", "icd", "befund_mrt"] {
            assert!(is_sensitive_key(k), "{k}");
        }
        for k in ["zip_code", "postcode", "full_name", "bundesland", "standard_tarif", "shipping_address"] {
            assert!(!is_sensitive_key(k), "{k}");
        }
        assert!(is_sensitive_value("Diagnose: Grippe") && !is_sensitive_value("Tan Nguyen"));
    }

    #[test]
    fn by_hand_estimate_follows_the_documented_formula() {
        let nothing = RunSummary { ok: true, ..Default::default() };
        assert_eq!(by_hand_secs(&nothing), 0, "no page, nothing typed, attached or sent");
        let only_page = RunSummary { url: Some("https://x.example".into()), ok: true, ..Default::default() };
        assert_eq!(by_hand_secs(&only_page), 0, "opening a page alone saves nothing");
        let mut r = run(true);
        r.typed.truncate(2);
        assert_eq!(by_hand_secs(&r), 120 + 90 + 120 + 30);
        r.url = None;
        r.attach_label = None;
        assert_eq!(by_hand_secs(&r), 90 + 30);
    }

    #[test]
    fn saved_time_adds_up_over_finished_runs_only() {
        let m = mem();
        let per_run = u64::from(by_hand_secs(&run(true)));
        assert_eq!(per_run, 120 + 3 * 45 + 120 + 30);
        let l = learn_from_run(&m, &run(true)).unwrap();
        assert_eq!((u64::from(l.by_hand_secs), l.saved_total_secs), (per_run, per_run));
        let l = learn_from_run(&m, &run(true)).unwrap();
        assert_eq!(l.saved_total_secs, 2 * per_run);
        let stored = || m.profile_get(PROFILE_SECONDS_SAVED).unwrap().unwrap().parse::<u64>().unwrap();
        assert_eq!(stored(), 2 * per_run);
        let l = learn_from_run(&m, &run(false)).unwrap();
        assert_eq!((l.by_hand_secs, l.saved_total_secs), (0, 0), "a failed run claims nothing");
        assert_eq!(stored(), 2 * per_run, "a failed run leaves the total alone");
        // A finished run without a site (no playbook) still counts.
        let mut no_site = run(true);
        no_site.site = None;
        assert_eq!(learn_from_run(&m, &no_site).unwrap().saved_total_secs, 3 * per_run);
    }

    #[test]
    fn forgetting_a_trusted_site() {
        let m = Memory::open_in_memory().unwrap();
        m.profile_set(PROFILE_TRUSTED_SEND, "a.example,b.example").unwrap();
        assert!(untrust_site(&m, "a.example").unwrap());
        assert_eq!(trusted_sites(&m), ["b.example"]);
        assert!(!untrust_site(&m, "a.example").unwrap());
        assert!(untrust_site(&m, "b.example").unwrap());
        assert!(m.profile_get(PROFILE_TRUSTED_SEND).unwrap().is_none());
    }
}
