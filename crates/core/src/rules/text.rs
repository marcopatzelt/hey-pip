//! Wording helpers for reminders: German/English dates and `tr` (i18n lookup + placeholders).
//! OWNER: agent "memory-rules".

use chrono::{DateTime, Datelike, Local, NaiveDate, Weekday};

use crate::i18n::Lang;

/// i18n keys this module uses (all live in `i18n::TABLE`; checked by a test).
#[cfg(test)]
static KEYS: &[&str] = &[
    "rem_au_ends_today",
    "rem_au_ends_on",
    "rem_au_ended",
    "rem_au_need_new_today",
    "rem_au_why_detail",
    "rem_widerspruch_ends",
    "rem_widerspruch_ends_today",
    "rem_widerspruch_action",
    "rem_widerspruch_source",
    "rem_deadline_today",
    "rem_deadline_passed",
    "rem_widerspruch_passed",
    "rem_passed_action",
    "rem_brief_action",
    "rem_brief_quote",
    "rem_brief_unsure",
    "btn_done",
    "ans_healthy",
    "ans_done",
    "ans_later",
    "ans_remind_on",
    "ev_deadline_done",
    "day_today",
    "day_tomorrow",
];

/// i18n lookup; placeholders `{name}` filled like `i18n::tf`.
pub fn tr(key: &str, lang: Lang, args: &[(&str, &str)]) -> String {
    crate::i18n::tf(key, lang, args)
}

/// Join two sentences with a space; the second may be empty.
pub fn join(a: &str, b: &str) -> String {
    if b.is_empty() { a.to_string() } else { format!("{a} {b}") }
}

pub fn weekday_name(w: Weekday, lang: Lang) -> &'static str {
    let i = w.num_days_from_monday() as usize;
    match lang {
        Lang::De => ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"][i],
        Lang::En => ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][i],
    }
}

fn month_name(m: u32, lang: Lang) -> &'static str {
    let i = (m.clamp(1, 12) - 1) as usize;
    match lang {
        Lang::De => [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ][i],
        Lang::En => [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ][i],
    }
}

/// "Montag, 2. November" / "Monday, November 2".
pub fn fmt_date(d: NaiveDate, lang: Lang) -> String {
    let w = weekday_name(d.weekday(), lang);
    let m = month_name(d.month(), lang);
    match lang {
        Lang::De => format!("{w}, {}. {m}", d.day()),
        Lang::En => format!("{w}, {m} {}", d.day()),
    }
}

/// "3. Oktober 2026" / "3 October 2026": a date as people write it, with the year (the approval card).
pub fn fmt_date_long(d: NaiveDate, lang: Lang) -> String {
    let m = month_name(d.month(), lang);
    match lang {
        Lang::De => format!("{}. {m} {}", d.day(), d.year()),
        Lang::En => format!("{} {m} {}", d.day(), d.year()),
    }
}

/// Like `fmt_date`, for the middle of a German sentence: the apposition gets its closing comma
/// ("bis Montag, 2. November, eine neue").
pub fn fmt_date_mid(d: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::De => format!("{},", fmt_date(d, lang)),
        Lang::En => fmt_date(d, lang),
    }
}

/// Short relative day for buttons and answers: "heute", "morgen", "Montag" (within 6 days),
/// else "am 9. November" / "on November 9".
pub fn fmt_day(d: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let diff = d.signed_duration_since(today).num_days();
    match diff {
        0 => tr("day_today", lang, &[]),
        1 => tr("day_tomorrow", lang, &[]),
        2..=6 => weekday_name(d.weekday(), lang).to_string(),
        _ => match lang {
            Lang::De => format!("am {}. {}", d.day(), month_name(d.month(), lang)),
            Lang::En => format!("on {} {}", month_name(d.month(), lang), d.day()),
        },
    }
}

/// "18:00"
pub fn fmt_time(t: DateTime<Local>) -> String {
    t.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::t;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn dates_in_both_languages() {
        assert_eq!(fmt_date(d(2026, 11, 2), Lang::De), "Montag, 2. November");
        assert_eq!(fmt_date(d(2026, 11, 2), Lang::En), "Monday, November 2");
        assert_eq!(fmt_date(d(2026, 3, 1), Lang::De), "Sonntag, 1. März");
    }

    #[test]
    fn long_dates_have_the_year() {
        assert_eq!(fmt_date_long(d(2026, 10, 3), Lang::En), "3 October 2026");
        assert_eq!(fmt_date_long(d(2026, 10, 3), Lang::De), "3. Oktober 2026");
        assert_eq!(fmt_date_long(d(2027, 1, 12), Lang::De), "12. Januar 2027");
    }

    #[test]
    fn relative_days() {
        let today = d(2026, 10, 29);
        assert_eq!(fmt_day(today, today, Lang::De), "heute");
        assert_eq!(fmt_day(d(2026, 10, 30), today, Lang::En), "tomorrow");
        assert_eq!(fmt_day(d(2026, 11, 2), today, Lang::De), "Montag");
        assert_eq!(fmt_day(d(2026, 11, 9), today, Lang::De), "am 9. November");
        assert_eq!(fmt_day(d(2026, 11, 9), today, Lang::En), "on November 9");
    }

    #[test]
    fn tr_reads_the_table() {
        assert_eq!(tr("btn_later", Lang::De, &[]), "In 2 Stunden");
        assert_eq!(tr("btn_done", Lang::En, &[]), "Done");
        assert_eq!(tr("rem_au_ends_on", Lang::De, &[("date", "Freitag")]), "Deine Krankschreibung endet am Freitag.");
        assert_eq!(tr("nope", Lang::De, &[]), "nope");
    }

    #[test]
    fn every_rule_key_is_in_the_table() {
        for k in KEYS {
            for lang in [Lang::De, Lang::En] {
                assert_ne!(t(k, lang), *k, "missing i18n key {k}");
            }
        }
    }
}
