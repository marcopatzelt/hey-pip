//! "Pip did this for me": the shareable impact card. Counts what Pip did from memory (no model call) and
//! renders one self-contained HTML page (inline CSS, Pip's picture as a data URL, no external requests).
//! The only personal value on the page is the first name; numbers only, never a letter's content.

use anyhow::Result;
use base64::Engine;
use chrono::{Datelike, NaiveDate};

use super::db::Memory;
use super::learn::PROFILE_SECONDS_SAVED;
use crate::agent::loop_::ATTACH_APPROVED_NOTE;
use crate::i18n::Lang;

/// What Pip did, counted from memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub letters_read: u64,
    /// Sum of `runs` over the playbooks (one per finished form on a site).
    pub forms_filled: u64,
    /// Commit actions that were executed with approval (a yes on the card, or a site the user trusted).
    pub sent_after_yes: u64,
    pub deadlines_kept: u64,
    pub reminders_on_time: u64,
    pub seconds_saved: u64,
    /// The day of the oldest thing Pip remembers.
    pub since: Option<NaiveDate>,
}

impl Tally {
    /// Nothing done together yet (`since` does not count).
    pub fn is_empty(&self) -> bool {
        self.letters_read == 0
            && self.forms_filled == 0
            && self.sent_after_yes == 0
            && self.deadlines_kept == 0
            && self.reminders_on_time == 0
            && self.seconds_saved == 0
    }
}

/// How many rows of the action log are looked at (the log keeps at most 5000).
const ACTIONS_LOOKED_AT: u32 = 5000;

/// Counts from memory, no model call.
pub fn tally(mem: &Memory) -> Result<Tally> {
    let forms_filled = mem.playbook_all()?.iter().map(|p| p.runs.max(0) as u64).sum();
    // An approved send and a send on a trusted site are both logged as risk "commit" with approved = true;
    // a "no", a timeout or a click that did not happen ("the page changed") is approved = false. An approved
    // attach card is also a commit with a yes, but nothing left the computer: its row carries the note.
    let sent_after_yes = mem
        .recent_actions(ACTIONS_LOOKED_AT)?
        .iter()
        .filter(|a| a.risk == "commit" && a.approved == Some(true) && !a.result.contains(ATTACH_APPROVED_NOTE))
        .count() as u64;
    let seconds_saved = mem.profile_get(PROFILE_SECONDS_SAVED)?.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    Ok(Tally {
        letters_read: mem.documents_count()?,
        forms_filled,
        sent_after_yes,
        deadlines_kept: mem.deadlines_done_count()?,
        reminders_on_time: mem.reminders_fired_count()?,
        seconds_saved,
        since: mem.first_event_at()?.map(|t| t.date_naive()),
    })
}

/// Pip, happy (the same picture as in the app).
const PIP_HAPPY_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/pip/png/happy.png"));

/// The fixed texts of the card. English first, German second.
struct Words {
    html_lang: &'static str,
    headline: &'static str,
    footer: &'static str,
    built_with: &'static str,
    copy: &'static str,
    copied: &'static str,
    about: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::En => Words {
            html_lang: "en",
            headline: "Pip did this for me",
            footer: "Nothing was sent without my yes. My letters stay on my computer.",
            built_with: "Built with Meta Muse Spark 1.3, SAM 3.1 and Muse Voice Transcribe · #MetaHackathon",
            copy: "Copy text for a post",
            copied: "Copied!",
            about: "about",
        },
        Lang::De => Words {
            html_lang: "de",
            headline: "Das hat Pip für mich erledigt",
            footer: "Nichts wurde ohne mein Ja gesendet. Meine Briefe bleiben auf meinem Computer.",
            built_with: "Gebaut mit Meta Muse Spark 1.3, SAM 3.1 und Muse Voice Transcribe · #MetaHackathon",
            copy: "Text für einen Post kopieren",
            copied: "Kopiert!",
            about: "etwa",
        },
    }
}

/// What a tile counts (the label follows the number: singular or plural).
#[derive(Clone, Copy)]
enum Kind {
    Letters,
    Forms,
    Sent,
    Deadlines,
    Reminders,
}

fn label(kind: Kind, n: u64, lang: Lang) -> &'static str {
    let one = n == 1;
    match (lang, kind) {
        (Lang::En, Kind::Letters) if one => "letter read and explained",
        (Lang::En, Kind::Letters) => "letters read and explained",
        (Lang::En, Kind::Forms) if one => "form filled in",
        (Lang::En, Kind::Forms) => "forms filled in",
        (Lang::En, Kind::Sent) => "sent, each after my yes",
        (Lang::En, Kind::Deadlines) if one => "deadline kept",
        (Lang::En, Kind::Deadlines) => "deadlines kept",
        (Lang::En, Kind::Reminders) if one => "reminder on time",
        (Lang::En, Kind::Reminders) => "reminders on time",
        (Lang::De, Kind::Letters) if one => "Brief gelesen und erklärt",
        (Lang::De, Kind::Letters) => "Briefe gelesen und erklärt",
        (Lang::De, Kind::Forms) if one => "Formular ausgefüllt",
        (Lang::De, Kind::Forms) => "Formulare ausgefüllt",
        (Lang::De, Kind::Sent) => "gesendet, jedes Mal nach meinem Ja",
        (Lang::De, Kind::Deadlines) if one => "Frist eingehalten",
        (Lang::De, Kind::Deadlines) => "Fristen eingehalten",
        (Lang::De, Kind::Reminders) if one => "Erinnerung pünktlich",
        (Lang::De, Kind::Reminders) => "Erinnerungen pünktlich",
    }
}

/// Saved time as (big part, label): (25, "minutes saved") or ("1 h 15 min", "saved"). Whole minutes, rounded
/// down (never claim more), like the "done" bubble; under a minute the seconds, so a first short run never
/// reads "0 minutes".
fn saved_parts(secs: u64, lang: Lang) -> (String, &'static str) {
    let minutes = secs / 60;
    if minutes >= 60 {
        let (h, m) = (minutes / 60, minutes % 60);
        let big = if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") };
        return (big, if lang == Lang::De { "gespart" } else { "saved" });
    }
    if minutes == 0 {
        let label = match (lang, secs) {
            (Lang::En, 1) => "second saved",
            (Lang::En, _) => "seconds saved",
            (Lang::De, 1) => "Sekunde gespart",
            (Lang::De, _) => "Sekunden gespart",
        };
        return (secs.to_string(), label);
    }
    let label = match (lang, minutes) {
        (Lang::En, 1) => "minute saved",
        (Lang::En, _) => "minutes saved",
        (Lang::De, 1) => "Minute gespart",
        (Lang::De, _) => "Minuten gespart",
    };
    (minutes.to_string(), label)
}

/// "about 25 minutes saved" / "etwa 1 h 15 min gespart".
fn saved_phrase(secs: u64, lang: Lang) -> String {
    let (big, label) = saved_parts(secs, lang);
    format!("{} {big} {label}", words(lang).about)
}

/// "14 October" / "14. Oktober".
fn day_month(d: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::En => d.format("%-d %B").to_string(),
        Lang::De => {
            const MONTHS: [&str; 12] = [
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
            ];
            format!("{}. {}", d.day(), MONTHS[(d.month0() as usize).min(11)])
        }
    }
}

/// "Erika's helper on the screen since 14 October" (without a name: "My helper …"; without a day: no "since").
fn subline(first_name: Option<&str>, since: Option<NaiveDate>, lang: Lang) -> String {
    let name = first_name.map(str::trim).filter(|n| !n.is_empty());
    let who = match (lang, name) {
        (Lang::En, Some(n)) => format!("{n}'s helper on the screen"),
        (Lang::En, None) => "My helper on the screen".to_string(),
        // German genitive of a name: "Erikas", but "Hans'" (ends in s, ß, x, z).
        (Lang::De, Some(n)) if n.ends_with(['s', 'ß', 'x', 'z', 'S', 'X', 'Z']) => {
            format!("{n}' Helfer auf dem Bildschirm")
        }
        (Lang::De, Some(n)) => format!("{n}s Helfer auf dem Bildschirm"),
        (Lang::De, None) => "Mein Helfer auf dem Bildschirm".to_string(),
    };
    match (since, lang) {
        (Some(d), Lang::En) => format!("{who} since {}", day_month(d, lang)),
        (Some(d), Lang::De) => format!("{who} seit {}", day_month(d, lang)),
        (None, _) => who,
    }
}

/// One tile: (big text, label, full sentence for screen readers and copy). Zero tiles are left out, but the
/// letters tile always stays (the card is never empty); the time tile stays as long as any time was saved.
fn tiles(t: &Tally, lang: Lang) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let counted = [
        (Kind::Letters, t.letters_read, true),
        (Kind::Forms, t.forms_filled, false),
        (Kind::Sent, t.sent_after_yes, false),
        (Kind::Deadlines, t.deadlines_kept, false),
        (Kind::Reminders, t.reminders_on_time, false),
    ];
    for (kind, n, always) in counted {
        if n > 0 || always {
            let l = label(kind, n, lang);
            out.push((n.to_string(), l.to_string(), format!("{n} {l}")));
        }
    }
    if t.seconds_saved > 0 {
        let (big, l) = saved_parts(t.seconds_saved, lang);
        let about = words(lang).about;
        out.push((format!("<small>{about}</small> {}", esc(&big)), l.to_string(), saved_phrase(t.seconds_saved, lang)));
    }
    out
}

/// HTML escape for text and attribute values.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The card: one self-contained HTML page, a 960x540 card on a soft background (a screenshot looks good).
/// `first_name` is the only personal value on it.
pub fn card_html(t: &Tally, first_name: Option<&str>, lang: Lang) -> String {
    let w = words(lang);
    let pip = base64::engine::general_purpose::STANDARD.encode(PIP_HAPPY_PNG);
    let mut tile_html = String::new();
    for (big, label, sentence) in tiles(t, lang) {
        // `big` is a number or the escaped saved time with its <small> word (built above, no user text).
        tile_html.push_str(&format!(
            "<div class=\"tile\" role=\"listitem\" aria-label=\"{}\"><div class=\"n\">{big}</div><div class=\"l\">{}</div></div>",
            esc(&sentence),
            esc(&label)
        ));
    }
    let share = share_text(t, lang);
    format!(
        r#"<!doctype html>
<html lang="{html_lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{headline}</title>
<style>
:root {{ --blue: #0866ff; --violet: #7b5cff; --ink: #1c2033; --muted: #5b6178; }}
* {{ box-sizing: border-box; }}
html, body {{ margin: 0; }}
body {{
  min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 22px;
  padding: 24px; background: radial-gradient(circle at 20% 10%, #e9efff 0, #f4f1ff 45%, #eef2f8 100%);
  font-family: "Segoe UI", system-ui, -apple-system, Roboto, "Helvetica Neue", Arial, sans-serif; color: var(--ink);
}}
.card {{
  width: 960px; height: 540px; max-width: 100%; background: #fff; border-radius: 28px; overflow: hidden;
  box-shadow: 0 24px 60px rgba(40, 50, 120, .18), 0 2px 6px rgba(40, 50, 120, .08);
  display: flex; flex-direction: column;
}}
header {{
  position: relative; height: 170px; padding: 30px 40px 0 210px; color: #fff;
  background: linear-gradient(110deg, var(--blue) 0%, var(--violet) 100%);
}}
header img {{ position: absolute; left: 40px; top: 18px; width: 150px; height: 150px; object-fit: contain;
  filter: drop-shadow(0 8px 14px rgba(0, 0, 40, .25)); }}
h1 {{ margin: 6px 0 8px; font-size: 50px; line-height: 1.05; font-weight: 800; letter-spacing: -.5px; }}
.sub {{ margin: 0; font-size: 21px; opacity: .92; }}
.tiles {{ flex: 1; display: flex; flex-wrap: wrap; justify-content: center; align-content: center; gap: 16px 18px;
  padding: 22px 40px 8px; }}
.tile {{ width: 278px; padding: 12px 18px; border-radius: 18px; background: #f3f5ff; }}
.n {{ font-size: 46px; font-weight: 800; line-height: 1.05; color: var(--blue); }}
.n small {{ font-size: 22px; font-weight: 700; color: var(--violet); }}
.l {{ margin-top: 2px; font-size: 18px; color: var(--muted); }}
footer {{ padding: 6px 40px 22px; }}
.promise {{ margin: 0; font-size: 19px; font-weight: 600; }}
.promise::before {{ content: "\2714"; color: #12a150; margin-right: 8px; }}
.small {{ margin: 6px 0 0; font-size: 13px; color: var(--muted); }}
button {{ font: inherit; font-size: 18px; font-weight: 600; color: #fff; border: 0; border-radius: 999px;
  padding: 12px 26px; cursor: pointer; background: linear-gradient(110deg, var(--blue), var(--violet));
  box-shadow: 0 8px 20px rgba(8, 102, 255, .25); }}
button:focus-visible {{ outline: 3px solid var(--ink); outline-offset: 3px; }}
#post {{ position: absolute; left: -9999px; }}
@media (max-width: 1000px) {{
  .card {{ height: auto; }}
  header {{ height: auto; padding: 24px 20px 24px 140px; }}
  header img {{ left: 16px; width: 110px; height: 110px; }}
  h1 {{ font-size: 32px; }}
  .tiles {{ padding: 18px 16px 8px; }}
  footer {{ padding: 6px 20px 20px; }}
}}
</style>
</head>
<body>
<main class="card">
<header>
<img alt="Pip" src="data:image/png;base64,{pip}">
<h1>{headline}</h1>
<p class="sub">{sub}</p>
</header>
<section class="tiles" role="list">{tile_html}</section>
<footer>
<p class="promise">{footer}</p>
<p class="small">{built_with}</p>
</footer>
</main>
<button id="copy" type="button">{copy}</button>
<textarea id="post" readonly aria-hidden="true" tabindex="-1">{share}</textarea>
<script>
(function () {{
  var btn = document.getElementById("copy"), post = document.getElementById("post");
  var label = btn.textContent;
  function done() {{ btn.textContent = "{copied}"; setTimeout(function () {{ btn.textContent = label; }}, 2000); }}
  function fallback() {{ post.select(); try {{ document.execCommand("copy"); }} catch (e) {{}} done(); }}
  btn.addEventListener("click", function () {{
    if (navigator.clipboard && navigator.clipboard.writeText) {{
      navigator.clipboard.writeText(post.value).then(done, fallback);
    }} else {{ fallback(); }}
  }});
}})();
</script>
</body>
</html>
"#,
        html_lang = w.html_lang,
        headline = esc(w.headline),
        sub = esc(&subline(first_name, t.since, lang)),
        footer = esc(w.footer),
        built_with = esc(w.built_with),
        copy = esc(w.copy),
        copied = esc(w.copied),
        share = esc(&share),
    )
}

/// The most a post on X may have.
const POST_MAX_CHARS: usize = 280;

/// A post-ready sentence (at most 280 characters, zero parts left out), e.g. "Pip did this for me: 3 letters
/// read, 2 forms filled in, … #MetaHackathon #BuildForYourWorld".
pub fn share_text(t: &Tally, lang: Lang) -> String {
    let n = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = Vec::new();
    let en = lang == Lang::En;
    if t.letters_read > 0 {
        parts.push(if en {
            n(t.letters_read, "letter read", "letters read")
        } else {
            n(t.letters_read, "Brief gelesen", "Briefe gelesen")
        });
    }
    if t.forms_filled > 0 {
        parts.push(if en {
            n(t.forms_filled, "form filled in", "forms filled in")
        } else {
            n(t.forms_filled, "Formular ausgefüllt", "Formulare ausgefüllt")
        });
    }
    if t.sent_after_yes > 0 {
        parts.push(if en {
            n(t.sent_after_yes, "sent after my yes", "sent after my yes")
        } else {
            n(t.sent_after_yes, "gesendet nach meinem Ja", "gesendet nach meinem Ja")
        });
    }
    if t.deadlines_kept > 0 {
        parts.push(if en {
            n(t.deadlines_kept, "deadline kept", "deadlines kept")
        } else {
            n(t.deadlines_kept, "Frist eingehalten", "Fristen eingehalten")
        });
    }
    if t.seconds_saved >= 60 {
        parts.push(saved_phrase(t.seconds_saved, lang));
    }
    let (head, helper, tags) = if en {
        (
            "Pip did this for me",
            "A small AI helper on my screen, built with Meta Muse Spark + SAM 3.1.",
            "#MetaHackathon #BuildForYourWorld",
        )
    } else {
        (
            "Das hat Pip für mich erledigt",
            "Ein kleiner KI-Helfer auf meinem Bildschirm, gebaut mit Meta Muse Spark + SAM 3.1.",
            "#MetaHackathon #BuildForYourWorld",
        )
    };
    let first = if parts.is_empty() { format!("{head}.") } else { format!("{head}: {}.", parts.join(", ")) };
    let full = format!("{first} {helper} {tags}");
    if full.chars().count() <= POST_MAX_CHARS {
        return full;
    }
    // Very large numbers: the helper sentence goes first, then whole parts from the end.
    let mut short = format!("{first} {tags}");
    while short.chars().count() > POST_MAX_CHARS && parts.len() > 1 {
        parts.pop();
        short = format!("{head}: {}. {tags}", parts.join(", "));
    }
    short
}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use super::*;
    use crate::memory::db::{ActionLog, NewDeadline, Playbook};
    use crate::rules::local_at;
    use crate::rules::tests_support::{au_doc, d};

    fn demo_tally() -> Tally {
        Tally {
            letters_read: 3,
            forms_filled: 2,
            sent_after_yes: 2,
            deadlines_kept: 1,
            reminders_on_time: 2,
            seconds_saved: 1500,
            since: Some(d(2026, 10, 14)),
        }
    }

    fn action(risk: &str, approved: Option<bool>) -> ActionLog {
        ActionLog {
            at: Local::now(),
            tool: "click".into(),
            args_redacted: "{}".into(),
            risk: risk.into(),
            approved,
            result: "clicked".into(),
            tokens: 0,
        }
    }

    #[test]
    fn tally_counts_each_source() {
        let m = Memory::open_in_memory().unwrap();
        assert_eq!(tally(&m).unwrap(), Tally::default());
        for (i, sha) in ["sha-1", "sha-2"].iter().enumerate() {
            let path = std::path::PathBuf::from(format!("C:/b/{i}.pdf"));
            m.insert_document(&path, sha, &au_doc(d(2026, 10, 20), d(2026, 10, 30)), "AU").unwrap();
        }
        m.playbook_upsert(&Playbook {
            site: "fixtures/demo-kasse".into(),
            task: "send_au".into(),
            url: "file:///x/index.html".into(),
            fields: vec![],
            attach_label: None,
            send_label: Some("Send".into()),
            steps: 6,
            seconds: 40,
            runs: 3,
            failed_since_ok: 0,
            updated_at: Local::now(),
        })
        .unwrap();
        m.log_action(&action("commit", Some(true))).unwrap();
        m.log_action(&action("commit", Some(false))).unwrap();
        m.log_action(&action("act", Some(true))).unwrap();
        // A yes to an attach card is a commit with approval, but nothing was sent.
        let attach =
            ActionLog { result: format!("clicked Attach{ATTACH_APPROVED_NOTE}"), ..action("commit", Some(true)) };
        m.log_action(&attach).unwrap();
        let now = local_at(d(2026, 10, 28), 9, 0);
        let new = |title: &str, remind| NewDeadline {
            title: title.into(),
            due: d(2026, 11, 2),
            rule_id: None,
            doc_id: None,
            remind_at: remind,
            why: String::new(),
            source: String::new(),
        };
        let done = m.add_deadline(&new("Done", vec![])).unwrap();
        m.set_deadline_status(done, crate::memory::DeadlineStatus::Done).unwrap();
        m.add_deadline(&new("Open", vec![now])).unwrap();
        let r = m.due_reminders(now).unwrap();
        m.mark_fired(r[0].id, now, None).unwrap();
        m.profile_set(PROFILE_SECONDS_SAVED, "1500").unwrap();
        let t = tally(&m).unwrap();
        assert_eq!(
            (t.letters_read, t.forms_filled, t.sent_after_yes, t.deadlines_kept, t.reminders_on_time, t.seconds_saved),
            (2, 3, 1, 1, 1, 1500)
        );
        assert_eq!(t.since, Some(Local::now().date_naive()));
    }

    #[test]
    fn card_shows_only_first_name_and_numbers() {
        let m = Memory::open_in_memory().unwrap();
        for (k, v) in [
            ("name", "Erika"),
            ("full_name", "Erika Beispiel"),
            ("versichertennummer", "G123456789"),
            ("iban", "DE02 1203 0000 0000 2020 51"),
            ("address", "Lindenweg 7, 40999 Beispielstadt"),
        ] {
            m.profile_set(k, v).unwrap();
        }
        let t = Tally { letters_read: 2, seconds_saved: 1500, ..demo_tally() };
        let name = m.profile_get("name").unwrap();
        let html = card_html(&t, name.as_deref(), Lang::En);
        assert!(html.contains("Erika"));
        assert!(html.contains("2 letters"), "{html}");
        assert!(html.contains("about 25 minutes"));
        for secret in ["G123456789", "Lindenweg", "DE02", "Beispiel"] {
            assert!(!html.contains(secret), "{secret} must not be on the card");
        }
        assert!(!html.contains("src=\"http") && !html.contains("href=\"http") && !html.contains("url(http"));
        assert!(!html.contains("@import"));
        assert!(html.contains("src=\"data:image/png;base64,iVBOR"));
        assert!(html.contains("Pip did this for me"));
        let evil = card_html(&t, Some("<script>alert(1)</script>"), Lang::En);
        assert!(evil.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!evil.contains("<script>alert"));
    }

    #[test]
    fn zero_tiles_are_hidden_but_letters_stay() {
        let html = card_html(&Tally::default(), None, Lang::En);
        assert_eq!(html.matches("class=\"tile\"").count(), 1);
        assert!(html.contains("0 letters read and explained"));
        assert!(!html.contains("minutes saved"), "no '0 minutes saved' tile: {html}");
        for hidden in ["forms filled", "after my yes", "deadline", "reminder"] {
            assert!(!html.contains(&format!("aria-label=\"0 {hidden}")), "{hidden}");
        }
        assert!(html.contains("My helper on the screen"));
        assert!(!html.contains(" since "), "no day without memory");

        // The first short run: seconds, never "0 minutes".
        let first =
            Tally { letters_read: 3, forms_filled: 1, sent_after_yes: 1, seconds_saved: 41, ..Tally::default() };
        let html = card_html(&first, Some("Erika"), Lang::En);
        assert_eq!(html.matches("class=\"tile\"").count(), 4);
        assert!(html.contains("about 41 seconds saved"), "{html}");
        assert!(!html.contains("0 minutes"));

        let one = Tally {
            letters_read: 1,
            deadlines_kept: 1,
            seconds_saved: 4500,
            since: Some(d(2026, 10, 3)),
            ..Tally::default()
        };
        let html = card_html(&one, Some("Erika"), Lang::En);
        assert_eq!(html.matches("class=\"tile\"").count(), 3);
        assert!(html.contains("1 letter read and explained"));
        assert!(html.contains("1 deadline kept"));
        assert!(html.contains("about 1 h 15 min saved"));
        assert!(html.contains("since 3 October"), "single-digit day without a zero: {html}");
        let full = card_html(&demo_tally(), Some("Erika"), Lang::En);
        assert_eq!(full.matches("class=\"tile\"").count(), 6);
        assert!(full.contains("Erika&#39;s helper on the screen since 14 October"));
    }

    #[test]
    fn share_text_is_short_and_honest() {
        let s = share_text(&demo_tally(), Lang::En);
        assert_eq!(
            s,
            "Pip did this for me: 3 letters read, 2 forms filled in, 2 sent after my yes, 1 deadline kept, about 25 \
             minutes saved. A small AI helper on my screen, built with Meta Muse Spark + SAM 3.1. #MetaHackathon \
             #BuildForYourWorld"
        );
        let few = share_text(&Tally { letters_read: 1, ..Tally::default() }, Lang::En);
        assert!(few.starts_with("Pip did this for me: 1 letter read. A small"), "{few}");
        assert!(!few.contains("forms") && !few.contains("minutes") && !few.contains(" 0 "));
        let huge = Tally {
            letters_read: u64::MAX,
            forms_filled: u64::MAX,
            sent_after_yes: u64::MAX,
            deadlines_kept: u64::MAX,
            reminders_on_time: u64::MAX,
            seconds_saved: u64::MAX,
            since: None,
        };
        for lang in [Lang::En, Lang::De] {
            for t in [demo_tally(), huge, Tally::default()] {
                let s = share_text(&t, lang);
                assert!(s.chars().count() <= POST_MAX_CHARS, "{} chars: {s}", s.chars().count());
                assert!(s.contains("#MetaHackathon"));
            }
        }
    }

    #[test]
    fn german_card_has_no_placeholders_left() {
        let t = demo_tally();
        let html = card_html(&t, Some("Hans"), Lang::De);
        let body = &html[html.find("<body>").unwrap()..html.find("<script>").unwrap()];
        assert!(!body.contains('{') && !body.contains('}'), "{body}");
        assert!(html.contains("Das hat Pip für mich erledigt"));
        assert!(html.contains("Hans&#39; Helfer auf dem Bildschirm seit 14. Oktober"));
        assert!(html.contains("3 Briefe gelesen und erklärt"));
        assert!(html.contains("etwa 25 Minuten gespart"));
        let s = share_text(&t, Lang::De);
        assert!(!s.contains('{') && s.starts_with("Das hat Pip für mich erledigt: 3 Briefe gelesen"), "{s}");
    }

    /// Writes target/card-preview.html from the demo numbers, to look at in a browser.
    #[test]
    fn card_writes_a_preview() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("card-preview.html");
        std::fs::write(&path, card_html(&demo_tally(), Some("Erika"), Lang::En)).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 10_000);
    }
}
