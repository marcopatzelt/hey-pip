//! Fake platform: Screen returns a PNG, Input records actions. OWNER: agent "platform".

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use copilot_core::platform::{Frame, Input, PhysPoint, PhysRect, Platform, Screen, UiElement, UiTree, WindowInfo};

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
        });
        let platform = Platform { screen: me.clone(), input: me.clone(), ui: me.clone() };
        (me, platform)
    }

    /// Replace the screen (e.g. the "after submit" screenshot in an eval task).
    pub fn set_screen(&self, img: image::RgbaImage) {
        *self.screen.lock().expect("screen lock") = img;
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
        self.record(Action::Click(p))
    }
    fn type_text(&self, s: &str) -> Result<()> {
        crate::keys::check_typeable(s)?;
        self.record(Action::Type(s.to_owned()))
    }
    fn key(&self, combo: &str) -> Result<()> {
        crate::keys::parse_combo(combo)?;
        self.record(Action::Key(combo.to_owned()))
    }
    fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()> {
        self.record(Action::Scroll(p, dy))
    }
}

impl UiTree for FakePlatform {
    fn element_at(&self, p: PhysPoint) -> Option<UiElement> {
        let els = self.elements.lock().ok()?;
        els.iter().find(|(r, _)| r.contains(p)).map(|(_, e)| e.clone())
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
        fake.elements.lock().unwrap().push((copilot_core::platform::PhysRect { x: 0, y: 0, w: 10, h: 10 }, el.clone()));
        assert_eq!(platform.ui.element_at(p), Some(el.clone()));
        assert_eq!(platform.ui.find_by_name("absend").len(), 1);
        assert!(platform.ui.find_by_name("").is_empty());
        assert!(platform.ui.focused().is_none() && platform.ui.foreground().is_none());

        fake.set_screen(image::RgbaImage::new(10, 10));
        assert_eq!(platform.screen.capture_primary().unwrap().image.dimensions(), (10, 10));
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
