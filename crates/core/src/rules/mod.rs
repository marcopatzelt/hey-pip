//! Deadline rules (Rust code with source + tests, never invented by the model),
//! NRW holidays 2026/27, reminder engine, and the page guard (`guard`: warning signs, no model call).
//!
//! OWNER: agent "memory-rules".

pub mod au;
pub mod brief;
pub mod calendar;
pub mod daily;
pub mod engine;
pub mod guard;
pub mod text;
pub mod widerspruch;

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveTime, TimeZone};

use crate::docs::DocumentExtract;

pub use crate::clock::{Clock, DemoClock, FixedClock, SystemClock};
pub use au::AuLueckenlos;
pub use brief::FristAusBrief;
pub use calendar::Calendar;
pub use daily::{
    AfterTask, AfterTaskOffer, Brief, BriefOffer, after_task, after_task_on, already_suggested, daily_brief,
    offer_after_task,
};
pub use engine::RuleEngine;
pub use widerspruch::WiderspruchBescheid;

#[derive(Debug, Clone, PartialEq)]
pub struct DeadlineProposal {
    pub title: String,
    pub due: NaiveDate,
    pub rule_id: &'static str,
    pub remind_at: Vec<DateTime<Local>>,
    /// Shown behind "Warum?" (ends with hint_not_legal_advice).
    pub why: String,
    /// Legal source, e.g. "§ 46 Satz 2 SGB V".
    pub source: String,
}

pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn apply(&self, doc: &DocumentExtract, today: NaiveDate) -> Vec<DeadlineProposal>;
}

/// AuLueckenlos, WiderspruchBescheid, FristAusBrief.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![Box::new(AuLueckenlos), Box::new(WiderspruchBescheid), Box::new(FristAusBrief)]
}

/// due-3d 09:00, due-1d 18:00, due 09:00 (local); past ones (relative to `now`) dropped.
pub fn default_reminders(due: NaiveDate, now: DateTime<Local>) -> Vec<DateTime<Local>> {
    let plan = [(3, 9), (1, 18), (0, 9)];
    let mut out: Vec<DateTime<Local>> = plan
        .iter()
        .map(|&(days, hour)| local_at(calendar::days_before(due, days), hour, 0))
        .filter(|at| *at >= now)
        .collect();
    out.dedup();
    out
}

/// Local wall-clock time on day `d`. DST gaps fall back to the earliest valid instant.
pub fn local_at(d: NaiveDate, hour: u32, minute: u32) -> DateTime<Local> {
    let naive = d.and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or(NaiveTime::MIN));
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) => t,
        LocalResult::Ambiguous(a, _) => a,
        // Inside a DST gap: one hour later always exists.
        LocalResult::None => Local
            .from_local_datetime(&(naive + chrono::Duration::hours(1)))
            .earliest()
            .unwrap_or_else(|| Local.from_utc_datetime(&naive)),
    }
}

/// Midnight at the start of `today`; rules keep reminders of the current day even if their hour
/// passed (they fire on the next tick, which is what a person who just scanned a letter needs).
pub(crate) fn start_of_day(today: NaiveDate) -> DateTime<Local> {
    local_at(today, 0, 0)
}

/// Common "why" ending.
pub(crate) const NOT_LEGAL_ADVICE_DE: &str = "Das ist ein Hinweis, keine Rechtsberatung.";

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn default_reminders_all_future() {
        let r = default_reminders(d(2026, 11, 30), local_at(d(2026, 11, 1), 12, 0));
        assert_eq!(
            r,
            vec![local_at(d(2026, 11, 27), 9, 0), local_at(d(2026, 11, 29), 18, 0), local_at(d(2026, 11, 30), 9, 0)]
        );
    }

    #[test]
    fn default_reminders_drop_past() {
        let r = default_reminders(d(2026, 11, 30), local_at(d(2026, 11, 29), 19, 0));
        assert_eq!(r, vec![local_at(d(2026, 11, 30), 9, 0)]);
        assert!(default_reminders(d(2026, 11, 30), local_at(d(2026, 11, 30), 9, 1)).is_empty());
        // exactly now is kept
        assert_eq!(default_reminders(d(2026, 11, 30), local_at(d(2026, 11, 30), 9, 0)).len(), 1);
    }

    #[test]
    fn default_reminders_across_month_and_dst() {
        // DST ends 25.10.2026 in Germany; reminders keep their wall-clock hour.
        let r = default_reminders(d(2026, 10, 27), local_at(d(2026, 10, 1), 0, 0));
        assert_eq!(r[0].date_naive(), d(2026, 10, 24));
        assert!(r.iter().all(|t| matches!(t.format("%H:%M").to_string().as_str(), "09:00" | "18:00")));
        let r = default_reminders(d(2027, 3, 1), local_at(d(2027, 2, 1), 0, 0));
        assert_eq!(r[0].date_naive(), d(2027, 2, 26));
    }

    #[test]
    fn all_rules_ids() {
        let ids: Vec<_> = all_rules().iter().map(|r| r.id()).collect();
        assert_eq!(ids, ["au_lueckenlos", "widerspruch_bescheid", "frist_aus_brief"]);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use chrono::NaiveDate;

    use crate::docs::{AuInfo, AuKind, DocType, DocumentExtract};

    pub fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Invented AU letter (Dr. Beispiel).
    pub fn au_doc(from: NaiveDate, until: NaiveDate) -> DocumentExtract {
        DocumentExtract {
            doc_type: DocType::AuBescheinigung,
            sender: "Praxis Dr. Beispiel".into(),
            recipient: "Erika Musterfrau".into(),
            date: Some(from),
            reference: Some("A123456789".into()),
            person: None,
            summary_simple: format!("Krankschreibung vom {from} bis {until}."),
            requests: vec![],
            deadlines: vec![],
            au: Some(AuInfo { from, until, first_or_follow: AuKind::Erst, issued: Some(from) }),
            consequences_if_ignored: None,
            confidence: 0.9,
        }
    }

    /// Invented decision letter (Jobcenter Beispielstadt).
    pub fn bescheid_doc(date: NaiveDate) -> DocumentExtract {
        DocumentExtract {
            doc_type: DocType::Bescheid,
            sender: "Jobcenter Beispielstadt".into(),
            recipient: "Erika Musterfrau".into(),
            date: Some(date),
            reference: Some("BG-0815".into()),
            person: None,
            summary_simple: "Bescheid über Leistungen. Ein Teil wird abgelehnt.".into(),
            requests: vec!["Unterlagen nachreichen".into()],
            deadlines: vec![],
            au: None,
            consequences_if_ignored: Some("Der Bescheid wird bestandskräftig.".into()),
            confidence: 0.8,
        }
    }
}
