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
    /// Field that gets typed text (last click) and whether Ctrl+A selected its text.
    caret: Mutex<(Option<PhysPoint>, bool)>,
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
            caret: Mutex::default(),
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

    pub fn take_actions(&self) -> Vec<Action> {
        std::mem::take(&mut *self.actions.lock().expect("actions lock"))
    }

    fn record(&self, a: Action) -> Result<()> {
        self.actions.lock().map_err(|_| anyhow::anyhow!("actions lock poisoned"))?.push(a);
        Ok(())
    }
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
        if let (Ok(mut caret), Ok(mut items)) = (self.caret.lock(), self.outline.lock()) {
            *caret = (Some(p), false);
            if let Some(it) = items.iter_mut().find(|i| i.rect.contains(p) && i.role == "checkbox") {
                let on = it.value.as_deref() == Some("checked");
                it.value = Some(if on { "unchecked" } else { "checked" }.into());
            }
        }
        self.record(Action::Click(p))
    }
    fn type_text(&self, s: &str) -> Result<()> {
        crate::keys::check_typeable(s)?;
        if let (Ok(mut caret), Ok(mut items)) = (self.caret.lock(), self.outline.lock())
            && let Some(p) = caret.0
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
    fn value_at(&self, p: PhysPoint) -> Option<String> {
        let items = self.outline.lock().ok()?;
        let it = items.iter().find(|i| !i.offscreen && i.rect.contains(p) && !i.is_password)?;
        it.value.clone()
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
