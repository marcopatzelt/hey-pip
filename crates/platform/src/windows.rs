//! Windows: xcap capture, enigo input, uiautomation element names. All physical pixels;
//! the process is per-monitor DPI aware (Tauri sets it; `WinPlatform::new` also calls `set_dpi_awareness`).
//! enigo lives in a Mutex; UI Automation (COM) is created once per thread (thread-local).
//! OWNER: agent "platform".

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use copilot_core::platform::{Frame, Input, PhysPoint, PhysRect, Platform, Screen, UiElement, UiTree, WindowInfo};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::{ControlType, Handle, Point};
use uiautomation::{UIAutomation, UIElement};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SetCursorPos,
};

use crate::keys::{KeyName, Modifier, check_typeable, parse_combo};
use crate::{FoundElement, MonitorInfo};

/// Characters per SendInput batch when typing; small batches keep browsers from dropping keys.
const TYPE_CHUNK: usize = 24;

pub struct WinPlatform {
    enigo: Mutex<Enigo>,
}

impl WinPlatform {
    #[allow(clippy::new_ret_no_self)] // design: `new` hands out the trait bundle
    pub fn new() -> Result<Platform> {
        Ok(Self::new_arc()?.into_platform())
    }

    /// The concrete type (for `find` and tools that need more than the traits).
    pub fn new_arc() -> Result<Arc<WinPlatform>> {
        set_dpi_awareness();
        let enigo = Enigo::new(&Settings::default()).map_err(|e| anyhow!("cannot control mouse and keyboard: {e}"))?;
        Ok(Arc::new(WinPlatform { enigo: Mutex::new(enigo) }))
    }

    pub fn into_platform(self: Arc<Self>) -> Platform {
        Platform { screen: self.clone(), input: self.clone(), ui: self }
    }

    fn with_enigo<R>(&self, f: impl FnOnce(&mut Enigo) -> Result<R, enigo::InputError>) -> Result<R> {
        let mut e = self.enigo.lock().map_err(|_| anyhow!("input lock poisoned"))?;
        f(&mut e).map_err(|e| anyhow!("input failed: {e}"))
    }

    /// Elements in the foreground window whose name contains `name` (case-insensitive), max 20,
    /// skipping off-screen ones. Physical rects.
    pub fn find(&self, name: &str) -> Vec<FoundElement> {
        let needle = name.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        with_uia(|uia| {
            let hwnd = unsafe { GetForegroundWindow() };
            let root = if hwnd.is_null() {
                uia.get_root_element()?
            } else {
                uia.element_from_handle(Handle::from(hwnd as isize))?
            };
            let needle = needle.clone();
            let matcher =
                uia.create_matcher().from(root).depth(25).timeout(0).filter_fn(Box::new(move |e: &UIElement| {
                    Ok(e.get_name().map(|n| n.to_lowercase().contains(&needle)).unwrap_or(false))
                }));
            let found = matcher.find_all().unwrap_or_default();
            let me = std::process::id();
            Ok(found
                .iter()
                .filter(|e| !e.is_offscreen().unwrap_or(false))
                .filter(|e| e.get_process_id().ok() != Some(me))
                .filter_map(|e| {
                    let r = e.get_bounding_rectangle().ok()?;
                    Some(FoundElement {
                        element: describe(e),
                        rect: PhysRect { x: r.get_left(), y: r.get_top(), w: r.get_width(), h: r.get_height() },
                    })
                })
                .take(20)
                .collect())
        })
        .unwrap_or_default()
    }
}

/// The foreground window's title and class; None if it is one of ours (Pip's overlay) or there is none.
fn foreground_window() -> Option<(isize, String, String)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == std::process::id() {
        return None;
    }
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) }.max(0) as usize;
    let title = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    let mut cls = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32) }.max(0) as usize;
    let class = String::from_utf16_lossy(&cls[..n.min(cls.len())]);
    Some((hwnd as isize, title, class))
}

/// Names of the address bar in Edge / Chrome / Firefox (German and English Windows).
const ADDRESS_BAR_NAMES: &[&str] = &[
    "address and search bar",
    "adress- und suchleiste",
    "search or enter address",
    "suchen oder adresse eingeben",
    "mit google suchen oder eine url eingeben",
    "search google or type a url",
    "suchbegriff oder adresse eingeben",
    "search with google or enter address",
];

/// The text in the browser's address bar (Edit control, read via the Value pattern).
fn address_bar(uia: &UIAutomation, hwnd: isize) -> Option<String> {
    let root = uia.element_from_handle(Handle::from(hwnd)).ok()?;
    let matcher = uia.create_matcher().from(root).depth(14).timeout(0).control_type(ControlType::Edit).filter_fn(
        Box::new(|e: &UIElement| {
            let n = e.get_name().unwrap_or_default().to_lowercase();
            Ok(ADDRESS_BAR_NAMES.iter().any(|a| n == *a) || n.contains("adressleiste") || n.contains("address bar"))
        }),
    );
    let bar = matcher.find_first().ok()?;
    let value = bar.get_pattern::<UIValuePattern>().ok()?.get_value().ok()?;
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Make the process per-monitor DPI aware (CLI only; Tauri does it itself). Safe to call twice.
pub fn set_dpi_awareness() {
    // Fails harmlessly when awareness was already set (Tauri manifest or an earlier call).
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// Mouse position in physical pixels.
pub fn cursor_pos() -> Option<PhysPoint> {
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some(PhysPoint { x: p.x, y: p.y })
}

pub fn monitors() -> Result<Vec<MonitorInfo>> {
    let all = xcap::Monitor::all().map_err(|e| anyhow!("cannot list screens: {e}"))?;
    Ok(all
        .iter()
        .map(|m| MonitorInfo {
            name: m.friendly_name().or_else(|_| m.name()).unwrap_or_default(),
            x: m.x().unwrap_or(0),
            y: m.y().unwrap_or(0),
            w: m.width().unwrap_or(0),
            h: m.height().unwrap_or(0),
            scale: m.scale_factor().unwrap_or(1.0),
            primary: m.is_primary().unwrap_or(false),
        })
        .collect())
}

fn primary_monitor() -> Result<xcap::Monitor> {
    let all = xcap::Monitor::all().map_err(|e| anyhow!("cannot list screens: {e}"))?;
    let idx = all.iter().position(|m| m.is_primary().unwrap_or(false)).unwrap_or(0);
    all.into_iter().nth(idx).context("no screen found")
}

impl Screen for WinPlatform {
    fn capture_primary(&self) -> Result<Frame> {
        let m = primary_monitor()?;
        let image = m.capture_image().map_err(|e| anyhow!("cannot take a screenshot: {e}"))?;
        let (w, h) = (m.width().unwrap_or(0), m.height().unwrap_or(0));
        if (w, h) != image.dimensions() {
            // Would mean the process is not DPI aware and coordinates are off.
            tracing::warn!(monitor = ?(w, h), image = ?image.dimensions(), "screenshot size differs from monitor size");
        }
        Ok(Frame {
            image,
            origin: PhysPoint { x: m.x().unwrap_or(0), y: m.y().unwrap_or(0) },
            scale: m.scale_factor().unwrap_or(1.0),
        })
    }
}

impl Input for WinPlatform {
    fn move_to(&self, p: PhysPoint) -> Result<()> {
        // SetCursorPos takes physical pixels in a per-monitor-aware process (no 0..65535 rounding).
        if unsafe { SetCursorPos(p.x, p.y) } == 0 {
            // Fallback: enigo absolute move (primary monitor only).
            self.with_enigo(|e| e.move_mouse(p.x, p.y, Coordinate::Abs))?;
        }
        Ok(())
    }

    fn click(&self, p: PhysPoint) -> Result<()> {
        self.move_to(p)?;
        sleep(Duration::from_millis(60)); // let hover states settle
        self.with_enigo(|e| {
            e.button(Button::Left, Direction::Press)?;
            sleep(Duration::from_millis(30));
            e.button(Button::Left, Direction::Release)
        })?;
        sleep(Duration::from_millis(40));
        Ok(())
    }

    fn type_text(&self, s: &str) -> Result<()> {
        // enigo turns '\n' into an Enter key press (could send a form past the gate): refuse control chars.
        check_typeable(s)?;
        let chars: Vec<char> = s.chars().collect();
        for chunk in chars.chunks(TYPE_CHUNK) {
            let part: String = chunk.iter().collect();
            self.with_enigo(|e| e.text(&part))?;
            sleep(Duration::from_millis(15));
        }
        Ok(())
    }

    fn key(&self, combo: &str) -> Result<()> {
        let combo = parse_combo(combo)?;
        let mut mods: Vec<Key> = combo.mods.iter().map(|m| modifier_key(*m)).collect();
        // A modifier alone ("win") is a click of that key.
        let main = combo.key.map(enigo_key).or_else(|| mods.pop());
        self.with_enigo(|e| {
            let mut pressed = Vec::new();
            let mut result = Ok(());
            for m in &mods {
                result = e.key(*m, Direction::Press);
                if result.is_err() {
                    break;
                }
                pressed.push(*m);
            }
            if result.is_ok() {
                if let Some(k) = main {
                    result = e.key(k, Direction::Click);
                }
            }
            // Always release what we pressed, in reverse order.
            for m in pressed.iter().rev() {
                let _ = e.key(*m, Direction::Release);
            }
            result
        })
    }

    fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()> {
        self.move_to(p)?;
        sleep(Duration::from_millis(30));
        self.with_enigo(|e| e.scroll(dy.clamp(-50, 50), Axis::Vertical))
    }
}

fn modifier_key(m: Modifier) -> Key {
    match m {
        Modifier::Ctrl => Key::Control,
        Modifier::Shift => Key::Shift,
        Modifier::Alt => Key::Alt,
        Modifier::Win => Key::Meta,
    }
}

fn enigo_key(k: KeyName) -> Key {
    match k {
        KeyName::Enter => Key::Return,
        KeyName::Tab => Key::Tab,
        KeyName::Escape => Key::Escape,
        KeyName::Space => Key::Space,
        KeyName::Backspace => Key::Backspace,
        KeyName::Delete => Key::Delete,
        KeyName::Insert => Key::Insert,
        KeyName::Up => Key::UpArrow,
        KeyName::Down => Key::DownArrow,
        KeyName::Left => Key::LeftArrow,
        KeyName::Right => Key::RightArrow,
        KeyName::Home => Key::Home,
        KeyName::End => Key::End,
        KeyName::PageUp => Key::PageUp,
        KeyName::PageDown => Key::PageDown,
        KeyName::F(n) => [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
            Key::F13,
            Key::F14,
            Key::F15,
            Key::F16,
            Key::F17,
            Key::F18,
            Key::F19,
            Key::F20,
            Key::F21,
            Key::F22,
            Key::F23,
            Key::F24,
        ][usize::from(n.clamp(1, 24) - 1)],
        KeyName::Char(c) => Key::Unicode(c),
    }
}

thread_local! {
    /// COM objects are bound to the thread that made them, so each thread keeps its own client.
    static UIA: RefCell<Option<UIAutomation>> = const { RefCell::new(None) };
}

/// Run `f` with this thread's UI Automation client. None if COM/UIA is unavailable or `f` fails.
fn with_uia<R>(f: impl FnOnce(&UIAutomation) -> uiautomation::Result<R>) -> Option<R> {
    UIA.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            // new() initialises COM as MTA; on an STA thread (e.g. the UI thread) that fails, so reuse its COM.
            *slot = UIAutomation::new().or_else(|_| UIAutomation::new_direct()).ok();
        }
        match f(slot.as_ref()?) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::debug!("ui automation: {e}");
                None
            }
        }
    })
}

fn describe(e: &UIElement) -> UiElement {
    UiElement {
        name: e.get_name().unwrap_or_default(),
        control: e.get_control_type().map(|c| format!("{c:?}")).unwrap_or_default(),
        is_password: e.is_password().unwrap_or(false),
    }
}

impl UiTree for WinPlatform {
    fn element_at(&self, p: PhysPoint) -> Option<UiElement> {
        with_uia(|uia| {
            let el = uia.element_from_point(Point::new(p.x, p.y))?;
            if el.get_process_id().ok() == Some(std::process::id()) {
                // Our own overlay is under the point (click-through was off); we cannot see below it.
                // "Unknown": the agent then reads the label from the picture and errs towards a card.
                return Ok(None);
            }
            Ok(Some(describe(&el)))
        })
        .flatten()
    }

    fn focused(&self) -> Option<UiElement> {
        with_uia(|uia| {
            let el = uia.get_focused_element()?;
            if el.get_process_id().ok() == Some(std::process::id()) {
                return Ok(None); // Pip's own window has the focus: unknown for the page
            }
            Ok(Some(describe(&el)))
        })
        .flatten()
    }

    fn find_by_name(&self, name: &str) -> Vec<(UiElement, PhysRect)> {
        self.find(name).into_iter().map(|f| (f.element, f.rect)).collect()
    }

    fn foreground(&self) -> Option<WindowInfo> {
        let (hwnd, title, class) = foreground_window()?;
        let address = with_uia(|uia| Ok(address_bar(uia, hwnd))).flatten();
        Some(WindowInfo { title, class, address })
    }
}
