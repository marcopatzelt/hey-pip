//! Demo story B through the public API with a real database file: letter in on Wednesday,
//! app restarted, demo clock jumps to Thursday 18:00, Pip speaks up once. OWNER: memory-rules.

use std::path::Path;
use std::sync::Arc;

use chrono::NaiveDate;
use pip_core::docs::{AuInfo, AuKind, DocType, DocumentExtract};
use pip_core::events::UiEvent;
use pip_core::i18n::Lang;
use pip_core::memory::{Memory, seed_demo_profile};
use pip_core::rules::{DemoClock, FixedClock, RuleEngine, daily_brief, local_at};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn sick_note() -> DocumentExtract {
    DocumentExtract {
        doc_type: DocType::AuBescheinigung,
        sender: "Praxis Dr. Beispiel".into(),
        recipient: "Erika Musterfrau".into(),
        date: Some(d(2026, 10, 20)),
        reference: None,
        person: None,
        summary_simple: "Krankschreibung vom 20.10. bis 30.10.".into(),
        requests: vec![],
        deadlines: vec![],
        au: Some(AuInfo { from: d(2026, 10, 20), until: d(2026, 10, 30), first_or_follow: AuKind::Erst, issued: None }),
        consequences_if_ignored: None,
        confidence: 0.95,
    }
}

#[test]
fn reminder_survives_restart_and_fires_once() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("memory.db");
    {
        let mem = Arc::new(Memory::open(&db).unwrap());
        let eng = RuleEngine::new(mem.clone(), Arc::new(FixedClock::new(local_at(d(2026, 10, 21), 9, 0))));
        let doc = sick_note();
        let id = mem.insert_document(Path::new("au.jpg"), "sha", &doc, "Arbeitsunfähigkeitsbescheinigung").unwrap();
        let dls = eng.apply_document(id, &doc).unwrap();
        assert_eq!(dls[0].due, d(2026, 11, 2));
    }
    // Restart with the demo clock (DEMO_NOW=2026-10-29T18:00).
    let mem = Arc::new(Memory::open(&db).unwrap());
    let eng = RuleEngine::new(mem.clone(), Arc::new(DemoClock::parse("2026-10-29T18:00").unwrap()));
    let fired = eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    assert!(eng.tick().unwrap().is_empty());
    let UiEvent::Reminder { text, .. } = eng.reminder_event(&fired[0], Lang::De) else { panic!() };
    assert!(text.starts_with("Deine Krankschreibung endet morgen."), "{text}");
    assert_eq!(mem.search_documents("Krankschreibung", 5).unwrap().len(), 1);
}

/// Demo opening shot: Pip starts on Wednesday 28.10. and offers to fill in the questionnaire online.
#[test]
fn demo_profile_brief_offers_the_questionnaire() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let now = local_at(d(2026, 10, 28), 14, 0);
    let mem = Arc::new(Memory::open_in_memory().unwrap());
    let rules = RuleEngine::new(mem.clone(), Arc::new(FixedClock::new(now)));
    assert!(seed_demo_profile(&mem, &rules, &root.join("letters"), now).unwrap());
    let html = std::fs::read_to_string(root.join("demo-kasse/services.html")).unwrap();
    let url = "file:///C:/Pip/fixtures/demo-kasse/services.html";
    let name = mem.profile_get("name").unwrap();
    let brief = daily_brief(&mem, now, Lang::En, name.as_deref(), Some((url, &html))).unwrap().expect("a brief");
    let offer = brief.offer.expect("an offer");
    assert_eq!(offer.heading, "Return your questionnaire online");
    assert!(offer.task.contains(url), "{}", offer.task);
    assert!(brief.text.contains("Erika") && brief.text.contains("sick note"), "{}", brief.text);
    assert!(!brief.text.contains('{'), "{}", brief.text);
    println!("{}", brief.text);
}

/// The same opening shot on the judges' day (Friday 6.11.): the story moved with the clock, so the insurer's
/// letter is in memory, nothing is overdue, and Pip still offers the questionnaire.
#[test]
fn demo_profile_brief_offers_the_questionnaire_on_any_day() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let now = local_at(d(2026, 11, 6), 9, 0);
    let mem = Arc::new(Memory::open_in_memory().unwrap());
    let rules = RuleEngine::new(mem.clone(), Arc::new(FixedClock::new(now)));
    assert!(seed_demo_profile(&mem, &rules, &root.join("letters"), now).unwrap());
    assert!(mem.open_deadlines().unwrap().iter().all(|dl| dl.due >= now.date_naive()));
    let html = std::fs::read_to_string(root.join("demo-kasse/services.html")).unwrap();
    let url = "file:///C:/Pip/fixtures/demo-kasse/services.html";
    let brief = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((url, &html))).unwrap().expect("a brief");
    assert_eq!(brief.offer.expect("an offer").heading, "Return your questionnaire online");
    assert!(brief.text.contains("sick note") && !brief.text.contains("passed"), "{}", brief.text);
}
