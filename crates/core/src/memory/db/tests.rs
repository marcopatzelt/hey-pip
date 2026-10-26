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
    assert_eq!(m.profile_get("insurer").unwrap(), None);
    m.profile_set("insurer", "Evergreen Health").unwrap();
    m.profile_set("insurer", "Riverside Health").unwrap();
    m.profile_set("name", "Erika").unwrap();
    assert_eq!(m.profile_get("insurer").unwrap().as_deref(), Some("Riverside Health"));
    assert_eq!(
        m.profile_all().unwrap(),
        vec![("insurer".to_string(), "Riverside Health".to_string()), ("name".to_string(), "Erika".to_string())]
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
fn old_profile_key_names_move_to_the_new_ones_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    let old_form = "file:///c:/Pip/fixtures/demo-kasse/index.html";
    let old_services = "file:///c:/Pip/fixtures/demo-kasse/services.html";
    {
        let m = Memory::open(&path).unwrap();
        // A database of an older version: the facts under their old (German) key names.
        let c = m.conn();
        let now = ts(Local::now());
        for (k, v) in [
            ("krankenkasse", "Evergreen Health"),
            ("krankenkasse_kontaktformular", old_form),
            ("krankenkasse_services", old_services),
        ] {
            c.execute(
                "INSERT INTO profile(key, value, updated_at, source) VALUES (?1, ?2, ?3, 'letter:4')",
                params![k, v, now],
            )
            .unwrap();
        }
    }
    let m = Memory::open(&path).unwrap();
    assert_eq!(m.profile_get("insurer").unwrap().as_deref(), Some("Evergreen Health"));
    assert_eq!(m.profile_meta("insurer").unwrap().unwrap().source, "letter:4", "source stays");
    assert_eq!(
        m.profile_get("insurer_contact_form").unwrap().as_deref(),
        Some("file:///c:/Pip/fixtures/demo-insurer/index.html")
    );
    assert_eq!(
        m.profile_get("insurer_services").unwrap().as_deref(),
        Some("file:///c:/Pip/fixtures/demo-insurer/services.html")
    );
    let keys: Vec<String> = m.profile_all().unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["insurer", "insurer_contact_form", "insurer_services"], "no old key left");
    // An old name still reads and writes the new key.
    assert_eq!(m.profile_get("krankenkasse").unwrap().as_deref(), Some("Evergreen Health"));
    m.profile_set("krankenkasse", "Riverside Health").unwrap();
    assert_eq!(m.profile_get("insurer").unwrap().as_deref(), Some("Riverside Health"));
    drop(m);
    // A newer value under the new name wins over a leftover old one.
    {
        let m = Memory::open(&path).unwrap();
        m.conn()
            .execute(
                "INSERT INTO profile(key, value, updated_at, source) VALUES ('krankenkasse', 'Old', ?1, 'user')",
                [ts(Local::now())],
            )
            .unwrap();
    }
    let m = Memory::open(&path).unwrap();
    assert_eq!(m.profile_get("insurer").unwrap().as_deref(), Some("Riverside Health"));
    assert_eq!(m.profile_all().unwrap().len(), 3);
}

#[test]
fn events_newest_first() {
    let m = mem();
    m.add_event("doc", "Krankschreibung gelesen").unwrap();
    m.add_event("sent", "Krankschreibung an Evergreen Health geschickt").unwrap();
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
    assert!(p.ends_with("Pip/memory.db") || p.ends_with("Pip\\memory.db"));
}

#[test]
fn memory_is_shareable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Memory>();
}

#[test]
fn an_existing_version_1_db_gets_the_learning_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    {
        // A database written before migration 2 (profile without source/uses, no playbooks).
        let c = Connection::open(&path).unwrap();
        c.execute_batch(crate::memory::SCHEMA_SQL).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute("INSERT INTO profile(key, value, updated_at) VALUES ('name', 'Erika', '2026-10-20T10:00:00Z')", [])
            .unwrap();
    }
    let m = Memory::open(&path).unwrap();
    let f = m.profile_meta("name").unwrap().unwrap();
    assert_eq!((f.value.as_str(), f.source.as_str(), f.uses), ("Erika", "user", 0));
    assert_eq!(f.confidence, 1.0);
    let version: i64 = m.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, MIGRATIONS.len() as i64);
    m.profile_touch(&["name".to_string()]).unwrap();
    assert_eq!(m.profile_meta("name").unwrap().unwrap().uses, 1);
    assert!(m.playbook_all().unwrap().is_empty());
    drop(m);
    // Opening again runs nothing twice.
    let m = Memory::open(&path).unwrap();
    assert_eq!(m.profile_meta("name").unwrap().unwrap().uses, 1);
}

#[test]
fn prune_keeps_memory_small() {
    let m = mem();
    for i in 0..60 {
        m.playbook_upsert(&Playbook {
            site: format!("s{i}.example"),
            task: "text".into(),
            url: String::new(),
            fields: vec![],
            attach_label: None,
            send_label: None,
            steps: 1,
            seconds: 1,
            runs: 1,
            failed_since_ok: i64::from(i == 0) * 2,
            updated_at: Local::now(),
        })
        .unwrap();
    }
    for i in 0..520 {
        m.add_event("x", &format!("e{i}")).unwrap();
    }
    m.conn().execute("UPDATE playbooks SET updated_at = '2020-01-01T00:00:00Z' WHERE site = 's1.example'", []).unwrap();
    m.prune().unwrap();
    let all = m.playbook_all().unwrap();
    assert_eq!(all.len(), 50);
    assert!(!all.iter().any(|p| p.site == "s0.example" || p.site == "s1.example"), "failed twice / too old");
    assert_eq!(m.recent_events(1000).unwrap().len(), 500);
    assert_eq!(m.recent_events(1).unwrap()[0].text, "e519", "the newest stay");
}

#[test]
fn forgetting_a_letter_removes_it_from_search_and_its_deadlines() {
    let m = mem();
    let id = m
        .insert_document(
            Path::new("C:/b/au.pdf"),
            "sha-x",
            &au_doc(d(2026, 10, 20), d(2026, 10, 30)),
            "Arbeitsunfähigkeit Evergreen Health",
        )
        .unwrap();
    m.add_deadline(&NewDeadline { doc_id: Some(id), ..deadline("Neue AU", d(2026, 11, 2), vec![]) }).unwrap();
    assert_eq!(m.search_documents("Arbeitsunfähigkeit", 5).unwrap().len(), 1);
    assert!(m.document_delete(id).unwrap());
    assert!(m.search_documents("Arbeitsunfähigkeit", 5).unwrap().is_empty());
    assert!(m.document_by_id(id).unwrap().is_none());
    assert!(m.open_deadlines().unwrap().is_empty());
    assert!(!m.document_delete(id).unwrap());
    let k = m.knowledge().unwrap();
    assert!(k.letters.is_empty());
}

#[test]
fn wipe_everything_empties_every_table() {
    let m = mem();
    m.profile_set("name", "Erika Beispiel").unwrap();
    let id = m
        .insert_document(
            Path::new("C:/b/au.pdf"),
            "sha-w",
            &au_doc(d(2026, 10, 20), d(2026, 10, 30)),
            "Arbeitsunfähigkeit Evergreen Health",
        )
        .unwrap();
    let now = local_at(d(2026, 10, 28), 9, 0);
    m.add_deadline(&NewDeadline { doc_id: Some(id), ..deadline("Neue AU", d(2026, 11, 2), vec![now]) }).unwrap();
    m.add_event("suggestion_accepted", "Return your questionnaire online").unwrap();
    m.wipe_everything().unwrap();
    for table in ["profile", "documents", "deadlines", "reminders", "events", "actions_log", "playbooks"] {
        let n: i64 = m.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert_eq!(m.profile_get("name").unwrap(), None);
    assert!(m.search_documents("Arbeitsunfähigkeit", 5).unwrap().is_empty(), "the search index is empty too");
    assert!(m.open_deadlines().unwrap().is_empty());
    // The same letter can come in again after a wipe.
    m.insert_document(Path::new("C:/b/au.pdf"), "sha-w", &au_doc(d(2026, 10, 20), d(2026, 10, 30)), "AU").unwrap();
}

#[test]
fn card_counters_count_done_deadlines_fired_reminders_and_the_first_event() {
    let m = mem();
    assert_eq!(m.documents_count().unwrap(), 0);
    assert_eq!(m.deadlines_done_count().unwrap(), 0);
    assert_eq!(m.reminders_fired_count().unwrap(), 0);
    assert_eq!(m.first_event_at().unwrap(), None);

    let early = local_at(d(2026, 10, 14), 16, 40);
    let r1 = local_at(d(2026, 10, 28), 9, 0);
    let r2 = local_at(d(2026, 10, 29), 9, 0);
    let r3 = local_at(d(2026, 10, 30), 9, 0);
    let a = m.add_deadline(&deadline("A", d(2026, 11, 2), vec![r1, r2, r3])).unwrap();
    let b = m.add_deadline(&deadline("B", d(2026, 11, 3), vec![])).unwrap();
    m.add_deadline(&deadline("C", d(2026, 11, 4), vec![])).unwrap();
    m.set_deadline_status(a, DeadlineStatus::Done).unwrap();
    m.set_deadline_status(b, DeadlineStatus::Dismissed).unwrap();
    assert_eq!(m.deadlines_done_count().unwrap(), 1);

    let ids: Vec<i64> = m
        .conn()
        .prepare("SELECT id FROM reminders ORDER BY at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    m.mark_fired(ids[0], r1, None).unwrap();
    m.mark_fired(ids[1], r2, Some("skipped")).unwrap();
    assert_eq!(m.reminders_fired_count().unwrap(), 1, "a skipped reminder never showed");

    m.insert_document(Path::new("C:/b/au.pdf"), "sha-c", &au_doc(d(2026, 10, 20), d(2026, 10, 30)), "AU").unwrap();
    assert_eq!(m.documents_count().unwrap(), 1);
    m.add_event_at(early, "done", "Sent the sick note").unwrap();
    assert_eq!(m.first_event_at().unwrap(), Some(early));
}
