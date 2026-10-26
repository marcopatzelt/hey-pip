//! Demo story B through the public API with a real database file: letter in on Wednesday,
//! app restarted, demo clock jumps to Thursday 18:00, Pip speaks up once. OWNER: memory-rules.

use std::path::Path;
use std::sync::Arc;

use chrono::NaiveDate;
use pip_core::docs::{AuInfo, AuKind, DocType, DocumentExtract};
use pip_core::events::UiEvent;
use pip_core::i18n::Lang;
use pip_core::memory::Memory;
use pip_core::rules::{DemoClock, FixedClock, RuleEngine, local_at};

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
