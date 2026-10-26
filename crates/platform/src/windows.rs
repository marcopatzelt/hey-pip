//! Windows: xcap capture, enigo input, uiautomation element names. All physical pixels;
//! the process is per-monitor DPI aware (Tauri sets it; `WinPlatform::new` also calls `set_dpi_awareness`).
//! enigo lives in a Mutex; UI Automation (COM) is created once per thread (thread-local).
//! OWNER: agent "platform".

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use pip_core::platform::{
    Frame, Input, OutlineItem, PhysPoint, PhysRect, Platform, Screen, UiElement, UiTree, WindowInfo, outline_role,
};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use uiautomation::core::{UICacheRequest, UICondition};
use uiautomation::patterns::{UIScrollItemPattern, UIValuePattern};
use uiautomation::types::{ControlType, Handle, Point, TreeScope, UIProperty};
use uiautomation::variants::Variant;
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

/// Windows opens Pip's file dialog in the top-left corner (its owner is the full-screen overlay).
/// Wait for our dialog (class "#32770") and move it to the middle of the screen, where the user looks.
pub fn center_own_dialog_soon() {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    std::thread::spawn(|| {
        let class: Vec<u16> = "#32770".encode_utf16().chain(Some(0)).collect();
        for _ in 0..50 {
            sleep(Duration::from_millis(100));
            let h = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
            if h.is_null() {
                continue;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, &mut pid) };
            if pid != std::process::id() {
                continue;
            }
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if unsafe { GetWindowRect(h, &mut r) } == 0 {
                return;
            }
            let (w, hgt) = (r.right - r.left, r.bottom - r.top);
            let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            let (x, y) = (((sw - w) / 2).max(0), ((sh - hgt) / 2).max(0));
            unsafe { SetWindowPos(h, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER) };
            return;
        }
    });
}

/// True if the top window under the point is Pip's overlay (click-through was off there).
fn overlay_at(p: PhysPoint) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
    let ours = OVERLAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if ours == 0 {
        return false;
    }
    let h = unsafe { WindowFromPoint(POINT { x: p.x, y: p.y }) };
    !h.is_null() && unsafe { GetAncestor(h, GA_ROOT) } as isize == ours
}

/// True if the foreground window belongs to Pip (the ask box or a question has the keyboard).
fn foreground_is_ours() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid == std::process::id() || hwnd as isize == OVERLAY_HWND.load(std::sync::atomic::Ordering::SeqCst)
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

/// Pip's overlay window (0 = none). While a screenshot is taken it is excluded from capture, so the model
/// sees the page under Pip's bubbles instead of grey masks; the user keeps seeing Pip.
static OVERLAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Called by the app once the overlay window exists.
pub fn set_overlay_window(hwnd: isize) {
    OVERLAY_HWND.store(hwnd, std::sync::atomic::Ordering::SeqCst);
}

/// Sets the display affinity of the overlay; false if there is no overlay or Windows refused.
fn exclude_overlay(on: bool) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    let h = OVERLAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if h == 0 {
        return false;
    }
    let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    unsafe { SetWindowDisplayAffinity(h as _, affinity) != 0 }
}

impl Screen for WinPlatform {
    fn hides_overlay(&self) -> bool {
        OVERLAY_HWND.load(std::sync::atomic::Ordering::SeqCst) != 0
    }

    fn capture_primary(&self) -> Result<Frame> {
        let m = primary_monitor()?;
        let excluded = exclude_overlay(true);
        if excluded {
            sleep(Duration::from_millis(60)); // the next desktop frame is drawn without the overlay
        } else if self.hides_overlay() {
            tracing::warn!("overlay could not be hidden from the screenshot");
        }
        let image = m.capture_image().map_err(|e| anyhow!("cannot take a screenshot: {e}"));
        if excluded {
            exclude_overlay(false);
        }
        let image = image?;
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
            // Pip's bubbles live in a WebView2 child process: check the window under the point too.
            if el.get_process_id().ok() == Some(std::process::id()) || overlay_at(p) {
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
            if el.get_process_id().ok() == Some(std::process::id()) || foreground_is_ours() {
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

    /// One UI Automation FindAll (with a cache request, so it is one cross-process round trip) on the web page
    /// of the window in front, on a helper thread. Slower than OUTLINE_TIMEOUT: empty (the agent then finds
    /// things in the picture, as before).
    fn outline(&self, max: usize) -> Vec<OutlineItem> {
        if OUTLINE_BUSY.swap(true, Ordering::SeqCst) {
            tracing::debug!("outline: the last one is still running");
            return Vec::new();
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("pip-outline".into()).spawn(move || {
            let started = Instant::now();
            let items = with_uia(|uia| read_outline(uia, max)).unwrap_or_default();
            OUTLINE_BUSY.store(false, Ordering::SeqCst);
            tracing::debug!(n = items.len(), ms = started.elapsed().as_millis() as u64, "outline read");
            let _ = tx.send(items);
        });
        if spawned.is_err() {
            OUTLINE_BUSY.store(false, Ordering::SeqCst);
            return Vec::new();
        }
        // The first read may make the browser build its accessibility tree: give it longer once.
        let wait = if OUTLINE_WARM.swap(true, Ordering::SeqCst) { OUTLINE_TIMEOUT } else { OUTLINE_FIRST_TIMEOUT };
        rx.recv_timeout(wait).unwrap_or_else(|_| {
            tracing::warn!(ms = wait.as_millis() as u64, "outline took too long; using the picture only");
            Vec::new()
        })
    }

    fn scroll_into_view(&self, item: &OutlineItem) -> bool {
        with_uia(|uia| {
            let Some(hwnd) = page_window() else { return Ok(false) };
            let Some(ct) = control_type_of(&item.role) else { return Ok(false) };
            let win = uia.element_from_handle(Handle::from(hwnd))?;
            let by_type = uia.create_property_condition(UIProperty::ControlType, Variant::from(ct as i32), None)?;
            let by_name = uia.create_property_condition(UIProperty::Name, Variant::from(item.name.as_str()), None)?;
            let cond = uia.create_and_condition(by_type, by_name)?;
            let found = win.find_all(TreeScope::Descendants, &cond)?;
            // Same name twice ("Service" in the menu and the breadcrumbs): the one nearest to where it was.
            let dist = |e: &UIElement| {
                e.get_bounding_rectangle()
                    .map(|r| {
                        (i64::from(r.get_left() - item.rect.x)).abs() + (i64::from(r.get_top() - item.rect.y)).abs()
                    })
                    .unwrap_or(i64::MAX)
            };
            let Some(el) = found.iter().min_by_key(|e| dist(e)) else { return Ok(false) };
            el.get_pattern::<UIScrollItemPattern>()?.scroll_into_view()?;
            Ok(true)
        })
        .unwrap_or(false)
    }

    fn value_at(&self, p: PhysPoint) -> Option<String> {
        with_uia(|uia| {
            if overlay_at(p) {
                return Ok(None);
            }
            let el = uia.element_from_point(Point::new(p.x, p.y))?;
            if el.get_process_id().ok() == Some(std::process::id()) {
                return Ok(None);
            }
            // The point may hit the text inside the field: try the element, then its parent.
            let parent = uia.get_control_view_walker().and_then(|w| w.get_parent(&el)).ok();
            for e in std::iter::once(&el).chain(parent.as_ref()) {
                if e.is_password().unwrap_or(false) {
                    return Ok(None);
                }
                if let Ok(v) = e.get_pattern::<UIValuePattern>().and_then(|v| v.get_value()) {
                    return Ok(Some(v));
                }
            }
            Ok(None)
        })
        .flatten()
    }
}

/// Normal wait for the outline, and the longer one for the very first read.
const OUTLINE_TIMEOUT: Duration = Duration::from_millis(400);
const OUTLINE_FIRST_TIMEOUT: Duration = Duration::from_millis(1200);
/// One outline read at a time (a slow one keeps running on its thread; the next step just gets none).
static OUTLINE_BUSY: AtomicBool = AtomicBool::new(false);
static OUTLINE_WARM: AtomicBool = AtomicBool::new(false);

/// Controls an outline lists.
const OUTLINE_TYPES: &[ControlType] = &[
    ControlType::Edit,
    ControlType::Button,
    ControlType::SplitButton,
    ControlType::CheckBox,
    ControlType::RadioButton,
    ControlType::ComboBox,
    ControlType::Hyperlink,
    ControlType::ListItem,
    ControlType::MenuItem,
    ControlType::TabItem,
];

fn control_type_of(role: &str) -> Option<ControlType> {
    Some(match role {
        "edit" => ControlType::Edit,
        "button" => ControlType::Button,
        "checkbox" => ControlType::CheckBox,
        "radio" => ControlType::RadioButton,
        "combo" => ControlType::ComboBox,
        "link" => ControlType::Hyperlink,
        "item" => ControlType::ListItem,
        "menuitem" => ControlType::MenuItem,
        "tab" => ControlType::TabItem,
        _ => return None,
    })
}

/// The window the user works in: the foreground window, or, when Pip's own window has the keyboard (the user
/// just typed the task into Pip), the next visible titled window below it.
fn page_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_HWNDNEXT, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, IsIconic, IsWindowVisible,
        WS_EX_TOOLWINDOW,
    };
    let me = std::process::id();
    let overlay = OVERLAY_HWND.load(Ordering::SeqCst);
    let mut h = unsafe { GetForegroundWindow() };
    for i in 0..256 {
        if h.is_null() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        let ours = pid == me || h as isize == overlay;
        if !ours && i == 0 {
            return Some(h as isize);
        }
        if !ours
            && unsafe { IsWindowVisible(h) } != 0
            && unsafe { IsIconic(h) } == 0
            && unsafe { GetWindowTextLengthW(h) } > 0
            && unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW == 0
        {
            return Some(h as isize);
        }
        h = unsafe { GetWindow(h, GW_HWNDNEXT) };
    }
    None
}

fn outline_cache(uia: &UIAutomation) -> uiautomation::Result<UICacheRequest> {
    let cache = uia.create_cache_request()?;
    for p in [
        UIProperty::Name,
        UIProperty::ControlType,
        UIProperty::BoundingRectangle,
        UIProperty::IsOffscreen,
        UIProperty::IsEnabled,
        UIProperty::IsPassword,
        UIProperty::ValueValue,
        UIProperty::ToggleToggleState,
        UIProperty::SelectionItemIsSelected,
        UIProperty::ProcessId,
    ] {
        cache.add_property(p)?;
    }
    Ok(cache)
}

/// Control elements of one of OUTLINE_TYPES.
fn outline_condition(uia: &UIAutomation) -> uiautomation::Result<UICondition> {
    let mut any: Option<UICondition> = None;
    for ct in OUTLINE_TYPES {
        let c = uia.create_property_condition(UIProperty::ControlType, Variant::from(*ct as i32), None)?;
        any = Some(match any {
            Some(a) => uia.create_or_condition(a, c)?,
            None => c,
        });
    }
    let control = uia.create_property_condition(UIProperty::IsControlElement, Variant::from(true), None)?;
    match any {
        Some(a) => uia.create_and_condition(control, a),
        None => Ok(control),
    }
}

fn cached_bool(e: &UIElement, p: UIProperty) -> Option<bool> {
    let v = e.get_cached_property_value(p).ok()?;
    TryInto::<bool>::try_into(&v).ok()
}

fn phys(r: &uiautomation::types::Rect) -> PhysRect {
    PhysRect { x: r.get_left(), y: r.get_top(), w: r.get_width(), h: r.get_height() }
}

fn short(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max { s } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// The outline of the page in front: the web page (the biggest Document) of a browser, else the whole window.
fn read_outline(uia: &UIAutomation, max: usize) -> uiautomation::Result<Vec<OutlineItem>> {
    let Some(hwnd) = page_window() else { return Ok(Vec::new()) };
    let win = uia.element_from_handle(Handle::from(hwnd))?;
    let doc_cond =
        uia.create_property_condition(UIProperty::ControlType, Variant::from(ControlType::Document as i32), None)?;
    let root = win.find_first(TreeScope::Descendants, &doc_cond).unwrap_or(win);
    let viewport = phys(&root.get_bounding_rectangle()?);
    let found = root.find_all_build_cache(TreeScope::Descendants, &outline_condition(uia)?, &outline_cache(uia)?)?;
    let me = std::process::id();
    let mut out = Vec::new();
    for e in &found {
        if out.len() >= max {
            break;
        }
        if e.get_cached_process_id().ok().map(|p| p as u32) == Some(me) {
            continue;
        }
        let Ok(ct) = e.get_cached_control_type() else { continue };
        let Some(role) = outline_role(&format!("{ct:?}")) else { continue };
        let Ok(r) = e.get_cached_bounding_rectangle() else { continue };
        let rect = phys(&r);
        if rect.w <= 1 || rect.h <= 1 {
            continue; // hidden (a file input behind its label, a collapsed menu)
        }
        let name = short(&e.get_cached_name().unwrap_or_default(), 100);
        if name.is_empty() && !matches!(role, "edit" | "combo" | "checkbox" | "radio") {
            continue; // decorative
        }
        let is_password = cached_bool(e, UIProperty::IsPassword).unwrap_or(false);
        let center = PhysPoint { x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 };
        let offscreen = cached_bool(e, UIProperty::IsOffscreen).unwrap_or(false) || !viewport.contains(center);
        let value = if is_password {
            None
        } else {
            match role {
                "checkbox" => e
                    .get_cached_property_value(UIProperty::ToggleToggleState)
                    .ok()
                    .and_then(|v| TryInto::<i32>::try_into(&v).ok())
                    .map(|t| if t == 1 { "checked" } else { "unchecked" }.to_string()),
                "radio" => cached_bool(e, UIProperty::SelectionItemIsSelected)
                    .map(|on| if on { "selected" } else { "not selected" }.to_string()),
                "edit" | "combo" => e
                    .get_cached_property_value(UIProperty::ValueValue)
                    .ok()
                    .filter(Variant::is_string)
                    .and_then(|v| v.get_string().ok())
                    .map(|v| short(&v, 60)),
                _ => None,
            }
        };
        out.push(OutlineItem {
            id: out.len() as u32 + 1,
            role: role.to_string(),
            name,
            value,
            rect,
            offscreen,
            enabled: cached_bool(e, UIProperty::IsEnabled).unwrap_or(true),
            is_password,
        });
    }
    Ok(out)
}
