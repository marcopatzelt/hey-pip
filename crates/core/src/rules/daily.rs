//! Daily brief: when Pip starts, he says what is coming up (the next deadlines) and offers to do the next
//! thing with one click, if the insurer's services page has a heading that fits one of them.
//! Deterministic, no model call, no IO (the app reads the services page).
//! OWNER: agent "memory-rules".

use anyhow::Result;
use chrono::{DateTime, Duration, Local};

use super::text::{fmt_date, fmt_date_mid, join, tr};
use crate::i18n::Lang;
use crate::memory::{Deadline, Memory};

/// Deadlines within this many days (and overdue ones) go into the brief.
pub const BRIEF_DAYS: i64 = 21;
pub const BRIEF_MAX_ITEMS: usize = 2;
/// Words shorter than this do not count as a match between a deadline and a page heading.
const MIN_WORD: usize = 6;

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
    /// Bubble and voice text: greeting, one or two deadlines, the offer.
    pub text: String,
    pub offer: Option<BriefOffer>,
}

/// The brief for `now`, or None when nothing is due within BRIEF_DAYS (the app then says "welcome back").
/// `services`: (page URL, its HTML) when the insurer's services page is known and readable.
pub fn daily_brief(
    mem: &Memory,
    now: DateTime<Local>,
    lang: Lang,
    name: Option<&str>,
    services: Option<(&str, &str)>,
) -> Result<Option<Brief>> {
    let today = now.date_naive();
    let until = today + Duration::days(BRIEF_DAYS);
    let items: Vec<Deadline> =
        mem.open_deadlines()?.into_iter().filter(|dl| dl.due <= until).take(BRIEF_MAX_ITEMS).collect();
    if items.is_empty() {
        return Ok(None);
    }

    let mut text = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => tr("brief_hi", lang, &[("name", n)]),
        None => tr("brief_hi_noname", lang, &[]),
    };
    for dl in &items {
        text = join(&text, &item_sentence(mem, dl, now, lang)?);
    }

    let offer = services.and_then(|(url, html)| find_offer(&items, url, html));
    if let Some(o) = &offer {
        text = join(&text, &tr("brief_offer", lang, &[("quote", &o.heading)]));
    }
    Ok(Some(Brief { text, offer }))
}

/// One or two sentences about one deadline, worded like the reminders.
fn item_sentence(mem: &Memory, dl: &Deadline, now: DateTime<Local>, lang: Lang) -> Result<String> {
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
            if first.is_empty() { second } else { join(&first, &second) }
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

/// First (deadline, heading) pair, in brief order, that shares a long word.
fn find_offer(items: &[Deadline], url: &str, html: &str) -> Option<BriefOffer> {
    let headings: Vec<String> = page_headings(html).into_iter().filter(|h| !h.is_empty()).collect();
    for dl in items {
        let words = long_words(&dl.title);
        if let Some(heading) = headings.iter().find(|h| long_words(h).iter().any(|w| words.contains(w))) {
            let task = format!(
                "Open {url}. Click „{heading}“ (or its 'Open form' link). Fill the form with facts from WHAT YOU \
                 KNOW; ask me only for what is missing. Sending needs approval (is_commit=true). This is for \
                 deadline #{}.",
                dl.id
            );
            return Some(BriefOffer { heading: heading.clone(), deadline_id: dl.id, task });
        }
    }
    None
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
    use crate::rules::tests_support::{au_doc, d};

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
        let q = add(&mem, "Send the questionnaire back to Gesundkasse", d(2026, 11, 13), "frist_aus_brief", None);
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
        assert!(b.text.contains("you need a new one by Monday, November 2."), "{}", b.text);
        assert!(
            b.text.contains("The deadline \"Send the questionnaire back to Gesundkasse\" ends on Friday, November 13."),
            "{}",
            b.text
        );
        assert!(!b.text.contains("object"), "{}", b.text);
        assert!(!b.text.contains('{'), "{}", b.text);
        assert_eq!(b.offer, None);
        assert!(!b.text.contains("I can fill in"), "{}", b.text);

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
        assert!(o.task.contains("„Return your questionnaire online“"), "{}", o.task);
        assert!(o.task.contains(&format!("#{q}")), "{}", o.task);
        assert!(o.task.contains("is_commit=true"), "{}", o.task);
        assert!(
            b.text.ends_with("I can do \"Return your questionnaire online\" for you right now."),
            "{}",
            b.text
        );
        let de = daily_brief(&mem, now, Lang::De, Some("Erika"), Some((URL, SERVICES))).unwrap().unwrap();
        assert!(!de.text.contains('{'), "{}", de.text);
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
}
