//! Daily brief: when Pip starts, he says what is coming up (the next deadlines) and offers to do the next
//! thing with one click, if the insurer's services page has a heading that fits one of them.
//! After a finished task the same matching gives the proactive offer (`after_task`): "Your insurer
//! also offers "…", which fits their letter of …". Deterministic, no model call, no IO (the app reads the
//! services page).
//! OWNER: agent "memory-rules".

use anyhow::Result;
use chrono::{DateTime, Duration, Local};

use super::flight::{self, CLAIM_HEADING, EU261_RULE, PROFILE_CLAIM_FORM};
use super::text::{fmt_date, fmt_date_mid, join, tr};
use super::tips::{self, Tip};
use crate::agent::tools::truncate;
use crate::i18n::{Lang, t};
use crate::memory::{Deadline, Memory};

/// Deadlines within this many days (and overdue ones) go into the brief.
pub const BRIEF_DAYS: i64 = 21;
pub const BRIEF_MAX_ITEMS: usize = 2;
/// Words shorter than this do not count as a match between a deadline and a page heading.
const MIN_WORD: usize = 6;
/// Event kinds of a suggestion and its answer (the same strings the agent's `suggest` tool writes).
pub const EV_SUGGESTED: &str = "suggested";
pub const EV_DECLINED: &str = "suggestion_declined";
pub const EV_ACCEPTED: &str = "suggestion_accepted";
/// After a "Not now" Pip offers nothing for this many days (PIP_DISCOVER=always skips it, for video retakes).
pub const DECLINE_COOLDOWN_DAYS: i64 = 7;
/// How many recent events the suggestion checks look at.
const EVENTS_SCAN: u32 = 200;

/// What Pip offers to do right away: open the page, click `heading`, fill the form.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefOffer {
    /// The heading exactly as written on the services page.
    pub heading: String,
    pub deadline_id: i64,
    /// The task for the agent (English, like an accepted suggestion).
    pub task: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Brief {
    /// Bubble and voice text: greeting, one or two deadlines, the "Good to know" tip, the offer.
    pub text: String,
    pub offer: Option<BriefOffer>,
    /// The "Good to know" tip said in `text` (rule + source), at most once in tips::TIP_REPEAT_DAYS.
    pub tip: Option<Tip>,
}

/// The brief for `now`, or None when nothing is due within BRIEF_DAYS and no flight claim is open (the app then
/// says "welcome back"). `services`: (page URL, its HTML) when the insurer's services page is known and readable.
/// Money first: the first open `eu261_claim` deadline comes before the dated ones whatever its due date (it is
/// due years ahead), said by its "Good to know" tip when that tip is due, else by a plain sentence. When profile
/// `airline_claim_form` is set, the offer is the airline's claim page (heading CLAIM_HEADING); it wins over a
/// services heading (one offer per brief).
/// A "Good to know" tip (`tips::for_brief`) comes after the deadlines and before the offer (whose buttons follow
/// it); its deadline then drops the sentence the tip says anyway. Saying it writes the event `tip_shown`.
pub fn daily_brief(
    mem: &Memory,
    now: DateTime<Local>,
    lang: Lang,
    name: Option<&str>,
    services: Option<(&str, &str)>,
) -> Result<Option<Brief>> {
    let today = now.date_naive();
    let until = today + Duration::days(BRIEF_DAYS);
    let open = mem.open_deadlines()?;
    let is_claim = |dl: &Deadline| dl.rule_id.as_deref() == Some(EU261_RULE);
    // The first claim whose letter still says a flight that may give money (by due date, then id).
    let mut claim = None;
    for dl in open.iter().filter(|dl| is_claim(dl) && dl.due >= today) {
        let Some(row) = dl.doc_id.map(|id| mem.document_by_id(id)).transpose()?.flatten() else { continue };
        if flight::claim_of(&row.extract).is_some() {
            claim = Some((dl.clone(), row.extract));
            break;
        }
    }
    let items: Vec<Deadline> =
        open.into_iter().filter(|dl| !is_claim(dl) && dl.due <= until).take(BRIEF_MAX_ITEMS).collect();
    if items.is_empty() && claim.is_none() {
        return Ok(None);
    }

    let mut text = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => tr("brief_hi", lang, &[("name", n)]),
        None => tr("brief_hi_noname", lang, &[]),
    };
    let tip = tips::for_brief(mem, now, lang)?;
    let tip_deadline = tip.as_ref().and_then(|t| t.deadline_id);
    let mut tip_said = false;
    if let Some((dl, extract)) = &claim {
        match &tip {
            Some(t) if tip_deadline == Some(dl.id) => {
                text = join(&text, &t.text);
                tip_said = true;
            }
            _ => {
                if let Some((f, c)) = flight::claim_of(extract) {
                    text = join(&text, &flight::brief_line(f, &c, lang));
                }
            }
        }
    }
    for dl in &items {
        text = join(&text, &item_sentence(mem, dl, now, lang, tip_deadline == Some(dl.id))?);
    }
    if let Some(t) = &tip {
        if !tip_said {
            text = join(&text, &t.text);
        }
        tips::mark_shown(mem, t, now)?;
    }

    // A heading the user already said yes or "Not now" to is not offered again: no sentence, no button.
    let asked = asked_headings(mem)?;
    let claim_offer = claim.as_ref().and_then(|(dl, extract)| {
        let url = mem
            .profile_get(PROFILE_CLAIM_FORM)
            .ok()
            .flatten()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())?;
        if is_asked(&asked, CLAIM_HEADING) {
            return None;
        }
        let (f, c) = flight::claim_of(extract)?;
        // The stored page belongs to the airline in the profile: never open one airline's page for another's flight.
        let known_airline = mem.profile_get("airline").ok().flatten().unwrap_or_default();
        if !known_airline.trim().is_empty()
            && !f.airline.trim().is_empty()
            && !known_airline.trim().eq_ignore_ascii_case(f.airline.trim())
        {
            return None;
        }
        let task = flight::claim_task(&url, f, &c, dl.id);
        Some(BriefOffer { heading: CLAIM_HEADING.to_string(), deadline_id: dl.id, task })
    });
    let offer =
        claim_offer.or_else(|| services.and_then(|(url, html)| find_offer(&items, url, html, |h| is_asked(&asked, h))));
    if let Some(o) = &offer {
        text = join(&text, &tr("brief_offer", lang, &[("quote", &o.heading)]));
    }
    Ok(Some(Brief { text, offer, tip }))
}

/// The offer right after a finished task: the first open deadline (by due date, any distance) that shares a
/// long word with a heading of the services page, skipping headings offered before (any answer).
#[derive(Debug, Clone, PartialEq)]
pub struct AfterTaskOffer {
    pub offer: BriefOffer,
    /// "their letter of 27 October" (the deadline's letter has a date) or "your deadline on 13 November".
    pub reason: String,
    /// Profile `insurer` (max. 60 characters), else "Your health insurer".
    pub insurer: String,
}

/// What Pip does after a finished task (`after_task`).
#[derive(Debug, Clone, PartialEq)]
pub enum AfterTask {
    /// A heading of the services page fits an open deadline: offer it (no model call).
    Offer(AfterTaskOffer),
    /// Open deadlines exist but Rust cannot read the services page (no headings, e.g. a web page): the model
    /// may look around once (Task::Discover). A readable page where nothing fits is Silent.
    LookAround,
    /// Nothing to do: no open deadline, the quiet days after "Not now", or every fitting heading was asked
    /// before. Pip stays idle, says nothing and opens nothing.
    Silent,
}

/// The next step after a finished task (`after_task_on` without a summary of the task just done):
/// no open deadline -> Silent; a "Not now" within DECLINE_COOLDOWN_DAYS before `now` (PIP_DISCOVER=always
/// ignores it) -> Silent; a heading not asked before fits -> Offer; a readable page where nothing fits (or
/// only headings asked before) -> Silent, since the model would find the same; LookAround only when the
/// page has no readable headings (empty HTML: a web page Rust cannot read).
/// `services`: (page URL, its HTML; empty when the page cannot be read).
pub fn after_task(mem: &Memory, now: DateTime<Local>, lang: Lang, services: (&str, &str)) -> Result<AfterTask> {
    after_task_on(mem, now, lang, services, "")
}

/// `after_task`, but the task that just finished is not offered again: a deadline whose title shares a
/// long word with `just_done` (the agent's done summary) is skipped, so right after the questionnaire Pip
/// offers the next fitting deadline or stays quiet. Words of the insurer's and the user's name and a few
/// generic words do not count ("Sick note sent to Evergreen Health" must not hide the questionnaire for
/// Evergreen Health). The reason of every Silent and LookAround is logged at info.
pub fn after_task_on(
    mem: &Memory,
    now: DateTime<Local>,
    lang: Lang,
    services: (&str, &str),
    just_done: &str,
) -> Result<AfterTask> {
    let quiet = |why: &str| {
        tracing::info!("after-task offer: Silent: {why}");
        Ok(AfterTask::Silent)
    };
    let open = mem.open_deadlines()?;
    if open.is_empty() {
        return quiet("no open deadline");
    }
    let events = mem.recent_events(EVENTS_SCAN)?;
    let always = std::env::var("PIP_DISCOVER").is_ok_and(|v| v.trim().eq_ignore_ascii_case("always"));
    let since = now - Duration::days(DECLINE_COOLDOWN_DAYS);
    if !always && events.iter().any(|e| e.kind == EV_DECLINED && e.at >= since) {
        return quiet("the user said 'Not now' in the last days");
    }
    let done_words = task_words(mem, just_done)?;
    let items: Vec<Deadline> =
        open.into_iter().filter(|dl| !long_words(&dl.title).iter().any(|w| done_words.contains(w))).collect();
    if items.is_empty() {
        return quiet("the only open deadlines are the task just done");
    }
    let asked = asked_of(&events);
    let (url, html) = services;
    let headings = page_headings(html);
    let titles: Vec<&str> = items.iter().map(|dl| dl.title.as_str()).collect();
    let Some(offer) = find_offer(&items, url, html, |h| is_asked(&asked, h)) else {
        if headings.iter().all(|h| h.is_empty()) {
            // Rust cannot read this page (a web page, or no headings): the model may look once.
            tracing::info!(deadlines = ?titles, "after-task offer: LookAround: the services page has no readable headings");
            return Ok(AfterTask::LookAround);
        }
        tracing::info!(deadlines = ?titles, headings = ?headings, "after-task offer: no heading fits");
        // Something fits, but only the asked-filter removed it: asking again would nag. Nothing fits: the
        // model would open the page and find the same headings. Either way Pip stays quiet.
        if find_offer(&items, url, html, |_| false).is_some() {
            return quiet("every fitting heading was offered before");
        }
        return quiet("no heading of the services page fits an open deadline");
    };
    let Some(dl) = items.iter().find(|dl| dl.id == offer.deadline_id) else { return Ok(AfterTask::Silent) };
    let doc = match dl.doc_id {
        Some(id) => mem.document_by_id(id)?,
        None => None,
    };
    let reason = match doc.and_then(|d| d.extract.date) {
        Some(date) => tr("sugg_reason_letter", lang, &[("date", &fmt_date(date, lang))]),
        None => tr("sugg_reason_deadline", lang, &[("date", &fmt_date(dl.due, lang))]),
    };
    let insurer = mem
        .profile_get("insurer")?
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| truncate(&v, 60))
        .unwrap_or_else(|| t("sugg_insurer", lang).to_string());
    Ok(AfterTask::Offer(AfterTaskOffer { offer, reason, insurer }))
}

/// The offer of `after_task`, None for LookAround and Silent. `services`: (page URL, its HTML).
pub fn offer_after_task(
    mem: &Memory,
    now: DateTime<Local>,
    lang: Lang,
    services: (&str, &str),
) -> Result<Option<AfterTaskOffer>> {
    Ok(match after_task(mem, now, lang, services)? {
        AfterTask::Offer(o) => Some(o),
        _ => None,
    })
}

/// `heading` was offered before (by the brief, the after-task offer or the agent), any answer.
pub fn already_suggested(mem: &Memory, heading: &str) -> Result<bool> {
    Ok(is_asked(&asked_headings(mem)?, heading))
}

/// Headings of every earlier offer (lowercase, trimmed), from the last EVENTS_SCAN events.
fn asked_headings(mem: &Memory) -> Result<Vec<String>> {
    Ok(asked_of(&mem.recent_events(EVENTS_SCAN)?))
}

fn asked_of(events: &[crate::memory::Event]) -> Vec<String> {
    events
        .iter()
        .filter(|e| matches!(e.kind.as_str(), EV_SUGGESTED | EV_DECLINED | EV_ACCEPTED))
        .map(|e| e.text.trim().to_lowercase())
        .collect()
}

fn is_asked(asked: &[String], heading: &str) -> bool {
    asked.iter().any(|a| a == &heading.trim().to_lowercase())
}

/// One or two sentences about one deadline, worded like the reminders. `tip_says_due`: the brief's tip names
/// this deadline's date, so the sick note's "you need a new one by …" is left out.
fn item_sentence(mem: &Memory, dl: &Deadline, now: DateTime<Local>, lang: Lang, tip_says_due: bool) -> Result<String> {
    let today = now.date_naive();
    let due_mid = fmt_date_mid(dl.due, lang);
    if dl.due < today {
        return Ok(match dl.rule_id.as_deref() {
            Some("widerspruch_bescheid") => tr("rem_widerspruch_passed", lang, &[("due", &due_mid)]),
            _ => tr("rem_deadline_passed", lang, &[("title", &dl.title), ("due", &due_mid)]),
        });
    }
    Ok(match dl.rule_id.as_deref() {
        Some("au_lueckenlos") => {
            let doc = match dl.doc_id {
                Some(id) => mem.document_by_id(id)?,
                None => None,
            };
            let first = match doc.and_then(|row| row.extract.au).map(|au| au.until) {
                Some(end) => match end.signed_duration_since(today).num_days() {
                    0 => tr("rem_au_ends_today", lang, &[]),
                    1 => tr("rem_au_ends_tomorrow", lang, &[]),
                    n if n > 1 => tr("rem_au_ends_on", lang, &[("date", &fmt_date(end, lang))]),
                    _ => tr("rem_au_ended", lang, &[("date", &fmt_date_mid(end, lang))]),
                },
                None => String::new(),
            };
            let second = if dl.due == today {
                tr("rem_au_need_new_today", lang, &[])
            } else {
                tr("rem_au_need_new", lang, &[("due", &due_mid)])
            };
            // The tip names the follow-up date: say it once (with no sick-note line, the tip is the sentence).
            match (first.is_empty(), tip_says_due) {
                (_, true) => first,
                (true, false) => second,
                (false, false) => join(&first, &second),
            }
        }
        Some("widerspruch_bescheid") => {
            let first = if dl.due == today {
                tr("rem_widerspruch_ends_today", lang, &[])
            } else {
                tr("rem_widerspruch_ends", lang, &[("due", &fmt_date(dl.due, lang))])
            };
            join(&first, &tr("rem_widerspruch_action", lang, &[]))
        }
        _ if dl.due == today => tr("rem_deadline_today", lang, &[("title", &dl.title)]),
        _ => tr("rem_deadline_soon", lang, &[("title", &dl.title), ("due", &fmt_date(dl.due, lang))]),
    })
}

/// First (deadline, heading) pair, in the order of `items`, that shares a long word; headings for which
/// `skip` is true do not count.
fn find_offer(items: &[Deadline], url: &str, html: &str, skip: impl Fn(&str) -> bool) -> Option<BriefOffer> {
    let headings: Vec<String> = page_headings(html).into_iter().filter(|h| !h.is_empty() && !skip(h)).collect();
    for dl in items {
        let words = long_words(&dl.title);
        if let Some(heading) = headings.iter().find(|h| long_words(h).iter().any(|w| words.contains(w))) {
            let task = format!(
                "Open {url}. Click \"{heading}\" (or its 'Open form' link). Fill the form with facts from WHAT YOU \
                 KNOW; ask me only for what is missing. Sending needs approval (is_commit=true). This is for \
                 deadline #{}.",
                dl.id
            );
            return Some(BriefOffer { heading: heading.clone(), deadline_id: dl.id, task });
        }
    }
    None
}

/// Words that name no task: they appear in many summaries and deadline titles alike.
const GENERIC_WORDS: &[&str] =
    &["insurer", "insurance", "health", "krankenkasse", "letter", "online", "deadline", "erledigt", "geschickt"];

/// Long words of a task summary that name the task: without the insurer's and the user's names and
/// GENERIC_WORDS. Empty for an empty summary.
fn task_words(mem: &Memory, summary: &str) -> Result<Vec<String>> {
    if summary.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = GENERIC_WORDS.iter().map(|w| w.to_string()).collect();
    for key in ["insurer", "full_name", "name"] {
        if let Some(v) = mem.profile_get(key)? {
            names.extend(long_words(&v));
        }
    }
    Ok(long_words(summary).into_iter().filter(|w| !names.contains(w)).collect())
}

/// Lowercase words of at least MIN_WORD letters.
fn long_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= MIN_WORD)
        .map(str::to_string)
        .collect()
}

/// Text of every <h1>..<h6> of an HTML page.
fn page_headings(html: &str) -> Vec<String> {
    // ASCII lowercase keeps every byte offset the same as in `html`.
    let low = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = low[from..].find("<h").map(|i| from + i) {
        from = at + 2;
        let Some(level) = low.as_bytes().get(at + 2).filter(|b| (b'1'..=b'6').contains(b)) else { continue };
        let close = format!("</h{}", *level as char);
        let Some(end) = low[at..].find(&close).map(|i| at + i) else { break };
        out.push(strip_tags(&html[at..end]));
        from = end;
    }
    out
}

/// Text without tags, common entities decoded, whitespace squashed.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::memory::NewDeadline;
    use crate::rules::local_at;
    use crate::rules::tests_support::{au_doc, bescheid_doc, d};

    const SERVICES: &str = "<h1>Services for you</h1><section><h2>Return your questionnaire online</h2></section>\
                            <h2>Order a new insurance card</h2>";
    const URL: &str = "file:///C:/x/services.html";

    fn add(mem: &Memory, title: &str, due: chrono::NaiveDate, rule: &str, doc_id: Option<i64>) -> i64 {
        mem.add_deadline(&NewDeadline {
            title: title.into(),
            due,
            rule_id: Some(rule.into()),
            doc_id,
            remind_at: vec![],
            why: String::new(),
            source: String::new(),
        })
        .unwrap()
    }

    /// Sick note 20.10.–30.10. (new one due 2.11.), questionnaire 13.11., objection 18.11.; returns the
    /// questionnaire deadline id.
    fn erika() -> (Memory, i64) {
        let mem = Memory::open_in_memory().unwrap();
        let doc = mem
            .insert_document(Path::new("au.jpg"), "sha-au", &au_doc(d(2026, 10, 20), d(2026, 10, 30)), "AU")
            .unwrap();
        add(&mem, "New sick note, if you are still ill", d(2026, 11, 2), "au_lueckenlos", Some(doc));
        let q = add(&mem, "Send the questionnaire back to Evergreen Health", d(2026, 11, 13), "frist_aus_brief", None);
        add(&mem, "Objection to the decision", d(2026, 11, 18), "widerspruch_bescheid", None);
        (mem, q)
    }

    #[test]
    fn nothing_due_soon_gives_no_brief() {
        let mem = Memory::open_in_memory().unwrap();
        let now = local_at(d(2026, 10, 28), 9, 0);
        assert_eq!(daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap(), None);
        add(&mem, "Far away", d(2026, 12, 7), "frist_aus_brief", None);
        assert_eq!(daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap(), None);
    }

    #[test]
    fn brief_names_the_next_two_deadlines() {
        let (mem, _) = erika();
        let now = local_at(d(2026, 10, 28), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert!(b.text.starts_with("Hi Erika! Here's what's coming up."), "{}", b.text);
        assert!(b.text.contains("Your sick note ends on Friday, October 30."), "{}", b.text);
        // The follow-up date is said once, by the "Good to know" tip (due within 7 days).
        assert!(!b.text.contains("you need a new one"), "{}", b.text);
        assert!(b.text.contains("issued by Monday, 2 November"), "{}", b.text);
        assert!(
            b.text.contains(
                "The deadline \"Send the questionnaire back to Evergreen Health\" ends on Friday, November 13."
            ),
            "{}",
            b.text
        );
        assert!(!b.text.contains("object"), "{}", b.text);
        assert!(!b.text.contains('{'), "{}", b.text);
        assert_eq!(b.offer, None);
        assert!(!b.text.contains("I can fill in"), "{}", b.text);

        // The tip was said: the next briefs on the same day name the follow-up date the old way.
        let again = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert_eq!(again.tip, None);
        assert!(again.text.contains("you need a new one by Monday, November 2."), "{}", again.text);
        let de = daily_brief(&mem, now, Lang::De, Some("Erika"), None).unwrap().unwrap();
        assert!(de.text.starts_with("Hallo Erika!"), "{}", de.text);
        assert!(!de.text.contains('{'), "{}", de.text);
        let anon = daily_brief(&mem, now, Lang::En, None, None).unwrap().unwrap();
        assert!(anon.text.starts_with("Hi! Here's what's coming up."), "{}", anon.text);
    }

    #[test]
    fn sick_note_ending_tomorrow() {
        let (mem, _) = erika();
        let b = daily_brief(&mem, local_at(d(2026, 10, 29), 9, 0), Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert!(b.text.contains("Your sick note ends tomorrow."), "{}", b.text);
    }

    /// Sentences of a brief after its greeting ("Hi Erika! Here's what's coming up.").
    fn body_sentences(text: &str) -> usize {
        let body = text.split_once("coming up.").map_or(text, |(_, b)| b);
        body.split_inclusive(['.', '!', '?']).filter(|s| !s.trim().is_empty()).count()
    }

    #[test]
    fn brief_ends_with_the_good_to_know_tip_once() {
        let (mem, _) = erika();
        let now = local_at(d(2026, 10, 29), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        let tip = b.tip.clone().expect("tip");
        assert_eq!(tip.id, "au_gap");
        assert_eq!(tip.source, "§ 46 Satz 2 SGB V");
        assert!(b.text.ends_with(&tip.text), "{}", b.text);
        assert!(tip.text.starts_with("Good to know"), "{}", tip.text);
        assert!(tip.text.contains("Monday, 2 November"), "{}", tip.text);
        assert!(b.text.contains("Your sick note ends tomorrow."), "{}", b.text);
        assert!(b.text.contains("questionnaire"), "two deadlines and the tip: {}", b.text);
        assert!(body_sentences(&b.text) < 4, "{}: {}", body_sentences(&b.text), b.text);
        let ev = mem.recent_events(10).unwrap();
        assert!(ev.iter().any(|e| e.kind == tips::EV_TIP_SHOWN && e.text == "au_gap"), "{ev:?}");

        // The next day: no tip again, and the brief still names the follow-up date.
        let next = daily_brief(&mem, now + Duration::days(1), Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert_eq!(next.tip, None, "{}", next.text);
        assert!(!next.text.contains("Good to know"), "{}", next.text);
        assert!(next.text.contains("you need a new one by Monday, November 2."), "{}", next.text);
    }

    #[test]
    fn brief_tip_comes_before_the_offer_and_can_be_switched_off() {
        let (mem, _) = erika();
        let now = local_at(d(2026, 10, 29), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert!(b.tip.is_some() && b.offer.is_some());
        assert!(b.text.ends_with("I can do \"Return your questionnaire online\" for you right now."), "{}", b.text);
        assert!(b.text.contains("please check with your insurer. I can do"), "{}", b.text);

        let (mem, _) = erika();
        mem.profile_set("tips", "off").unwrap();
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert_eq!(b.tip, None);
        assert!(!b.text.contains("Good to know"), "{}", b.text);
        assert!(b.text.contains("you need a new one by Monday, November 2."), "{}", b.text);

        let (mem, _) = erika();
        let de = daily_brief(&mem, now, Lang::De, Some("Erika"), None).unwrap().unwrap();
        assert!(de.text.contains("Gut zu wissen") && de.text.contains("Montag, 2. November"), "{}", de.text);
        assert!(!de.text.contains('{'), "{}", de.text);
    }

    /// A sick-note deadline without its letter: the tip is the only sentence that names the follow-up date.
    #[test]
    fn brief_tip_says_the_follow_up_date_once_without_the_letter() {
        let mem = Memory::open_in_memory().unwrap();
        add(&mem, "New sick note, if you are still ill", d(2026, 11, 2), "au_lueckenlos", None);
        let now = local_at(d(2026, 10, 29), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert!(b.tip.is_some(), "{}", b.text);
        assert!(!b.text.contains("you need a new one"), "{}", b.text);
        assert_eq!(b.text.matches("2 November").count() + b.text.matches("November 2").count(), 1, "{}", b.text);
        assert!(body_sentences(&b.text) < 4, "{}", b.text);
    }

    #[test]
    fn overdue_deadline_is_said_as_passed() {
        let mem = Memory::open_in_memory().unwrap();
        add(&mem, "Send the form", d(2026, 10, 20), "frist_aus_brief", None);
        let b = daily_brief(&mem, local_at(d(2026, 10, 28), 9, 0), Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert!(b.text.contains("The deadline \"Send the form\" ended on Tuesday, October 20."), "{}", b.text);
    }

    #[test]
    fn offer_fits_a_heading_on_the_services_page() {
        let (mem, q) = erika();
        let now = local_at(d(2026, 10, 28), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        let o = b.offer.clone().expect("offer");
        assert_eq!(o.heading, "Return your questionnaire online");
        assert_eq!(o.deadline_id, q);
        assert!(o.task.contains(URL), "{}", o.task);
        assert!(o.task.contains("\"Return your questionnaire online\""), "{}", o.task);
        assert!(o.task.contains(&format!("#{q}")), "{}", o.task);
        assert!(o.task.contains("is_commit=true"), "{}", o.task);
        assert!(b.text.ends_with("I can do \"Return your questionnaire online\" for you right now."), "{}", b.text);
        let de = daily_brief(&mem, now, Lang::De, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert!(!de.text.contains('{'), "{}", de.text);
    }

    /// Once the user answered an offer for a heading (yes or "Not now"), the brief neither promises it in
    /// the text nor shows the button: text and buttons always agree.
    #[test]
    fn brief_drops_an_answered_heading_text_and_offer() {
        let now = local_at(d(2026, 10, 28), 9, 0);
        for kind in [EV_SUGGESTED, EV_DECLINED, EV_ACCEPTED] {
            let (mem, _) = erika();
            mem.add_event_at(now - Duration::days(1), kind, " RETURN your questionnaire online ").unwrap();
            let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
            assert_eq!(b.offer, None, "{kind}");
            assert!(!b.text.contains("I can do"), "{kind}: {}", b.text);
            assert!(b.text.contains("questionnaire"), "the deadline itself is still named: {}", b.text);
        }
    }

    #[test]
    fn short_shared_words_give_no_offer() {
        let mem = Memory::open_in_memory().unwrap();
        add(&mem, "Get a sick note", d(2026, 11, 2), "frist_aus_brief", None);
        let html = "<h2>Sick pay</h2><h2>Order a new card</h2>";
        let b = daily_brief(&mem, local_at(d(2026, 10, 28), 9, 0), Lang::En, None, Some((URL, html))).unwrap().unwrap();
        assert_eq!(b.offer, None);
    }

    #[test]
    fn headings_are_read_without_tags() {
        assert_eq!(
            page_headings("<H1 class=x>A &amp; <b>B</b></H1><p>no</p><h3>\n C </h3><hr>"),
            vec!["A & B".to_string(), "C".to_string()]
        );
    }
    /// Tests that read or set PIP_DISCOVER hold this (the environment is shared by all tests in this binary).
    static DISCOVER_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        DISCOVER_ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now() -> DateTime<Local> {
        local_at(d(2026, 10, 28), 14, 0)
    }

    /// The questionnaire deadline comes from a letter dated 27 October.
    fn with_letter() -> (Memory, i64) {
        let mem = Memory::open_in_memory().unwrap();
        let doc =
            mem.insert_document(Path::new("brief.pdf"), "sha-q", &bescheid_doc(d(2026, 10, 27)), "Brief").unwrap();
        let q =
            add(&mem, "Send the questionnaire back to Evergreen Health", d(2026, 11, 13), "frist_aus_brief", Some(doc));
        (mem, q)
    }

    #[test]
    fn after_task_offer_names_the_letter() {
        let _env = env_lock();
        let (mem, q) = with_letter();
        mem.profile_set("insurer", "  Evergreen Health  ").unwrap();
        let o = offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().expect("offer");
        assert_eq!(o.offer.heading, "Return your questionnaire online");
        assert_eq!(o.offer.deadline_id, q);
        assert!(o.offer.task.contains(URL) && o.offer.task.contains("is_commit=true"), "{}", o.offer.task);
        assert_eq!(o.reason, "their letter of Tuesday, October 27");
        assert_eq!(o.insurer, "Evergreen Health");
        let de = offer_after_task(&mem, now(), Lang::De, (URL, SERVICES)).unwrap().expect("offer");
        assert!(de.reason.starts_with("ihrem Brief vom"), "{}", de.reason);
    }

    #[test]
    fn after_task_offer_without_letter_names_the_deadline() {
        let _env = env_lock();
        let (mem, q) = erika();
        let o = offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().expect("offer");
        assert_eq!(o.offer.deadline_id, q);
        assert_eq!(o.reason, "your deadline on Friday, November 13");
        assert_eq!(o.insurer, "Your health insurer");
        let long = "X".repeat(80);
        mem.profile_set("insurer", &long).unwrap();
        let o = offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().expect("offer");
        assert_eq!(o.insurer.chars().count(), 61, "60 characters and an ellipsis");
    }

    #[test]
    fn after_task_offer_sees_far_deadlines() {
        let _env = env_lock();
        let mem = Memory::open_in_memory().unwrap();
        let q = add(&mem, "Questionnaire for the insurer", d(2027, 1, 30), "frist_aus_brief", None);
        assert_eq!(daily_brief(&mem, now(), Lang::En, None, Some((URL, SERVICES))).unwrap(), None);
        let o = offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().expect("offer");
        assert_eq!(o.offer.deadline_id, q);
    }

    #[test]
    fn after_task_offer_skips_headings_asked_before() {
        let _env = env_lock();
        let html = "<h2>Return your questionnaire online</h2><h2>Questionnaire help line</h2>";
        for kind in [EV_SUGGESTED, EV_DECLINED, EV_ACCEPTED] {
            let (mem, _) = erika();
            // A decline long ago: no cooldown, but the heading still counts as asked.
            mem.add_event_at(now() - Duration::days(30), kind, " return YOUR questionnaire online ").unwrap();
            let o = offer_after_task(&mem, now(), Lang::En, (URL, html)).unwrap().expect("next heading");
            assert_eq!(o.offer.heading, "Questionnaire help line", "{kind}");
            mem.add_event_at(now() - Duration::days(30), kind, "Questionnaire help line").unwrap();
            assert_eq!(offer_after_task(&mem, now(), Lang::En, (URL, html)).unwrap(), None, "{kind}");
        }
        // Other event kinds with the same text do not count.
        let (mem, _) = erika();
        mem.add_event_at(now(), "task_done", "Return your questionnaire online").unwrap();
        assert!(offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().is_some());
    }

    #[test]
    fn after_task_offer_is_quiet_after_not_now() {
        let _env = env_lock();
        let (mem, _) = erika();
        mem.add_event_at(now() - Duration::days(3), EV_DECLINED, "Something else entirely").unwrap();
        assert_eq!(offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), None);

        // SAFETY: DISCOVER_ENV serialises every test in this binary that reads PIP_DISCOVER.
        unsafe { std::env::set_var("PIP_DISCOVER", " Always ") };
        let o = offer_after_task(&mem, now(), Lang::En, (URL, SERVICES));
        unsafe { std::env::remove_var("PIP_DISCOVER") };
        assert!(o.unwrap().is_some(), "PIP_DISCOVER=always ignores the quiet days");

        let (mem, _) = erika();
        mem.add_event_at(now() - Duration::days(8), EV_DECLINED, "Something else entirely").unwrap();
        assert!(offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap().is_some());
    }

    #[test]
    fn after_task_offer_ignores_done_deadlines() {
        let _env = env_lock();
        let (mem, q) = erika();
        mem.set_deadline_status(q, crate::memory::DeadlineStatus::Done).unwrap();
        assert_eq!(offer_after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), None);
    }

    #[test]
    fn after_task_offers_when_a_heading_fits() {
        let _env = env_lock();
        let (mem, q) = with_letter();
        match after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap() {
            AfterTask::Offer(o) => {
                assert_eq!(o.offer.heading, "Return your questionnaire online");
                assert_eq!(o.offer.deadline_id, q);
                assert!(o.offer.task.contains("\"Return your questionnaire online\""), "{}", o.offer.task);
                assert!(!o.offer.task.contains('„'), "{}", o.offer.task);
            }
            other => panic!("expected an offer, got {other:?}"),
        }
    }

    #[test]
    fn after_task_is_silent_without_open_deadlines() {
        let _env = env_lock();
        let mem = Memory::open_in_memory().unwrap();
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), AfterTask::Silent);
        let (mem, _) = erika();
        for dl in mem.open_deadlines().unwrap() {
            mem.set_deadline_status(dl.id, crate::memory::DeadlineStatus::Done).unwrap();
        }
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), AfterTask::Silent);
        // Without open deadlines even an unreadable page does not start the model.
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, "")).unwrap(), AfterTask::Silent);
    }

    #[test]
    fn after_task_is_silent_when_every_fitting_heading_was_asked() {
        let _env = env_lock();
        for kind in [EV_ACCEPTED, EV_DECLINED] {
            let (mem, _) = with_letter();
            mem.add_event_at(now() - Duration::days(30), kind, "Return your questionnaire online").unwrap();
            assert_eq!(after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), AfterTask::Silent, "{kind}");
        }
    }

    #[test]
    fn after_task_is_silent_in_the_cooldown() {
        let _env = env_lock();
        let (mem, _) = with_letter();
        mem.add_event_at(now() - Duration::days(3), EV_DECLINED, "Something else entirely").unwrap();
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, SERVICES)).unwrap(), AfterTask::Silent);
        // A page with nothing fitting does not turn the quiet days into a look around either.
        let html = "<h2>Order a new insurance card</h2>";
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, html)).unwrap(), AfterTask::Silent);

        // SAFETY: DISCOVER_ENV serialises every test in this binary that reads PIP_DISCOVER.
        unsafe { std::env::set_var("PIP_DISCOVER", "always") };
        let r = after_task(&mem, now(), Lang::En, (URL, SERVICES));
        unsafe { std::env::remove_var("PIP_DISCOVER") };
        assert!(matches!(r.unwrap(), AfterTask::Offer(_)), "PIP_DISCOVER=always ignores the quiet days");
    }

    #[test]
    fn after_task_looks_around_when_nothing_fits() {
        let _env = env_lock();
        let mem = Memory::open_in_memory().unwrap();
        add(&mem, "Objection to the decision", d(2026, 11, 18), "widerspruch_bescheid", None);
        // A readable page where nothing fits: the model would find the same, so Pip stays quiet.
        let html = "<h1>Services for you</h1><h2>Order a new insurance card</h2><h2>Change your address</h2>";
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, html)).unwrap(), AfterTask::Silent);
        // An unreadable page (empty HTML, or no headings) with an open deadline: the model may look.
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, "")).unwrap(), AfterTask::LookAround);
        assert_eq!(after_task(&mem, now(), Lang::En, (URL, "<p>Loading…</p>")).unwrap(), AfterTask::LookAround);
    }

    #[test]
    fn after_task_skips_the_deadline_just_done() {
        let _env = env_lock();
        let html = "<h2>Return your questionnaire online</h2><h2>Objection to a decision: write to us</h2>";
        let done = "Sent the questionnaire to Evergreen Health";
        // The questionnaire is skipped, the next fitting deadline is offered.
        let (mem, q) = erika();
        mem.profile_set("insurer", "Evergreen Health").unwrap();
        match after_task_on(&mem, now(), Lang::En, (URL, html), done).unwrap() {
            AfterTask::Offer(o) => {
                assert_ne!(o.offer.deadline_id, q);
                assert_eq!(o.offer.heading, "Objection to a decision: write to us");
            }
            other => panic!("expected the next offer, got {other:?}"),
        }
        // Nothing else fits on a readable page: silent, never a look around.
        assert_eq!(after_task_on(&mem, now(), Lang::En, (URL, SERVICES), done).unwrap(), AfterTask::Silent);
        // The insurer's name alone does not hide the questionnaire (the sick note went to the same insurer).
        let sick = "Sent your sick note to Evergreen Health.";
        match after_task_on(&mem, now(), Lang::En, (URL, SERVICES), sick).unwrap() {
            AfterTask::Offer(o) => assert_eq!(o.offer.deadline_id, q),
            other => panic!("expected the questionnaire, got {other:?}"),
        }
    }

    #[test]
    fn after_task_wrapper_equals_after_task_on_with_empty_summary() {
        let _env = env_lock();
        let html_none = "<h2>Order a new insurance card</h2>";
        let empty = Memory::open_in_memory().unwrap();
        let (fits, _) = with_letter();
        let (asked, _) = with_letter();
        asked.add_event_at(now() - Duration::days(30), EV_ACCEPTED, "Return your questionnaire online").unwrap();
        for (mem, html) in [(&empty, SERVICES), (&fits, SERVICES), (&fits, html_none), (&fits, ""), (&asked, SERVICES)]
        {
            assert_eq!(
                after_task(mem, now(), Lang::En, (URL, html)).unwrap(),
                after_task_on(mem, now(), Lang::En, (URL, html), "").unwrap(),
                "{html}"
            );
        }
    }

    #[test]
    fn after_task_on_the_real_services_page() {
        let _env = env_lock();
        let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo-insurer/services.html");
        let html = std::fs::read_to_string(page).unwrap();
        let (mem, q) = with_letter();
        mem.profile_set("insurer", "Evergreen Health").unwrap();
        match after_task_on(&mem, now(), Lang::En, (URL, &html), "Sent your sick note to Evergreen Health.").unwrap() {
            AfterTask::Offer(o) => assert_eq!(o.offer.deadline_id, q),
            other => panic!("expected the questionnaire, got {other:?}"),
        }
        let done = "Sent the questionnaire to Evergreen Health";
        assert_eq!(after_task_on(&mem, now(), Lang::En, (URL, &html), done).unwrap(), AfterTask::Silent);
    }

    #[test]
    fn offer_after_task_wrapper_matches_after_task() {
        let _env = env_lock();
        let html_none = "<h2>Order a new insurance card</h2>";
        let empty = Memory::open_in_memory().unwrap();
        let (fits, _) = with_letter();
        let (asked, _) = with_letter();
        asked.add_event_at(now() - Duration::days(30), EV_ACCEPTED, "Return your questionnaire online").unwrap();
        for (mem, html) in [(&empty, SERVICES), (&fits, SERVICES), (&fits, html_none), (&asked, SERVICES)] {
            let full = after_task(mem, now(), Lang::En, (URL, html)).unwrap();
            let wrapped = offer_after_task(mem, now(), Lang::En, (URL, html)).unwrap();
            match full {
                AfterTask::Offer(o) => assert_eq!(wrapped, Some(o)),
                _ => assert_eq!(wrapped, None, "{full:?}"),
            }
        }
    }

    #[test]
    fn already_suggested_checks_the_three_kinds() {
        let mem = Memory::open_in_memory().unwrap();
        assert!(!already_suggested(&mem, "Return your questionnaire online").unwrap());
        mem.add_event("task_done", "Return your questionnaire online").unwrap();
        assert!(!already_suggested(&mem, "Return your questionnaire online").unwrap());
        mem.add_event(EV_ACCEPTED, "Return your questionnaire online").unwrap();
        assert!(already_suggested(&mem, " return your QUESTIONNAIRE online").unwrap());
        assert!(!already_suggested(&mem, "Order a new insurance card").unwrap());
    }

    /// Erika's memory plus Beispiel Air's delay notice and its open claim (due 31 Dec 2029).
    fn erika_with_flight() -> (Memory, i64, i64) {
        use crate::rules::flight::tests::{flight, notice_doc};
        let (mem, q) = erika();
        let doc = mem
            .insert_document(
                Path::new("air.pdf"),
                "sha-air",
                &notice_doc(flight("DUS", "TFS", Some(250), false, None)),
                "x",
            )
            .unwrap();
        let claim = add(
            &mem,
            "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)",
            d(2029, 12, 31),
            EU261_RULE,
            Some(doc),
        );
        (mem, q, claim)
    }

    const AIR: &str = "file:///C:/Pip/fixtures/demo-air/index.html";

    #[test]
    fn brief_starts_with_the_money_and_offers_the_claim() {
        let (mem, _, claim) = erika_with_flight();
        mem.profile_set(PROFILE_CLAIM_FORM, AIR).unwrap();
        let now = local_at(d(2026, 10, 28), 9, 0);
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        let body = b.text.strip_prefix("Hi Erika! Here's what's coming up. ").expect("greeting first");
        assert!(
            body.starts_with(
                "Good to know: your flight BX 2041 on Saturday, 3 October was about 4 hours late; under EU rule \
                 261/2004 you may be owed EUR 400 from Beispiel Air; please check."
            ),
            "{}",
            b.text
        );
        assert_eq!(b.tip.as_ref().map(|t| t.id), Some("eu261_delay"));
        assert_eq!(b.text.matches("Good to know").count(), 1, "one tip per brief: {}", b.text);
        // The dated deadlines follow the money.
        assert!(b.text.contains("Your sick note ends on Friday, October 30."), "{}", b.text);
        let offer = b.offer.expect("the claim wins over the questionnaire heading");
        assert_eq!(offer.heading, CLAIM_HEADING);
        assert_eq!(offer.deadline_id, claim);
        assert_eq!(
            offer.task,
            format!(
                "Open {AIR} and claim EUR 400 from Beispiel Air for flight BX 2041 on 3 October 2026 (deadline \
                 #{claim}). Fill in what you know about me, attach my boarding pass, and stop at the send button."
            )
        );
        assert!(b.text.ends_with(&format!("I can do \"{CLAIM_HEADING}\" for you right now.")), "{}", b.text);

        // The tip was said today: the next brief names the money in a plain sentence, still first.
        let again = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert!(
            again.text.starts_with(
                "Hi Erika! Here's what's coming up. Your flight BX 2041 on Saturday, 3 October was about 4 hours late; \
                 you may be owed EUR 400 from Beispiel Air."
            ),
            "{}",
            again.text
        );
        for text in [&b.text, &again.text] {
            let low = text.to_lowercase();
            assert!(!low.contains("earned") && !low.contains("entitled") && !low.contains("will get"), "{text}");
        }
    }

    #[test]
    fn claim_offer_needs_the_page_and_is_asked_only_once() {
        let (mem, q, _) = erika_with_flight();
        let now = local_at(d(2026, 10, 28), 9, 0);
        // No claim page known: the services heading as before.
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert_eq!(b.offer.map(|o| o.deadline_id), Some(q));
        // A claim heading already offered: not again; the services heading may come.
        mem.profile_set(PROFILE_CLAIM_FORM, AIR).unwrap();
        mem.add_event(EV_DECLINED, CLAIM_HEADING).unwrap();
        assert!(already_suggested(&mem, CLAIM_HEADING).unwrap());
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert_eq!(b.offer.map(|o| o.heading), Some("Return your questionnaire online".to_string()));
        // The stored page is another airline's: no claim offer there (the services heading instead).
        let (mem, q, _) = erika_with_flight();
        mem.profile_set(PROFILE_CLAIM_FORM, AIR).unwrap();
        mem.profile_set("airline", "Other Air").unwrap();
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert_eq!(b.offer.map(|o| o.deadline_id), Some(q));
        mem.profile_set("airline", "beispiel air").unwrap();
        let b = daily_brief(&mem, now, Lang::En, Some("Erika"), None).unwrap().unwrap();
        assert_eq!(b.offer.map(|o| o.heading), Some(CLAIM_HEADING.to_string()), "case does not count");
    }

    #[test]
    fn a_claim_alone_is_enough_for_a_brief() {
        use crate::rules::flight::tests::{flight, notice_doc};
        let mem = Memory::open_in_memory().unwrap();
        let doc = mem
            .insert_document(
                Path::new("air.pdf"),
                "sha-air",
                &notice_doc(flight("DUS", "TFS", Some(250), false, None)),
                "x",
            )
            .unwrap();
        add(&mem, "Claim", d(2029, 12, 31), EU261_RULE, Some(doc));
        let b = daily_brief(&mem, local_at(d(2026, 10, 28), 9, 0), Lang::De, Some("Erika"), None).unwrap().unwrap();
        assert!(b.text.contains("Dein Flug BX 2041 am Samstag, 3. Oktober war etwa 4 Stunden zu spät"), "{}", b.text);
        assert!(b.text.contains("400 Euro von Beispiel Air"), "{}", b.text);
    }
}
