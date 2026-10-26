//! Applies rules to new letters and fires reminders (tick every 60 s from the app).
//! OWNER: agent "memory-rules".

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, Local, NaiveDate, SecondsFormat};

use super::au::au_end;
use super::calendar::days_before;
use super::text::{fmt_date, fmt_date_mid, fmt_day, fmt_time, join, tr};
use super::{DeadlineProposal, all_rules, local_at};
use crate::clock::Clock;
use crate::docs::{AuInfo, DocType, DocumentExtract};
use crate::events::{Button, UiEvent};
use crate::i18n::Lang;
use crate::memory::{Deadline, DeadlineStatus, Memory, NewDeadline, Reminder};

/// "Später" snoozes this long.
pub const LATER: Duration = Duration::hours(2);

pub struct RuleEngine {
    memory: Arc<Memory>,
    clock: Arc<dyn Clock>,
}

/// Does AU `new` close the gap after an older AU that ended on `old_end` (follow-up due `old_due`)?
fn au_covers(new: &AuInfo, old_end: NaiveDate, old_due: NaiveDate) -> bool {
    new.until > old_end && new.from <= old_due
}

/// Two reads of one letter: same type, date, sender and (non-empty) reference.
fn same_letter(a: &DocumentExtract, b: &DocumentExtract) -> bool {
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect::<String>();
    let reference = |d: &DocumentExtract| d.reference.as_deref().map(norm).filter(|r| !r.is_empty());
    a.doc_type == b.doc_type
        && a.date.is_some()
        && a.date == b.date
        && norm(&a.sender) == norm(&b.sender)
        && reference(a).is_some()
        && reference(a) == reference(b)
}

/// Rules write German titles and reasons; for English users they are rewritten here by rule id.
fn localize(mut p: DeadlineProposal, doc: &DocumentExtract, lang: Lang) -> DeadlineProposal {
    if lang != Lang::En {
        return p;
    }
    let en = |d: NaiveDate| d.format("%-d %b %Y").to_string();
    const HINT: &str = "This is a hint, not legal advice.";
    match p.rule_id {
        "au_lueckenlos" => {
            p.title = "New sick note, if you are still ill".into();
            if let Some(au) = &doc.au {
                p.why = format!(
                    "Your sick note ends on {}. If you are still ill, the new one must be issued by {} at the latest \
                     (the next working day; Saturdays do not count). Otherwise the health insurer cannot pay sick pay \
                     for the gap. Rule: {}. {HINT}",
                    en(au.until),
                    en(p.due),
                    p.source
                );
            }
        }
        "widerspruch_bescheid" => {
            let sender = doc.sender.trim();
            p.title = if sender.is_empty() {
                "Objection (Widerspruch) to the decision".into()
            } else {
                format!("Objection (Widerspruch) to the decision from {sender}")
            };
            if let Some((known, _)) = doc.date.and_then(super::widerspruch::widerspruch_due) {
                p.why = format!(
                    "You can object to an official decision for one month. A letter by post counts as received on the \
                     fourth day after its date, here on {}. If the end falls on a weekend or holiday, the deadline ends \
                     on the next working day. Rule: {}. {HINT}",
                    en(known),
                    p.source
                );
            }
        }
        _ => {
            if p.title == "Frist aus dem Brief" {
                p.title = "Deadline from the letter".into();
            }
        }
    }
    p
}

fn rfc(t: DateTime<Local>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, false)
}

impl RuleEngine {
    pub fn new(memory: Arc<Memory>, clock: Arc<dyn Clock>) -> Self {
        Self { memory, clock }
    }

    /// The user's language from the profile (default English), for stored deadline titles and reasons.
    fn lang(&self) -> Lang {
        self.memory.profile_get("lang").ok().flatten().map(|s| Lang::from_code(&s)).unwrap_or_default()
    }

    /// Run all_rules, store deadlines + reminders (no duplicates for the same doc/due).
    /// A new AU covering E+1 marks the open au_lueckenlos deadline of the older AU as done;
    /// an AU whose gap is already covered by a stored later AU gets no deadline.
    /// Returns the open deadlines of this letter (new and already stored), ordered by due.
    pub fn apply_document(&self, doc_id: i64, doc: &DocumentExtract) -> Result<Vec<Deadline>> {
        let today = self.clock.now().date_naive();
        let new_au = (doc.doc_type == DocType::AuBescheinigung).then_some(doc.au.as_ref()).flatten();

        if let Some(au) = new_au {
            for dl in self.memory.open_deadlines()? {
                if dl.rule_id.as_deref() != Some("au_lueckenlos") || dl.doc_id == Some(doc_id) {
                    continue;
                }
                let old_end = self.au_end_of(dl.doc_id)?;
                if old_end.is_some_and(|e| au_covers(au, e, dl.due)) {
                    self.memory.set_deadline_status(dl.id, DeadlineStatus::Done)?;
                    self.memory.add_event("deadline_done", &format!("Folge-Krankschreibung da: {}", dl.title))?;
                }
            }
        }

        let mut existing = self.memory.deadlines_for_document(doc_id)?;
        let open_elsewhere: Vec<Deadline> =
            self.memory.open_deadlines()?.into_iter().filter(|d| d.doc_id != Some(doc_id)).collect();
        for rule in all_rules() {
            for p in rule.apply(doc, today).into_iter().map(|p| localize(p, doc, self.lang())) {
                if existing.iter().any(|d| d.due == p.due) {
                    continue;
                }
                if p.rule_id == "au_lueckenlos" {
                    let end = au_end(doc).context("au rule without AU")?;
                    if self.au_gap_covered(doc_id, end, p.due)? {
                        continue;
                    }
                }
                // The same letter scanned twice (photo + PDF) -> keep the one deadline it already has.
                if let Some(same) = self.same_deadline_elsewhere(doc, &p, &open_elsewhere)? {
                    if !existing.iter().any(|d| d.id == same.id) {
                        existing.push(same);
                    }
                    continue;
                }
                let id = self.memory.add_deadline(&NewDeadline {
                    title: p.title,
                    due: p.due,
                    rule_id: Some(p.rule_id.to_string()),
                    doc_id: Some(doc_id),
                    remind_at: p.remind_at,
                    why: p.why,
                    source: p.source,
                })?;
                existing.push(self.memory.deadline(id)?.ok_or_else(|| anyhow!("deadline {id} vanished"))?);
            }
        }
        existing.retain(|d| d.status == DeadlineStatus::Open);
        existing.sort_by_key(|d| (d.due, d.id));
        Ok(existing)
    }

    /// An open deadline of another letter that is the same as proposal `p`: same rule, due and
    /// title, and (except for AU, where one follow-up covers all) the same letter by type, date,
    /// sender and a non-empty reference.
    fn same_deadline_elsewhere(
        &self,
        doc: &DocumentExtract,
        p: &DeadlineProposal,
        open_elsewhere: &[Deadline],
    ) -> Result<Option<Deadline>> {
        for dl in open_elsewhere {
            if dl.rule_id.as_deref() != Some(p.rule_id) || dl.due != p.due || dl.title != p.title {
                continue;
            }
            if p.rule_id == "au_lueckenlos" {
                return Ok(Some(dl.clone()));
            }
            let Some(other) = dl.doc_id.map(|id| self.memory.document_by_id(id)).transpose()?.flatten() else {
                continue;
            };
            if same_letter(doc, &other.extract) {
                return Ok(Some(dl.clone()));
            }
        }
        Ok(None)
    }

    fn au_end_of(&self, doc_id: Option<i64>) -> Result<Option<NaiveDate>> {
        let Some(id) = doc_id else { return Ok(None) };
        Ok(self.memory.document_by_id(id)?.and_then(|r| au_end(&r.extract)))
    }

    fn au_gap_covered(&self, doc_id: i64, end: NaiveDate, due: NaiveDate) -> Result<bool> {
        Ok(self.memory.documents_of_type(DocType::AuBescheinigung)?.iter().any(|row| {
            row.id != doc_id
                && row.extract.doc_type == DocType::AuBescheinigung
                && row.extract.au.as_ref().is_some_and(|a| au_covers(a, end, due))
        }))
    }

    /// due_reminders(now) -> mark_fired -> return them (never fires twice).
    /// After a long pause (computer off) several reminders of one deadline may be due at once:
    /// only the latest is returned, the older ones are marked fired with answer "skipped".
    pub fn tick(&self) -> Result<Vec<Reminder>> {
        let now = self.clock.now();
        let due = self.memory.due_reminders(now)?;
        let mut latest: HashMap<i64, Reminder> = HashMap::new();
        for r in due {
            match latest.get(&r.deadline.id) {
                Some(prev) if prev.at > r.at => {
                    self.memory.mark_fired(r.id, now, Some("skipped"))?;
                }
                Some(prev) => {
                    self.memory.mark_fired(prev.id, now, Some("skipped"))?;
                    latest.insert(r.deadline.id, r);
                }
                None => {
                    latest.insert(r.deadline.id, r);
                }
            }
        }
        let mut out: Vec<Reminder> = latest.into_values().collect();
        for r in &out {
            self.memory.mark_fired(r.id, now, None)?;
        }
        out.sort_by_key(|r| (r.at, r.id));
        Ok(out)
    }

    /// The bubble for one reminder: two short sentences (`text`), reasons behind "Warum?" (`why`,
    /// ends with hint_not_legal_advice), buttons e.g. "Erinnere mich Montag" / "Ich bin gesund" / "Später".
    /// Button ids: "remind:<RFC3339>", "done", "later".
    pub fn reminder_event(&self, r: &Reminder, lang: Lang) -> UiEvent {
        let now = self.clock.now();
        let today = now.date_naive();
        let dl = &r.deadline;
        let doc = dl.doc_id.and_then(|id| self.memory.document_by_id(id).ok().flatten()).map(|row| row.extract);
        let hint = tr("hint_not_legal_advice", lang, &[]);
        let due_s = fmt_date(dl.due, lang);
        let is_au = dl.rule_id.as_deref() == Some("au_lueckenlos");
        // A snooze can ring after the due day. Then say so instead of "endet heute"
        // (AU keeps its text: "Wenn du noch krank bist, brauchst du heute eine neue" stays true).
        let passed = dl.due < today && !is_au;

        let (text, why) = match dl.rule_id.as_deref() {
            Some("widerspruch_bescheid") if passed => {
                let first = tr("rem_widerspruch_passed", lang, &[("due", &fmt_date_mid(dl.due, lang))]);
                let why = [tr("rem_widerspruch_why", lang, &[]), tr("rem_widerspruch_source", lang, &[]), hint.clone()]
                    .join(" ");
                (join(&first, &tr("rem_passed_action", lang, &[])), why)
            }
            _ if passed => {
                let first =
                    tr("rem_deadline_passed", lang, &[("title", &dl.title), ("due", &fmt_date_mid(dl.due, lang))]);
                let why = if dl.why.trim().is_empty() { hint.clone() } else { dl.why.trim().to_string() };
                (join(&first, &tr("rem_passed_action", lang, &[])), why)
            }
            Some("au_lueckenlos") => {
                let first = match doc.as_ref().and_then(au_end) {
                    Some(end) => match end.signed_duration_since(today).num_days() {
                        1 => tr("rem_au_ends_tomorrow", lang, &[]),
                        0 => tr("rem_au_ends_today", lang, &[]),
                        n if n > 1 => tr("rem_au_ends_on", lang, &[("date", &fmt_date(end, lang))]),
                        _ => tr("rem_au_ended", lang, &[("date", &fmt_date_mid(end, lang))]),
                    },
                    None => String::new(),
                };
                let second = if dl.due <= today {
                    tr("rem_au_need_new_today", lang, &[])
                } else {
                    tr("rem_au_need_new", lang, &[("due", &fmt_date_mid(dl.due, lang))])
                };
                let text = if first.is_empty() { second } else { join(&first, &second) };
                let why = [tr("rem_au_why", lang, &[]), tr("rem_au_why_detail", lang, &[]), hint.clone()].join(" ");
                (text, why)
            }
            Some("widerspruch_bescheid") => {
                let first = if dl.due <= today {
                    tr("rem_widerspruch_ends_today", lang, &[])
                } else {
                    tr("rem_widerspruch_ends", lang, &[("due", &due_s)])
                };
                let text = join(&first, &tr("rem_widerspruch_action", lang, &[]));
                let why = [tr("rem_widerspruch_why", lang, &[]), tr("rem_widerspruch_source", lang, &[]), hint.clone()]
                    .join(" ");
                (text, why)
            }
            rule => {
                let first = if dl.due <= today {
                    tr("rem_deadline_today", lang, &[("title", &dl.title)])
                } else {
                    tr("rem_deadline_soon", lang, &[("title", &dl.title), ("due", &due_s)])
                };
                let from_letter = rule == Some("frist_aus_brief");
                let text = if from_letter { join(&first, &tr("rem_brief_action", lang, &[])) } else { first };
                let quote = doc
                    .as_ref()
                    .and_then(|d| d.deadlines.iter().find(|x| x.due == dl.due))
                    .map(|x| x.source_text.trim().to_string());
                let why = match (from_letter, quote) {
                    (true, Some(q)) if !q.is_empty() => join(&tr("rem_brief_quote", lang, &[("quote", &q)]), &hint),
                    (true, _) => join(&tr("rem_brief_unsure", lang, &[]), &hint),
                    _ if dl.why.trim().is_empty() => hint.clone(),
                    _ if dl.why.ends_with(&hint) || dl.why.ends_with(super::NOT_LEGAL_ADVICE_DE) => dl.why.clone(),
                    _ => join(dl.why.trim(), &hint),
                };
                (text, why)
            }
        };

        let mut buttons = Vec::new();
        if let Some(at) = remind_target(dl.due, is_au, now) {
            let day = fmt_day(at.date_naive(), today, lang);
            buttons.push(Button::new(format!("remind:{}", rfc(at)), tr("btn_remind_on", lang, &[("day", &day)]), true));
        }
        let done_label = if is_au { tr("btn_healthy", lang, &[]) } else { tr("btn_done", lang, &[]) };
        let primary_done = buttons.is_empty();
        buttons.push(Button::new("done", done_label, primary_done));
        if !passed {
            buttons.push(Button::new("later", tr("btn_later", lang, &[]), false));
        }

        UiEvent::Reminder { reminder_id: r.id, text, why, buttons }
    }

    /// Handle a ReminderAnswer button id (see reminder_event). Returns the bubble text to show.
    pub fn answer(&self, reminder_id: i64, choice: &str, lang: Lang) -> Result<String> {
        let r = self.memory.reminder(reminder_id)?.ok_or_else(|| anyhow!("no reminder {reminder_id}"))?;
        let now = self.clock.now();
        let dl = &r.deadline;
        let msg = match choice {
            "done" => {
                self.memory.set_deadline_status(dl.id, DeadlineStatus::Done)?;
                self.memory.add_event("deadline_done", &tr("ev_deadline_done", lang, &[("title", &dl.title)]))?;
                if dl.rule_id.as_deref() == Some("au_lueckenlos") {
                    tr("ans_healthy", lang, &[])
                } else {
                    tr("ans_done", lang, &[])
                }
            }
            "later" => {
                self.memory.add_reminder(dl.id, now + LATER)?;
                tr("ans_later", lang, &[])
            }
            c if c.starts_with("remind:") => {
                let at = DateTime::parse_from_rfc3339(&c["remind:".len()..])
                    .with_context(|| format!("bad remind button {c:?}"))?
                    .with_timezone(&Local);
                let at = if at <= now { now + LATER } else { at };
                self.memory.add_reminder(dl.id, at)?;
                // The person picked a time for that day: planned reminders later that day would
                // only repeat it (e.g. "Montag 08:00" chosen, Monday 09:00 planned).
                let end_of_day = local_at(at.date_naive() + Duration::days(1), 0, 0);
                self.memory.skip_reminders(dl.id, at, end_of_day, now, "replaced")?;
                let date = fmt_day(at.date_naive(), now.date_naive(), lang);
                tr("ans_remind_on", lang, &[("date", &date), ("time", &fmt_time(at))])
            }
            other => bail!("unknown reminder answer {other:?}"),
        };
        // Sets fired_at if the answer came before the tick, and stores the answer.
        self.memory.mark_fired(reminder_id, now, Some(choice))?;
        Ok(msg)
    }

    /// "Rückgängig" after "Ich bin gesund" / "Erledigt": the deadline is open again, so its planned
    /// reminders ring again (plus one in two hours if none is left). Returns the bubble text.
    pub fn undo_done(&self, reminder_id: i64, lang: Lang) -> Result<String> {
        let r = self.memory.reminder(reminder_id)?.ok_or_else(|| anyhow!("no reminder {reminder_id}"))?;
        self.reopen(r.deadline.id)?;
        self.memory.set_reminder_answer(reminder_id, "undone")?;
        Ok(tr("ans_undone", lang, &[]))
    }

    /// "Erledigt" in the deadline list.
    pub fn deadline_done(&self, deadline_id: i64, lang: Lang) -> Result<()> {
        let dl = self.memory.deadline(deadline_id)?.ok_or_else(|| anyhow!("no deadline {deadline_id}"))?;
        self.memory.set_deadline_status(deadline_id, DeadlineStatus::Done)?;
        self.memory.add_event("deadline_done", &tr("ev_deadline_done", lang, &[("title", &dl.title)]))?;
        Ok(())
    }

    /// Open a deadline again ("Rückgängig"); make sure one reminder is still ahead.
    pub fn reopen(&self, deadline_id: i64) -> Result<()> {
        self.memory.set_deadline_status(deadline_id, DeadlineStatus::Open)?;
        let now = self.clock.now();
        let dl = self.memory.deadline(deadline_id)?.ok_or_else(|| anyhow!("no deadline {deadline_id}"))?;
        let ahead = dl.remind_at.iter().any(|t| *t > now);
        if !ahead && dl.due >= now.date_naive() {
            self.memory.add_reminder(deadline_id, now + LATER)?;
        }
        Ok(())
    }
}

/// When "Erinnere mich …" should ring: AU -> morning of the due day (08:00, go to the doctor);
/// others -> the day before at 09:00, else the due morning. None if that is not at least
/// an hour away (then only "Später" makes sense).
fn remind_target(due: NaiveDate, is_au: bool, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let mut candidates = Vec::new();
    if !is_au {
        candidates.push(local_at(days_before(due, 1), 9, 0));
    }
    candidates.push(local_at(due, 8, 0));
    candidates.into_iter().find(|t| *t > now + Duration::hours(1) && t.date_naive() != now.date_naive())
}

#[cfg(test)]
mod tests;
