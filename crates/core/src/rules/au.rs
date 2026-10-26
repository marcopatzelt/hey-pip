//! Rule `au_lueckenlos` (§ 46 Satz 2 SGB V). OWNER: agent "memory-rules".
//!
//! § 46 Satz 2 SGB V: sick pay continues if the follow-up incapacity is certified "spätestens am
//! nächsten Werktag nach dem zuletzt bescheinigten Ende der Arbeitsunfähigkeit; Samstage gelten
//! insoweit nicht als Werktage". Sundays and public holidays are no working days either.
//!
//! AU until = E -> due = next_business_day(E); reminders: E-1 18:00 ("endet morgen") and due 09:00.
//! The "no follow-up AU" check needs memory, so `apply` proposes and RuleEngine drops it
//! when a stored AU covers the gap (and closes the old deadline when a new AU arrives).

use chrono::NaiveDate;

use super::calendar::{Calendar, days_before};
use super::{DeadlineProposal, NOT_LEGAL_ADVICE_DE, Rule, local_at, start_of_day};
use crate::docs::{DocType, DocumentExtract};

pub struct AuLueckenlos;

pub const AU_TITLE_DE: &str = "Neue Krankschreibung, falls du noch krank bist";
pub const AU_SOURCE: &str = "§ 46 Satz 2 SGB V";

/// Last certified day of an AU letter, if this is one.
pub fn au_end(doc: &DocumentExtract) -> Option<NaiveDate> {
    (doc.doc_type == DocType::AuBescheinigung).then_some(())?;
    doc.au.as_ref().map(|a| a.until)
}

/// Latest day a follow-up AU may be issued for an AU ending on `end`.
pub fn au_due(end: NaiveDate) -> NaiveDate {
    Calendar::next_business_day(end)
}

impl Rule for AuLueckenlos {
    fn id(&self) -> &'static str {
        "au_lueckenlos"
    }
    fn apply(&self, doc: &DocumentExtract, today: NaiveDate) -> Vec<DeadlineProposal> {
        let Some(end) = au_end(doc) else { return Vec::new() };
        let due = au_due(end);
        if due < today {
            return Vec::new();
        }
        let floor = start_of_day(today);
        let mut remind_at: Vec<_> =
            [local_at(days_before(end, 1), 18, 0), local_at(due, 9, 0)].into_iter().filter(|t| *t >= floor).collect();
        remind_at.dedup();
        vec![DeadlineProposal {
            title: AU_TITLE_DE.to_string(),
            due,
            rule_id: "au_lueckenlos",
            remind_at,
            why: format!(
                "Die Krankschreibung endet am {}. Wenn du noch krank bist, muss die neue spätestens am {} ausgestellt sein \
                 (nächster Werktag, Samstage zählen nicht). Sonst kann die Krankenkasse für die Lücke kein Krankengeld zahlen. \
                 Regel: {AU_SOURCE}. {NOT_LEGAL_ADVICE_DE}",
                end.format("%d.%m.%Y"),
                due.format("%d.%m.%Y"),
            ),
            source: AU_SOURCE.to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::tests_support::{au_doc, d};

    fn due_for(end: NaiveDate) -> NaiveDate {
        let p = AuLueckenlos.apply(&au_doc(d(2026, 1, 1), end), d(2026, 1, 1));
        assert_eq!(p.len(), 1);
        p[0].due
    }

    #[test]
    fn ends_friday_due_monday() {
        assert_eq!(due_for(d(2026, 10, 30)), d(2026, 11, 2));
    }

    #[test]
    fn ends_thursday_due_friday() {
        assert_eq!(due_for(d(2026, 10, 29)), d(2026, 10, 30));
    }

    #[test]
    fn ends_saturday_or_sunday_due_monday() {
        assert_eq!(due_for(d(2026, 10, 24)), d(2026, 10, 26));
        assert_eq!(due_for(d(2026, 10, 25)), d(2026, 10, 26));
    }

    #[test]
    fn ends_before_easter_due_tuesday_after() {
        // Thursday 2.4.2026 -> Karfreitag, Sa, So, Ostermontag -> Tuesday 7.4.
        assert_eq!(due_for(d(2026, 4, 2)), d(2026, 4, 7));
        assert_eq!(due_for(d(2027, 3, 25)), d(2027, 3, 30));
    }

    #[test]
    fn ends_before_holidays() {
        assert_eq!(due_for(d(2026, 4, 30)), d(2026, 5, 4)); // Thu -> 1. Mai (Fri) -> Mon
        assert_eq!(due_for(d(2026, 5, 13)), d(2026, 5, 15)); // Himmelfahrt
        assert_eq!(due_for(d(2026, 6, 3)), d(2026, 6, 5)); // Fronleichnam
        assert_eq!(due_for(d(2026, 10, 2)), d(2026, 10, 5)); // Fri, 3.10. is Saturday
        assert_eq!(due_for(d(2026, 12, 23)), d(2026, 12, 24)); // Heiligabend is a business day
        assert_eq!(due_for(d(2026, 12, 24)), d(2026, 12, 28));
        assert_eq!(due_for(d(2026, 12, 31)), d(2027, 1, 4));
    }

    #[test]
    fn demo_reminders_thursday_evening_and_monday_morning() {
        let p = AuLueckenlos.apply(&au_doc(d(2026, 10, 20), d(2026, 10, 30)), d(2026, 10, 21));
        assert_eq!(p[0].remind_at, vec![local_at(d(2026, 10, 29), 18, 0), local_at(d(2026, 11, 2), 9, 0)]);
        assert!(p[0].why.ends_with(NOT_LEGAL_ADVICE_DE));
        assert!(p[0].why.contains("02.11.2026"));
        assert_eq!(p[0].source, "§ 46 Satz 2 SGB V");
    }

    #[test]
    fn scanned_late_keeps_todays_reminder_drops_older() {
        // Scanned on Friday (E): the Thursday reminder is gone, Monday stays.
        let p = AuLueckenlos.apply(&au_doc(d(2026, 10, 20), d(2026, 10, 30)), d(2026, 10, 30));
        assert_eq!(p[0].remind_at, vec![local_at(d(2026, 11, 2), 9, 0)]);
        // Scanned on Thursday afternoon: 18:00 that day is still kept.
        let p = AuLueckenlos.apply(&au_doc(d(2026, 10, 20), d(2026, 10, 30)), d(2026, 10, 29));
        assert_eq!(p[0].remind_at.len(), 2);
    }

    #[test]
    fn past_deadline_or_not_au_gives_nothing() {
        assert!(AuLueckenlos.apply(&au_doc(d(2026, 10, 1), d(2026, 10, 9)), d(2026, 10, 13)).is_empty());
        let mut doc = au_doc(d(2026, 10, 20), d(2026, 10, 30));
        doc.doc_type = DocType::Brief;
        assert!(AuLueckenlos.apply(&doc, d(2026, 10, 21)).is_empty());
        doc.doc_type = DocType::AuBescheinigung;
        doc.au = None;
        assert!(AuLueckenlos.apply(&doc, d(2026, 10, 21)).is_empty());
    }

    #[test]
    fn due_on_today_still_proposed() {
        let p = AuLueckenlos.apply(&au_doc(d(2026, 10, 20), d(2026, 10, 30)), d(2026, 11, 2));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].remind_at, vec![local_at(d(2026, 11, 2), 9, 0)]);
    }
}
