use super::*;
use crate::rules::local_at;
use crate::rules::tests_support::{au_doc, bescheid_doc, d};

fn mem() -> Memory {
    Memory::open_in_memory().unwrap()
}

fn deadline(title: &str, due: NaiveDate, remind: Vec<DateTime<Local>>) -> NewDeadline {
    NewDeadline {
        title: title.into(),
        due,
        rule_id: Some("frist_aus_brief".into()),
        doc_id: None,
        remind_at: remind,
        why: "weil".into(),
        source: "Brief".into(),
    }
}

#[test]
fn open_file_twice_is_idempotent_and_versioned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/dir/memory.db");
    {
        let m = Memory::open(&path).unwrap();
        m.profile_set("name", "Erika").unwrap();
    }
    let m = Memory::open(&path).unwrap();
    assert_eq!(m.profile_get("name").unwrap().as_deref(), Some("Erika"));
    let v: i64 = m.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v as usize, MIGRATIONS.len());
    let mode: String = m.conn().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn profile_upsert_and_all() {
    let m = mem();
    assert_eq!(m.profile_get("kasse").unwrap(), None);
    m.profile_set("kasse", "Gesundkasse").unwrap();
    m.profile_set("kasse", "TK Beispiel").unwrap();
    m.profile_set("name", "Erika").unwrap();
    assert_eq!(m.profile_get("kasse").unwrap().as_deref(), Some("TK Beispiel"));
    assert_eq!(
        m.profile_all().unwrap(),
        vec![("kasse".to_string(), "TK Beispiel".to_string()), ("name".to_string(), "Erika".to_string())]
    );
}

#[test]
fn documents_insert_dedupe_and_lookup() {
    let m = mem();
    let au = au_doc(d(2026, 10, 20), d(2026, 10, 30));
    let id = m
        .insert_document(
            Path::new("C:\\Users\\Erika\\au.png"),
            "sha-au",
            &au,
            "Arbeitsunfähigkeitsbescheinigung Erstbescheinigung",
        )
        .unwrap();
    let again = m.insert_document(Path::new("other.png"), "sha-au", &au, "x").unwrap();
    assert_eq!(id, again);
    let row = m.document_by_sha("sha-au").unwrap().unwrap();
    assert_eq!(row.id, id);
    assert_eq!(row.doc_type, DocType::AuBescheinigung);
    assert_eq!(row.extract, au);
    assert_eq!(row.path, PathBuf::from("C:\\Users\\Erika\\au.png"));
    assert!(m.document_by_sha("nope").unwrap().is_none());
    assert_eq!(m.document_by_id(id).unwrap().unwrap().sha256, "sha-au");

    let b = bescheid_doc(d(2026, 10, 12));
    let bid = m.insert_document(Path::new("b.pdf"), "sha-b", &b, "Bescheid Text").unwrap();
    assert_eq!(m.latest_document(DocType::Bescheid).unwrap().unwrap().id, bid);
    assert_eq!(m.latest_document(DocType::AuBescheinigung).unwrap().unwrap().id, id);
    assert!(m.latest_document(DocType::Rechnung).unwrap().is_none());

    let au2 = au_doc(d(2026, 11, 2), d(2026, 11, 6));
    let id2 = m.insert_document(Path::new("au2.png"), "sha-au2", &au2, "Folgebescheinigung").unwrap();
    let all = m.documents_of_type(DocType::AuBescheinigung).unwrap();
    assert_eq!(all.iter().map(|r| r.id).collect::<Vec<_>>(), vec![id2, id]);
}

#[test]
fn search_survives_special_input_and_ranks() {
    let m = mem();
    let au = au_doc(d(2026, 10, 20), d(2026, 10, 30));
    let au_id = m.insert_document(Path::new("au.png"), "a", &au, "Arbeitsunfähigkeitsbescheinigung nach § 46").unwrap();
    let b = bescheid_doc(d(2026, 10, 12));
    let b_id =
        m.insert_document(Path::new("b.pdf"), "b", &b, "Bewilligungsbescheid Bürgergeld Widerspruch möglich").unwrap();

    // Type words make "Krankschreibung" find the AU even though the text says otherwise.
    let r = m.search_documents("Krankschreibung", 10).unwrap();
    assert_eq!(r.iter().map(|x| x.id).collect::<Vec<_>>(), vec![au_id]);
    // Inputs that are FTS5 syntax must not error.
    for q in
        ["§ 46", "AU-Bescheinigung", "\"", "NEAR(", "a OR", "*", "Jobcenter\" OR 1=1 --", "(((", "bürger:geld", "-"]
    {
        m.search_documents(q, 5).unwrap_or_else(|e| panic!("query {q:?}: {e}"));
    }
    // Diacritics folded, prefix match on longer words.
    let r = m.search_documents("Burgergeld", 10).unwrap();
    assert_eq!(r.first().map(|x| x.id), Some(b_id));
    let r = m.search_documents("Widerspr", 10).unwrap();
    assert_eq!(r.first().map(|x| x.id), Some(b_id));
    // Sender and reference are searchable.
    assert_eq!(m.search_documents("Jobcenter", 10).unwrap()[0].id, b_id);
    assert_eq!(m.search_documents("BG-0815", 10).unwrap()[0].id, b_id);
    // OR + rank: the doc matching more words comes first.
    let r = m.search_documents("Jobcenter Bescheid Krankschreibung", 10).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].id, b_id);
    // Empty query -> newest first; limit respected.
    let r = m.search_documents("  ", 1).unwrap();
    assert_eq!(r.iter().map(|x| x.id).collect::<Vec<_>>(), vec![b_id]);
    assert!(m.search_documents("Zahnarzt", 10).unwrap().is_empty());
}

#[test]
fn fts_query_builder() {
    assert_eq!(fts_query("§ 46").as_deref(), Some("\"46\""));
    assert_eq!(fts_query("AU-Bescheinigung").as_deref(), Some("\"AU\" OR \"Bescheinigung\"*"));
    assert_eq!(fts_query("\"\" ()"), None);
}

#[test]
fn deadlines_and_status() {
    let m = mem();
    let a = m.add_deadline(&deadline("später", d(2026, 12, 1), vec![])).unwrap();
    let b = m.add_deadline(&deadline("früher", d(2026, 11, 2), vec![local_at(d(2026, 11, 1), 18, 0)])).unwrap();
    let open = m.open_deadlines().unwrap();
    assert_eq!(open.iter().map(|x| x.id).collect::<Vec<_>>(), vec![b, a]);
    assert_eq!(open[0].remind_at, vec![local_at(d(2026, 11, 1), 18, 0)]);
    assert_eq!(open[0].status, DeadlineStatus::Open);
    m.set_deadline_status(b, DeadlineStatus::Done).unwrap();
    assert_eq!(m.open_deadlines().unwrap().len(), 1);
    assert_eq!(m.deadline(b).unwrap().unwrap().status, DeadlineStatus::Done);
    m.set_deadline_status(a, DeadlineStatus::Dismissed).unwrap();
    assert!(m.open_deadlines().unwrap().is_empty());
    assert!(m.set_deadline_status(999, DeadlineStatus::Done).is_err());
}

#[test]
fn deadline_with_doc_foreign_key() {
    let m = mem();
    let doc = m.insert_document(Path::new("b.pdf"), "b", &bescheid_doc(d(2026, 10, 12)), "").unwrap();
    let mut nd = deadline("Widerspruch", d(2026, 11, 16), vec![]);
    nd.doc_id = Some(doc);
    let id = m.add_deadline(&nd).unwrap();
    assert_eq!(m.deadlines_for_document(doc).unwrap()[0].id, id);
    nd.doc_id = Some(4242);
    assert!(m.add_deadline(&nd).is_err(), "foreign key enforced");
}

#[test]
fn reminders_fire_once_and_skip_closed_deadlines() {
    let m = mem();
    let r1 = local_at(d(2026, 10, 29), 18, 0);
    let r2 = local_at(d(2026, 11, 2), 9, 0);
    let id = m.add_deadline(&deadline("AU", d(2026, 11, 2), vec![r1, r2])).unwrap();

    assert!(m.due_reminders(local_at(d(2026, 10, 29), 17, 59)).unwrap().is_empty());
    let due = m.due_reminders(local_at(d(2026, 10, 29), 18, 0)).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].at, r1);
    assert_eq!(due[0].deadline.id, id);
    let now = local_at(d(2026, 10, 29), 18, 1);
    m.mark_fired(due[0].id, now, None).unwrap();
    m.mark_fired(due[0].id, local_at(d(2026, 10, 30), 1, 0), Some("later")).unwrap();
    assert!(m.due_reminders(local_at(d(2026, 10, 30), 0, 0)).unwrap().is_empty());
    let fired_at: String =
        m.conn().query_row("SELECT fired_at FROM reminders WHERE id = ?1", [due[0].id], |r| r.get(0)).unwrap();
    assert_eq!(fired_at, ts(now), "first fire time kept");
    m.set_reminder_answer(due[0].id, "done").unwrap();
    assert!(m.set_reminder_answer(999, "x").is_err());

    // Extra reminder; deadline closed -> nothing fires.
    let extra = m.add_reminder(id, local_at(d(2026, 10, 31), 10, 0)).unwrap();
    assert_eq!(m.deadline(id).unwrap().unwrap().remind_at.len(), 3);
    assert_eq!(m.reminder(extra).unwrap().unwrap().at, local_at(d(2026, 10, 31), 10, 0));
    assert_eq!(m.due_reminders(local_at(d(2026, 11, 5), 0, 0)).unwrap().len(), 2);
    m.set_deadline_status(id, DeadlineStatus::Done).unwrap();
    assert!(m.due_reminders(local_at(d(2026, 11, 5), 0, 0)).unwrap().is_empty());
    assert!(m.add_reminder(999, r1).is_err());
}

#[test]
fn reminder_order_across_dst_change() {
    // 25.10.2026 03:00 CEST -> 02:00 CET. Stored as UTC, so order by text is order by time.
    let m = mem();
    let a = local_at(d(2026, 10, 24), 23, 30);
    let b = local_at(d(2026, 10, 25), 9, 0);
    m.add_deadline(&deadline("x", d(2026, 10, 26), vec![b, a])).unwrap();
    let due = m.due_reminders(local_at(d(2026, 10, 26), 0, 0)).unwrap();
    assert_eq!(due.iter().map(|r| r.at).collect::<Vec<_>>(), vec![a, b]);
}

#[test]
fn events_newest_first() {
    let m = mem();
    m.add_event("doc", "Krankschreibung gelesen").unwrap();
    m.add_event("sent", "Krankschreibung an Kasse geschickt").unwrap();
    let e = m.recent_events(5).unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].kind, "sent");
    assert_eq!(m.recent_events(1).unwrap().len(), 1);
}

#[test]
fn action_log_and_tokens() {
    let m = mem();
    let now = Local::now();
    let log = |at: DateTime<Local>, tokens: u64, approved: Option<bool>| ActionLog {
        at,
        tool: "click".into(),
        args_redacted: "{\"description\":\"Absenden\"}".into(),
        risk: "commit".into(),
        approved,
        result: "ok".into(),
        tokens,
    };
    m.log_action(&log(now, 1200, Some(true))).unwrap();
    m.log_action(&log(now, 800, None)).unwrap();
    m.log_action(&log(now - chrono::Duration::days(2), 99_999, Some(false))).unwrap();
    assert_eq!(m.tokens_today().unwrap(), 2000);
    assert_eq!(m.tokens_since(now - chrono::Duration::days(3)).unwrap(), 101_999);
    let approved: Option<bool> =
        m.conn().query_row("SELECT approved FROM actions_log ORDER BY id LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(approved, Some(true));
}

#[test]
fn default_path_shape() {
    let p = default_db_path().unwrap();
    assert!(p.ends_with("papierkram-copilot/memory.db") || p.ends_with("papierkram-copilot\\memory.db"));
}

#[test]
fn memory_is_shareable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Memory>();
}
