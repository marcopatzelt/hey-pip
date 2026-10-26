//! Rule `widerspruch_bescheid`. OWNER: agent "memory-rules".
//!
//! Sources (checked 26.10.2026):
//! - Widerspruchsfrist one month after Bekanntgabe: § 84 Abs. 1 SGG (social law), § 70 Abs. 1 VwGO.
//! - Bekanntgabe by post: "am vierten Tag nach der Aufgabe zur Post" — § 37 Abs. 2 Satz 1 SGB X
//!   (and § 41 Abs. 2 VwVfG), four days instead of three since 1.1.2025 (Postrechtsmodernisierungsgesetz).
//! - The Bekanntgabe day itself is NOT moved when it falls on a weekend/holiday: it is a point in
//!   time, not a deadline (BSG, 6.5.2010, B 14 AS 12/09 R).
//! - Month deadline ends on the same day number of the next month, or the month's last day
//!   (§ 64 Abs. 2 SGG); if that end falls on Saturday, Sunday or a public holiday it ends on the
//!   next working day (§ 64 Abs. 3 SGG, § 26 Abs. 3 SGB X; VwGO via § 57 Abs. 2 VwGO, § 222 ZPO).
//!
//! We use the letter date D as "Aufgabe zur Post". The real posting day is never earlier than the
//! printed date, so our due date is never later than the real one.
//!
//! due = this_or_next_business_day((D + 4 days) + 1 month); reminders: default_reminders.

use chrono::{Days, Months, NaiveDate};

use super::calendar::Calendar;
use super::{DeadlineProposal, NOT_LEGAL_ADVICE_DE, Rule, default_reminders, start_of_day};
use crate::docs::{DocType, DocumentExtract};

pub struct WiderspruchBescheid;

pub const WIDERSPRUCH_SOURCE: &str = "§ 84 SGG / § 70 VwGO, § 37 Abs. 2 SGB X, § 64 SGG";

/// (Bekanntgabe, last day to object) for a decision dated `letter_date`.
pub fn widerspruch_due(letter_date: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let known = letter_date.checked_add_days(Days::new(4))?;
    let end = known.checked_add_months(Months::new(1))?;
    Some((known, Calendar::this_or_next_business_day(end)))
}

impl Rule for WiderspruchBescheid {
    fn id(&self) -> &'static str {
        "widerspruch_bescheid"
    }
    fn apply(&self, doc: &DocumentExtract, today: NaiveDate) -> Vec<DeadlineProposal> {
        if doc.doc_type != DocType::Bescheid {
            return Vec::new();
        }
        let Some((known, due)) = doc.date.and_then(widerspruch_due) else { return Vec::new() };
        if due < today {
            return Vec::new();
        }
        let sender = doc.sender.trim();
        let title = if sender.is_empty() {
            "Widerspruch gegen den Bescheid".to_string()
        } else {
            format!("Widerspruch gegen den Bescheid von {sender}")
        };
        vec![DeadlineProposal {
            title,
            due,
            rule_id: "widerspruch_bescheid",
            remind_at: default_reminders(due, start_of_day(today)),
            why: format!(
                "Gegen einen Bescheid kannst du einen Monat lang Widerspruch einlegen. Ein Brief mit der Post gilt am \
                 vierten Tag nach dem Datum als angekommen, hier am {}. Fällt das Ende auf ein Wochenende oder einen \
                 Feiertag, endet die Frist am nächsten Werktag. Regel: {WIDERSPRUCH_SOURCE}. {NOT_LEGAL_ADVICE_DE}",
                known.format("%d.%m.%Y"),
            ),
            source: WIDERSPRUCH_SOURCE.to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::tests_support::{bescheid_doc, d};

    fn due(letter: NaiveDate) -> NaiveDate {
        widerspruch_due(letter).unwrap().1
    }

    #[test]
    fn plain_case() {
        // Mo 12.10.2026 -> known Fr 16.10. -> Mo 16.11.2026 (business day)
        assert_eq!(widerspruch_due(d(2026, 10, 12)), Some((d(2026, 10, 16), d(2026, 11, 16))));
    }

    #[test]
    fn known_on_weekend_is_not_moved() {
        // Tue 13.10. -> known Sat 17.10. (stays, BSG) -> end Tue 17.11.
        assert_eq!(widerspruch_due(d(2026, 10, 13)), Some((d(2026, 10, 17), d(2026, 11, 17))));
    }

    #[test]
    fn end_on_weekend_moves_to_monday() {
        // business-day ends stay: 10.12. -> known 14.12. -> Thu 14.1.2027; 26.10. -> 30.10. -> Mon 30.11.
        assert_eq!(due(d(2026, 12, 10)), d(2027, 1, 14));
        assert_eq!(due(d(2026, 10, 26)), d(2026, 11, 30));
        // letter 3.10.2026 -> known Wed 7.10. -> end Sat 7.11. -> Mon 9.11.
        assert_eq!(due(d(2026, 10, 3)), d(2026, 11, 9));
    }

    #[test]
    fn end_on_holiday_moves() {
        // letter 27.11.2026 -> known 1.12. -> end Fri 1.1.2027 (Neujahr) -> Mon 4.1.2027
        assert_eq!(due(d(2026, 11, 27)), d(2027, 1, 4));
        // letter 22.11.2026 -> known 26.11. -> end Sat 26.12. -> Mon 28.12.
        assert_eq!(due(d(2026, 11, 22)), d(2026, 12, 28));
        // letter 22.2.2027 -> known 26.2. -> end Fri 26.3.2027 Karfreitag -> Tue 30.3.
        assert_eq!(due(d(2027, 2, 22)), d(2027, 3, 30));
    }

    #[test]
    fn short_month_clamps_to_last_day() {
        // letter 27.12.2026 -> known 31.12. -> 31.1.2027 is a Sunday -> Mon 1.2.
        assert_eq!(due(d(2026, 12, 27)), d(2027, 2, 1));
        // letter 27.1.2027 -> known 31.1. -> Feb has no 31st -> 28.2.2027 (Sunday) -> Mon 1.3.
        assert_eq!(due(d(2027, 1, 27)), d(2027, 3, 1));
        // letter 26.1.2026 -> known 30.1. -> 28.2.2026 (Saturday) -> Mon 2.3.
        assert_eq!(due(d(2026, 1, 26)), d(2026, 3, 2));
    }

    #[test]
    fn apply_builds_proposal() {
        let p = WiderspruchBescheid.apply(&bescheid_doc(d(2026, 10, 12)), d(2026, 10, 20));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].due, d(2026, 11, 16));
        assert_eq!(p[0].remind_at.len(), 3);
        assert!(p[0].title.contains("Jobcenter Beispielstadt"));
        assert!(p[0].why.contains("16.10.2026"));
        assert!(p[0].why.ends_with(NOT_LEGAL_ADVICE_DE));
    }

    #[test]
    fn no_date_or_wrong_type_or_past() {
        let mut doc = bescheid_doc(d(2026, 10, 12));
        assert!(WiderspruchBescheid.apply(&doc, d(2026, 11, 17)).is_empty());
        doc.date = None;
        assert!(WiderspruchBescheid.apply(&doc, d(2026, 10, 20)).is_empty());
        let mut doc = bescheid_doc(d(2026, 10, 12));
        doc.doc_type = DocType::Rechnung;
        assert!(WiderspruchBescheid.apply(&doc, d(2026, 10, 20)).is_empty());
    }
}
