//! Live accuracy check of the document pipeline on the invented letters (real Meta API, ~5 cents).
//! Run: `cargo test -p pip-core --test docs_live -- --ignored --nocapture`
//! Optional: DOCS_LIVE_FILES="au_erika_erst_foto.jpg,bescheid_wohngeld.pdf" to limit the run.
//! Writes the SAM crop of the photo to target/docs_live/au_erika_erst_foto.crop.png.
//! OWNER: agent "docs-cli".

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, NaiveDate, TimeZone};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::DocPipeline;
use pip_core::i18n::Lang;
use pip_core::memory::Memory;
use pip_core::meta::MetaClient;
use pip_core::rules::RuleEngine;
use serde_json::Value;

const ALL: &[&str] = &[
    "au_erika_erst_foto.jpg",
    "au_erika_erst.pdf",
    "au_erika_folge.pdf",
    "kasse_brief.pdf",
    "bescheid_wohngeld.pdf",
    "uni_rueckmeldung.pdf",
];

fn letters() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters")
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

#[tokio::test]
#[ignore = "real API call, costs cents"]
async fn live_fixture_accuracy() {
    let meta = Arc::new(MetaClient::from_env().expect("MODEL_API_KEY missing (.env)"));
    let files: Vec<String> = std::env::var("DOCS_LIVE_FILES")
        .map(|s| s.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect())
        .unwrap_or_else(|_| ALL.iter().map(|s| s.to_string()).collect());

    let (mut ok, mut total) = (0u32, 0u32);
    let mut report = Vec::new();
    for file in &files {
        let memory = Arc::new(Memory::open_in_memory().unwrap());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()));
        let rules = Arc::new(RuleEngine::new(memory.clone(), clock));
        let pipeline = DocPipeline::new(meta.clone(), memory.clone(), rules);
        let path = letters().join(file);
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(letters().join(format!("{stem}.expected.json"))).unwrap())
                .unwrap();

        if file.ends_with(".jpg") {
            let crop = pipeline.crop_sheet(&std::fs::read(&path).unwrap()).await.unwrap();
            let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/docs_live");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join(format!("{stem}.crop.png")), crop).unwrap();
        }

        let started = std::time::Instant::now();
        let r = match pipeline.ingest(&path, Lang::De).await {
            Ok(r) => r,
            Err(e) => {
                report.push(format!("{file}: FAILED {e:#}"));
                total += 1;
                continue;
            }
        };
        let secs = started.elapsed().as_secs_f32();
        let e = &r.extract;
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let date = |v: &Value| v.as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        let mut checks: Vec<(&str, bool, String)> = vec![
            ("doc_type", e.doc_type.as_str() == s(&expected["doc_type"]), e.doc_type.as_str().to_string()),
            ("date", e.date == date(&expected["date"]), format!("{:?}", e.date)),
            (
                "reference",
                e.reference.as_deref().map(norm) == Some(norm(&s(&expected["reference"]))),
                format!("{:?}", e.reference),
            ),
            ("sender", norm(&e.sender).contains(&norm(&s(&expected["sender_contains"]))), e.sender.clone()),
            ("recipient", norm(&e.recipient).contains(&norm(&s(&expected["recipient_contains"]))), e.recipient.clone()),
        ];
        match (&e.au, expected["au"].is_object()) {
            (Some(au), true) => {
                let x = &expected["au"];
                checks.push(("au.from", Some(au.from) == date(&x["from"]), au.from.to_string()));
                checks.push(("au.until", Some(au.until) == date(&x["until"]), au.until.to_string()));
                checks.push((
                    "au.kind",
                    serde_json::to_value(au.first_or_follow).unwrap() == x["first_or_follow"],
                    format!("{:?}", au.first_or_follow),
                ));
            }
            (None, false) => checks.push(("au", true, "none".into())),
            (got, _) => checks.push(("au", false, format!("{got:?}"))),
        }
        for due in expected["deadlines_due"].as_array().unwrap() {
            let due = date(due).unwrap();
            let found = e.deadlines.iter().find(|d| d.due == due);
            checks.push((
                "letter deadline",
                found.is_some_and(|d| !d.source_text.is_empty()),
                found.map_or("missing".into(), |d| format!("{} «{}»", d.due, d.source_text)),
            ));
        }
        for rd in expected["rule_deadlines"].as_array().unwrap() {
            let (rule, due) = (s(&rd["rule_id"]), date(&rd["due"]).unwrap());
            let hit = r.deadlines.iter().any(|d| d.rule_id.as_deref() == Some(rule.as_str()) && d.due == due);
            let got: Vec<String> =
                r.deadlines.iter().map(|d| format!("{}={}", d.rule_id.as_deref().unwrap_or("-"), d.due)).collect();
            checks.push(("rule deadline", hit, got.join(", ")));
        }

        let passed = checks.iter().filter(|c| c.1).count() as u32;
        ok += passed;
        total += checks.len() as u32;
        report.push(format!(
            "{file}: {passed}/{} in {secs:.1}s, confidence {:.2}\n  bubble: {}\n{}",
            checks.len(),
            e.confidence,
            r.bubble,
            checks
                .iter()
                .map(|(k, pass, got)| format!("  [{}] {k}: {got}", if *pass { "ok" } else { "XX" }))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let usage = meta.usage_total();
    println!("\n{}\n", report.join("\n"));
    println!(
        "field accuracy: {ok}/{total} = {:.0} %   tokens in/out: {}/{}",
        100.0 * ok as f32 / total.max(1) as f32,
        usage.input_tokens,
        usage.output_tokens
    );
    assert!(ok * 10 >= total * 8, "accuracy below 80 %");
}
