//! Pure parts of the Windows outline reader (no UI Automation), so they are tested on every OS:
//! when a remembered outline may be used again, how long to wait for a new one, which browser
//! child window holds the web page, how the day/month/year parts of a date input fold into it, and
//! the names of the browser's "Restore pages" popup.
//! OWNER: agent "platform".
#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

use pip_core::platform::{PhysPoint, PhysRect};

/// An outline younger than this is used again without reading the page (if nothing was done since).
pub const FRESH_FOR: Duration = Duration::from_millis(1500);
/// A late reader may still give an older outline of the same untouched page, up to this age.
pub const STALE_UP_TO: Duration = Duration::from_secs(10);
/// Wait for the first read of a page (the browser may first build its accessibility tree).
pub const FIRST_BUDGET: Duration = Duration::from_millis(1500);
/// Wait for later reads of the same page.
pub const NEXT_BUDGET: Duration = Duration::from_millis(900);
/// An empty first answer is asked again once after this pause (Chromium builds its tree lazily).
pub const EMPTY_RETRY_AFTER: Duration = Duration::from_millis(150);

/// What an outline belongs to: the window, its title (changes with the page) and the cap asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineKey {
    pub hwnd: isize,
    pub title: String,
    pub max: usize,
}

/// One remembered outline.
#[derive(Debug, Clone)]
pub struct Cached<T> {
    pub key: OutlineKey,
    /// The action counter when the read was asked for (any click/type/key/scroll after it makes it old).
    pub epoch: u64,
    /// When the read began.
    pub at: Instant,
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Same page, nothing done since, young: use it, do not read.
    Fresh,
    /// Same page, nothing done since, but older: only a fallback when a new read is late.
    Stale,
    /// Another page or window, or something was done since: never use it.
    Miss,
}

impl<T> Cached<T> {
    pub fn hit(&self, key: &OutlineKey, epoch: u64, now: Instant) -> Hit {
        if &self.key != key || self.epoch != epoch {
            return Hit::Miss;
        }
        let age = now.saturating_duration_since(self.at);
        if age < FRESH_FOR {
            Hit::Fresh
        } else if age < STALE_UP_TO {
            Hit::Stale
        } else {
            Hit::Miss
        }
    }
}

/// How long to wait for a read: longer for a page we have no outline of yet.
pub fn budget<T>(last: Option<&Cached<T>>, key: &OutlineKey) -> Duration {
    match last {
        Some(c) if c.key.hwnd == key.hwnd && c.key.title == key.title => NEXT_BUDGET,
        _ => FIRST_BUDGET,
    }
}

/// A child window of the browser window: its class, if it is visible, and its area in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildWindow {
    pub hwnd: isize,
    pub class: String,
    pub visible: bool,
    pub area: i64,
}

/// Chromium (Edge, Chrome, Brave, Opera, Electron) draws the web page into this child window.
pub const CHROMIUM_PAGE_CLASS: &str = "Chrome_RenderWidgetHostHWND";

/// The child window that shows the page of the active tab: the biggest visible Chromium page window.
/// Hidden tabs keep their windows but hidden; side panels are smaller.
pub fn page_child(children: &[ChildWindow]) -> Option<isize> {
    children
        .iter()
        .filter(|c| c.visible && c.area > 0 && c.class == CHROMIUM_PAGE_CLASS)
        .max_by_key(|c| c.area)
        .map(|c| c.hwnd)
}

/// One part of a date input (a UI Automation Spinner inside the Edit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePart {
    Day,
    Month,
    Year,
}

/// Which part a spinner is, by its name ("Tag", "Month", "Jahr", "année"). Chromium names the parts of a
/// date input in the browser's language.
pub fn date_part(name: &str) -> Option<DatePart> {
    const DAY: &[&str] = &["tag", "day", "jour", "día", "dia", "giorno", "dag", "dzień"];
    const MONTH: &[&str] = &["monat", "month", "mois", "mes", "mese", "maand", "miesiąc"];
    const YEAR: &[&str] = &["jahr", "year", "année", "annee", "año", "ano", "anno", "jaar", "rok"];
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    if has(DAY) {
        Some(DatePart::Day)
    } else if has(MONTH) {
        Some(DatePart::Month)
    } else if has(YEAR) {
        Some(DatePart::Year)
    } else {
        None
    }
}

/// What a date input shows, built from its parts in visual order (name, value): "TT.MM.JJJJ" (German names,
/// all empty), "19.10.2026", "TT.10.2026", "mm/dd/yyyy" (English, month first), "yyyy-mm-dd". A part counts as
/// filled only when its value is digits. None unless the three parts are day, month and year once each.
/// Edge gives the Edit itself no value, so this is how the agent learns the order and what is filled.
pub fn date_text(parts: &[(String, Option<String>)]) -> Option<String> {
    let kinds: Vec<DatePart> = parts.iter().map(|(n, _)| date_part(n)).collect::<Option<_>>()?;
    if kinds.len() != 3 || [DatePart::Day, DatePart::Month, DatePart::Year].iter().any(|k| !kinds.contains(k)) {
        return None;
    }
    let german = parts.iter().any(|(n, _)| {
        let n = n.to_lowercase();
        n.contains("tag") || n.contains("monat") || n.contains("jahr")
    });
    let sep = match (kinds[0], german) {
        (DatePart::Year, _) => "-",
        (_, true) => ".",
        (_, false) => "/",
    };
    let shown: Vec<String> = parts
        .iter()
        .zip(&kinds)
        .map(|((_, v), k)| {
            let v = v.as_deref().unwrap_or("").trim();
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                return v.to_string();
            }
            match (k, german) {
                (DatePart::Day, true) => "TT",
                (DatePart::Month, true) => "MM",
                (DatePart::Year, true) => "JJJJ",
                (DatePart::Day, false) => "dd",
                (DatePart::Month, false) => "mm",
                (DatePart::Year, false) => "yyyy",
            }
            .to_string()
        })
        .collect();
    Some(shown.join(sep))
}

/// The field (index) a date part belongs to: the smallest one holding the part's centre.
pub fn owner_of(part: PhysRect, fields: &[PhysRect]) -> Option<usize> {
    let c = PhysPoint { x: part.x + part.w / 2, y: part.y + part.h / 2 };
    fields
        .iter()
        .enumerate()
        .filter(|(_, f)| f.contains(c))
        .min_by_key(|(_, f)| i64::from(f.w) * i64::from(f.h))
        .map(|(i, _)| i)
}

/// A button of the browser's "Restore pages" popup after a crash or a forced close (Edge, Chrome).
pub fn is_restore_button(name: &str) -> bool {
    let n = name.trim().trim_end_matches('?').trim().to_lowercase();
    matches!(n.as_str(), "wiederherstellen" | "seiten wiederherstellen" | "restore" | "restore pages")
}

/// The ✕ of a popup.
pub fn is_close_button(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    matches!(n.as_str(), "schließen" | "close" | "×" | "✕" | "x")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_parts_by_name_and_what_the_input_shows() {
        let p = |n: &str, v: Option<&str>| (n.to_string(), v.map(String::from));
        assert_eq!(date_part("Tag"), Some(DatePart::Day));
        assert_eq!(date_part("Monat"), Some(DatePart::Month));
        assert_eq!(date_part("Jahr"), Some(DatePart::Year));
        assert_eq!(date_part("Day of month"), Some(DatePart::Day));
        assert_eq!(date_part("année"), Some(DatePart::Year));
        assert_eq!(date_part("Vertrag"), None, "\"tag\" only as a word");
        assert_eq!(date_part(""), None);
        let de = [p("Tag", None), p("Monat", Some("leer")), p("Jahr", Some(""))];
        assert_eq!(date_text(&de).as_deref(), Some("TT.MM.JJJJ"));
        let filled = [p("Tag", Some("19")), p("Monat", Some("10")), p("Jahr", Some("2026"))];
        assert_eq!(date_text(&filled).as_deref(), Some("19.10.2026"));
        let bug = [p("Tag", Some("")), p("Monat", None), p("Jahr", Some("102026"))];
        assert_eq!(date_text(&bug).as_deref(), Some("TT.MM.102026"));
        let us = [p("Month", None), p("Day", Some("5")), p("Year", None)];
        assert_eq!(date_text(&us).as_deref(), Some("mm/5/yyyy"));
        let iso = [p("Year", None), p("Month", None), p("Day", None)];
        assert_eq!(date_text(&iso).as_deref(), Some("yyyy-mm-dd"));
        assert_eq!(date_text(&de[..2]), None, "two parts");
        assert_eq!(date_text(&[p("Tag", None), p("Tag", None), p("Jahr", None)]), None, "day twice");
        assert_eq!(date_text(&[p("Stunden", None), p("Minuten", None), p("Jahr", None)]), None, "a time input");
    }

    #[test]
    fn a_part_belongs_to_the_smallest_field_around_it() {
        let r = |x, y, w, h| PhysRect { x, y, w, h };
        let fields = [r(0, 0, 1000, 800), r(100, 100, 200, 30), r(100, 200, 200, 30)];
        assert_eq!(owner_of(r(105, 105, 30, 20), &fields), Some(1));
        assert_eq!(owner_of(r(180, 205, 60, 20), &fields), Some(2));
        assert_eq!(owner_of(r(900, 900, 10, 10), &fields), None);
        assert_eq!(owner_of(r(0, 0, 1, 1), &[]), None);
    }

    #[test]
    fn restore_popup_names() {
        for n in ["Wiederherstellen", "Seiten wiederherstellen?", "Restore", " restore pages "] {
            assert!(is_restore_button(n), "{n}");
        }
        for n in ["Wiederherstellung", "Restore defaults", "Schließen", ""] {
            assert!(!is_restore_button(n), "{n}");
        }
        assert!(is_close_button("Schließen") && is_close_button("Close") && !is_close_button("Closed"));
    }

    fn key(hwnd: isize, title: &str) -> OutlineKey {
        OutlineKey { hwnd, title: title.into(), max: 80 }
    }

    #[test]
    fn same_page_untouched_is_fresh_then_stale_then_gone() {
        let t0 = Instant::now();
        let c = Cached { key: key(1, "Questionnaire"), epoch: 3, at: t0, items: vec![1, 2] };
        assert_eq!(c.hit(&key(1, "Questionnaire"), 3, t0 + Duration::from_millis(200)), Hit::Fresh);
        assert_eq!(c.hit(&key(1, "Questionnaire"), 3, t0 + Duration::from_millis(1600)), Hit::Stale);
        assert_eq!(c.hit(&key(1, "Questionnaire"), 3, t0 + Duration::from_secs(11)), Hit::Miss);
        // a clock that went backwards (or `at` in the future) is not a panic, just fresh
        assert_eq!(c.hit(&key(1, "Questionnaire"), 3, t0), Hit::Fresh);
    }

    #[test]
    fn other_window_title_cap_or_an_action_misses() {
        let t0 = Instant::now();
        let c = Cached { key: key(1, "Questionnaire"), epoch: 3, at: t0, items: Vec::<u8>::new() };
        assert_eq!(c.hit(&key(2, "Questionnaire"), 3, t0), Hit::Miss, "other window");
        assert_eq!(c.hit(&key(1, "Danke"), 3, t0), Hit::Miss, "page changed (title)");
        assert_eq!(c.hit(&key(1, "Questionnaire"), 4, t0), Hit::Miss, "clicked or typed since");
        let mut wider = key(1, "Questionnaire");
        wider.max = 200;
        assert_eq!(c.hit(&wider, 3, t0), Hit::Miss, "asked for more items");
    }

    #[test]
    fn first_read_of_a_page_waits_longer() {
        let c = Cached { key: key(1, "A"), epoch: 0, at: Instant::now(), items: vec![0u8] };
        assert_eq!(budget::<u8>(None, &key(1, "A")), FIRST_BUDGET);
        assert_eq!(budget(Some(&c), &key(1, "B")), FIRST_BUDGET);
        assert_eq!(budget(Some(&c), &key(1, "A")), NEXT_BUDGET);
        assert!(FIRST_BUDGET > NEXT_BUDGET && FRESH_FOR < STALE_UP_TO);
    }

    #[test]
    fn page_child_is_the_biggest_visible_chromium_page() {
        let w = |hwnd, class: &str, visible, area| ChildWindow { hwnd, class: class.into(), visible, area };
        let kids = [
            w(10, CHROMIUM_PAGE_CLASS, false, 2_000_000), // a background tab
            w(11, "Intermediate D3D Window", true, 3_000_000),
            w(12, CHROMIUM_PAGE_CLASS, true, 300_000),   // side panel
            w(13, CHROMIUM_PAGE_CLASS, true, 1_800_000), // the tab in front
            w(14, CHROMIUM_PAGE_CLASS, true, 0),
        ];
        assert_eq!(page_child(&kids), Some(13));
        assert_eq!(page_child(&kids[..2]), None, "no visible page window (Firefox, Explorer)");
        assert_eq!(page_child(&[]), None);
    }
}
