//! Fake platform: Screen returns a PNG, Input records actions. OWNER: agent "platform".

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use pip_core::platform::{
    Frame, Input, OutlineItem, PhysPoint, PhysRect, Platform, Screen, UiElement, UiTree, WindowInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Move(PhysPoint),
    Click(PhysPoint),
    Type(String),
    Key(String),
    Scroll(PhysPoint, i32),
}

pub struct FakePlatform {
    /// Recorded input, in order.
    pub actions: Arc<Mutex<Vec<Action>>>,
    /// The screen; tests/eval may swap it between steps.
    pub screen: Arc<Mutex<image::RgbaImage>>,
    /// Pretend UI tree: named elements with their rects (empty = like Linux, no names).
    pub elements: Arc<Mutex<Vec<(PhysRect, UiElement)>>>,
    /// Pretend keyboard focus and foreground window (None = unknown).
    pub focus: Arc<Mutex<Option<UiElement>>>,
    pub window: Arc<Mutex<Option<WindowInfo>>>,
    /// Pretend page outline (empty = like Linux). Clicks, Ctrl+A and typing change it like a real form:
    /// typing goes into the field clicked last, a click ticks a box, the wheel moves everything by 100 px a notch.
    pub outline: Arc<Mutex<Vec<OutlineItem>>>,
    /// Height of the pretend screen for "off-screen" after scrolling (0 = do not recompute).
    pub screen_h: Arc<Mutex<i32>>,
    /// Names of the elements pressed through `UiTree::invoke`, in order.
    pub invoked: Arc<Mutex<Vec<String>>>,
    /// Pretend browser "Restore pages" popup: its close button (None = not shown). Invoking it closes it.
    pub restore_popup: Arc<Mutex<Option<OutlineItem>>>,
    /// The pretend home folder ("in my Downloads" is looked up below it); the dry run sets the fixtures folder.
    pub home: Arc<Mutex<std::path::PathBuf>>,
    /// Field that gets typed text (last click) and whether Ctrl+A selected its text.
    caret: Mutex<(Option<PhysPoint>, bool)>,
    /// Pretend Edge date inputs (`add_date_input`) and which part has the keys.
    dates: Mutex<DateInputs>,
}

/// One pretend Edge `<input type=date>` (German: TT.MM.JJJJ). Lock order: caret, outline, dates.
#[derive(Debug, Clone)]
struct FakeDate {
    /// Its outline item's id.
    id: u32,
    /// Typed digits of day, month and year.
    digits: [String; 3],
    /// Whether UI Automation shows the field's text as its value (Edge may give none).
    exposes_value: bool,
}

#[derive(Debug, Default)]
struct DateInputs {
    all: Vec<FakeDate>,
    /// (date index, part 0..3, the next digit starts the part afresh).
    focus: Option<(usize, usize, bool)>,
}

/// Day, month, year: name, placeholder, share of the field's width where the part starts and ends. The rest of
/// the field is the calendar icon, so the field's centre lands on the year part, as in Edge.
const DATE_PARTS: [(&str, &str, f32, f32); 3] =
    [("Tag", "TT", 0.0, 0.2), ("Monat", "MM", 0.2, 0.4), ("Jahr", "JJJJ", 0.4, 0.7)];
/// Edge's year part keeps up to six digits (years up to 275760).
const YEAR_DIGITS: usize = 6;

impl FakeDate {
    fn shown(&self) -> String {
        let part =
            |i: usize| if self.digits[i].is_empty() { DATE_PARTS[i].1.to_string() } else { self.digits[i].clone() };
        format!("{}.{}.{}", part(0), part(1), part(2))
    }

    fn part_rect(field: &PhysRect, i: usize) -> PhysRect {
        let (_, _, a, b) = DATE_PARTS[i];
        let x0 = field.x + (field.w as f32 * a).round() as i32;
        let x1 = field.x + (field.w as f32 * b).round() as i32;
        PhysRect { x: x0, y: field.y, w: (x1 - x0).max(1), h: field.h }
    }

    /// One digit into part `i`, like Chromium: day and month move on once full (or when no second digit can
    /// follow), the year keeps its last six digits. Returns the part that has the keys afterwards.
    fn digit(&mut self, i: usize, fresh: bool, d: char) -> (usize, bool) {
        if fresh {
            self.digits[i].clear();
        }
        self.digits[i].push(d);
        if i == 2 {
            let n = self.digits[2].chars().count();
            if n > YEAR_DIGITS {
                self.digits[2] = self.digits[2].chars().skip(n - YEAR_DIGITS).collect();
            }
            return (2, false);
        }
        let first = self.digits[i].chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0);
        let full = self.digits[i].len() >= 2 || (i == 0 && first > 3) || (i == 1 && first > 1);
        if full {
            self.digits[i] = format!("{:0>2}", self.digits[i]);
            (i + 1, true)
        } else {
            (i, false)
        }
    }
}

impl FakePlatform {
    /// Load a PNG as the screen (origin 0,0, scale 1.0). Returns the handle (for inspection)
    /// and the `Platform` to hand to the agent.
    pub fn from_png(path: &Path) -> Result<(Arc<FakePlatform>, Platform)> {
        let img = image::open(path).with_context(|| format!("cannot open screenshot {}", path.display()))?;
        Ok(Self::from_image(img.into_rgba8()))
    }

    pub fn from_image(img: image::RgbaImage) -> (Arc<FakePlatform>, Platform) {
        let me = Arc::new(FakePlatform {
            actions: Arc::new(Mutex::new(Vec::new())),
            screen: Arc::new(Mutex::new(img)),
            elements: Arc::default(),
            focus: Arc::default(),
            window: Arc::default(),
            outline: Arc::default(),
            screen_h: Arc::default(),
            invoked: Arc::default(),
            restore_popup: Arc::default(),
            home: Arc::new(Mutex::new(std::path::PathBuf::from("fixtures"))),
            caret: Mutex::default(),
            dates: Mutex::default(),
        });
        let platform = Platform { screen: me.clone(), input: me.clone(), ui: me.clone() };
        (me, platform)
    }

    /// Replace the screen (e.g. the "after submit" screenshot in an eval task).
    pub fn set_screen(&self, img: image::RgbaImage) {
        *self.screen.lock().expect("screen lock") = img;
    }

    /// Load a page outline (`[items]` or `{"items": [...]}`, see fixtures/screens/*.outline.json).
    pub fn load_outline(&self, path: &Path) -> Result<usize> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read outline {}", path.display()))?;
        let v: serde_json::Value = serde_json::from_str(&text).context("outline is not valid JSON")?;
        let items = v.get("items").cloned().unwrap_or(v);
        let items: Vec<OutlineItem> = serde_json::from_value(items).context("outline items do not fit")?;
        let n = items.len();
        *self.outline.lock().map_err(|_| anyhow::anyhow!("outline lock poisoned"))? = items;
        Ok(n)
    }

    /// Add a pretend Edge date input (German order TT.MM.JJJJ) to the outline, like Edge 141 shows it: three
    /// spinner parts (`date_parts`), text set through the UI tree is ignored (but reported as done), a click
    /// gives the keys to the part under the pointer (the field's centre is on the year), digits go into the
    /// part with the keys and move on by themselves, the year keeps up to six digits, other characters are
    /// dropped. `exposes_value` = UI Automation shows "TT.MM.JJJJ" / "19.10.2026" as the value; false = no
    /// value at all (what the run-2 log suggests for Edge).
    pub fn add_date_input(&self, mut item: OutlineItem, exposes_value: bool) {
        let (Ok(mut items), Ok(mut dates)) = (self.outline.lock(), self.dates.lock()) else { return };
        let date = FakeDate { id: item.id, digits: Default::default(), exposes_value };
        item.is_date = true;
        item.localized_role = Some("Datumsauswahl".into());
        item.value = exposes_value.then(|| date.shown());
        items.retain(|i| i.id != item.id);
        items.push(item);
        items.sort_by_key(|i| i.id);
        dates.all.retain(|d| d.id != date.id);
        dates.all.push(date);
    }

    /// What a pretend date input shows ("TT.MM.JJJJ", "19.10.2026", "TT.MM.102026"), whatever UI Automation
    /// would say. None if there is no date input with this outline id.
    pub fn date_shown(&self, id: u32) -> Option<String> {
        let dates = self.dates.lock().ok()?;
        dates.all.iter().find(|d| d.id == id).map(FakeDate::shown)
    }

    pub fn take_actions(&self) -> Vec<Action> {
        std::mem::take(&mut *self.actions.lock().expect("actions lock"))
    }

    fn record(&self, a: Action) -> Result<()> {
        self.actions.lock().map_err(|_| anyhow::anyhow!("actions lock poisoned"))?.push(a);
        Ok(())
    }
}

/// The date input (index) whose field holds the point, with the field's rect.
fn date_at(items: &[OutlineItem], dates: &DateInputs, p: PhysPoint) -> Option<(usize, PhysRect)> {
    dates.all.iter().enumerate().find_map(|(k, d)| {
        let it = items.iter().find(|i| i.id == d.id && !i.offscreen && i.rect.contains(p))?;
        Some((k, it.rect))
    })
}

/// Keep the outline value of each date input in step with its parts.
fn sync_dates(items: &mut [OutlineItem], dates: &DateInputs) {
    for d in &dates.all {
        if let Some(it) = items.iter_mut().find(|i| i.id == d.id) {
            it.value = d.exposes_value.then(|| d.shown());
        }
    }
}

/// The same element (role and name) still sits under the item's centre.
fn same_at(candidates: &[OutlineItem], item: &OutlineItem) -> bool {
    let c = item.center();
    candidates.iter().any(|i| !i.offscreen && i.role == item.role && i.name == item.name && i.rect.contains(c))
}

impl Screen for FakePlatform {
    fn capture_primary(&self) -> Result<Frame> {
        let image = self.screen.lock().map_err(|_| anyhow::anyhow!("screen lock poisoned"))?.clone();
        Ok(Frame { image, origin: PhysPoint { x: 0, y: 0 }, scale: 1.0 })
    }
}

impl Input for FakePlatform {
    fn move_to(&self, p: PhysPoint) -> Result<()> {
        self.record(Action::Move(p))
    }
    fn click(&self, p: PhysPoint) -> Result<()> {
        if let (Ok(mut caret), Ok(mut items), Ok(mut dates)) =
            (self.caret.lock(), self.outline.lock(), self.dates.lock())
        {
            *caret = (Some(p), false);
            if let Some(it) = items.iter_mut().find(|i| i.rect.contains(p) && i.role == "checkbox") {
                let on = it.value.as_deref() == Some("checked");
                it.value = Some(if on { "unchecked" } else { "checked" }.into());
            }
            // A click on a date input gives the keys to the part under the pointer; on its calendar icon, to none.
            dates.focus = date_at(&items, &dates, p).and_then(|(k, field)| {
                (0..3).find(|&i| FakeDate::part_rect(&field, i).contains(p)).map(|i| (k, i, true))
            });
        }
        self.record(Action::Click(p))
    }
    fn type_text(&self, s: &str) -> Result<()> {
        crate::keys::check_typeable(s)?;
        let (Ok(mut caret), Ok(mut items), Ok(mut dates)) = (self.caret.lock(), self.outline.lock(), self.dates.lock())
        else {
            return self.record(Action::Type(s.to_owned()));
        };
        let in_date = caret.0.and_then(|p| date_at(&items, &dates, p)).is_some();
        if let Some((k, mut part, mut fresh)) = dates.focus {
            for c in s.chars().filter(char::is_ascii_digit) {
                (part, fresh) = dates.all[k].digit(part, fresh, c);
            }
            dates.focus = Some((k, part.min(2), fresh));
            sync_dates(&mut items, &dates);
        } else if in_date {
            // The calendar icon has the keys: typed text goes nowhere.
        } else if let Some(p) = caret.0
            && let Some(it) = items.iter_mut().find(|i| i.rect.contains(p) && i.takes_text() && !i.is_password)
        {
            let old = if caret.1 { String::new() } else { it.value.clone().unwrap_or_default() };
            it.value = Some(format!("{old}{s}"));
            caret.1 = false;
        }
        self.record(Action::Type(s.to_owned()))
    }
    fn key(&self, combo: &str) -> Result<()> {
        crate::keys::parse_combo(combo)?;
        if combo.eq_ignore_ascii_case("ctrl+a")
            && let Ok(mut caret) = self.caret.lock()
        {
            caret.1 = true;
        }
        if let (Ok(mut items), Ok(mut dates)) = (self.outline.lock(), self.dates.lock())
            && let Some((k, part, _)) = dates.focus
        {
            dates.focus = match combo.to_lowercase().as_str() {
                "backspace" | "delete" => {
                    dates.all[k].digits[part].clear();
                    Some((k, part, false))
                }
                "tab" | "right" if part < 2 => Some((k, part + 1, true)),
                "shift+tab" | "left" if part > 0 => Some((k, part - 1, true)),
                "right" | "left" => Some((k, part, true)),
                "tab" | "shift+tab" | "esc" | "escape" | "enter" => None,
                _ => Some((k, part, false)),
            };
            sync_dates(&mut items, &dates);
        }
        self.record(Action::Key(combo.to_owned()))
    }
    fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()> {
        let h = self.screen_h.lock().map(|h| *h).unwrap_or(0);
        if let Ok(mut items) = self.outline.lock() {
            for it in items.iter_mut() {
                it.rect.y -= dy * 100;
                if h > 0 {
                    let cy = it.rect.y + it.rect.h / 2;
                    it.offscreen = cy < 0 || cy >= h;
                }
            }
        }
        self.record(Action::Scroll(p, dy))
    }
}

impl UiTree for FakePlatform {
    fn element_at(&self, p: PhysPoint) -> Option<UiElement> {
        let els = self.elements.lock().ok()?;
        if let Some((_, e)) = els.iter().find(|(r, _)| r.contains(p)) {
            return Some(e.clone());
        }
        let items = self.outline.lock().ok()?;
        items.iter().find(|i| !i.offscreen && i.rect.contains(p)).map(OutlineItem::as_element)
    }
    fn focused(&self) -> Option<UiElement> {
        self.focus.lock().ok()?.clone()
    }
    fn find_by_name(&self, name: &str) -> Vec<(UiElement, PhysRect)> {
        let n = name.trim().to_lowercase();
        let Ok(els) = self.elements.lock() else { return Vec::new() };
        els.iter()
            .filter(|(_, e)| !n.is_empty() && e.name.to_lowercase().contains(&n))
            .map(|(r, e)| (e.clone(), *r))
            .collect()
    }
    fn foreground(&self) -> Option<WindowInfo> {
        self.window.lock().ok()?.clone()
    }
    fn outline(&self, max: usize) -> Vec<OutlineItem> {
        self.outline.lock().map(|o| o.iter().take(max).cloned().collect()).unwrap_or_default()
    }
    fn set_value_at(&self, p: PhysPoint, text: &str) -> bool {
        let Ok(mut items) = self.outline.lock() else { return false };
        if let Ok(dates) = self.dates.lock()
            && date_at(&items, &dates, p).is_some()
        {
            return true; // Edge drops it silently: the call "works", the field stays as it was
        }
        match items.iter_mut().find(|i| !i.offscreen && i.rect.contains(p) && !i.is_password) {
            Some(it) => {
                it.value = Some(text.to_string());
                true
            }
            None => false,
        }
    }

    fn value_at(&self, p: PhysPoint) -> Option<String> {
        let items = self.outline.lock().ok()?;
        if let Ok(dates) = self.dates.lock()
            && let Some((k, field)) = date_at(&items, &dates, p)
            && let Some(i) = (0..3).find(|&i| FakeDate::part_rect(&field, i).contains(p))
        {
            // The point is on one part: Edge reads that part's own value.
            let d = &dates.all[k];
            return Some(if d.digits[i].is_empty() { DATE_PARTS[i].1.to_string() } else { d.digits[i].clone() });
        }
        let it = items.iter().find(|i| !i.offscreen && i.rect.contains(p) && !i.is_password)?;
        it.value.clone()
    }

    fn date_parts(&self, item: &OutlineItem) -> Vec<OutlineItem> {
        let (Ok(items), Ok(dates)) = (self.outline.lock(), self.dates.lock()) else { return Vec::new() };
        let Some(d) = dates.all.iter().find(|d| d.id == item.id) else { return Vec::new() };
        let Some(field) = items.iter().find(|i| i.id == d.id) else { return Vec::new() };
        (0..3)
            .map(|i| OutlineItem {
                id: i as u32 + 1,
                role: "spinner".into(),
                name: DATE_PARTS[i].0.into(),
                value: Some(if d.digits[i].is_empty() { DATE_PARTS[i].1.to_string() } else { d.digits[i].clone() }),
                rect: FakeDate::part_rect(&field.rect, i),
                offscreen: field.offscreen,
                enabled: field.enabled,
                is_password: false,
                localized_role: Some("Drehfeld".into()),
                is_date: false,
            })
            .collect()
    }

    fn focus_element(&self, item: &OutlineItem) -> bool {
        let (Ok(mut caret), Ok(items), Ok(mut dates)) = (self.caret.lock(), self.outline.lock(), self.dates.lock())
        else {
            return false;
        };
        let c = item.center();
        if let Some((k, field)) = date_at(&items, &dates, c) {
            // The input itself gives the keys to its first part; a part takes them itself.
            let part = if item.role == "spinner" {
                match (0..3).find(|&i| FakeDate::part_rect(&field, i).contains(c)) {
                    Some(i) => i,
                    None => return false,
                }
            } else {
                0
            };
            *caret = (Some(c), false);
            dates.focus = Some((k, part, true));
            return true;
        }
        if !same_at(&items, item) {
            return false;
        }
        *caret = (Some(c), false);
        dates.focus = None;
        true
    }

    fn element_matches(&self, item: &OutlineItem) -> bool {
        let Ok(items) = self.outline.lock().map(|o| o.clone()) else { return false };
        if item.role == "spinner" {
            return items.iter().filter(|f| f.is_date).any(|f| same_at(&self.date_parts(f), item));
        }
        same_at(&items, item)
    }

    fn invoke(&self, item: &OutlineItem) -> bool {
        if let Ok(mut popup) = self.restore_popup.lock()
            && popup.as_ref().is_some_and(|b| b.name == item.name && b.rect == item.rect)
        {
            *popup = None;
            drop(popup);
            if let Ok(mut log) = self.invoked.lock() {
                log.push(item.name.clone());
            }
            return true;
        }
        let Ok(mut items) = self.outline.lock() else { return false };
        let c = item.center();
        let Some(it) =
            items.iter_mut().find(|i| !i.offscreen && i.role == item.role && i.name == item.name && i.rect.contains(c))
        else {
            return false;
        };
        match it.role.as_str() {
            "checkbox" => {
                let on = it.value.as_deref() == Some("checked");
                it.value = Some(if on { "unchecked" } else { "checked" }.into());
            }
            "radio" => it.value = Some("selected".into()),
            "button" | "link" | "item" | "menuitem" | "tab" => {}
            _ => return false,
        }
        drop(items);
        if let Ok(mut log) = self.invoked.lock() {
            log.push(item.name.clone());
        }
        true
    }

    fn find_restore_popup(&self) -> Option<OutlineItem> {
        self.restore_popup.lock().ok()?.clone()
    }
    fn home_dir(&self) -> std::path::PathBuf {
        self.home.lock().map(|h| h.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_actions_and_returns_screen() {
        let (fake, platform) = FakePlatform::from_image(image::RgbaImage::new(64, 32));
        let frame = platform.screen.capture_primary().unwrap();
        assert_eq!(frame.image.dimensions(), (64, 32));
        assert_eq!(frame.origin, PhysPoint { x: 0, y: 0 });

        let p = PhysPoint { x: 5, y: 6 };
        platform.input.click(p).unwrap();
        platform.input.type_text("Grüße").unwrap();
        platform.input.key("ctrl+a").unwrap();
        assert!(platform.input.key("ctrl+nonsense").is_err());
        assert!(platform.input.type_text("Zeile 1\nZeile 2").is_err(), "a hidden Enter must not pass");
        platform.input.scroll(p, 3).unwrap();
        assert_eq!(
            fake.take_actions(),
            vec![Action::Click(p), Action::Type("Grüße".into()), Action::Key("ctrl+a".into()), Action::Scroll(p, 3)]
        );
        assert!(fake.take_actions().is_empty());
        assert!(platform.ui.element_at(p).is_none());

        let el = UiElement { name: "Absenden".into(), control: "Button".into(), is_password: false };
        fake.elements.lock().unwrap().push((pip_core::platform::PhysRect { x: 0, y: 0, w: 10, h: 10 }, el.clone()));
        assert_eq!(platform.ui.element_at(p), Some(el.clone()));
        assert_eq!(platform.ui.find_by_name("absend").len(), 1);
        assert!(platform.ui.find_by_name("").is_empty());
        assert!(platform.ui.focused().is_none() && platform.ui.foreground().is_none());

        fake.set_screen(image::RgbaImage::new(10, 10));
        assert_eq!(platform.screen.capture_primary().unwrap().image.dimensions(), (10, 10));
    }

    #[test]
    fn outline_follows_clicks_typing_and_scrolling() {
        let (fake, platform) = FakePlatform::from_image(image::RgbaImage::new(100, 100));
        assert!(platform.ui.outline(80).is_empty(), "no outline = like Linux");
        let item = |id, role: &str, name: &str, y, value: Option<&str>| OutlineItem {
            id,
            role: role.into(),
            name: name.into(),
            value: value.map(String::from),
            rect: PhysRect { x: 0, y, w: 50, h: 20 },
            offscreen: y >= 100,
            enabled: true,
            is_password: false,
            localized_role: None,
            is_date: false,
        };
        *fake.outline.lock().unwrap() = vec![
            item(1, "edit", "Full name", 0, Some("old")),
            item(2, "checkbox", "Copy", 30, Some("unchecked")),
            item(3, "button", "Submit", 150, None),
        ];
        *fake.screen_h.lock().unwrap() = 100;
        assert_eq!(platform.ui.outline(2).len(), 2, "capped");
        let name = PhysPoint { x: 10, y: 10 };
        platform.input.click(name).unwrap();
        platform.input.type_text("Erika").unwrap();
        assert_eq!(platform.ui.value_at(name).as_deref(), Some("oldErika"));
        platform.input.key("ctrl+a").unwrap();
        platform.input.type_text("Erika Beispiel").unwrap();
        assert_eq!(platform.ui.value_at(name).as_deref(), Some("Erika Beispiel"));
        platform.input.click(PhysPoint { x: 10, y: 40 }).unwrap();
        assert_eq!(platform.ui.outline(9)[1].value.as_deref(), Some("checked"));
        assert_eq!(platform.ui.element_at(PhysPoint { x: 10, y: 40 }).unwrap().control, "CheckBox");
        assert!(platform.ui.outline(9)[2].offscreen);
        platform.input.scroll(name, 1).unwrap();
        let after = platform.ui.outline(9);
        assert_eq!(after[2].rect.y, 50);
        assert!(!after[2].offscreen && after[0].offscreen);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("o.json");
        std::fs::write(&path, serde_json::json!({"items": [item(1, "edit", "X", 0, None)]}).to_string()).unwrap();
        assert_eq!(fake.load_outline(&path).unwrap(), 1);
        assert!(fake.load_outline(&dir.path().join("none.json")).is_err());
    }

    fn date_item(id: u32, y: i32) -> OutlineItem {
        OutlineItem {
            id,
            role: "edit".into(),
            name: "First day of illness".into(),
            value: None,
            rect: PhysRect { x: 100, y, w: 200, h: 30 },
            offscreen: false,
            enabled: true,
            is_password: false,
            localized_role: None,
            is_date: false,
        }
    }

    #[test]
    fn edge_date_input_bug_and_the_way_around_it() {
        let (fake, platform) = FakePlatform::from_image(image::RgbaImage::new(400, 300));
        fake.add_date_input(date_item(1, 50), true);
        let field = platform.ui.outline(9)[0].clone();
        assert!(field.is_date && field.looks_like_date());
        assert_eq!(field.value.as_deref(), Some("TT.MM.JJJJ"));
        // SetValue is dropped silently.
        assert!(platform.ui.set_value_at(field.center(), "2026-10-19"));
        assert_eq!(fake.date_shown(1).as_deref(), Some("TT.MM.JJJJ"));
        // Clicking the centre lands on the year: all digits go there, the last six stay (the run-2 bug).
        platform.input.click(field.center()).unwrap();
        platform.input.type_text("19.10.2026").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("TT.MM.102026"));
        assert_eq!(platform.ui.outline(9)[0].value.as_deref(), Some("TT.MM.102026"));
        // The parts, left to right; focus the day part and type digits: day and month move on by themselves.
        let parts = platform.ui.date_parts(&field);
        assert_eq!(parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Tag", "Monat", "Jahr"]);
        assert!(parts.windows(2).all(|w| w[0].rect.x < w[1].rect.x));
        assert!(parts.iter().all(|p| p.role == "spinner" && field.rect.contains(p.center())));
        assert_eq!(parts[2].value.as_deref(), Some("102026"));
        assert!(platform.ui.element_matches(&parts[0]));
        assert!(platform.ui.focus_element(&parts[2]));
        platform.input.key("backspace").unwrap();
        assert!(platform.ui.focus_element(&parts[0]));
        platform.input.type_text("19102026").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("19.10.2026"));
        assert_eq!(platform.ui.value_at(parts[2].center()).as_deref(), Some("2026"), "a part reads its own value");
        // Typing again into a focused part starts it afresh; a single 4 in the day part is a full day.
        assert!(platform.ui.focus_element(&field), "the input gives the keys to its first part");
        platform.input.type_text("4").unwrap();
        platform.input.type_text("3").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("04.03.2026"));
        // The month moved on to the year, which starts afresh; Tab past the year leaves the field.
        platform.input.type_text("7").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("04.03.7"));
        platform.input.key("tab").unwrap();
        platform.input.type_text("99").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("04.03.7"));
        // The calendar icon takes no digits either.
        platform.input.click(PhysPoint { x: 290, y: 65 }).unwrap();
        platform.input.type_text("1").unwrap();
        assert_eq!(fake.date_shown(1).as_deref(), Some("04.03.7"));
    }

    #[test]
    fn edge_date_input_without_a_value_and_other_fields() {
        let (fake, platform) = FakePlatform::from_image(image::RgbaImage::new(400, 300));
        let mut name = date_item(1, 0);
        name.name = "Full name".into();
        name.value = Some(String::new());
        *fake.outline.lock().unwrap() = vec![name.clone()];
        fake.add_date_input(date_item(2, 50), false);
        let items = platform.ui.outline(9);
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [1, 2]);
        assert!(items[1].value.is_none() && items[1].localized_role.as_deref() == Some("Datumsauswahl"));
        assert!(platform.ui.date_parts(&items[0]).is_empty(), "a plain field has no parts");
        // Focus without a click: typing goes to that field.
        assert!(platform.ui.focus_element(&items[0]));
        platform.input.type_text("Erika").unwrap();
        assert_eq!(platform.ui.outline(9)[0].value.as_deref(), Some("Erika"));
        let parts = platform.ui.date_parts(&items[1]);
        assert!(platform.ui.focus_element(&parts[0]));
        platform.input.type_text("19102026").unwrap();
        assert_eq!(fake.date_shown(2).as_deref(), Some("19.10.2026"));
        assert!(platform.ui.outline(9)[1].value.is_none(), "still no value through the UI tree");
        assert_eq!(platform.ui.outline(9)[0].value.as_deref(), Some("Erika"), "the name field kept its text");
        // Stale check: after scrolling, the old rect holds something else.
        assert!(platform.ui.element_matches(&items[0]));
        *fake.screen_h.lock().unwrap() = 300;
        platform.input.scroll(PhysPoint { x: 1, y: 1 }, 1).unwrap();
        assert!(!platform.ui.element_matches(&items[0]));
        assert!(!platform.ui.focus_element(&items[0]));
    }

    #[test]
    fn invoke_and_restore_popup() {
        let (fake, platform) = FakePlatform::from_image(image::RgbaImage::new(400, 300));
        let mut box_ = date_item(1, 0);
        (box_.role, box_.name, box_.value) = ("checkbox".into(), "Copy".into(), Some("unchecked".into()));
        let mut send = date_item(2, 50);
        (send.role, send.name) = ("button".into(), "Send".into());
        let mut field = date_item(3, 100);
        field.name = "Name".into();
        *fake.outline.lock().unwrap() = vec![box_.clone(), send.clone(), field.clone()];
        assert!(platform.ui.invoke(&box_));
        assert_eq!(platform.ui.outline(9)[0].value.as_deref(), Some("checked"));
        assert!(platform.ui.invoke(&send));
        assert!(!platform.ui.invoke(&field), "a text field has nothing to press");
        assert!(fake.take_actions().is_empty(), "no mouse");
        assert!(platform.ui.find_restore_popup().is_none());
        let mut close = date_item(0, 0);
        (close.role, close.name) = ("button".into(), "Schließen".into());
        *fake.restore_popup.lock().unwrap() = Some(close.clone());
        assert_eq!(platform.ui.find_restore_popup(), Some(close.clone()));
        assert!(platform.ui.invoke(&close));
        assert!(platform.ui.find_restore_popup().is_none());
        assert_eq!(*fake.invoked.lock().unwrap(), ["Copy", "Send", "Schließen"]);
    }

    #[test]
    fn from_png_loads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.png");
        image::RgbaImage::new(20, 10).save(&path).unwrap();
        let (_fake, platform) = FakePlatform::from_png(&path).unwrap();
        assert_eq!(platform.screen.capture_primary().unwrap().image.dimensions(), (20, 10));
        assert!(FakePlatform::from_png(&dir.path().join("missing.png")).is_err());
    }
}
