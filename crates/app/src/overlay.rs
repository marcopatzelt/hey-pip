//! Full-screen transparent overlay with click-through: every 33 ms we compare the cursor with the
//! rects the UI reports (Pip, bubble, card) and let clicks pass everywhere else.
//! The page reports its rects in CSS px; they become screen pixels here with the window's live position and the
//! page's devicePixelRatio (`css_to_phys`), so a page holding a stale window position never misplaces them.
//! On Windows a low-level mouse hook does the same on every mouse move, straight on the window style (no 33 ms lag,
//! no round trip through tao's event loop), and logs every left press ("click" lines: cursor, hit rect, live
//! click-through bit, the window under the cursor). A press on Pip's window that would still fall through is
//! swallowed and sent again once the window takes clicks. The poll keeps tao's own flags in step.
//! Fallback (`PIP_WINDOWED=1`): a 420x360 corner window that is always clickable.
//! The same poll also notices a person moving the mouse on purpose while Pip acts (SPEC §0: move = stop):
//! more than 120 px within 300 ms, or moving again at least 1 s after an earlier motion. A nudge is not a stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use pip_core::events::Bus;
use pip_core::platform::{PhysPoint, PhysRect};
use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

pub const CORNER_W: u32 = 420;
pub const CORNER_H: u32 = 360;

/// What the UI needs to turn physical screen pixels into CSS pixels: css = (phys - origin) / scale.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct OverlayInfo {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
    pub windowed: bool,
}

/// A rect of the page in CSS px, from the window's top-left (getBoundingClientRect).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Page rects -> screen pixels: origin + css * scale, rounded outward (a button's edge pixel is never left out).
/// `origin` is the window's top-left on the screen (physical px; a monitor left of the primary one is negative),
/// `scale` the page's devicePixelRatio (1.0 at 100 %, 1.25 at 125 %).
pub fn css_to_phys(css: &[CssRect], origin: PhysPoint, scale: f64) -> Vec<PhysRect> {
    css.iter()
        .filter(|r| [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0)
        .map(|r| {
            let x0 = (f64::from(origin.x) + r.x * scale).floor() as i32;
            let y0 = (f64::from(origin.y) + r.y * scale).floor() as i32;
            let x1 = (f64::from(origin.x) + (r.x + r.w) * scale).ceil() as i32;
            let y1 = (f64::from(origin.y) + (r.y + r.h) * scale).ceil() as i32;
            PhysRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
        })
        .collect()
}

/// The rects the cursor poll and the screenshot masks use. The page's CSS rects at the window's live position
/// when both are there and the page's scale is sane; else the page's own screen rects (`fallback`).
pub fn resolve_hit_rects(
    css: &[CssRect],
    scale: f64,
    origin: Option<PhysPoint>,
    fallback: Vec<PhysRect>,
) -> Vec<PhysRect> {
    match origin {
        Some(o) if scale.is_finite() && (0.25..=8.0).contains(&scale) => css_to_phys(css, o, scale),
        _ => fallback,
    }
}

/// The cursor on the screen (physical px). On Windows straight from the OS: no round trip through the event loop,
/// which a busy main thread would hold up (and the click-through with it).
pub fn cursor_now(win: &WebviewWindow) -> Option<PhysPoint> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            return Some(PhysPoint { x: pt.x, y: pt.y });
        }
    }
    let pos = win.cursor_position().ok()?;
    Some(PhysPoint { x: pos.x.round() as i32, y: pos.y.round() as i32 })
}

/// Whether the window really lets clicks through right now (WS_EX_TRANSPARENT); None where unknown.
fn click_through_now(win: &WebviewWindow) -> Option<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_TRANSPARENT};
        let h = win.hwnd().ok()?.0 as isize;
        let style = unsafe { GetWindowLongPtrW(h as _, GWL_EXSTYLE) } as u32;
        Some(style & WS_EX_TRANSPARENT != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = win;
        None
    }
}

pub fn info(win: &WebviewWindow, windowed: bool) -> Result<OverlayInfo> {
    let pos = win.inner_position()?;
    let size = win.inner_size()?;
    Ok(OverlayInfo { x: pos.x, y: pos.y, w: size.width, h: size.height, scale: win.scale_factor()?, windowed })
}

/// While Pip acts, a person moving the mouse stops him. `armed` says when to watch;
/// `on_user_move` runs once per detected move.
pub struct MouseStop {
    pub armed: Arc<AtomicBool>,
    /// No watching until then (e.g. a few seconds after "Ja": the presenter's hand moves on).
    pub quiet_until: Arc<Mutex<Option<Instant>>>,
    pub on_user_move: Box<dyn Fn() + Send + Sync>,
}

impl MouseStop {
    fn watching(&self, now: Instant) -> bool {
        let quiet = self.quiet_until.lock().map(|q| q.is_some_and(|t| now < t)).unwrap_or(false);
        self.armed.load(Ordering::SeqCst) && !quiet
    }
}

/// Keyboard focus. The overlay never takes it by a click (not focusable: clicking "Ja" keeps the browser in
/// front, so Pip's next Enter or Tab goes to the page). Text fields (ask box, name) take it on purpose and
/// give it back to the window that had it.
#[derive(Default)]
pub struct FocusKeeper {
    prev: Mutex<Option<isize>>,
}

impl FocusKeeper {
    pub fn take(&self, win: &WebviewWindow) {
        #[cfg(windows)]
        {
            let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize;
            let ours = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
            if fg != 0 && fg != ours {
                *self.prev.lock().expect("focus lock") = Some(fg);
            }
        }
        let _ = win.set_focusable(true);
        let _ = win.set_focus();
    }

    /// Back to the window that had the keyboard before (if any); the overlay is not focusable again.
    pub fn give_back(&self, win: &WebviewWindow) {
        let _ = win.set_focusable(false);
        let prev = self.prev.lock().expect("focus lock").take();
        #[cfg(windows)]
        if let Some(h) = prev {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(h as _) };
        }
        #[cfg(not(windows))]
        let _ = prev;
    }
}

/// Size the window to the primary monitor (or the corner fallback), show it, start the cursor poll
/// (click-through outside Pip's rects; mouse-move stop).
pub fn setup(win: &WebviewWindow, bus: Bus, windowed: bool, mouse_stop: MouseStop) -> Result<()> {
    if let Some(m) = win.primary_monitor()? {
        // The work area leaves the taskbar free (Pip must never sit on it).
        let wa = m.work_area();
        let (mpos, msize) =
            if wa.size.width > 0 && wa.size.height > 0 { (wa.position, wa.size) } else { (*m.position(), *m.size()) };
        if windowed {
            let s = m.scale_factor();
            let (w, h) = ((CORNER_W as f64 * s) as u32, (CORNER_H as f64 * s) as u32);
            win.set_size(PhysicalSize::new(w, h))?;
            let x = mpos.x + msize.width as i32 - w as i32 - (16.0 * s) as i32;
            let y = mpos.y + msize.height as i32 - h as i32 - (56.0 * s) as i32;
            win.set_position(PhysicalPosition::new(x, y))?;
        } else {
            win.set_position(mpos)?;
            win.set_size(msize)?;
        }
    }
    win.show()?;
    #[cfg(windows)]
    if let Ok(h) = win.hwnd() {
        pip_platform::windows::set_overlay_window(h.0 as isize);
    }
    // Clicks on Pip, the card or a bubble must not pull the keyboard away from the page Pip works on.
    if let Err(e) = win.set_focusable(false) {
        tracing::debug!("set_focusable: {e}");
    }
    // Wayland ignores positions; maximizing is the only way to cover the screen there ("Linux in theory").
    #[cfg(target_os = "linux")]
    if !windowed {
        let _ = win.set_resizable(true);
        let _ = win.maximize();
    }
    if !windowed {
        win.set_ignore_cursor_events(true)?;
    }
    // The mouse hook writes the click-through bit itself; `want` is the state it (or the poll) wrote last.
    #[cfg(windows)]
    let direct = if windowed { None } else { win.hwnd().ok().map(|h| h.0 as isize) };
    #[cfg(windows)]
    let (want, clicks) = {
        let want = Arc::new(AtomicBool::new(true));
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(h) = direct {
            hook::start(h, bus.hit_rects.clone(), want.clone(), tx);
        }
        (want, rx)
    };
    let win = win.clone();
    tauri::async_runtime::spawn(async move {
        let mut ignoring = true;
        #[cfg(windows)]
        let mut sync = SyncCheck::default();
        let mut watch = MouseWatch::default();
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut n: u64 = 0;
        let mut off_sync = 0u32;
        loop {
            tick.tick().await;
            n += 1;
            let Some(p) = cursor_now(&win) else { continue };
            let now = Instant::now();
            if watch.feed(p, mouse_stop.watching(now), now) {
                (mouse_stop.on_user_move)();
            }
            if windowed {
                continue;
            }
            let ignore = !inside_any(&bus.hit_rects(), p);
            #[cfg(windows)]
            if let Some(h) = direct {
                while let Ok(c) = clicks.try_recv() {
                    hook::log_click(h, &c);
                }
                // The hook follows mouse moves; this catches rects that change under a resting cursor.
                // `want` first, then the style: a reader seeing the new bit never compares it with the old wish.
                if ignore != want.load(Ordering::SeqCst) {
                    want.store(ignore, Ordering::SeqCst);
                    hook::write_click_through(h, ignore);
                }
                // tao keeps its own copy of the flags and rewrites the whole style from it (set_focusable):
                // tell it afterwards, off the hook's path.
                let w = want.load(Ordering::SeqCst);
                if w != ignoring {
                    match win.set_ignore_cursor_events(w) {
                        Ok(()) => ignoring = w,
                        Err(e) => tracing::debug!("set_ignore_cursor_events: {e}"),
                    }
                }
                // The live bit must be what we wrote (tao applying an older state late would undo it).
                let live = hook::click_through(h);
                if want.load(Ordering::SeqCst) == w {
                    match sync.see(w, live) {
                        StyleSync::InStep => {}
                        StyleSync::FirstMismatch => {
                            tracing::warn!(want_click_through = w, "overlay click-through was off sync: fixed");
                            hook::write_click_through(h, w);
                        }
                        StyleSync::Mismatch(n) => {
                            tracing::debug!(want_click_through = w, n, "overlay click-through off sync again: fixed");
                            hook::write_click_through(h, w);
                        }
                    }
                }
                continue;
            }
            if ignore != ignoring {
                match win.set_ignore_cursor_events(ignore) {
                    Ok(()) => {
                        ignoring = ignore;
                        off_sync = 0;
                    }
                    Err(e) => tracing::debug!("set_ignore_cursor_events: {e}"),
                }
                continue;
            }
            // Safety net (every ~0.5 s): the window's real style must match what we set. Seen off twice in a row
            // (not a toggle still on its way): set it again, through the other state so it is really applied.
            if n % 15 == 0 {
                match click_through_now(&win) {
                    Some(actual) if actual != ignoring => {
                        off_sync += 1;
                        if off_sync >= 2 {
                            tracing::warn!(want_click_through = ignoring, "overlay click-through was off sync: fixed");
                            let _ = win.set_ignore_cursor_events(!ignoring);
                            let _ = win.set_ignore_cursor_events(ignoring);
                            off_sync = 0;
                        }
                    }
                    _ => off_sync = 0,
                }
            }
        }
    });
    Ok(())
}

// ---------- click-through decisions (pure: the Windows mouse hook runs them, the tests feed fake events) ----------

/// One low-level mouse event, as far as click-through cares.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEv {
    Move,
    LeftDown,
    LeftUp,
}

/// What the hook does with one event.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Let it go on; `Some(t)`: set click-through to `t` first.
    Pass(Option<bool>),
    /// A person's left press on Pip's window while it still lets clicks through: take clicks now, swallow the press
    /// and send it again (it would land on the page under Pip otherwise).
    Rescue,
    /// The release of a rescued press came before the press went out again: swallow it, it is sent after the press.
    HoldUp,
    /// A press Pip sent again no longer lands on a hit rect (the cursor slipped off in the few ms between): swallow
    /// it, it must never reach the page under Pip. Its release is swallowed too.
    Drop,
}

/// What to send again after a rescue.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resend {
    Nothing,
    Down,
    DownUp,
}

#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Rescue {
    #[default]
    Idle,
    DownOwed,
    UpOwed,
    DownSent,
    /// Pip's own press was dropped (it slipped off the rect); its own release is dropped too.
    Dropped,
}

/// The hook's decisions: click-through follows the cursor; a press that would fall through is rescued.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Default)]
pub struct ClickGuard {
    rescue: Rescue,
}

#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
impl ClickGuard {
    /// `own`: an event Pip sent again itself; `inside`: the cursor is on a hit rect; `click_through`: the window's
    /// live WS_EX_TRANSPARENT bit.
    pub fn decide(&mut self, ev: MouseEv, own: bool, inside: bool, click_through: bool) -> Verdict {
        // On a hit rect the window takes clicks, everywhere else they go through.
        let follow = Verdict::Pass((click_through == inside).then_some(!inside));
        match ev {
            MouseEv::LeftDown if !own => {
                if inside && click_through {
                    self.rescue = Rescue::DownOwed;
                    Verdict::Rescue
                } else {
                    self.rescue = Rescue::Idle;
                    follow
                }
            }
            // Pip's own press (sent again) no longer lands on a rect: it must not reach the page under Pip.
            MouseEv::LeftDown if !inside => {
                self.rescue = Rescue::Dropped;
                Verdict::Drop
            }
            MouseEv::LeftUp if !own => match self.rescue {
                Rescue::DownOwed => {
                    self.rescue = Rescue::UpOwed;
                    Verdict::HoldUp
                }
                Rescue::DownSent | Rescue::Dropped => {
                    self.rescue = Rescue::Idle;
                    Verdict::Pass(None)
                }
                Rescue::Idle | Rescue::UpOwed => Verdict::Pass(None),
            },
            MouseEv::LeftUp if self.rescue == Rescue::Dropped => {
                self.rescue = Rescue::Idle;
                Verdict::Drop
            }
            MouseEv::LeftUp => Verdict::Pass(None),
            MouseEv::Move | MouseEv::LeftDown => follow,
        }
    }

    /// The hook thread is free to send again: what goes out.
    pub fn resend(&mut self) -> Resend {
        match std::mem::take(&mut self.rescue) {
            Rescue::DownOwed => {
                self.rescue = Rescue::DownSent;
                Resend::Down
            }
            Rescue::UpOwed => Resend::DownUp,
            other => {
                self.rescue = other;
                Resend::Nothing
            }
        }
    }
}

/// The window's live click-through bit compared with the one Pip wrote last.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleSync {
    InStep,
    /// The first time ever: worth a warning in the log.
    FirstMismatch,
    /// Again (how many so far).
    Mismatch(u32),
}

#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Default)]
pub struct SyncCheck {
    mismatches: u32,
}

#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
impl SyncCheck {
    pub fn see(&mut self, want: bool, live: Option<bool>) -> StyleSync {
        match live {
            Some(l) if l != want => {
                self.mismatches += 1;
                if self.mismatches == 1 { StyleSync::FirstMismatch } else { StyleSync::Mismatch(self.mismatches) }
            }
            _ => StyleSync::InStep,
        }
    }
}

/// One left press by a person, as the hook saw it (logged by the poll, so the hook itself stays fast).
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows mouse hook (and the tests) use it
#[derive(Debug, Clone, PartialEq)]
pub struct ClickRecord {
    pub at: PhysPoint,
    /// On one of Pip's hit rects.
    pub inside: bool,
    /// WS_EX_TRANSPARENT at that moment.
    pub click_through: bool,
    /// Swallowed and sent again.
    pub rescued: bool,
    /// The class of the window holding the mouse capture in the foreground thread (a browser popup), if any.
    pub capture_class: Option<String>,
}

/// The low-level mouse hook (WH_MOUSE_LL) on its own thread. Pattern: set or clear only WS_EX_TRANSPARENT with
/// SetWindowLongPtrW on every move, as Electron's `SetIgnoreMouseEvents` forwarding and tauri-plugin-polygon
/// (`src/grab.rs`, MIT) do; WS_EX_LAYERED stays on.
#[cfg(windows)]
mod hook {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex, OnceLock, RwLock};

    use pip_core::platform::{PhysPoint, PhysRect};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GA_ROOT, GUITHREADINFO, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetGUIThreadInfo, GetMessageW,
        GetWindowLongPtrW, HC_ACTION, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowLongPtrW, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WS_EX_LAYERED,
        WS_EX_TRANSPARENT, WindowFromPoint,
    };

    use super::{ClickGuard, ClickRecord, MouseEv, Resend, Verdict, inside_any};

    /// Marks the presses Pip sends again (dwExtraInfo), so the hook lets them pass.
    const OWN_TAG: usize = 0x5049_5021;
    /// Posted to the hook thread: send the rescued press again (SendInput must not run inside the hook).
    const WM_RESEND: u32 = WM_APP + 0x51;

    struct Shared {
        hwnd: isize,
        rects: Arc<RwLock<Vec<PhysRect>>>,
        want: Arc<AtomicBool>,
        clicks: Mutex<Sender<ClickRecord>>,
        guard: Mutex<ClickGuard>,
        thread: AtomicU32,
    }

    static HOOK: OnceLock<Shared> = OnceLock::new();

    /// The window lets clicks through right now (WS_EX_TRANSPARENT).
    pub fn click_through(hwnd: isize) -> Option<bool> {
        let style = unsafe { GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE) } as isize;
        Some(style & WS_EX_TRANSPARENT as isize != 0)
    }

    /// Set or clear only WS_EX_TRANSPARENT (WS_EX_LAYERED stays on: no rebuild of the page's hit test).
    pub fn write_click_through(hwnd: isize, on: bool) {
        let h = hwnd as HWND;
        let style = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as isize;
        let next = if on {
            style | WS_EX_TRANSPARENT as isize | WS_EX_LAYERED as isize
        } else {
            style & !(WS_EX_TRANSPARENT as isize)
        };
        if next != style {
            unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, next as _) };
        }
    }

    /// Start the hook thread once. If the hook cannot start, the cursor poll does the click-through alone.
    pub fn start(hwnd: isize, rects: Arc<RwLock<Vec<PhysRect>>>, want: Arc<AtomicBool>, clicks: Sender<ClickRecord>) {
        let shared = Shared {
            hwnd,
            rects,
            want,
            clicks: Mutex::new(clicks),
            guard: Mutex::new(ClickGuard::default()),
            thread: AtomicU32::new(0),
        };
        if HOOK.set(shared).is_err() {
            return;
        }
        let spawned = std::thread::Builder::new().name("pip-mouse-hook".into()).spawn(|| {
            let Some(s) = HOOK.get() else { return };
            s.thread.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
            if hook.is_null() {
                tracing::warn!("mouse hook did not start: the cursor poll does the click-through alone");
                return;
            }
            tracing::info!("mouse hook on: click-through follows every mouse move");
            // The hook's calls arrive inside GetMessageW; thread messages are ours.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                if msg.message == WM_RESEND {
                    let r = s.guard.lock().map(|mut g| g.resend()).unwrap_or(Resend::Nothing);
                    send_again(r);
                }
            }
            unsafe { UnhookWindowsHookEx(hook) };
        });
        if let Err(e) = spawned {
            tracing::warn!("mouse hook thread: {e}");
        }
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32
            && lparam != 0
            && let Some(s) = HOOK.get()
        {
            // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
            let ms = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            if handle(s, wparam as u32, ms) {
                return 1;
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// One event; true = swallow it. Kept short: Windows drops a hook that is too slow.
    fn handle(s: &Shared, msg: u32, ms: &MSLLHOOKSTRUCT) -> bool {
        let ev = match msg {
            WM_MOUSEMOVE => MouseEv::Move,
            WM_LBUTTONDOWN => MouseEv::LeftDown,
            WM_LBUTTONUP => MouseEv::LeftUp,
            _ => return false,
        };
        let p = PhysPoint { x: ms.pt.x, y: ms.pt.y };
        let own = ms.dwExtraInfo == OWN_TAG;
        let inside = s.rects.read().map(|r| inside_any(&r, p)).unwrap_or(false);
        let live = click_through(s.hwnd).unwrap_or(false);
        let verdict = s.guard.lock().map(|mut g| g.decide(ev, own, inside, live)).unwrap_or(Verdict::Pass(None));
        if ev == MouseEv::LeftDown && !own {
            // The window under the cursor is looked up by the poll (WindowFromPoint sends WM_NCHITTEST to other
            // threads' windows: a busy one would stall the hook, and Windows drops a slow hook without a word).
            let rec = ClickRecord {
                at: p,
                inside,
                click_through: live,
                rescued: verdict == Verdict::Rescue,
                capture_class: capture(),
            };
            if let Ok(tx) = s.clicks.lock() {
                let _ = tx.send(rec);
            }
        }
        // `want` before the style: the poll compares the live bit with `want` and must never see the new bit
        // next to the old wish.
        match verdict {
            Verdict::Pass(Some(on)) => {
                s.want.store(on, Ordering::SeqCst);
                write_click_through(s.hwnd, on);
                false
            }
            Verdict::Pass(None) => false,
            Verdict::Rescue => {
                s.want.store(false, Ordering::SeqCst);
                write_click_through(s.hwnd, false);
                unsafe { PostThreadMessageW(s.thread.load(Ordering::SeqCst), WM_RESEND, 0, 0) };
                true
            }
            Verdict::HoldUp => true,
            Verdict::Drop => {
                if ev == MouseEv::LeftDown {
                    tracing::warn!(x = p.x, y = p.y, "the press sent again on Pip slipped off his window: dropped");
                }
                true
            }
        }
    }

    /// One "click" log line for a person's left press (from the poll, off the hook's path). `ours`: the top-level
    /// window under the press is Pip's now; after a rescue that is expected, as the window takes clicks by then.
    pub fn log_click(hwnd: isize, c: &ClickRecord) {
        let root = unsafe { GetAncestor(WindowFromPoint(POINT { x: c.at.x, y: c.at.y }), GA_ROOT) };
        tracing::info!(
            x = c.at.x,
            y = c.at.y,
            inside = c.inside,
            click_through = c.click_through,
            rescued = c.rescued,
            ours = root as isize == hwnd,
            root = %class_of(root),
            capture = c.capture_class.as_deref().unwrap_or("-"),
            "click"
        );
    }

    fn send_again(r: Resend) {
        let flags: &[u32] = match r {
            Resend::Nothing => return,
            Resend::Down => &[MOUSEEVENTF_LEFTDOWN],
            Resend::DownUp => &[MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP],
        };
        // No move flag: the press goes out where the cursor is.
        let inputs: Vec<INPUT> = flags
            .iter()
            .map(|&f| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: f, time: 0, dwExtraInfo: OWN_TAG },
                },
            })
            .collect();
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            tracing::warn!("the press on Pip could not be sent again");
        }
    }

    fn class_of(h: HWND) -> String {
        if h.is_null() {
            return "-".into();
        }
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// The window holding the mouse capture in the foreground thread (e.g. a browser's open picker).
    fn capture() -> Option<String> {
        let mut gui = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if unsafe { GetGUIThreadInfo(0, &mut gui) } == 0 || gui.hwndCapture.is_null() {
            return None;
        }
        Some(class_of(gui.hwndCapture))
    }
}

/// A deliberate move, not a nudge: more than this path (physical px) …
const FAST_PATH: f64 = 120.0;
/// … within this time, in one continuous motion of at least FAST_TICKS polls (the agent jumps the cursor in
/// one poll and then it rests).
const FAST_WINDOW: Duration = Duration::from_millis(300);
const FAST_TICKS: u32 = 3;
/// Or: two separate motions that start at least this far apart (a hand that keeps moving) …
const SLOW_GAP: Duration = Duration::from_secs(1);
/// … but not further apart than this (two nudges a minute apart are not a decision).
const SLOW_SPAN: Duration = Duration::from_secs(5);
/// A motion: at least two polls in a row with a few px each (a resting hand jitters 1 px; the agent jumps once).
const MOTION_TICKS: u32 = 2;
const MOTION_PATH: f64 = 6.0;
/// Less than this per poll is no motion.
const JITTER: f64 = 2.0;
/// After arming (Pip starts, or the user just clicked "Ja"), the hand is still on the mouse.
const MOVE_GRACE: Duration = Duration::from_millis(2000);

/// Tells a person moving the mouse on purpose from the agent's own cursor jumps and from a small nudge.
#[derive(Debug, Default)]
pub struct MouseWatch {
    last: Option<PhysPoint>,
    armed_at: Option<Instant>,
    /// Polls in a row with motion, and (time, px) of each of them.
    streak: u32,
    recent: std::collections::VecDeque<(Instant, f64)>,
    /// The current streak already counted as a motion.
    counted: bool,
    /// Start of the last motion that did not fire yet.
    last_motion: Option<Instant>,
}

impl MouseWatch {
    /// Feed one cursor sample (every ~33 ms). True once when a person moved the mouse on purpose while armed:
    /// more than 120 px within 300 ms, or moving again at least 1 s after an earlier motion.
    pub fn feed(&mut self, p: PhysPoint, armed: bool, now: Instant) -> bool {
        if !armed {
            *self = Self { last: Some(p), ..Self::default() };
            return false;
        }
        let since = *self.armed_at.get_or_insert(now);
        let Some(last) = self.last.replace(p) else { return false };
        let d = f64::from(p.x - last.x).hypot(f64::from(p.y - last.y));
        if now.duration_since(since) < MOVE_GRACE {
            self.end_streak();
            self.last_motion = None;
            return false;
        }
        if d < JITTER {
            self.end_streak();
            return false;
        }
        self.streak += 1;
        self.recent.push_back((now, d));
        while self.recent.front().is_some_and(|(t, _)| now.duration_since(*t) > FAST_WINDOW) {
            self.recent.pop_front();
        }
        let path: f64 = self.recent.iter().map(|(_, d)| d).sum();
        if self.recent.len() as u32 >= FAST_TICKS && path > FAST_PATH {
            self.reset_after_fire();
            return true;
        }
        if !self.counted && self.streak >= MOTION_TICKS && path >= MOTION_PATH {
            self.counted = true;
            let start = self.recent.front().map(|(t, _)| *t).unwrap_or(now);
            match self.last_motion {
                Some(prev) if start.duration_since(prev) >= SLOW_GAP && start.duration_since(prev) <= SLOW_SPAN => {
                    self.reset_after_fire();
                    return true;
                }
                Some(prev) if start.duration_since(prev) < SLOW_GAP => {} // same gesture: keep the first start
                _ => self.last_motion = Some(start),
            }
        }
        false
    }

    fn end_streak(&mut self) {
        self.streak = 0;
        self.recent.clear();
        self.counted = false;
    }

    fn reset_after_fire(&mut self) {
        self.end_streak();
        self.last_motion = None;
    }
}

pub fn inside_any(rects: &[PhysRect], p: PhysPoint) -> bool {
    rects.iter().any(|r| r.contains(p))
}

/// Keep only sane rects (the UI may send empty or absurd ones while animating).
pub fn sanitize(rects: Vec<PhysRect>) -> Vec<PhysRect> {
    rects.into_iter().filter(|r| r.w > 0 && r.h > 0 && r.w < 20_000 && r.h < 20_000).take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test() {
        let rects = vec![PhysRect { x: 100, y: 100, w: 50, h: 50 }, PhysRect { x: 0, y: 0, w: 10, h: 10 }];
        assert!(inside_any(&rects, PhysPoint { x: 120, y: 149 }));
        assert!(inside_any(&rects, PhysPoint { x: 0, y: 0 }));
        assert!(!inside_any(&rects, PhysPoint { x: 150, y: 120 }));
        assert!(!inside_any(&[], PhysPoint { x: 1, y: 1 }));
    }

    fn css(x: f64, y: f64, w: f64, h: f64) -> CssRect {
        CssRect { x, y, w, h }
    }

    /// The page's "Skip it" button at CSS (300, 500) 120 x 48, in the middle of the left half.
    const SKIP: (f64, f64, f64, f64) = (300.0, 500.0, 120.0, 48.0);

    #[test]
    fn hit_test_at_100_percent() {
        let o = PhysPoint { x: 0, y: 0 };
        let r = css_to_phys(&[css(SKIP.0, SKIP.1, SKIP.2, SKIP.3)], o, 1.0);
        assert_eq!(r, vec![PhysRect { x: 300, y: 500, w: 120, h: 48 }]);
        assert!(inside_any(&r, PhysPoint { x: 360, y: 524 }), "the button's middle");
        assert!(inside_any(&r, PhysPoint { x: 300, y: 500 }) && inside_any(&r, PhysPoint { x: 419, y: 547 }));
        assert!(!inside_any(&r, PhysPoint { x: 420, y: 524 }), "right of it: the page");
    }

    #[test]
    fn hit_test_at_125_percent() {
        let o = PhysPoint { x: 0, y: 0 };
        let r = css_to_phys(&[css(SKIP.0, SKIP.1, SKIP.2, SKIP.3)], o, 1.25);
        assert_eq!(r, vec![PhysRect { x: 375, y: 625, w: 150, h: 60 }]);
        // The button's middle on the screen is at 1.25 x its CSS middle (450, 655), not at (360, 524).
        assert!(inside_any(&r, PhysPoint { x: 450, y: 655 }));
        assert!(!inside_any(&r, PhysPoint { x: 360, y: 524 }), "CSS px taken as screen px miss the button");
        // Fractional edges round outward: x 10.3 -> 12.875 .. 25.75 covers pixels 12..=25.
        let r = css_to_phys(&[css(10.3, 0.0, 10.3, 4.0)], o, 1.25);
        assert_eq!(r, vec![PhysRect { x: 12, y: 0, w: 14, h: 5 }]);
    }

    #[test]
    fn hit_test_with_a_monitor_origin() {
        // The overlay on a monitor left of the primary one, or below a top taskbar: its top-left is not 0,0.
        for (o, scale) in [(PhysPoint { x: -1920, y: 0 }, 1.0), (PhysPoint { x: 0, y: 40 }, 1.25)] {
            let r = css_to_phys(&[css(SKIP.0, SKIP.1, SKIP.2, SKIP.3)], o, scale);
            let mid = PhysPoint {
                x: o.x + ((SKIP.0 + SKIP.2 / 2.0) * scale) as i32,
                y: o.y + ((SKIP.1 + SKIP.3 / 2.0) * scale) as i32,
            };
            assert!(inside_any(&r, mid), "{o:?} {scale}: {r:?}");
            let without_origin = PhysPoint { x: mid.x - o.x, y: mid.y - o.y };
            assert!(!inside_any(&r, without_origin), "{o:?}: the rect must move with the window");
        }
    }

    #[test]
    fn live_window_position_wins_over_the_pages_copy() {
        // The page computed its screen rects with a window at 0,0; the window really sits at 0,40.
        let page = vec![PhysRect { x: 300, y: 500, w: 120, h: 48 }];
        let c = [css(SKIP.0, SKIP.1, SKIP.2, SKIP.3)];
        let r = resolve_hit_rects(&c, 1.0, Some(PhysPoint { x: 0, y: 40 }), page.clone());
        assert_eq!(r, vec![PhysRect { x: 300, y: 540, w: 120, h: 48 }]);
        // No live position, or an absurd scale: the page's own rects.
        assert_eq!(resolve_hit_rects(&c, 1.0, None, page.clone()), page);
        assert_eq!(resolve_hit_rects(&c, f64::NAN, Some(PhysPoint { x: 0, y: 0 }), page.clone()), page);
        // Broken page numbers are dropped, not turned into giant rects.
        assert!(
            css_to_phys(&[css(f64::NAN, 0.0, 5.0, 5.0), css(0.0, 0.0, 0.0, 5.0)], PhysPoint { x: 0, y: 0 }, 1.0)
                .is_empty()
        );
    }

    #[test]
    fn sanitize_drops_empty() {
        let r = sanitize(vec![PhysRect { x: 0, y: 0, w: 0, h: 5 }, PhysRect { x: 1, y: 1, w: 5, h: 5 }]);
        assert_eq!(r.len(), 1);
    }

    use MouseEv::{LeftDown, LeftUp, Move};

    #[test]
    fn click_through_follows_the_cursor() {
        let mut g = ClickGuard::default();
        // onto a bubble while the window lets clicks through: take clicks
        assert_eq!(g.decide(Move, false, true, true), Verdict::Pass(Some(false)));
        // still on it, bit already off: nothing to write
        assert_eq!(g.decide(Move, false, true, false), Verdict::Pass(None));
        // off the bubble: let clicks through again
        assert_eq!(g.decide(Move, false, false, false), Verdict::Pass(Some(true)));
        assert_eq!(g.decide(Move, false, false, true), Verdict::Pass(None));
        // the agent's own cursor jumps count the same (it moves over Pip too)
        assert_eq!(g.decide(Move, true, true, true), Verdict::Pass(Some(false)));
    }

    #[test]
    fn a_press_on_a_bubble_that_would_fall_through_is_sent_again() {
        let mut g = ClickGuard::default();
        // "Continue" pressed while the bit is still set (it lagged): swallow, take clicks, send again
        assert_eq!(g.decide(LeftDown, false, true, true), Verdict::Rescue);
        assert_eq!(g.resend(), Resend::Down);
        // the press Pip sent again passes (window takes clicks now)
        assert_eq!(g.decide(LeftDown, true, true, false), Verdict::Pass(None));
        // the person's release passes and ends the rescue
        assert_eq!(g.decide(LeftUp, false, true, false), Verdict::Pass(None));
        assert_eq!(g.resend(), Resend::Nothing);
    }

    #[test]
    fn a_quick_release_before_the_press_went_out_is_held_back() {
        let mut g = ClickGuard::default();
        assert_eq!(g.decide(LeftDown, false, true, true), Verdict::Rescue);
        // the release arrives first: hold it, both go out in order
        assert_eq!(g.decide(LeftUp, false, true, false), Verdict::HoldUp);
        assert_eq!(g.resend(), Resend::DownUp);
        assert_eq!(g.decide(LeftDown, true, true, false), Verdict::Pass(None));
        assert_eq!(g.decide(LeftUp, true, true, false), Verdict::Pass(None));
        // nothing owed any more: a later release passes
        assert_eq!(g.decide(LeftUp, false, true, false), Verdict::Pass(None));
        assert_eq!(g.resend(), Resend::Nothing);
    }

    #[test]
    fn presses_that_need_no_rescue_pass() {
        let mut g = ClickGuard::default();
        // on the bubble, window already takes clicks
        assert_eq!(g.decide(LeftDown, false, true, false), Verdict::Pass(None));
        // on the page, click-through on
        assert_eq!(g.decide(LeftDown, false, false, true), Verdict::Pass(None));
        // on the page but the bit is off (a stale state): turn it on first, the press still passes
        assert_eq!(g.decide(LeftDown, false, false, false), Verdict::Pass(Some(true)));
        // the agent's own press on a rect is never swallowed
        assert_eq!(g.decide(LeftDown, true, true, true), Verdict::Pass(Some(false)));
        assert_eq!(g.resend(), Resend::Nothing);
        for _ in 0..3 {
            assert_eq!(g.decide(LeftUp, false, false, true), Verdict::Pass(None));
        }
    }

    #[test]
    fn a_press_sent_again_that_slipped_off_pip_never_reaches_the_page() {
        let mut g = ClickGuard::default();
        assert_eq!(g.decide(LeftDown, false, true, true), Verdict::Rescue);
        assert_eq!(g.resend(), Resend::Down);
        // the cursor left the bubble's edge in the meantime: Pip's own press is swallowed, not let through
        assert_eq!(g.decide(LeftDown, true, false, false), Verdict::Drop);
        // the person's release passes (a lone release does nothing on a page) and ends it
        assert_eq!(g.decide(LeftUp, false, false, false), Verdict::Pass(None));
        assert_eq!(g.resend(), Resend::Nothing);
        // same with press and release sent together: both are swallowed
        assert_eq!(g.decide(LeftDown, false, true, true), Verdict::Rescue);
        assert_eq!(g.decide(LeftUp, false, true, false), Verdict::HoldUp);
        assert_eq!(g.resend(), Resend::DownUp);
        assert_eq!(g.decide(LeftDown, true, false, false), Verdict::Drop);
        assert_eq!(g.decide(LeftUp, true, false, false), Verdict::Drop);
        // and the guard is clean again
        assert_eq!(g.decide(LeftDown, false, false, true), Verdict::Pass(None));
        assert_eq!(g.decide(LeftUp, false, false, true), Verdict::Pass(None));
    }

    #[test]
    fn a_new_press_forgets_an_old_rescue() {
        let mut g = ClickGuard::default();
        assert_eq!(g.decide(LeftDown, false, true, true), Verdict::Rescue);
        assert_eq!(g.resend(), Resend::Down);
        // the release got lost somewhere; the next press on the page is an ordinary one
        assert_eq!(g.decide(LeftDown, false, false, true), Verdict::Pass(None));
        assert_eq!(g.decide(LeftUp, false, false, true), Verdict::Pass(None));
        assert_eq!(g.resend(), Resend::Nothing);
    }

    #[test]
    fn the_first_style_mismatch_is_reported() {
        let mut c = SyncCheck::default();
        assert_eq!(c.see(true, Some(true)), StyleSync::InStep);
        assert_eq!(c.see(true, None), StyleSync::InStep, "unknown is not a mismatch");
        assert_eq!(c.see(false, Some(true)), StyleSync::FirstMismatch, "warned at once, not after two in a row");
        assert_eq!(c.see(false, Some(false)), StyleSync::InStep);
        assert_eq!(c.see(true, Some(false)), StyleSync::Mismatch(2));
    }

    fn feed_path(w: &mut MouseWatch, t0: Instant, start_ms: u64, pts: &[(i32, i32)]) -> bool {
        let mut hit = false;
        for (i, (x, y)) in pts.iter().enumerate() {
            let now = t0 + Duration::from_millis(start_ms + 33 * i as u64);
            hit |= w.feed(PhysPoint { x: *x, y: *y }, true, now);
        }
        hit
    }

    #[test]
    fn a_deliberate_fast_move_is_noticed() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        assert!(!w.feed(PhysPoint { x: 500, y: 500 }, true, t0));
        // a hand gliding across the screen after the grace time: 150 px in about 130 ms
        assert!(feed_path(&mut w, t0, 2100, &[(500, 500), (530, 505), (570, 512), (610, 520), (650, 530)]));
    }

    #[test]
    fn a_small_nudge_is_not_a_stop() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 500, y: 500 }, true, t0);
        // the old threshold (60 px) stopped Pip here: a short slide of ~70 px
        assert!(!feed_path(&mut w, t0, 2100, &[(500, 500), (520, 505), (545, 512), (570, 520)]));
        // the hand rests afterwards, then nudges again only much later: still no stop
        assert!(!feed_path(&mut w, t0, 2300, &[(570, 520), (570, 520), (570, 520)]));
        assert!(!feed_path(&mut w, t0, 9000, &[(570, 520), (580, 524), (590, 528)]));
    }

    #[test]
    fn moving_again_a_second_later_is_noticed() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 500, y: 500 }, true, t0);
        // two slow motions (~20 px each), 1.2 s apart
        assert!(!feed_path(&mut w, t0, 2100, &[(500, 500), (507, 500), (514, 500), (520, 500)]));
        assert!(!feed_path(&mut w, t0, 2300, &[(520, 500), (520, 500)]));
        assert!(feed_path(&mut w, t0, 3300, &[(520, 500), (527, 500), (534, 500), (540, 500)]));
    }

    #[test]
    fn agent_jumps_and_jitter_are_not_a_person() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 100, y: 100 }, true, t0);
        // the agent clicks: one jump, then the cursor rests (repeatedly, more than a second apart)
        assert!(!feed_path(&mut w, t0, 2100, &[(900, 700), (900, 700), (900, 700), (300, 200), (300, 200)]));
        assert!(!feed_path(&mut w, t0, 3500, &[(800, 100), (800, 100), (800, 100)]));
        assert!(!feed_path(&mut w, t0, 5000, &[(100, 600), (100, 600), (100, 600)]));
        // a resting hand: 1 px jitter
        assert!(!feed_path(&mut w, t0, 6000, &[(100, 601), (101, 601), (101, 600), (100, 600), (101, 601)]));
    }

    #[test]
    fn quiet_time_after_yes() {
        let ms = MouseStop {
            armed: Arc::new(AtomicBool::new(true)),
            quiet_until: Arc::new(Mutex::new(None)),
            on_user_move: Box::new(|| {}),
        };
        let t0 = Instant::now();
        assert!(ms.watching(t0));
        *ms.quiet_until.lock().unwrap() = Some(t0 + Duration::from_secs(5));
        assert!(!ms.watching(t0 + Duration::from_secs(1)));
        assert!(ms.watching(t0 + Duration::from_secs(6)));
        ms.armed.store(false, Ordering::SeqCst);
        assert!(!ms.watching(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn grace_time_and_disarm() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 0, y: 0 }, true, t0);
        // the user still moves away from the "Ja" button right after clicking it
        assert!(!feed_path(&mut w, t0, 100, &[(30, 0), (60, 0), (90, 0), (120, 0), (150, 0)]));
        // not armed (card open, Pip idle): never fires
        let mut w = MouseWatch::default();
        for i in 0..200 {
            assert!(!w.feed(PhysPoint { x: i * 20, y: 0 }, false, t0 + Duration::from_millis(33 * i as u64)));
        }
    }
}
