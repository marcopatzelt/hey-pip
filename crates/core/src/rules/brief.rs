//! Rule `frist_aus_brief`: every deadline the letter itself states becomes a proposal.
//! Without a quote (`source_text`) the "why" says the date is unsure. AU letters are skipped
//! (their end date is handled by `au_lueckenlos`). OWNER: agent "memory-rules".

use chrono::NaiveDate;

use super::{DeadlineProposal, NOT_LEGAL_ADVICE_DE, Rule, default_reminders, start_of_day};
use crate::docs::{DocType, DocumentExtract};

pub struct FristAusBrief;

impl Rule for FristAusBrief {
    fn id(&self) -> &'static str {
        "frist_aus_brief"
    }
    fn apply(&self, doc: &DocumentExtract, today: NaiveDate) -> Vec<DeadlineProposal> {
        if doc.doc_type == DocType::AuBescheinigung {
            return Vec::new();
        }
        let sender = doc.sender.trim();
        let source = if sender.is_empty() { "Brief".to_string() } else { format!("Brief von {sender}") };
        let mut out: Vec<DeadlineProposal> = Vec::new();
        for x in &doc.deadlines {
            if x.due < today || out.iter().any(|p| p.due == x.due) {
                continue;
            }
            let title =
                if x.title.trim().is_empty() { "Frist aus dem Brief".to_string() } else { x.title.trim().to_string() };
            let quote = x.source_text.trim();
            let why = if quote.is_empty() {
                format!("Das Datum habe ich aus dem Brief gelesen. Bitte prüfe es noch einmal. {NOT_LEGAL_ADVICE_DE}")
            } else {
                format!("Im Brief steht: „{quote}“ {NOT_LEGAL_ADVICE_DE}")
            };
            out.push(DeadlineProposal {
                title,
                due: x.due,
                rule_id: "frist_aus_brief",
                remind_at: default_reminders(x.due, start_of_day(today)),
                why,
                source: source.clone(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::ExtractedDeadline;
    use crate::rules::tests_support::{au_doc, bescheid_doc, d};

    fn letter(deadlines: Vec<(&str, NaiveDate, &str)>) -> DocumentExtract {
        let mut doc = bescheid_doc(d(2026, 10, 1));
        doc.doc_type = DocType::Brief;
        doc.sender = "Hochschule Beispielstadt".into();
        doc.deadlines = deadlines
            .into_iter()
            .map(|(t, due, q)| ExtractedDeadline { title: t.into(), due, source_text: q.into() })
            .collect();
        doc
    }

    #[test]
    fn one_proposal_per_future_deadline() {
        let doc = letter(vec![
            ("Rückmeldung", d(2027, 1, 15), "Bitte melden Sie sich bis zum 15.01.2027 zurück."),
            ("Alte Frist", d(2026, 9, 1), "bis 01.09.2026"),
            ("Doppelt", d(2027, 1, 15), "nochmal"),
            ("", d(2026, 12, 1), ""),
        ]);
        let p = FristAusBrief.apply(&doc, d(2026, 10, 26));
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "Rückmeldung");
        assert!(p[0].why.starts_with("Im Brief steht: „Bitte melden"));
        assert_eq!(p[0].source, "Brief von Hochschule Beispielstadt");
        assert_eq!(p[1].title, "Frist aus dem Brief");
        assert!(p[1].why.contains("Bitte prüfe es"));
        assert!(p.iter().all(|x| x.why.ends_with(NOT_LEGAL_ADVICE_DE) && x.rule_id == "frist_aus_brief"));
    }

    #[test]
    fn letter_date_is_not_shifted() {
        // A deadline on a Sunday stays as the letter says.
        let p = FristAusBrief.apply(&letter(vec![("X", d(2026, 11, 29), "q")]), d(2026, 10, 26));
        assert_eq!(p[0].due, d(2026, 11, 29));
    }

    #[test]
    fn au_letters_skipped() {
        let mut doc = au_doc(d(2026, 10, 20), d(2026, 10, 30));
        doc.deadlines = vec![ExtractedDeadline { title: "AU".into(), due: d(2026, 10, 30), source_text: "bis".into() }];
        assert!(FristAusBrief.apply(&doc, d(2026, 10, 21)).is_empty());
    }
}
