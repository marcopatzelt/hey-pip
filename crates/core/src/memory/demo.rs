//! Demo profile: an invented person ("Erika Beispiel") with letters, deadlines and a short history,
//! so Pip already knows the user when the demo starts (`PIP_DEMO_PROFILE=1`).
//! Everything here is made up and matches the invented letters in `fixtures/letters/`.
//! OWNER: agent "memory-rules".

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate};

use super::Memory;
use crate::docs::extract::FlightInfo;
use crate::docs::pipeline::{bubble_text, sha256_hex};
use crate::docs::{AuInfo, AuKind, DocType, DocumentExtract, ExtractedDeadline};
use crate::i18n::Lang;
use crate::rules::{RuleEngine, local_at};

/// The letter files the demo profile points to (the app writes them into `<data_dir>/demo-letters/`): three
/// letters, Beispiel Air's delay notice and the boarding pass (a document Pip may attach to the claim).
pub const DEMO_LETTER_FILES: &[&str] = &[
    "bescheid_wohngeld.pdf",
    "au_erika_erst_foto.jpg",
    "insurer_letter.pdf",
    "flight_delay_bx2041.pdf",
    "boarding_pass_bx2041.pdf",
];

/// Files of DEMO_LETTER_FILES that are only attachments: skipped when the file is missing (the file dialog can
/// only pick a file that exists), and not needed for a complete seed.
const ATTACHMENT_FILES: &[&str] = &["boarding_pass_bx2041.pdf"];

/// The airline's claim page in the demo (next to the letters folder, like demo-insurer).
const DEMO_AIR_PAGE: &str = "demo-air/index.html";

/// Profile of the invented person. Address and birth date are the ones printed on the invented letters.
/// Keys: `name` (first name, the greeting uses it), `insurer`, `versichertennummer`, `bundesland`,
/// `lang` and `onboarded` are read by app, rules and agent; the rest is context for the agent.
const PROFILE: &[(&str, &str)] = &[
    ("name", "Erika"),
    ("full_name", "Erika Beispiel"),
    ("date_of_birth", "1971-08-12"),
    ("address", "Lindenweg 7, 40999 Beispielstadt"),
    ("phone", "0999 1234567"),
    ("email", "erika.beispiel@example.com"),
    ("insurer", "Evergreen Health"),
    ("versichertennummer", "G123456789"),
    ("airline", "Beispiel Air"),
    ("arbeitgeber", "Bäckerei Sonnenschein GmbH"),
    ("iban", "DE02 1203 0000 0000 2020 51"),
    ("bundesland", "Nordrhein-Westfalen"),
    ("lang", "en"),
    ("onboarded", "1"),
];

/// The demo person's profile facts (one source: the app's demo profile seeds them, the eval's `full` memory on
/// the flight rows too, and the web lookup's privacy test checks no query carries any of them).
pub fn demo_profile() -> &'static [(&'static str, &'static str)] {
    PROFILE
}

/// The day the demo story is told: the day after the insurer's letter, two days before the sick note ends.
/// Every demo date moves by `now - STORY_DAY`, so the story lives on any day.
pub const STORY_DAY: NaiveDate = NaiveDate::from_ymd_opt(2026, 10, 28).expect("valid story day");

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid demo date")
}

/// "30 October 2026"
fn en(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
}

/// "20 October"
fn en_day(d: NaiveDate) -> String {
    d.format("%-d %B").to_string()
}

/// "13 Oct"
fn en_short(d: NaiveDate) -> String {
    d.format("%-d %b").to_string()
}

/// "30.10.2026"
fn de(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

struct DemoLetter {
    file: &'static str,
    /// When Erika gave it to Pip (local time).
    read_at: (NaiveDate, u32, u32),
    extract: DocumentExtract,
    text: String,
}

/// The three letters with every story date moved by `shift` (zero on STORY_DAY).
fn letters(shift: Duration) -> Vec<DemoLetter> {
    let d = |y, m, day| d(y, m, day) + shift;
    let wg_date = d(2026, 10, 14);
    let (au_from, au_until) = (d(2026, 10, 20), d(2026, 10, 30));
    let (insurer_date, insurer_due) = (d(2026, 10, 27), d(2026, 11, 13));
    // Beispiel Air: the flight 25 days before the story day, the delay notice the day after.
    let (flight_date, notice_date) = (d(2026, 10, 3), d(2026, 10, 4));
    vec![
        DemoLetter {
            file: "bescheid_wohngeld.pdf",
            read_at: (d(2026, 10, 16), 18, 5),
            extract: DocumentExtract {
                doc_type: DocType::Bescheid,
                sender: "Stadt Beispielstadt, Amt für Wohnen und Soziales".into(),
                recipient: "Erika Beispiel".into(),
                date: Some(wg_date),
                reference: Some("50.2-WG-2026-04711".into()),
                person: Some("Erika Beispiel".into()),
                // The benefit period is not a story date: it stays as printed.
                summary_simple: "The city grants you housing benefit (Wohngeld) of 96.00 euros a month from \
                                 1 October 2026 to 30 September 2027. Your sick pay was counted as income. Tell the \
                                 city at once if your income or rent changes."
                    .into(),
                requests: vec!["Report any change of your income or rent at once.".into()],
                deadlines: vec![],
                au: None,
                flight: None,
                consequences_if_ignored: Some(
                    "If you do not object within one month, the decision becomes final.".into(),
                ),
                confidence: 0.93,
            },
            text: format!(
                "Stadt Beispielstadt – Amt für Wohnen und Soziales. Aktenzeichen 50.2-WG-2026-04711. \
                 Datum {}. Bescheid über Wohngeld (Mietzuschuss). Ihr Antrag vom 01.09.2026. \
                 Sehr geehrte Frau Beispiel, auf Ihren Antrag vom 01.09.2026 bewillige ich Ihnen für die Zeit \
                 vom 01.10.2026 bis 30.09.2027 Wohngeld in Höhe von monatlich 96,00 Euro. Begründung: Bei der \
                 Berechnung wurde ein monatliches Gesamteinkommen von 1.684,00 Euro und eine zu \
                 berücksichtigende Miete von 612,00 Euro zugrunde gelegt. Ihr Krankengeld wurde als Einkommen \
                 angerechnet. Ändert sich Ihr Einkommen oder Ihre Miete, teilen Sie mir das bitte sofort mit. \
                 Rechtsbehelfsbelehrung: Gegen diesen Bescheid kann innerhalb eines Monats nach Bekanntgabe \
                 Widerspruch erhoben werden.",
                de(wg_date)
            ),
        },
        DemoLetter {
            file: "au_erika_erst_foto.jpg",
            read_at: (au_from, 11, 40),
            extract: DocumentExtract {
                doc_type: DocType::AuBescheinigung,
                sender: "Dr. med. Max Beispiel".into(),
                recipient: "Evergreen Health".into(),
                date: Some(au_from),
                reference: Some("G123456789".into()),
                person: Some("Erika Beispiel".into()),
                summary_simple: format!(
                    "Sick note from Dr. Max Beispiel: you are unable to work from {} to {}. It is a first \
                     certificate. Keep this copy for your records.",
                    en_day(au_from),
                    en(au_until)
                ),
                requests: vec![],
                deadlines: vec![],
                au: Some(AuInfo {
                    from: au_from,
                    until: au_until,
                    first_or_follow: AuKind::Erst,
                    issued: Some(au_from),
                }),
                flight: None,
                consequences_if_ignored: Some(
                    "If you are still ill afterwards and the next sick note leaves a gap, the health insurer \
                     may not pay sick pay for those days."
                        .into(),
                ),
                confidence: 0.9,
            },
            text: format!(
                "Arbeitsunfähigkeitsbescheinigung. Krankenkasse: Evergreen Health. Beispiel, Erika, \
                 Lindenweg 7, 40999 Beispielstadt, geb. am 12.08.1971. Versicherten-Nr. G123456789. \
                 Erstbescheinigung. Arbeitsunfähig seit {from}, voraussichtlich arbeitsunfähig bis \
                 einschließlich {until}, festgestellt am {from}. Ausgestellt am {from}. \
                 Dr. med. Max Beispiel, Facharzt für Allgemeinmedizin.",
                from = de(au_from),
                until = de(au_until)
            ),
        },
        DemoLetter {
            file: "insurer_letter.pdf",
            read_at: (d(2026, 10, 28), 9, 20),
            extract: DocumentExtract {
                doc_type: DocType::Brief,
                sender: "Evergreen Health".into(),
                recipient: "Erika Beispiel".into(),
                date: Some(insurer_date),
                reference: Some("G123456789".into()),
                person: Some("Erika Beispiel".into()),
                summary_simple: format!(
                    "Evergreen Health got your sick note of {}. To check your sick pay they need the enclosed \
                     questionnaire, filled in and signed, and your IBAN if it changed. Please send both by {}.",
                    en_day(au_from),
                    en(insurer_due)
                ),
                requests: vec![
                    "Fill in and sign the questionnaire about your inability to work.".into(),
                    "Send your bank details (IBAN) if they changed.".into(),
                ],
                deadlines: vec![ExtractedDeadline {
                    title: "Send the questionnaire back to Evergreen Health".into(),
                    due: insurer_due,
                    source_text: format!(
                        "Bitte senden Sie uns die Unterlagen bis spätestens {} zurück.",
                        de(insurer_due)
                    ),
                }],
                au: None,
                flight: None,
                consequences_if_ignored: Some("Your sick pay may be paid later.".into()),
                confidence: 0.94,
            },
            text: format!(
                "Evergreen Health – Die Krankenkasse für Beispielstadt. Frau Erika Beispiel, Lindenweg 7, 40999 \
                 Beispielstadt. Versicherten-Nr. G123456789. Datum {}. Ihre Krankschreibung vom \
                 {} – bitte Fragebogen zurücksenden. Damit wir Ihren Anspruch auf Krankengeld prüfen \
                 können, brauchen wir noch: den beiliegenden Fragebogen zur Arbeitsunfähigkeit, ausgefüllt und \
                 unterschrieben, Ihre Bankverbindung (IBAN), falls sie sich geändert hat. Bitte senden Sie uns \
                 die Unterlagen bis spätestens {} zurück. Wenn die Unterlagen später kommen, kann sich \
                 die Zahlung verzögern. Kontaktformular: www.evergreen-health.example/kontakt",
                de(insurer_date),
                de(au_from),
                de(insurer_due)
            ),
        },
        DemoLetter {
            file: "flight_delay_bx2041.pdf",
            read_at: (notice_date, 19, 10),
            extract: DocumentExtract {
                doc_type: DocType::Brief,
                sender: "Beispiel Air".into(),
                recipient: "Erika Beispiel".into(),
                date: Some(notice_date),
                reference: Some("BX7Q2K".into()),
                person: Some("Erika Beispiel".into()),
                summary_simple: format!(
                    "Beispiel Air says sorry: your flight BX 2041 from Düsseldorf to Tenerife on {} arrived about \
                     4 hours late. You can ask for compensation on their claim page.",
                    en(flight_date)
                ),
                requests: vec!["Use the claim page if you want compensation.".into()],
                deadlines: vec![],
                au: None,
                flight: Some(FlightInfo {
                    airline: "Beispiel Air".into(),
                    flight_no: "BX 2041".into(),
                    date: flight_date,
                    from: "DUS".into(),
                    to: "TFS".into(),
                    delay_minutes: Some(250),
                    cancelled: false,
                    notice_days: None,
                    booking_ref: Some("BX7Q2K".into()),
                }),
                consequences_if_ignored: None,
                confidence: 0.92,
            },
            text: format!(
                "Beispiel Air – Customer Care. Dear Ms Beispiel, booking reference BX7Q2K. We are sorry that your \
                 flight BX 2041 from Düsseldorf (DUS) to Tenerife South (TFS) on {} arrived 4 hours and 10 minutes \
                 late (planned 10:05, landed 14:15). You can claim compensation on our claim page. Date {}.",
                de(flight_date),
                de(notice_date)
            ),
        },
        DemoLetter {
            file: "boarding_pass_bx2041.pdf",
            read_at: (flight_date, 7, 30),
            extract: DocumentExtract {
                doc_type: DocType::Sonstiges,
                sender: "Beispiel Air".into(),
                recipient: "Erika Beispiel".into(),
                date: Some(flight_date),
                reference: Some("BX7Q2K".into()),
                person: Some("Erika Beispiel".into()),
                summary_simple: format!(
                    "Boarding pass for flight BX 2041, Erika Beispiel, Düsseldorf → Tenerife, {}",
                    en_day(flight_date)
                ),
                requests: vec![],
                deadlines: vec![],
                au: None,
                flight: None,
                consequences_if_ignored: None,
                confidence: 0.95,
            },
            text: format!(
                "Boarding pass. Beispiel Air. BEISPIEL/ERIKA MS. Flight BX 2041. From Düsseldorf DUS to Tenerife South \
                 TFS. Date {}. Booking reference BX7Q2K. Seat 14C.",
                de(flight_date)
            ),
        },
    ]
}

/// Earlier history (kind, local time, text), oldest first, moved by `shift` like the letters.
fn history(shift: Duration) -> Vec<(&'static str, (NaiveDate, u32, u32), String)> {
    let d = |y, m, day| d(y, m, day) + shift;
    vec![(
        "done",
        (d(2026, 10, 14), 16, 40),
        format!("Sent the sick note of {} to Evergreen Health.", en_short(d(2026, 10, 13))),
    )]
}

/// Fill an empty memory with the invented demo person. Only runs when the profile has no `name`
/// yet (never overwrites a real profile); returns true if it seeded.
/// The story is told relative to `now` (DEMO_NOW or the real clock): the sick note ends in two days,
/// the insurer's letter came yesterday, the housing-benefit decision is two weeks old. The invented
/// PDFs/photo keep their printed October dates; the demo only needs the memory.
/// Letters dated after `now` are left out (a guard only; after the shift every letter lies before `now`), and `rules` (with its own clock) creates deadlines and
/// reminders exactly like a real read, so past reminders are not planned again.
/// `letters_dir` holds DEMO_LETTER_FILES (paths in memory point there).
pub fn seed_demo_profile(
    memory: &Memory,
    rules: &RuleEngine,
    letters_dir: &Path,
    now: DateTime<Local>,
) -> Result<bool> {
    if memory.profile_get("name")?.is_some() {
        return Ok(false);
    }
    // Language first: the rules write deadline titles in the profile language.
    for (k, v) in PROFILE {
        memory.profile_set(k, v)?;
    }
    // The airline's claim page of the demo, when it lies next to the letters folder: `fixtures/demo-air/` beside
    // `fixtures/letters/`, or `<data_dir>/fixtures/demo-air/` beside the app's `<data_dir>/demo-letters/`.
    let air = letters_dir
        .parent()
        .into_iter()
        .flat_map(|p| [p.join(DEMO_AIR_PAGE), p.join("fixtures").join(DEMO_AIR_PAGE)])
        .find_map(|p| p.canonicalize().ok());
    match air {
        Some(page) => memory.profile_set(crate::rules::flight::PROFILE_CLAIM_FORM, &crate::config::file_url(&page))?,
        None => tracing::warn!("demo airline page {DEMO_AIR_PAGE} not found next to the letters: no claim page"),
    }
    let lang = Lang::En;

    let shift = now.date_naive() - STORY_DAY;
    let mut events: Vec<(DateTime<Local>, &str, String)> = Vec::new();
    for l in letters(shift) {
        if l.extract.date.is_some_and(|date| date > now.date_naive()) {
            continue;
        }
        let path = letters_dir.join(l.file);
        // The real file hash, so giving Pip the same file again is recognised as known.
        let bytes = std::fs::read(&path);
        if bytes.is_err() {
            if ATTACHMENT_FILES.contains(&l.file) {
                tracing::warn!(file = l.file, "demo attachment missing: skipped");
                continue;
            }
            tracing::warn!(file = l.file, "demo letter file missing: memory keeps the letter without its file");
        }
        let sha = bytes.map(|b| sha256_hex(&b)).unwrap_or_else(|_| format!("demo:{}", l.file));
        let id = memory.insert_document(&path, &sha, &l.extract, &l.text)?;
        let deadlines = rules.apply_document(id, &l.extract)?;
        let (day, h, m) = l.read_at;
        let at = local_at(day, h, m).min(now - Duration::minutes(5));
        events.push((at, "letter", bubble_text(&l.extract, &deadlines, lang)));
    }
    for (kind, (day, h, m), text) in history(shift) {
        let at = local_at(day, h, m);
        if at < now {
            events.push((at, kind, text));
        }
    }
    events.sort_by_key(|(at, _, _)| *at);
    for (at, kind, text) in &events {
        memory.add_event_at(*at, kind, text)?;
    }
    Ok(true)
}

/// The demo person's memory has the whole story: `full_name` is exactly Erika's, every letter of
/// DEMO_LETTER_FILES is in memory (the boarding pass only when its file exists), and the insurer's
/// questionnaire deadline (`frist_aus_brief`) and the flight claim (`eu261_claim`) exist, in any status. A questionnaire the user already sent (deadline Done) is progress, not a stale
/// seed: what Pip learned on the way (playbooks, facts) must survive a restart. False for any other
/// person (the caller never touches their memory) and for an Erika seeded on an older day or by an older
/// build (a letter or the deadline missing: the older seed skipped the future-dated letter): the app then
/// seeds her again, so the proactive offer has something to offer on every recording day.
pub fn demo_seed_is_complete(memory: &Memory) -> Result<bool> {
    if memory.profile_get("full_name")?.as_deref() != Some(DEMO_FULL_NAME) {
        return Ok(false);
    }
    let mut files: Vec<String> = Vec::new();
    let mut questionnaire = false;
    let mut claim = false;
    for t in ALL_DOC_TYPES {
        for row in memory.documents_of_type(t)? {
            if let Some(name) = row.path.file_name().and_then(|n| n.to_str()) {
                files.push(name.to_string());
            }
            let rules: Vec<_> =
                memory.deadlines_for_document(row.id)?.into_iter().filter_map(|dl| dl.rule_id).collect();
            questionnaire |= rules.iter().any(|r| r == "frist_aus_brief");
            claim |= rules.iter().any(|r| r == crate::rules::flight::EU261_RULE);
        }
    }
    let letters =
        DEMO_LETTER_FILES.iter().filter(|f| !ATTACHMENT_FILES.contains(f)).all(|f| files.iter().any(|have| have == f));
    Ok(letters && questionnaire && claim)
}

/// The demo person's full name (the profile's `full_name` after `seed_demo_profile`).
const DEMO_FULL_NAME: &str = "Erika Beispiel";

const ALL_DOC_TYPES: [DocType; 7] = [
    DocType::AuBescheinigung,
    DocType::Bescheid,
    DocType::Rechnung,
    DocType::Mahnung,
    DocType::Vertrag,
    DocType::Brief,
    DocType::Sonstiges,
];

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::{Clock, FixedClock};
    use crate::memory::DeadlineStatus;

    fn letters_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters")
    }

    /// 1 when the boarding pass file exists (it is seeded only then), else 0.
    fn boarding() -> usize {
        usize::from(letters_dir().join("boarding_pass_bx2041.pdf").exists())
    }

    fn setup(now: DateTime<Local>) -> (Arc<Memory>, RuleEngine) {
        let mem = Arc::new(Memory::open_in_memory().unwrap());
        let rules = RuleEngine::new(mem.clone(), Arc::new(FixedClock::new(now)) as Arc<dyn Clock>);
        (mem, rules)
    }

    #[test]
    fn seeds_profile_letters_deadlines_and_events_once() {
        let now = local_at(d(2026, 10, 28), 14, 0);
        let (mem, rules) = setup(now);
        assert!(seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());

        for (k, v) in [
            ("name", "Erika"),
            ("full_name", "Erika Beispiel"),
            ("insurer", "Evergreen Health"),
            ("versichertennummer", "G123456789"),
            ("lang", "en"),
            ("onboarded", "1"),
            ("bundesland", "Nordrhein-Westfalen"),
            ("airline", "Beispiel Air"),
        ] {
            assert_eq!(mem.profile_get(k).unwrap().as_deref(), Some(v), "{k}");
        }
        // The claim page is set only when fixtures/demo-air exists next to the letters.
        let air = letters_dir().join("../demo-air/index.html");
        let claim_form = mem.profile_get("airline_claim_form").unwrap();
        assert_eq!(claim_form.is_some(), air.exists());
        assert!(claim_form.is_none_or(|u| u.starts_with("file:") && u.ends_with("demo-air/index.html")));

        let docs: Vec<_> = [DocType::AuBescheinigung, DocType::Bescheid, DocType::Brief]
            .iter()
            .flat_map(|t| mem.documents_of_type(*t).unwrap())
            .collect();
        assert_eq!(docs.len(), 4);
        assert_eq!(mem.documents_of_type(DocType::Sonstiges).unwrap().len(), boarding());
        assert!(docs.iter().all(|r| r.path.starts_with(letters_dir())));
        // The real file hash: the same file dropped again is a known letter.
        let foto = std::fs::read(letters_dir().join("au_erika_erst_foto.jpg")).unwrap();
        assert!(mem.document_by_sha(&sha256_hex(&foto)).unwrap().is_some());

        let open: Vec<_> = mem.open_deadlines().unwrap().into_iter().map(|dl| (dl.rule_id.unwrap(), dl.due)).collect();
        assert_eq!(
            open,
            vec![
                ("au_lueckenlos".to_string(), d(2026, 11, 2)),
                ("frist_aus_brief".to_string(), d(2026, 11, 13)),
                ("widerspruch_bescheid".to_string(), d(2026, 11, 18)),
                ("eu261_claim".to_string(), d(2029, 12, 31)),
            ]
        );
        let claim = mem.open_deadlines().unwrap().pop().unwrap();
        assert_eq!(claim.title, "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)");
        assert_eq!(claim.remind_at, vec![local_at(d(2026, 10, 29), 9, 0)], "tomorrow morning");
        let all_open = mem.open_deadlines().unwrap();
        assert!(all_open.iter().all(|dl| dl.status == DeadlineStatus::Open && !dl.remind_at.is_empty()));
        assert!(all_open.iter().all(|dl| dl.remind_at.iter().all(|t| *t >= local_at(now.date_naive(), 0, 0))));
        assert!(all_open.iter().any(|dl| dl.title == "New sick note, if you are still ill"));

        let events = mem.recent_events(10).unwrap();
        assert_eq!(events.len(), 5 + boarding());
        assert!(events.iter().all(|e| e.at < now));
        assert!(events.iter().any(|e| e.kind == "done" && e.text.contains("sick note of 13 Oct")));
        assert_eq!(events.iter().filter(|e| e.kind == "letter").count(), 4 + boarding());
        assert!(demo_seed_is_complete(&mem).unwrap());

        // Second call: no-op.
        assert!(!seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        assert_eq!(mem.open_deadlines().unwrap().len(), 4);
        assert_eq!(mem.recent_events(10).unwrap().len(), 5 + boarding());
    }

    #[test]
    fn story_moves_with_today() {
        // 26 Oct: two days before STORY_DAY, the whole story moves back two days.
        let now = local_at(d(2026, 10, 26), 10, 0);
        let (mem, rules) = setup(now);
        assert!(seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        let mut briefe = mem.documents_of_type(DocType::Brief).unwrap();
        assert_eq!(briefe.len(), 2);
        briefe.retain(|b| b.extract.flight.is_none());
        assert!(briefe[0].extract.summary_simple.contains("11 November 2026"), "{}", briefe[0].extract.summary_simple);
        // The flight moves too: two days earlier, the claim stays due at the end of 2029.
        let air = mem.search_documents("BX7Q2K", 5).unwrap();
        let flight = air.iter().find_map(|r| r.extract.flight.clone()).expect("the delay notice");
        assert_eq!(flight.date, d(2026, 10, 1));
        let au = mem.documents_of_type(DocType::AuBescheinigung).unwrap();
        assert!(au[0].extract.summary_simple.contains("28 October 2026"));
        assert!(!mem.search_documents("28.10.2026", 5).unwrap().is_empty());
        assert!(!mem.search_documents("11.11.2026", 5).unwrap().is_empty());
        // The sick note ends Wednesday 28 Oct: the next one is due Thursday 29 Oct.
        let due: Vec<_> = mem.open_deadlines().unwrap().iter().map(|dl| dl.due).collect();
        assert_eq!(due, vec![d(2026, 10, 29), d(2026, 11, 11), d(2026, 11, 16), d(2029, 12, 31)]);
        assert!(mem.recent_events(10).unwrap().iter().all(|e| e.at < now));
        assert_eq!(mem.recent_events(10).unwrap().len(), 5 + boarding());

        let (mem, rules) = setup(now);
        mem.profile_set("name", "Jonas").unwrap();
        assert!(!seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        assert_eq!(mem.profile_get("insurer").unwrap(), None);
        assert!(mem.open_deadlines().unwrap().is_empty());
    }

    #[test]
    fn stale_erika_seed_is_detected() {
        let now = local_at(d(2026, 10, 26), 10, 0);
        // An Erika from an older build: name and full name, but no letters and no deadlines.
        let (mem, rules) = setup(now);
        mem.profile_set("name", "Erika").unwrap();
        mem.profile_set("full_name", "Erika Beispiel").unwrap();
        assert!(!demo_seed_is_complete(&mem).unwrap());
        mem.wipe_everything().unwrap();
        assert!(seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        assert!(demo_seed_is_complete(&mem).unwrap());
        // The questionnaire sent (deadline Done): progress, not a stale seed. What Pip learned stays.
        let q = mem
            .open_deadlines()
            .unwrap()
            .into_iter()
            .find(|dl| dl.rule_id.as_deref() == Some("frist_aus_brief"))
            .unwrap();
        mem.set_deadline_status(q.id, DeadlineStatus::Done).unwrap();
        assert!(demo_seed_is_complete(&mem).unwrap());

        // Someone else: never "complete", and this check changes nothing.
        let (mem, _) = setup(now);
        mem.profile_set("name", "Jonas").unwrap();
        mem.profile_set("full_name", "Jonas Muster").unwrap();
        assert!(!demo_seed_is_complete(&mem).unwrap());
        assert_eq!(mem.profile_get("full_name").unwrap().as_deref(), Some("Jonas Muster"));
    }

    #[test]
    fn judges_day_is_alive() {
        // Friday 6 Nov, nine days after STORY_DAY.
        let now = local_at(d(2026, 11, 6), 9, 0);
        let (mem, rules) = setup(now);
        assert!(seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        let au = mem.documents_of_type(DocType::AuBescheinigung).unwrap();
        assert_eq!(au[0].extract.au.as_ref().unwrap().until, d(2026, 11, 8));
        let open: Vec<_> = mem.open_deadlines().unwrap().into_iter().map(|dl| (dl.rule_id.unwrap(), dl.due)).collect();
        assert_eq!(
            open,
            vec![
                ("au_lueckenlos".to_string(), d(2026, 11, 9)),
                ("frist_aus_brief".to_string(), d(2026, 11, 22)),
                ("widerspruch_bescheid".to_string(), d(2026, 11, 27)),
                ("eu261_claim".to_string(), d(2029, 12, 31)),
            ]
        );
        assert!(open.iter().all(|(_, due)| *due >= now.date_naive()));
        assert!(!mem.search_documents("22.11.2026", 5).unwrap().is_empty());
        let events = mem.recent_events(10).unwrap();
        assert_eq!(events.len(), 5 + boarding());
        assert!(events.iter().all(|e| e.at < now));
        assert!(events.iter().any(|e| e.kind == "done" && e.text.contains("sick note of 22 Oct")));
    }

    #[test]
    fn thursday_evening_reminder_after_story_day() {
        use crate::clock::DemoClock;
        use crate::events::UiEvent;

        let now = local_at(STORY_DAY, 14, 0);
        let (mem, rules) = setup(now);
        assert!(seed_demo_profile(&mem, &rules, &letters_dir(), now).unwrap());
        // Restart with DEMO_NOW=2026-10-29T18:00.
        let rules =
            RuleEngine::new(mem.clone(), Arc::new(DemoClock::parse("2026-10-29T18:00").unwrap()) as Arc<dyn Clock>);
        // The sick note at 18:00, and the flight claim's morning reminder that rang while Pip was off.
        let fired = rules.tick().unwrap();
        let rules_fired: Vec<_> = fired.iter().filter_map(|r| r.deadline.rule_id.clone()).collect();
        assert_eq!(rules_fired, ["eu261_claim", "au_lueckenlos"]);
        let au = fired.iter().find(|r| r.deadline.rule_id.as_deref() == Some("au_lueckenlos")).unwrap();
        let UiEvent::Reminder { text, .. } = rules.reminder_event(au, Lang::En) else { panic!() };
        assert!(text.starts_with("Your sick note ends tomorrow."), "{text}");
    }
}
