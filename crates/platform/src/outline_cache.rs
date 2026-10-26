//! Pure parts of the Windows outline reader (no UI Automation), so they are tested on every OS:
//! when a remembered outline may be used again, how long to wait for a new one, and which browser
//! child window holds the web page.
//! OWNER: agent "platform".
#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(hwnd: isize, title: &str) -> OutlineKey {
        OutlineKey { hwnd, title: title.into(), max: 80 }
    }

    #[test]
    fn same_page_untouched_is_fresh_then_stale_then_gone() {
        let t0 = Instant::now();
        let c = Cached { key: key(1, "Fragebogen"), epoch: 3, at: t0, items: vec![1, 2] };
        assert_eq!(c.hit(&key(1, "Fragebogen"), 3, t0 + Duration::from_millis(200)), Hit::Fresh);
        assert_eq!(c.hit(&key(1, "Fragebogen"), 3, t0 + Duration::from_millis(1600)), Hit::Stale);
        assert_eq!(c.hit(&key(1, "Fragebogen"), 3, t0 + Duration::from_secs(11)), Hit::Miss);
        // a clock that went backwards (or `at` in the future) is not a panic, just fresh
        assert_eq!(c.hit(&key(1, "Fragebogen"), 3, t0), Hit::Fresh);
    }

    #[test]
    fn other_window_title_cap_or_an_action_misses() {
        let t0 = Instant::now();
        let c = Cached { key: key(1, "Fragebogen"), epoch: 3, at: t0, items: Vec::<u8>::new() };
        assert_eq!(c.hit(&key(2, "Fragebogen"), 3, t0), Hit::Miss, "other window");
        assert_eq!(c.hit(&key(1, "Danke"), 3, t0), Hit::Miss, "page changed (title)");
        assert_eq!(c.hit(&key(1, "Fragebogen"), 4, t0), Hit::Miss, "clicked or typed since");
        let mut wider = key(1, "Fragebogen");
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
