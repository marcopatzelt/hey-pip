//! `read` (one letter through the real pipeline) and `rules` (deadlines + reminders at a given time).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::{DocError, DocPipeline, DocumentExtract};
use pip_core::events::UiEvent;
use pip_core::memory::{Deadline, Memory, NewDeadline};
use pip_core::meta::{MetaClient, OpenWeightsClient};
use pip_core::rules::{RuleEngine, default_reminders, local_at};
use serde_json::Value;

use crate::{Ctx, expected};

/// The demo letters `rules` loads when none are given.
pub const DEMO_SET: &[&str] = &[
    "au_erika_erst.expected.json",
    "insurer_letter.expected.json",
    "bescheid_wohngeld.expected.json",
    "uni_rueckmeldung.expected.json",
];

pub async fn read(ctx: &Ctx, file: &Path, crop: Option<&Path>) -> Result<()> {
    // Private mode (PRIVATE_MODEL_BASE): a Meta open-weights model reads the letter; the cloud
    // client is never called for it, so a missing MODEL_API_KEY does not matter there.
    let private = OpenWeightsClient::from_config(&ctx.config).map(Arc::new);
    let meta = match (ctx.meta(), &private) {
        (Ok(m), _) => m,
        (Err(_), Some(_)) => Arc::new(MetaClient::new("http://127.0.0.1:9", "unused")),
        (Err(e), None) => return Err(e),
    };
    let rules = Arc::new(RuleEngine::new(ctx.memory.clone(), ctx.clock.clone()));
    let mut pipeline = DocPipeline::new(meta.clone(), ctx.memory.clone(), rules);
    if let Some(p) = &private {
        println!(
            "Private mode: reading with {} at {} (Meta open weights; the letter stays local, SAM crop skipped)",
            p.model(),
            p.base()
        );
        pipeline = pipeline.with_private(p.clone());
    }
    if let Some(out) = crop.filter(|_| private.is_some()) {
        println!("--crop skipped: in private mode the photo is not sent to SAM ({}).", out.display());
    } else if let Some(out) = crop {
        let bytes = std::fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
        let png = pipeline.crop_sheet(&bytes).await?;
        write_file(out, &png)?;
        println!("SAM 3.1 sheet crop saved: {}", out.display());
    }
    let started = Instant::now();
    let r = match pipeline.ingest(file, ctx.lang).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(d) = e.downcast_ref::<DocError>() {
                println!("Pip says: {}", d.user_text(ctx.lang));
            }
            return Err(e);
        }
    };
    let secs = started.elapsed().as_secs_f32();
    println!("== {} ({secs:.1} s{})", file.display(), if r.duplicate { ", already known" } else { "" });
    println!("Pip says: {}", r.bubble);
    println!("\nDocumentExtract:\n{}", serde_json::to_string_pretty(&r.extract)?);
    print_deadlines(&r.deadlines);
    // Not part of Pip's bubble: the rule with its source, said on its own.
    if let Some(tip) = &r.tip {
        println!("\n{} (source: {}, {})", tip.text, tip.source, tip.url);
    }
    if let Some(xp) = expected::path_for(file) {
        let x = expected::load(&xp)?;
        let mut checks = expected::check(&x, &r.extract, &r.deadlines);
        checks.extend(extra_checks(&x, &r.extract, r.tip.as_ref().map(|t| t.id)));
        let ok = checks.iter().filter(|c| c.ok).count();
        println!("\nCheck against {}: {ok}/{} fields", xp.display(), checks.len());
        for c in &checks {
            println!("  [{}] {}: {}", if c.ok { "ok" } else { "XX" }, c.name, c.got);
        }
    }
    if r.private {
        println!("\nTokens: n/a (private mode)");
    } else {
        let u = meta.usage_total();
        println!("\nTokens: {} in, {} out", u.input_tokens, u.output_tokens);
    }
    Ok(())
}

pub fn print_deadlines(deadlines: &[Deadline]) {
    if deadlines.is_empty() {
        println!("\nDeadlines: none");
        return;
    }
    println!("\nDeadlines:");
    for d in deadlines {
        let times: Vec<String> = d.remind_at.iter().map(|t| t.format("%a %d.%m. %H:%M").to_string()).collect();
        println!(
            "  {} [{}] {} | reminders: {}",
            d.due.format("%a %d.%m.%Y"),
            d.rule_id.as_deref().unwrap_or("-"),
            d.title,
            if times.is_empty() { "-".into() } else { times.join(", ") }
        );
        if !d.why.is_empty() {
            println!("      why: {}", d.why);
        }
    }
}

/// The checks of expected.json that `expected::check` does not make: the `flight` block (flight_no, date, from,
/// to, delay_minutes, cancelled, notice_days; a key the file leaves out is not checked; `"flight": null` means the
/// letter must give none, like `au`) and `tip` (the id of the "Good to know" rule the letter must give, null =
/// none). A file without these keys adds no check. The extract is read as JSON, so a field the core does not have
/// yet reads as missing (a failed check, not a build error).
pub fn extra_checks(x: &Value, extract: &DocumentExtract, tip: Option<&str>) -> Vec<expected::Check> {
    let mut checks = Vec::new();
    if let Some(want) = x.get("flight") {
        let got = serde_json::to_value(extract).map(|v| v["flight"].clone()).unwrap_or_default();
        match (want.is_object(), got.is_object()) {
            (true, true) => {
                for (name, key) in FLIGHT_KEYS {
                    let Some(w) = want.get(*key) else { continue };
                    let g = &got[*key];
                    checks.push(expected::Check { name, ok: same_flight_value(key, w, g), got: g.to_string() });
                }
            }
            (false, false) => checks.push(expected::Check { name: "flight", ok: true, got: "none".into() }),
            (_, _) => checks.push(expected::Check { name: "flight", ok: false, got: got.to_string() }),
        }
    }
    if let Some(want) = x.get("tip") {
        checks.push(expected::Check { name: "tip", ok: tip == want.as_str(), got: tip.unwrap_or("none").to_string() });
    }
    checks
}

/// The checked keys of the `flight` block with their check names.
const FLIGHT_KEYS: &[(&str, &str)] = &[
    ("flight.flight_no", "flight_no"),
    ("flight.date", "date"),
    ("flight.from", "from"),
    ("flight.to", "to"),
    ("flight.delay_minutes", "delay_minutes"),
    ("flight.cancelled", "cancelled"),
    ("flight.notice_days", "notice_days"),
];

/// Flight numbers and airports match without spaces and case ("BX2041" = "BX 2041", "dus" = "DUS"); an airport
/// may also be printed with its city ("Düsseldorf (DUS)" holds "DUS"). Numbers, dates and flags match exactly.
fn same_flight_value(key: &str, want: &Value, got: &Value) -> bool {
    let norm =
        |v: &Value| v.as_str().map(|s| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase());
    match key {
        "flight_no" => norm(want).is_some() && norm(want) == norm(got),
        "from" | "to" => match (norm(want), norm(got)) {
            (Some(w), Some(g)) => !w.is_empty() && (g == w || g.contains(&format!("({w})"))),
            _ => false,
        },
        _ => want == got,
    }
}

/// Store a fixture letter without the API: the extract comes from its expected.json.
/// The letter counts as read the day after its date at 12:00 (never after `latest`).
pub fn seed_letter(
    memory: &Arc<Memory>,
    rules: &RuleEngine,
    clock: &FixedClock,
    expected_path: &Path,
    latest: DateTime<Local>,
) -> Result<(i64, DocumentExtract, Vec<Deadline>)> {
    let x = expected::load(expected_path)?;
    seed_document(memory, rules, clock, &x, &expected::letter_file(expected_path), latest)
}

/// Store one document from an expected.json-shaped value (an eval task's inline `documents` entry, or a letter's
/// expected.json) under `file`, without the API. `summary` (optional) replaces the stand-in's one-line summary, so
/// a seeded flight notice tells Pip the flight, the route and the delay. A `rule_deadlines` entry the rules did not
/// create from the stand-in is added as it stands (with reminders), so "deadline #1" exists in a dry run.
pub fn seed_document(
    memory: &Arc<Memory>,
    rules: &RuleEngine,
    clock: &FixedClock,
    x: &Value,
    file: &Path,
    latest: DateTime<Local>,
) -> Result<(i64, DocumentExtract, Vec<Deadline>)> {
    let mut extract = expected::to_extract(x)?;
    if let Some(summary) = x["summary"].as_str().filter(|s| !s.trim().is_empty()) {
        extract.summary_simple = summary.to_string();
    }
    let read_at = extract.date.map(|d| local_at(d, 12, 0) + Duration::days(1)).unwrap_or(latest).min(latest);
    clock.set(read_at);
    let sha = format!("fixture:{}", file.file_name().and_then(|s| s.to_str()).unwrap_or_default());
    let id = memory.insert_document(file, &sha, &extract, &extract.summary_simple)?;
    let mut deadlines = rules.apply_document(id, &extract)?;
    for rd in x["rule_deadlines"].as_array().into_iter().flatten() {
        let (Some(rule), Some(due)) =
            (rd["rule_id"].as_str(), rd["due"].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()))
        else {
            continue;
        };
        if memory.deadlines_for_document(id)?.iter().any(|d| d.rule_id.as_deref() == Some(rule)) {
            continue;
        }
        let flight = &x["flight"];
        let title = match flight["flight_no"].as_str() {
            Some(no) => format!(
                "Claim compensation from {} for flight {no} on {}",
                extract.sender,
                flight["date"].as_str().unwrap_or_default()
            ),
            None => format!("{rule} ({})", extract.sender),
        };
        let did = memory.add_deadline(&NewDeadline {
            title,
            due,
            rule_id: Some(rule.to_string()),
            doc_id: Some(id),
            remind_at: default_reminders(due, read_at),
            why: String::new(),
            source: "fixture".into(),
        })?;
        deadlines.extend(memory.deadline(did)?);
    }
    Ok((id, extract, deadlines))
}

pub fn rules(ctx: &Ctx, letters: &[PathBuf]) -> Result<()> {
    let clock = Arc::new(FixedClock::new(ctx.now));
    let engine = RuleEngine::new(ctx.memory.clone(), clock.clone() as Arc<dyn Clock>);
    let letters: Vec<PathBuf> = if letters.is_empty() {
        DEMO_SET.iter().map(|f| ctx.fixture(&format!("letters/{f}"))).collect()
    } else {
        letters.to_vec()
    };
    println!("Letters (from expected.json, no API call):");
    for p in &letters {
        let (id, x, _) = seed_letter(&ctx.memory, &engine, &clock, p, ctx.now)?;
        println!(
            "  #{id} {} {} from {} (read {})",
            x.doc_type.as_str(),
            x.date.map(|d| d.to_string()).unwrap_or_default(),
            x.sender,
            clock.now().format("%d.%m. %H:%M")
        );
    }
    clock.set(ctx.now);
    print_deadlines(&ctx.memory.open_deadlines()?);
    println!("\nNow: {}", ctx.now.format("%A %d.%m.%Y %H:%M"));
    let fired = engine.tick()?;
    if fired.is_empty() {
        println!("No reminder is due now.");
    }
    for r in &fired {
        if let UiEvent::Reminder { text, why, buttons, .. } = engine.reminder_event(r, ctx.lang) {
            println!("\nPip reminds (for \"{}\", due {}):", r.deadline.title, r.deadline.due);
            println!("  {text}");
            let labels: Vec<String> = buttons.iter().map(|b| format!("[{}]", b.label)).collect();
            println!("  {}", labels.join(" "));
            println!("  Warum? {why}");
        }
    }
    let again = engine.tick()?;
    println!("\nSecond tick at the same time fires {} reminder(s) (never twice).", again.len());
    Ok(())
}

pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters")
    }

    const FLIGHT: [&str; 4] =
        ["flight_delay_bx2041", "flight_delay_short", "flight_cancel_notice", "flight_cancel_late"];

    #[test]
    fn the_flight_letters_check_their_flight_block_and_tip() {
        for name in FLIGHT {
            let xp = letters().join(format!("{name}.expected.json"));
            let x = expected::load(&xp).unwrap();
            assert!(expected::letter_file(&xp).is_file(), "{name}");
            let mut e = expected::to_extract(&x).unwrap();
            let tip = x["tip"].as_str();
            let checks = extra_checks(&x, &e, tip);
            let flight: Vec<&str> = checks.iter().map(|c| c.name).filter(|n| n.starts_with("flight")).collect();
            assert!(flight.len() >= 6, "{name}: {flight:?}");
            for c in &checks {
                assert!(c.ok, "{name}: {} = {}", c.name, c.got);
            }
            // A wrong tip and a wrong flight number are misses.
            assert!(extra_checks(&x, &e, Some("au_gap")).iter().any(|c| c.name == "tip" && !c.ok));
            let mut v = serde_json::to_value(&e).unwrap();
            v["flight"]["flight_no"] = "BX 9999".into();
            e = serde_json::from_value(v).unwrap();
            assert!(extra_checks(&x, &e, tip).iter().any(|c| c.name == "flight.flight_no" && !c.ok), "{name}");
        }
        let x = expected::load(&letters().join("flight_delay_bx2041.expected.json")).unwrap();
        assert_eq!(x["flight"]["delay_minutes"], 250);
        assert_eq!(
            (x["tip"].as_str(), x["rule_deadlines"][0]["due"].as_str()),
            (Some("eu261_delay"), Some("2029-12-31"))
        );
        for name in ["flight_delay_short", "flight_cancel_notice"] {
            let x = expected::load(&letters().join(format!("{name}.expected.json"))).unwrap();
            assert!(x["tip"].is_null() && x["rule_deadlines"].as_array().unwrap().is_empty(), "{name}: no tip");
        }
    }

    #[test]
    fn a_missing_flight_block_fails_and_old_letters_add_no_flight_check() {
        let x = expected::load(&letters().join("flight_delay_bx2041.expected.json")).unwrap();
        let mut v = serde_json::to_value(expected::to_extract(&x).unwrap()).unwrap();
        v["flight"] = Value::Null;
        let none: DocumentExtract = serde_json::from_value(v).unwrap();
        let checks = extra_checks(&x, &none, Some("eu261_delay"));
        assert!(checks.iter().any(|c| c.name == "flight" && !c.ok));
        let au = expected::load(&letters().join("au_erika_erst.expected.json")).unwrap();
        let checks = extra_checks(&au, &expected::to_extract(&au).unwrap(), Some("au_gap"));
        assert_eq!(checks.iter().map(|c| (c.name, c.ok)).collect::<Vec<_>>(), [("tip", true)]);
        assert!(same_flight_value("from", &"DUS".into(), &"Düsseldorf (DUS)".into()));
        assert!(same_flight_value("flight_no", &"BX 2041".into(), &"bx2041".into()));
        assert!(!same_flight_value("to", &"TFS".into(), &"Tenerife".into()));
    }

    #[test]
    fn seeding_the_flight_notice_gives_its_summary_and_the_claim_deadline() {
        let memory = Arc::new(Memory::open_in_memory().unwrap());
        let now = local_at(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(), 12, 0);
        let clock = Arc::new(FixedClock::new(now));
        let rules = RuleEngine::new(memory.clone(), clock.clone() as Arc<dyn Clock>);
        let xp = letters().join("flight_delay_bx2041.expected.json");
        let (id, x, deadlines) = seed_letter(&memory, &rules, &clock, &xp, now).unwrap();
        assert!(
            x.summary_simple.contains("BX 2041") && x.summary_simple.contains("4 h 10 min"),
            "{}",
            x.summary_simple
        );
        let claim: Vec<&Deadline> = deadlines.iter().filter(|d| d.rule_id.as_deref() == Some("eu261_claim")).collect();
        assert_eq!(claim.len(), 1, "{deadlines:?}");
        assert_eq!(claim[0].due, NaiveDate::from_ymd_opt(2029, 12, 31).unwrap());
        assert_eq!(memory.deadlines_for_document(id).unwrap().len(), deadlines.len(), "no duplicate");
        // The boarding pass as an inline document (eval `documents`): stored with its file and summary.
        let doc = serde_json::json!({"doc_type": "sonstiges", "sender_contains": "Beispiel Air",
            "date": "2026-10-03", "reference": "BX7Q2K", "summary": "Boarding pass BX 2041"});
        let file = letters().join("boarding_pass_bx2041.pdf");
        let (bid, b, dl) = seed_document(&memory, &rules, &clock, &doc, &file, now).unwrap();
        assert!(bid != id && dl.is_empty() && b.summary_simple == "Boarding pass BX 2041");
        assert!(memory.document_by_id(bid).unwrap().unwrap().path.ends_with("boarding_pass_bx2041.pdf"));
    }
}
