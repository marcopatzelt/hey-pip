//! `fixtures/letters/<name>.expected.json`: what a letter must give (checks) and a stand-in
//! extract for commands that should not call the API (rules, agent dry run).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use copilot_core::docs::{AuInfo, AuKind, DocType, DocumentExtract, ExtractedDeadline};
use copilot_core::memory::Deadline;
use serde_json::Value;

pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub got: String,
}

/// `<dir>/<stem>.expected.json` next to a letter file, if it exists.
pub fn path_for(letter: &Path) -> Option<PathBuf> {
    let stem = letter.file_stem()?.to_str()?;
    let stem = stem.strip_suffix(".expected").unwrap_or(stem);
    let p = letter.with_file_name(format!("{stem}.expected.json"));
    p.is_file().then_some(p)
}

pub fn load(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))
}

/// The letter file an expected.json belongs to (PDF first, then PNG, then JPEG).
pub fn letter_file(expected_path: &Path) -> PathBuf {
    let name = expected_path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    let stem = name.strip_suffix(".expected.json").unwrap_or(name);
    ["pdf", "png", "jpg"]
        .iter()
        .map(|ext| expected_path.with_file_name(format!("{stem}.{ext}")))
        .find(|p| p.is_file())
        .unwrap_or_else(|| expected_path.with_file_name(format!("{stem}.pdf")))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn date(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

/// Field-by-field comparison of a read letter with its expected.json.
pub fn check(x: &Value, e: &DocumentExtract, deadlines: &[Deadline]) -> Vec<Check> {
    let mut checks = vec![
        Check { name: "doc_type", ok: e.doc_type.as_str() == s(&x["doc_type"]), got: e.doc_type.as_str().into() },
        Check { name: "date", ok: e.date == date(&x["date"]), got: format!("{:?}", e.date) },
        Check {
            name: "reference",
            ok: e.reference.as_deref().map(norm) == Some(norm(&s(&x["reference"]))),
            got: format!("{:?}", e.reference),
        },
        Check { name: "sender", ok: norm(&e.sender).contains(&norm(&s(&x["sender_contains"]))), got: e.sender.clone() },
        Check {
            name: "recipient",
            ok: norm(&e.recipient).contains(&norm(&s(&x["recipient_contains"]))),
            got: e.recipient.clone(),
        },
    ];
    match (&e.au, x["au"].is_object()) {
        (Some(au), true) => {
            let a = &x["au"];
            checks.push(Check { name: "au.from", ok: Some(au.from) == date(&a["from"]), got: au.from.to_string() });
            checks.push(Check { name: "au.until", ok: Some(au.until) == date(&a["until"]), got: au.until.to_string() });
            let kind = serde_json::to_value(au.first_or_follow).unwrap_or_default();
            checks.push(Check { name: "au.kind", ok: kind == a["first_or_follow"], got: kind.to_string() });
        }
        (None, false) => checks.push(Check { name: "au", ok: true, got: "none".into() }),
        (got, _) => checks.push(Check { name: "au", ok: false, got: format!("{got:?}") }),
    }
    for due in x["deadlines_due"].as_array().into_iter().flatten() {
        let due = date(due);
        let found = e.deadlines.iter().find(|d| Some(d.due) == due);
        checks.push(Check {
            name: "letter deadline",
            ok: found.is_some_and(|d| !d.source_text.is_empty()),
            got: found.map_or("missing".into(), |d| format!("{} «{}»", d.due, d.source_text)),
        });
    }
    let got: Vec<String> =
        deadlines.iter().map(|d| format!("{}={}", d.rule_id.as_deref().unwrap_or("-"), d.due)).collect();
    for rd in x["rule_deadlines"].as_array().into_iter().flatten() {
        let (rule, due) = (s(&rd["rule_id"]), date(&rd["due"]));
        let ok = deadlines.iter().any(|d| d.rule_id.as_deref() == Some(rule.as_str()) && Some(d.due) == due);
        checks.push(Check { name: "rule deadline", ok, got: got.join(", ") });
    }
    checks
}

/// A stand-in extract built from expected.json (no API call). Text fields are short and plain.
pub fn to_extract(x: &Value) -> Result<DocumentExtract> {
    let doc_type = DocType::parse(&s(&x["doc_type"])).context("expected.json: unknown doc_type")?;
    let au = if x["au"].is_object() {
        let a = &x["au"];
        Some(AuInfo {
            from: date(&a["from"]).context("expected.json: au.from")?,
            until: date(&a["until"]).context("expected.json: au.until")?,
            first_or_follow: if s(&a["first_or_follow"]) == "folge" { AuKind::Folge } else { AuKind::Erst },
            issued: date(&x["date"]),
        })
    } else {
        None
    };
    let deadlines = x["deadlines_due"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(date)
        .map(|due| ExtractedDeadline { title: "Frist aus dem Brief".into(), due, source_text: String::new() })
        .collect();
    let summary = match &au {
        Some(au) => format!("Krankschreibung vom {} bis {}.", au.from.format("%d.%m."), au.until.format("%d.%m.%Y")),
        None => format!("Brief von {}.", s(&x["sender_contains"])),
    };
    Ok(DocumentExtract {
        doc_type,
        sender: s(&x["sender_contains"]),
        recipient: s(&x["recipient_contains"]),
        date: date(&x["date"]),
        reference: x["reference"].as_str().map(str::to_string),
        summary_simple: summary,
        requests: vec![],
        deadlines,
        au,
        consequences_if_ignored: None,
        confidence: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters")
    }

    #[test]
    fn every_fixture_round_trips_through_the_stand_in_extract() {
        for name in ["au_erika_erst", "au_erika_folge", "kasse_brief", "bescheid_wohngeld", "uni_rueckmeldung"] {
            let xp = fixtures().join(format!("{name}.expected.json"));
            let x = load(&xp).unwrap();
            let e = to_extract(&x).unwrap();
            let checks = check(&x, &e, &[]);
            // Every field matches; only rule deadlines (none given here) and quotes (none in a stand-in) fail.
            for c in &checks {
                assert!(c.ok || c.name.ends_with("deadline"), "{name}: {} = {}", c.name, c.got);
            }
            assert!(letter_file(&xp).is_file(), "{name}");
            assert_eq!(path_for(&letter_file(&xp)), Some(xp));
        }
    }

    #[test]
    fn photo_has_its_own_expected_file() {
        let photo = fixtures().join("au_erika_erst_foto.jpg");
        assert_eq!(path_for(&photo), Some(fixtures().join("au_erika_erst_foto.expected.json")));
        assert_eq!(path_for(Path::new("/nowhere/x.pdf")), None);
    }
}
