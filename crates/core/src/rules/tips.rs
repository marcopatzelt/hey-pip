//! "Good to know": curated rules with a source that Pip says once, when a letter is read or in the daily brief.
//! OWNER: builder C (rules).
//!
//! What Pip may say: a general rule that fits the letter or deadline in front of the user ("Good to know: …"),
//! worded with "usually" or "may", the law it comes from (source + link) and "Please check …" at the end.
//! What Pip may not say: advice (no "you must", "sue", "claim now", "lawyer"), verdicts ("scam", "guaranteed"),
//! or anything about health or money beyond the rule itself. The user decides; the letter's own notice wins.
//! The model only reads the letter (Muse Spark); this table knows the rules; Pip only phrases one of them.
//! At most one tip per letter or brief; the same tip at most once in TIP_REPEAT_DAYS; profile `tips` = "off" stops all.

use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};

use super::au::{au_due, au_end};
use super::text::{fmt_date, weekday_name};
use crate::docs::{DocType, DocumentExtract};
use crate::i18n::Lang;
use crate::memory::{Deadline, Memory};

/// Event kind written when a tip was said (text = the tip id).
pub const EV_TIP_SHOWN: &str = "tip_shown";
/// The same tip is said at most once in this many days.
pub const TIP_REPEAT_DAYS: i64 = 7;
/// The daily brief names an AU gap tip when the follow-up note is due within this many days.
pub const BRIEF_TIP_DAYS: i64 = 7;
/// How many recent events the repeat check looks at.
const EVENTS_SCAN: u32 = 200;

/// A tip ready to say.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub id: &'static str,
    /// Placeholders filled, in the user's language.
    pub text: String,
    /// The law, e.g. "§ 46 Satz 2 SGB V".
    pub source: &'static str,
    /// Where the user can read the law (https).
    pub url: &'static str,
    /// The deadline a brief tip is about (None for a letter tip).
    pub deadline_id: Option<i64>,
}

/// When a rule fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A sick note with a last day (`au.until`).
    AuLetter,
    /// A decision letter (doc_type bescheid) for which the `widerspruch_bescheid` rule made a deadline.
    BescheidWithRechtsbehelf,
    /// An open deadline of this rule id, due within this many days (daily brief).
    DeadlineWithin(i64, &'static str),
}

/// One curated rule. Placeholders: `{due}`, `{until}` (dates like "Monday, 2 November").
#[derive(Debug, Clone, Copy)]
pub struct TipRule {
    pub id: &'static str,
    pub triggers: &'static [Trigger],
    pub text_en: &'static str,
    pub text_de: &'static str,
    /// ONE sentence for the daily brief (the brief stays short, so "please check" ends it after a semicolon);
    /// None = this tip is not said in the brief.
    pub brief_en: Option<&'static str>,
    pub brief_de: Option<&'static str>,
    pub source: &'static str,
    pub url: &'static str,
    /// Where the rule holds (ISO country), None = everywhere.
    pub region: Option<&'static str>,
}

/// The curated table. The first rule that fits wins.
pub const TIPS: &[TipRule] = &[
    TipRule {
        id: "au_gap",
        triggers: &[Trigger::AuLetter, Trigger::DeadlineWithin(BRIEF_TIP_DAYS, "au_lueckenlos")],
        text_en: "Good to know: your insurer may only pay sick pay without a gap. If you're still ill, the next note \
                  usually has to be issued by {due} (the next working day after {until}). Please check with your insurer.",
        text_de: "Gut zu wissen: Deine Krankenkasse darf Krankengeld meist nur ohne Lücke zahlen. Wenn du noch krank \
                  bist, muss die nächste Krankschreibung in der Regel bis {due} ausgestellt sein (der nächste Werktag \
                  nach {until}). Bitte frag bei deiner Krankenkasse nach.",
        brief_en: Some(
            "Good to know: if you're still ill, the next note usually has to be issued by {due}, or your insurer may \
             not pay sick pay for the gap; please check with your insurer.",
        ),
        brief_de: Some(
            "Gut zu wissen: Wenn du noch krank bist, muss die nächste Krankschreibung in der Regel bis {due} \
             ausgestellt sein, sonst zahlt die Krankenkasse für die Lücke vielleicht kein Krankengeld; bitte frag bei \
             ihr nach.",
        ),
        source: "§ 46 Satz 2 SGB V",
        url: "https://www.gesetze-im-internet.de/sgb_5/__46.html",
        region: Some("DE"),
    },
    TipRule {
        id: "widerspruch_month",
        triggers: &[Trigger::BescheidWithRechtsbehelf],
        text_en: "Good to know: you usually have one month from the day the letter arrived to object, in writing. \
                  After that the decision usually stands. Please check the last page of the letter.",
        text_de: "Gut zu wissen: Du hast meist einen Monat ab dem Tag, an dem der Brief kam, um schriftlich zu \
                  widersprechen. Danach gilt die Entscheidung in der Regel. Bitte schau auf die letzte Seite des Briefs.",
        brief_en: None,
        brief_de: None,
        source: "§ 84 SGG / § 70 VwGO",
        url: "https://www.gesetze-im-internet.de/sgg/__84.html",
        region: Some("DE"),
    },
];

/// Profile key `tips` is "off": no tip anywhere.
pub fn tips_off(mem: &Memory) -> Result<bool> {
    Ok(mem.profile_get("tips")?.is_some_and(|v| v.trim().eq_ignore_ascii_case("off")))
}

/// The tip for a letter just read (at most one; the first rule that fits wins). `deadlines`: what the rules made
/// of the letter. A sick note whose follow-up date is before `today` gets no tip.
pub fn for_letter(extract: &DocumentExtract, deadlines: &[Deadline], today: NaiveDate, lang: Lang) -> Option<Tip> {
    TIPS.iter().find_map(|rule| {
        rule.triggers.iter().find_map(|trigger| match trigger {
            Trigger::AuLetter => {
                let until = au_end(extract)?;
                let due = au_due(until);
                (due >= today).then(|| tip(rule, text_of(rule, lang), lang, due, until, None))
            }
            Trigger::BescheidWithRechtsbehelf => {
                let has = extract.doc_type == DocType::Bescheid
                    && deadlines.iter().any(|d| d.rule_id.as_deref() == Some("widerspruch_bescheid"));
                has.then(|| fill_tip(rule, text_of(rule, lang), &[], None))
            }
            Trigger::DeadlineWithin(..) => None,
        })
    })
}

/// `for_letter`, unless the user switched tips off.
pub fn for_letter_in(
    mem: &Memory,
    extract: &DocumentExtract,
    deadlines: &[Deadline],
    today: NaiveDate,
    lang: Lang,
) -> Result<Option<Tip>> {
    if tips_off(mem)? {
        return Ok(None);
    }
    Ok(for_letter(extract, deadlines, today, lang))
}

/// The tip for the daily brief: the nearest open deadline of a `DeadlineWithin` rule, due within its days (not
/// overdue), whose tip was not said in the last TIP_REPEAT_DAYS. The text is the rule's one-sentence brief form.
/// Read only: the caller writes `mark_shown` when Pip says it.
pub fn for_brief(mem: &Memory, now: DateTime<Local>, lang: Lang) -> Result<Option<Tip>> {
    if tips_off(mem)? {
        return Ok(None);
    }
    let today = now.date_naive();
    let open = mem.open_deadlines()?;
    let since = now - Duration::days(TIP_REPEAT_DAYS);
    let events = mem.recent_events(EVENTS_SCAN)?;
    let shown = |id: &str| events.iter().any(|e| e.kind == EV_TIP_SHOWN && e.text.trim() == id && e.at >= since);
    for rule in TIPS {
        let Some(text) = brief_of(rule, lang) else { continue };
        for trigger in rule.triggers {
            let Trigger::DeadlineWithin(days, rule_id) = *trigger else { continue };
            if shown(rule.id) {
                continue;
            }
            let near = open
                .iter()
                .filter(|d| {
                    d.rule_id.as_deref() == Some(rule_id) && d.due >= today && d.due <= today + Duration::days(days)
                })
                .min_by_key(|d| d.due);
            let Some(dl) = near else { continue };
            let doc = match dl.doc_id {
                Some(id) => mem.document_by_id(id)?,
                None => None,
            };
            // The sick note's last day: from its letter; without one, the day before the due date is enough
            // for the wording (the brief form names only {due}).
            let until = doc.and_then(|row| row.extract.au).map(|au| au.until).unwrap_or(dl.due - Duration::days(1));
            return Ok(Some(tip(rule, text, lang, dl.due, until, Some(dl.id))));
        }
    }
    Ok(None)
}

/// Remember that Pip said `tip` at `now` (event `tip_shown`, text = tip id).
pub fn mark_shown(mem: &Memory, tip: &Tip, now: DateTime<Local>) -> Result<()> {
    mem.add_event_at(now, EV_TIP_SHOWN, tip.id)
}

fn text_of(rule: &TipRule, lang: Lang) -> &'static str {
    match lang {
        Lang::En => rule.text_en,
        Lang::De => rule.text_de,
    }
}

fn brief_of(rule: &TipRule, lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::En => rule.brief_en,
        Lang::De => rule.brief_de,
    }
}

fn tip(rule: &TipRule, text: &str, lang: Lang, due: NaiveDate, until: NaiveDate, deadline_id: Option<i64>) -> Tip {
    fill_tip(rule, text, &[("due", &tip_date(due, lang)), ("until", &tip_date(until, lang))], deadline_id)
}

fn fill_tip(rule: &TipRule, text: &str, args: &[(&str, &str)], deadline_id: Option<i64>) -> Tip {
    let mut out = text.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    Tip { id: rule.id, text: out, source: rule.source, url: rule.url, deadline_id }
}

/// "Monday, 2 November" / "Montag, 2. November".
pub fn tip_date(d: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::De => fmt_date(d, lang),
        Lang::En => {
            let month = d.format("%B").to_string();
            format!("{}, {} {month}", weekday_name(d.weekday(), lang), d.day())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::Value;

    use super::*;
    use crate::docs::{AuInfo, AuKind};
    use crate::memory::{DeadlineStatus, NewDeadline};
    use crate::rules::local_at;
    use crate::rules::tests_support::{au_doc, bescheid_doc, d};

    const BANNED_EN: &[&str] = &["must sue", "claim now", "you must", "scam", "guaranteed", "lawyer"];
    const BANNED_DE: &[&str] = &["du musst", "klag", "sofort", "betrug", "garantiert", "anwalt"];

    fn deadline(id: i64, rule: &str, due: NaiveDate) -> Deadline {
        Deadline {
            id,
            title: rule.into(),
            due,
            rule_id: Some(rule.into()),
            doc_id: Some(1),
            status: DeadlineStatus::Open,
            remind_at: vec![],
            why: String::new(),
            source: String::new(),
        }
    }

    fn check_en(text: &str) {
        assert!(text.contains("Please check"), "{text}");
        check_brief_en(text);
    }

    /// The one-sentence brief form: the same words, "please check" after a semicolon.
    fn check_brief_en(text: &str) {
        assert!(text.starts_with("Good to know"), "{text}");
        assert!(text.contains("usually") || text.contains("may"), "{text}");
        assert!(text.to_lowercase().contains("please check"), "{text}");
        let low = text.to_lowercase();
        for w in BANNED_EN {
            assert!(!low.contains(w), "{w}: {text}");
        }
    }

    fn check_de(text: &str) {
        assert!(text.starts_with("Gut zu wissen"), "{text}");
        assert!(text.contains("meist") || text.contains("in der Regel") || text.contains("vielleicht"), "{text}");
        assert!(text.to_lowercase().contains("bitte"), "{text}");
        let low = text.to_lowercase();
        for w in BANNED_DE.iter().chain(BANNED_EN) {
            assert!(!low.contains(w), "{w}: {text}");
        }
    }

    #[test]
    fn every_rule_has_a_source_a_link_and_safe_words() {
        let mut ids = Vec::new();
        for r in TIPS {
            assert!(!r.source.trim().is_empty(), "{}", r.id);
            assert!(r.source.contains('§'), "{}", r.id);
            assert!(r.url.starts_with("https://") && r.url.len() > "https://".len() + 3, "{}", r.id);
            assert!(!r.triggers.is_empty(), "{}", r.id);
            assert!(!ids.contains(&r.id), "duplicate id {}", r.id);
            ids.push(r.id);
            check_en(r.text_en);
            check_de(r.text_de);
            assert_eq!(r.brief_en.is_some(), r.brief_de.is_some(), "{}", r.id);
            if let (Some(en), Some(de)) = (r.brief_en, r.brief_de) {
                check_brief_en(en);
                check_de(de);
                for b in [en, de] {
                    assert_eq!(b.trim_end().matches(['.', '!', '?']).count(), 1, "one sentence: {b}");
                }
            }
            // Brief form only for rules the brief can trigger.
            let brief = r.triggers.iter().any(|t| matches!(t, Trigger::DeadlineWithin(..)));
            assert_eq!(brief, r.brief_en.is_some(), "{}", r.id);
        }
    }

    #[test]
    fn filled_tips_keep_the_rules_and_have_no_placeholders() {
        let au = au_doc(d(2026, 10, 20), d(2026, 10, 30));
        let bescheid = bescheid_doc(d(2026, 10, 14));
        let wd = [deadline(1, "widerspruch_bescheid", d(2026, 11, 18))];
        for lang in [Lang::En, Lang::De] {
            for (x, dls) in [(&au, &[][..]), (&bescheid, &wd[..])] {
                let t = for_letter(x, dls, d(2026, 10, 21), lang).expect("tip");
                assert!(!t.text.contains('{'), "{}", t.text);
                match lang {
                    Lang::En => check_en(&t.text),
                    Lang::De => check_de(&t.text),
                }
            }
        }
    }

    #[test]
    fn au_gap_names_the_next_working_day() {
        let x = au_doc(d(2026, 10, 20), d(2026, 10, 30));
        let t = for_letter(&x, &[], d(2026, 10, 27), Lang::En).unwrap();
        assert_eq!(t.id, "au_gap");
        assert_eq!(t.source, "§ 46 Satz 2 SGB V");
        assert_eq!(t.url, "https://www.gesetze-im-internet.de/sgb_5/__46.html");
        assert!(t.text.contains("by Monday, 2 November (the next working day after Friday, 30 October)"), "{}", t.text);
        let de = for_letter(&x, &[], d(2026, 10, 27), Lang::De).unwrap();
        assert!(de.text.contains("bis Montag, 2. November ausgestellt"), "{}", de.text);
        // Past follow-up date: nothing to say any more.
        assert_eq!(for_letter(&x, &[], d(2026, 11, 3), Lang::En), None);
    }

    #[test]
    fn a_bescheid_without_the_widerspruch_deadline_gets_no_tip() {
        let x = bescheid_doc(d(2026, 10, 14));
        assert_eq!(for_letter(&x, &[], d(2026, 10, 20), Lang::En), None);
        let other = [deadline(1, "frist_aus_brief", d(2026, 11, 18))];
        assert_eq!(for_letter(&x, &other, d(2026, 10, 20), Lang::En), None);
        let wd = [deadline(1, "widerspruch_bescheid", d(2026, 11, 18))];
        let t = for_letter(&x, &wd, d(2026, 10, 20), Lang::En).unwrap();
        assert_eq!((t.id, t.source), ("widerspruch_month", "§ 84 SGG / § 70 VwGO"));
        // A plain letter with a widerspruch-like deadline is no decision letter.
        let mut brief = x.clone();
        brief.doc_type = DocType::Brief;
        assert_eq!(for_letter(&brief, &wd, d(2026, 10, 20), Lang::En), None);
    }

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters")
    }

    fn date(v: &Value) -> Option<NaiveDate> {
        v.as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    /// The extract and rule deadlines of a fixture letter, from its expected.json (no API call).
    fn fixture(name: &str) -> (DocumentExtract, Vec<Deadline>, Value) {
        let x: Value =
            serde_json::from_str(&std::fs::read_to_string(fixtures().join(format!("{name}.expected.json"))).unwrap())
                .unwrap();
        let doc_type = DocType::parse(x["doc_type"].as_str().unwrap()).unwrap();
        let au = x["au"].as_object().map(|a| AuInfo {
            from: date(&a["from"]).unwrap(),
            until: date(&a["until"]).unwrap(),
            first_or_follow: if a["first_or_follow"] == "folge" { AuKind::Folge } else { AuKind::Erst },
            issued: None,
        });
        let mut extract = bescheid_doc(date(&x["date"]).unwrap());
        extract.doc_type = doc_type;
        extract.au = au;
        let deadlines = x["rule_deadlines"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, r)| deadline(i as i64 + 1, r["rule_id"].as_str().unwrap(), date(&r["due"]).unwrap()))
            .collect();
        (extract, deadlines, x)
    }

    #[test]
    fn the_fixture_letters_give_exactly_the_expected_tip() {
        let names = [
            ("au_erika_erst", Some("au_gap")),
            ("au_erika_erst_foto", Some("au_gap")),
            ("au_erika_folge", Some("au_gap")),
            ("bescheid_wohngeld", Some("widerspruch_month")),
            ("insurer_letter", None),
            ("uni_rueckmeldung", None),
        ];
        for (name, want) in names {
            let (extract, deadlines, x) = fixture(name);
            // The expected.json says the same as this table.
            assert!(x.get("tip").is_some(), "{name}.expected.json has no \"tip\" field");
            assert_eq!(x["tip"].as_str(), want, "{name}.expected.json");
            let today = extract.date.unwrap();
            let got = for_letter(&extract, &deadlines, today, Lang::En);
            assert_eq!(got.as_ref().map(|t| t.id), want, "{name}: {got:?}");
            if let Some(t) = got {
                check_en(&t.text);
            }
        }
    }

    fn add(mem: &Memory, due: NaiveDate, rule: &str, doc_id: Option<i64>) -> i64 {
        mem.add_deadline(&NewDeadline {
            title: "New sick note, if you are still ill".into(),
            due,
            rule_id: Some(rule.into()),
            doc_id,
            remind_at: vec![],
            why: String::new(),
            source: String::new(),
        })
        .unwrap()
    }

    /// Sick note 20.10.–30.10., follow-up due Monday 2.11.
    fn erika() -> (Memory, i64) {
        let mem = Memory::open_in_memory().unwrap();
        let doc = mem
            .insert_document(Path::new("au.jpg"), "sha-au", &au_doc(d(2026, 10, 20), d(2026, 10, 30)), "AU")
            .unwrap();
        let id = add(&mem, d(2026, 11, 2), "au_lueckenlos", Some(doc));
        (mem, id)
    }

    #[test]
    fn brief_tip_within_seven_days_only() {
        let (mem, id) = erika();
        let at = |day: NaiveDate| local_at(day, 9, 0);
        assert_eq!(for_brief(&mem, at(d(2026, 10, 25)), Lang::En).unwrap(), None, "8 days away");
        let t = for_brief(&mem, at(d(2026, 10, 29)), Lang::En).unwrap().expect("tip");
        assert_eq!(t.id, "au_gap");
        assert_eq!(t.deadline_id, Some(id));
        assert!(t.text.contains("by Monday, 2 November"), "{}", t.text);
        check_brief_en(&t.text);
        assert_eq!(for_brief(&mem, at(d(2026, 11, 3)), Lang::En).unwrap(), None, "overdue");
        // Other rules' deadlines give no brief tip.
        let mem = Memory::open_in_memory().unwrap();
        add(&mem, d(2026, 11, 2), "frist_aus_brief", None);
        assert_eq!(for_brief(&mem, at(d(2026, 10, 29)), Lang::En).unwrap(), None);
    }

    #[test]
    fn brief_tip_never_twice_in_seven_days() {
        let (mem, _) = erika();
        let now = local_at(d(2026, 10, 27), 9, 0);
        let t = for_brief(&mem, now, Lang::En).unwrap().expect("tip");
        mark_shown(&mem, &t, now).unwrap();
        for day in [27, 28, 29, 30] {
            assert_eq!(for_brief(&mem, local_at(d(2026, 10, day), 18, 0), Lang::En).unwrap(), None, "{day}");
        }
        // Other event kinds or other ids do not count.
        let (mem, _) = erika();
        mem.add_event_at(now, "suggested", "au_gap").unwrap();
        mem.add_event_at(now, EV_TIP_SHOWN, "widerspruch_month").unwrap();
        assert!(for_brief(&mem, now, Lang::En).unwrap().is_some());
        // After 7 days the tip may come again (a new sick note with a new gap).
        let (mem, _) = erika();
        mem.add_event_at(now - Duration::days(8), EV_TIP_SHOWN, "au_gap").unwrap();
        assert!(for_brief(&mem, now, Lang::En).unwrap().is_some());
    }

    #[test]
    fn tips_off_stops_both() {
        let (mem, _) = erika();
        let x = au_doc(d(2026, 10, 20), d(2026, 10, 30));
        assert!(for_letter_in(&mem, &x, &[], d(2026, 10, 27), Lang::En).unwrap().is_some());
        mem.profile_set("tips", " Off ").unwrap();
        assert!(tips_off(&mem).unwrap());
        assert_eq!(for_letter_in(&mem, &x, &[], d(2026, 10, 27), Lang::En).unwrap(), None);
        assert_eq!(for_brief(&mem, local_at(d(2026, 10, 29), 9, 0), Lang::En).unwrap(), None);
        mem.profile_set("tips", "on").unwrap();
        assert!(for_brief(&mem, local_at(d(2026, 10, 29), 9, 0), Lang::En).unwrap().is_some());
    }

    #[test]
    fn tip_dates() {
        assert_eq!(tip_date(d(2026, 11, 2), Lang::En), "Monday, 2 November");
        assert_eq!(tip_date(d(2026, 11, 2), Lang::De), "Montag, 2. November");
    }
}
