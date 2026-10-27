use super::*;
use crate::clock::FixedClock;
use crate::docs::ExtractedDeadline;
use crate::rules::tests_support::{au_doc, bescheid_doc, d};
use std::path::Path;

struct Env {
    mem: Arc<Memory>,
    clock: Arc<FixedClock>,
    eng: RuleEngine,
}

fn env(now: DateTime<Local>) -> Env {
    let mem = Arc::new(Memory::open_in_memory().unwrap());
    let clock = Arc::new(FixedClock::new(now));
    let eng = RuleEngine::new(mem.clone(), clock.clone());
    Env { mem, clock, eng }
}

fn ingest(e: &Env, sha: &str, doc: &DocumentExtract) -> (i64, Vec<Deadline>) {
    let id = e.mem.insert_document(Path::new("x.png"), sha, doc, &doc.summary_simple).unwrap();
    (id, e.eng.apply_document(id, doc).unwrap())
}

fn reminder_parts(ev: UiEvent) -> (String, String, Vec<Button>) {
    match ev {
        UiEvent::Reminder { text, why, buttons, .. } => (text, why, buttons),
        other => panic!("not a reminder: {other:?}"),
    }
}

#[test]
fn demo_story_au_reminder_thursday_evening() {
    // AU 20.10.–30.10.2026 scanned on 21.10.; demo clock jumps to Thu 29.10. 18:00.
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    let (_, dls) = ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    assert_eq!(dls.len(), 1);
    assert_eq!(dls[0].due, d(2026, 11, 2));
    assert_eq!(dls[0].rule_id.as_deref(), Some("au_lueckenlos"));

    e.clock.set(local_at(d(2026, 10, 29), 17, 59));
    assert!(e.eng.tick().unwrap().is_empty());
    e.clock.set(local_at(d(2026, 10, 29), 18, 0));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    assert!(e.eng.tick().unwrap().is_empty(), "never fires twice");

    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(
        text,
        "Deine Krankschreibung endet morgen. Wenn du noch krank bist, brauchst du bis Montag, 2. November, eine neue."
    );
    assert!(why.contains("§ 46 Satz 2 SGB V"));
    assert!(why.ends_with("Das ist ein Hinweis, keine Rechtsberatung."));
    let labels: Vec<_> = buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Erinnere mich Montag", "Ich bin gesund", "In 2 Stunden"]);
    assert!(buttons[0].primary && !buttons[1].primary);
    assert!(buttons[0].id.starts_with("remind:2026-11-02T08:00:00"));
    assert_eq!(buttons[1].id, "done");
    assert_eq!(buttons[2].id, "later");

    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::En));
    assert_eq!(text, "Your sick note ends tomorrow. If you're still sick, you need a new one by Monday, November 2.");
    assert!(why.ends_with("This is a hint, not legal advice."));
    assert_eq!(buttons[0].label, "Remind me Monday");

    // "Erinnere mich Montag" -> new reminder Monday 08:00 fires then.
    let msg = e.eng.answer(fired[0].id, &buttons[0].id, Lang::De).unwrap();
    assert_eq!(msg, "Okay, ich erinnere dich Montag um 08:00 Uhr.");
    e.clock.set(local_at(d(2026, 11, 2), 8, 30));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1, "08:00 extra fires, 09:00 not yet");
    assert_eq!(fired[0].at, local_at(d(2026, 11, 2), 8, 0));
    let (text, _, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(
        text,
        "Deine Krankschreibung ist am Freitag, 30. Oktober, zu Ende gegangen. Wenn du noch krank bist, brauchst du heute eine neue."
    );
    assert_eq!(buttons.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["done", "later"]);
    assert!(buttons[0].primary);

    // "Ich bin gesund" closes the deadline: the 09:00 reminder never fires.
    assert_eq!(
        e.eng.answer(fired[0].id, "done", Lang::De).unwrap(),
        "Schön, dass es dir besser geht. Ich erinnere dich nicht mehr daran."
    );
    e.clock.set(local_at(d(2026, 11, 2), 12, 0));
    assert!(e.eng.tick().unwrap().is_empty());
    assert!(e.mem.open_deadlines().unwrap().is_empty());
    assert_eq!(e.mem.recent_events(1).unwrap()[0].kind, "deadline_done");
}

#[test]
fn follow_up_au_closes_old_deadline() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    e.clock.set(local_at(d(2026, 10, 30), 11, 0));
    let mut follow = au_doc(d(2026, 11, 2), d(2026, 11, 13));
    follow.au.as_mut().unwrap().first_or_follow = crate::docs::AuKind::Folge;
    let (_, dls) = ingest(&e, "au2", &follow);
    assert_eq!(dls.len(), 1);
    assert_eq!(dls[0].due, d(2026, 11, 16)); // Fri 13.11. -> Mon 16.11.
    let open = e.mem.open_deadlines().unwrap();
    assert_eq!(open.len(), 1, "old AU deadline closed");
    assert_eq!(open[0].due, d(2026, 11, 16));
    e.clock.set(local_at(d(2026, 11, 2), 10, 0));
    assert!(e.eng.tick().unwrap().is_empty(), "old Monday reminder does not fire");
}

#[test]
fn late_follow_up_does_not_close_and_older_au_scanned_after_newer_gets_no_deadline() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    // Follow-up from Tue 3.11. is too late for an AU ending Fri 30.10. (due Mon 2.11.).
    ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    ingest(&e, "late", &au_doc(d(2026, 11, 3), d(2026, 11, 6)));
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 2);

    // Order reversed: the newer AU is already stored, the older one comes later.
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    ingest(&e, "new", &au_doc(d(2026, 11, 2), d(2026, 11, 6)));
    let (_, dls) = ingest(&e, "old", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    assert!(dls.is_empty());
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 1);
}

#[test]
fn apply_twice_no_duplicates() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    let doc = au_doc(d(2026, 10, 20), d(2026, 10, 30));
    let (id, a) = ingest(&e, "au1", &doc);
    let b = e.eng.apply_document(id, &doc).unwrap();
    let (id2, c) = ingest(&e, "au1", &doc); // same sha -> same id
    assert_eq!(id, id2);
    assert_eq!(a.iter().map(|x| x.id).collect::<Vec<_>>(), b.iter().map(|x| x.id).collect::<Vec<_>>());
    assert_eq!(b.len(), c.len());
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 1);
}

#[test]
fn bescheid_widerspruch_reminders() {
    let e = env(local_at(d(2026, 10, 14), 10, 0));
    let mut doc = bescheid_doc(d(2026, 10, 12));
    // The letter also states the deadline itself: same due -> no second deadline.
    doc.deadlines = vec![ExtractedDeadline {
        title: "Widerspruch".into(),
        due: d(2026, 11, 16),
        source_text: "innerhalb eines Monats".into(),
    }];
    let (_, dls) = ingest(&e, "b", &doc);
    assert_eq!(dls.len(), 1);
    assert_eq!(dls[0].rule_id.as_deref(), Some("widerspruch_bescheid"));
    assert_eq!(dls[0].due, d(2026, 11, 16));

    e.clock.set(local_at(d(2026, 11, 13), 9, 5));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(
        text,
        "Die Frist für einen Widerspruch gegen den Bescheid endet am Montag, 16. November. Wenn du nicht einverstanden bist, musst du bis dahin widersprechen."
    );
    assert!(why.contains("§ 37 Abs. 2 SGB X"));
    assert!(why.ends_with("keine Rechtsberatung."));
    // day before 09:00 = Sunday 15.11.
    assert_eq!(buttons[0].label, "Erinnere mich Sonntag");
    assert_eq!(buttons[1].label, "Erledigt");
    assert_eq!(e.eng.answer(fired[0].id, "later", Lang::De).unwrap(), "Okay, ich melde mich in zwei Stunden wieder.");
    e.clock.set(local_at(d(2026, 11, 13), 11, 6));
    assert_eq!(e.eng.tick().unwrap().len(), 1, "snooze fires after 2 h");
}

#[test]
fn letter_deadline_reminder_quotes_letter() {
    let e = env(local_at(d(2026, 10, 26), 10, 0));
    let mut doc = bescheid_doc(d(2026, 10, 20));
    doc.doc_type = crate::docs::DocType::Brief;
    doc.sender = "Hochschule Beispielstadt".into();
    doc.deadlines = vec![ExtractedDeadline {
        title: "Rückmeldung zum Sommersemester".into(),
        due: d(2027, 1, 15),
        source_text: "Rückmeldung bis 15.01.2027".into(),
    }];
    let (_, dls) = ingest(&e, "uni", &doc);
    assert_eq!(dls.len(), 1);
    e.clock.set(local_at(d(2027, 1, 15), 9, 0));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1, "catch-up: only the latest of three due reminders");
    assert!(e.eng.tick().unwrap().is_empty());
    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(text, "Die Frist „Rückmeldung zum Sommersemester“ endet heute. Das steht so im Brief.");
    assert_eq!(why, "Im Brief steht: „Rückmeldung bis 15.01.2027“ Das ist ein Hinweis, keine Rechtsberatung.");
    assert_eq!(buttons.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["done", "later"]);
    let (_, why, _) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::En));
    assert_eq!(why, "The letter says: \"Rückmeldung bis 15.01.2027\" This is a hint, not legal advice.");
}

#[test]
fn agent_added_deadline_reminder() {
    let e = env(local_at(d(2026, 10, 26), 10, 0));
    let id = e
        .mem
        .add_deadline(&NewDeadline {
            title: "Rechnung bezahlen".into(),
            due: d(2026, 11, 5),
            rule_id: None,
            doc_id: None,
            remind_at: vec![local_at(d(2026, 11, 2), 9, 0)],
            why: "Das hast du mir gesagt.".into(),
            source: "Nutzer".into(),
        })
        .unwrap();
    e.clock.set(local_at(d(2026, 11, 2), 9, 0));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired[0].deadline.id, id);
    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(text, "Die Frist „Rechnung bezahlen“ endet am Donnerstag, 5. November.");
    assert_eq!(why, "Das hast du mir gesagt. Das ist ein Hinweis, keine Rechtsberatung.");
    assert_eq!(buttons[0].label, "Erinnere mich Mittwoch");
    assert_eq!(e.eng.answer(fired[0].id, "done", Lang::En).unwrap(), "Good. I won't remind you about it again.");
}

#[test]
fn answer_errors_and_past_remind_time() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    e.clock.set(local_at(d(2026, 10, 29), 18, 0));
    let r = e.eng.tick().unwrap().remove(0);
    assert!(e.eng.answer(r.id, "explode", Lang::De).is_err());
    assert!(e.eng.answer(r.id, "remind:not-a-date", Lang::De).is_err());
    assert!(e.eng.answer(4242, "done", Lang::De).is_err());
    // A remind time already in the past becomes "in two hours".
    let msg = e.eng.answer(r.id, "remind:2026-10-01T08:00:00+02:00", Lang::De).unwrap();
    assert_eq!(msg, "Okay, ich erinnere dich heute um 20:00 Uhr.");
}

#[test]
fn reminder_answer_before_tick_counts_as_fired() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    let dl = e.mem.open_deadlines().unwrap().remove(0);
    let rid = e.mem.add_reminder(dl.id, local_at(d(2026, 10, 22), 9, 0)).unwrap();
    e.eng.answer(rid, "later", Lang::De).unwrap();
    e.clock.set(local_at(d(2026, 10, 22), 9, 30));
    // the answered one does not fire; the snooze (21.10. 12:00) does
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    assert_ne!(fired[0].id, rid);
}

#[test]
fn same_letter_scanned_twice_keeps_one_deadline() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    let (_, a) = ingest(&e, "photo", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    let (_, b) = ingest(&e, "pdf", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    assert_eq!(b.len(), 1, "the second scan still reports the deadline");
    assert_eq!(a[0].id, b[0].id);
    let (_, w1) = ingest(&e, "bescheid-photo", &bescheid_doc(d(2026, 10, 12)));
    let mut again = bescheid_doc(d(2026, 10, 12));
    again.reference = Some(" bg-0815 ".into());
    let (_, w2) = ingest(&e, "bescheid-pdf", &again);
    assert_eq!(w1[0].id, w2[0].id);
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 2);
    e.clock.set(local_at(d(2026, 10, 29), 18, 0));
    assert_eq!(e.eng.tick().unwrap().len(), 1, "one AU bubble, not two");
}

#[test]
fn different_letters_with_same_date_stay_apart() {
    let e = env(local_at(d(2026, 10, 14), 10, 0));
    ingest(&e, "b1", &bescheid_doc(d(2026, 10, 12)));
    let mut other = bescheid_doc(d(2026, 10, 12));
    other.reference = Some("BG-4711".into());
    ingest(&e, "b2", &other);
    let mut no_ref = bescheid_doc(d(2026, 10, 12));
    no_ref.reference = None;
    ingest(&e, "b3", &no_ref);
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 3);
}

#[test]
fn chosen_remind_time_replaces_later_reminders_that_day() {
    let e = env(local_at(d(2026, 10, 21), 10, 0));
    ingest(&e, "au1", &au_doc(d(2026, 10, 20), d(2026, 10, 30)));
    e.clock.set(local_at(d(2026, 10, 29), 18, 0));
    let r = e.eng.tick().unwrap().remove(0);
    let (_, _, buttons) = reminder_parts(e.eng.reminder_event(&r, Lang::De));
    e.eng.answer(r.id, &buttons[0].id, Lang::De).unwrap();
    e.clock.set(local_at(d(2026, 11, 2), 8, 0));
    assert_eq!(e.eng.tick().unwrap().len(), 1);
    e.clock.set(local_at(d(2026, 11, 2), 9, 0));
    assert!(e.eng.tick().unwrap().is_empty(), "09:00 replaced by the chosen 08:00");
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 1, "deadline itself stays open");
}

#[test]
fn snooze_after_due_day_says_the_deadline_ended() {
    let e = env(local_at(d(2026, 10, 14), 10, 0));
    ingest(&e, "b", &bescheid_doc(d(2026, 10, 12))); // due Mon 16.11.
    e.clock.set(local_at(d(2026, 11, 16), 23, 0));
    let r = e.eng.tick().unwrap().remove(0);
    e.eng.answer(r.id, "later", Lang::De).unwrap();
    e.clock.set(local_at(d(2026, 11, 17), 1, 0));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::De));
    assert_eq!(
        text,
        "Die Frist für einen Widerspruch gegen den Bescheid ist am Montag, 16. November, abgelaufen. Wenn noch etwas offen ist, frag am besten direkt beim Absender nach."
    );
    assert!(why.ends_with("keine Rechtsberatung."));
    assert_eq!(buttons.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["done"]);
    assert!(buttons[0].primary);
    let (text, _, _) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::En));
    assert!(text.starts_with("The deadline to object to the decision ended on Monday, November 16."), "{text}");
}

#[test]
fn delayed_flight_gives_one_claim_deadline_in_english() {
    use crate::rules::flight::tests::{flight, notice_doc};
    let e = env(local_at(d(2026, 10, 4), 10, 0));
    e.mem.profile_set("lang", "en").unwrap();
    let doc = notice_doc(flight("DUS", "TFS", Some(250), false, None));
    let (_, dls) = ingest(&e, "air1", &doc);
    assert_eq!(dls.len(), 1, "{dls:?}");
    let dl = &dls[0];
    assert_eq!((dl.rule_id.as_deref(), dl.due), (Some("eu261_claim"), d(2029, 12, 31)));
    assert_eq!(dl.title, "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)");
    assert!(dl.why.contains("may be owed EUR 400 from Beispiel Air"), "{}", dl.why);
    assert!(dl.why.contains("The airline can refuse this in extraordinary circumstances, for example a storm."));
    assert!(dl.why.ends_with("This is a hint, not legal advice."), "{}", dl.why);
    assert_eq!(dl.remind_at, vec![local_at(d(2026, 10, 5), 9, 0)]);

    // The same notice read again (photo after PDF), even with another letter date: no second deadline.
    let mut again = doc.clone();
    again.date = Some(d(2026, 10, 5));
    again.reference = None;
    let (_, dls2) = ingest(&e, "air2", &again);
    assert_eq!(dls2.iter().map(|x| x.id).collect::<Vec<_>>(), vec![dl.id]);
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 1);
    // Another flight is another claim.
    let mut other = doc.clone();
    other.flight.as_mut().unwrap().flight_no = "BX 2042".into();
    ingest(&e, "air3", &other);
    assert_eq!(e.mem.open_deadlines().unwrap().len(), 2);

    // A short delay or a cancellation told three weeks ahead: no deadline.
    let e = env(local_at(d(2026, 10, 4), 10, 0));
    assert!(ingest(&e, "s", &notice_doc(flight("DUS", "PMI", Some(80), false, None))).1.is_empty());
    assert!(ingest(&e, "c", &notice_doc(flight("CGN", "LIS", None, true, Some(21)))).1.is_empty());
    let (_, c4) = ingest(&e, "c4", &notice_doc(flight("CGN", "LIS", None, true, Some(4))));
    assert_eq!(c4.len(), 1);
}

#[test]
fn flight_claim_reminder_says_the_money_and_offers_tomorrow() {
    use crate::rules::flight::tests::{flight, notice_doc};
    let e = env(local_at(d(2026, 10, 4), 10, 0));
    e.mem.profile_set("lang", "en").unwrap();
    ingest(&e, "air1", &notice_doc(flight("DUS", "TFS", Some(250), false, None)));
    e.clock.set(local_at(d(2026, 10, 5), 9, 0));
    let fired = e.eng.tick().unwrap();
    assert_eq!(fired.len(), 1);
    let (text, why, buttons) = reminder_parts(e.eng.reminder_event(&fired[0], Lang::En));
    assert_eq!(
        text,
        "Your flight BX 2041 on Saturday, 3 October was about 4 hours late; you may be owed EUR 400 from Beispiel \
         Air. Shall I help you claim it?"
    );
    assert!(why.contains("Regulation (EC) No 261/2004"), "{why}");
    assert!(buttons[0].id.starts_with("remind:2026-10-06T09:00:00"), "{buttons:?}");
}
