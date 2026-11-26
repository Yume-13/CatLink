// MIT License
//
// Copyright © 2026 Yume-13
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! Tray icon + flyout UI (Win32). Runs on the main thread; the engine runs on its own
//! tokio runtime. The UI reads its status and sends it commands.

pub mod flyout;
pub mod gdi;
pub mod glyph;
pub mod preview;
pub mod theme;

use catlink_core::engine::{Engine, Severity, State, human_duration, human_rate};
use flyout::{Fonts, HISTORY, Hit, ViewModel};
use gdi::{hicon_from_pixmap, wide};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;
use theme::Theme;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::w;

const WM_TRAY: u32 = WM_APP + 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1; // NIN_SELECT | NINF_KEY
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Posted by a second instance: show the flyout.
pub const WM_SHOW_FLYOUT: u32 = WM_APP + 2;
const TIMER_SAMPLE: usize = 1;
const TRAY_ID: u32 = 1;

const MENU_SHOW: usize = 10;
const MENU_PAUSE: usize = 11;
const MENU_LOG: usize = 12;
const MENU_QUIT: usize = 13;
const MENU_PAIR: usize = 14;
const MENU_WIFI: usize = 15;

pub const MAIN_CLASS: windows_sys::core::PCWSTR = w!("CatLinkTrayWindow");
const FLYOUT_CLASS: windows_sys::core::PCWSTR = w!("CatLinkFlyout");

struct App {
    engine: Arc<Engine>,
    main: HWND,
    flyout: HWND,
    flyout_scale: f32,
    fonts: Option<Fonts>,
    theme: Theme,
    taskbar_dark: bool,
    hover: Hit,
    /// When a deliberate deactivation (a click elsewhere) last hid the flyout.
    hidden_at: Option<Instant>,
    /// When the flyout was last shown.
    shown_at: Option<Instant>,
    /// Opened by double-clicking the icon: stays open and can be dragged.
    pinned: bool,
    history: VecDeque<(f64, f64)>,
    last_bytes: (u64, u64),
    last_sample: Instant,
    icon: HICON,
    icon_key: Option<(Severity, u8, bool, u32)>,
    tip: String,
    last_state: State,
    taskbar_created: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    for (i, c) in s.encode_utf16().take(dst.len() - 1).enumerate() {
        dst[i] = c;
        n = i + 1;
    }
    dst[n] = 0;
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

/// Lets popup menus follow the dark theme via undocumented uxtheme ordinals 135/136;
/// no-op where unavailable.
fn enable_dark_menus() {
    unsafe {
        let ux = LoadLibraryW(w!("uxtheme.dll"));
        if ux.is_null() {
            return;
        }
        if let Some(f) = GetProcAddress(ux, 135 as _) {
            let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_preferred_app_mode(1); // AllowDark
        }
        if let Some(f) = GetProcAddress(ux, 136 as _) {
            let flush_menu_themes: extern "system" fn() = std::mem::transmute(f);
            flush_menu_themes();
        }
    }
}

/// Utilization: 0 below 2 KB/s, otherwise 1..=5 on a log scale up to ~20 MB/s.
fn level_for(rate: f64) -> u8 {
    if rate < 2048.0 {
        return 0;
    }
    let x = (1.0 + rate / 16384.0).log2() / (1.0 + 20.0 * 1048576.0 / 16384.0f64).log2();
    (1.0 + x.clamp(0.0, 1.0) * 4.0).round() as u8
}

impl App {
    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.main;
        nid.uID = TRAY_ID;
        nid
    }

    fn add_tray(&mut self) {
        let mut nid = self.tray_data();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = self.icon;
        copy_wide(&mut nid.szTip, &self.tip);
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    fn remove_tray(&self) {
        let nid = self.tray_data();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    fn notify(&self, title: &str, text: &str) {
        let mut nid = self.tray_data();
        nid.uFlags = NIF_INFO;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, text);
        nid.dwInfoFlags = NIIF_NONE | NIIF_RESPECT_QUIET_TIME;
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Once a second: sample throughput, refresh icon/tooltip, notify on state changes.
    fn tick(&mut self) {
        let sh = &self.engine.shared;
        let (up, down) = (sh.bytes_up.load(Relaxed), sh.bytes_down.load(Relaxed));
        let dt = self.last_sample.elapsed().as_secs_f64().max(0.2);
        let rate = (
            down.saturating_sub(self.last_bytes.1) as f64 / dt,
            up.saturating_sub(self.last_bytes.0) as f64 / dt,
        );
        self.last_bytes = (up, down);
        self.last_sample = Instant::now();
        let status = self.engine.status();
        if status.state == State::Connected || !self.history.is_empty() {
            self.history.push_back(rate);
            while self.history.len() > HISTORY {
                self.history.pop_front();
            }
        }

        // Messages from the engine (switching to Wi-Fi, ...).
        for text in self.engine.take_events() {
            self.notify("CatLink", &text);
        }

        // Balloon notifications on connect / disconnect.
        if status.state != self.last_state {
            let was = self.last_state == State::Connected;
            let is = status.state == State::Connected;
            if is && !was {
                let phone = status.phone.as_ref().map(|p| p.model.clone()).unwrap_or_default();
                let via = status.phone.as_ref().map(|p| p.via()).unwrap_or_default();
                self.notify(&format!("Connected to {phone}"), &format!("{via}."));
            } else if was && !is && status.state != State::Paused {
                self.notify("Phone disconnected", "This PC is back on its own network.");
            }
            self.last_state = status.state.clone();
        }

        self.update_icon(status.state.severity(), level_for(rate.0 + rate.1));
        let tip = match status.state {
            State::Connected => format!(
                "CatLink \u{2013} Connected\n\u{2193} {}   \u{2191} {}\n{}",
                human_rate(rate.0),
                human_rate(rate.1),
                status.phone.as_ref().map(|p| p.via()).unwrap_or_default()
            ),
            ref s => format!("CatLink \u{2013} {}\n{}", s.title(), s.hint()),
        };
        if tip != self.tip {
            self.tip = tip;
            let mut nid = self.tray_data();
            nid.uFlags = NIF_TIP | NIF_SHOWTIP;
            copy_wide(&mut nid.szTip, &self.tip);
            unsafe {
                Shell_NotifyIconW(NIM_MODIFY, &nid);
            }
        }
        if !self.flyout.is_null() && unsafe { IsWindowVisible(self.flyout) } != 0 {
            unsafe {
                InvalidateRect(self.flyout, std::ptr::null(), 0);
            }
        }
    }

    fn update_icon(&mut self, sev: Severity, level: u8) {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
        let key = (sev, level, self.taskbar_dark, size);
        if self.icon_key == Some(key) {
            return;
        }
        let Some(mut pm) = tiny_skia::Pixmap::new(size, size) else { return };
        let (fg, fill) = if self.taskbar_dark { (0xffffff, 0x5aa2f0) } else { (0x1b1d21, 0x2a78d6) };
        let badge = match sev {
            Severity::Good => Some(theme::GOOD),
            Severity::Warning => Some(theme::WARNING),
            Severity::Critical => Some(theme::CRITICAL),
            Severity::Off => None,
        };
        let fg = if sev == Severity::Off { glyph::rgba(fg, 0.55) } else { glyph::rgb(fg) };
        glyph::tray_glyph(&mut pm, size as f32, fg, glyph::rgb(fill), level as f32 / 5.0, badge.map(glyph::rgb));
        let icon = hicon_from_pixmap(&pm);
        if icon.is_null() {
            return;
        }
        let mut nid = self.tray_data();
        nid.uFlags = NIF_ICON;
        nid.hIcon = icon;
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
        }
        self.icon = icon;
        self.icon_key = Some(key);
    }

    fn view_model(&self) -> ViewModel {
        let st = self.engine.status();
        let sh = &self.engine.shared;
        let (headline, detail) = match &st.state {
            _ if st.pairing.is_some() => (
                "Pair a phone over Wi-Fi".to_string(),
                "On the phone: Developer options › Wireless debugging › Pair device with QR code."
                    .to_string(),
            ),
            State::Connected => {
                let p = st.phone.clone().unwrap_or_default();
                let mut d = p.via();
                if st.wireless {
                    d.push_str(" \u{00b7} over Wi-Fi");
                }
                if let Some(t) = st.connected_since {
                    d.push_str(&format!(" \u{00b7} for {}", human_duration(t.elapsed())));
                }
                (p.summary(), d)
            }
            s => {
                let h = match s {
                    State::NoDevice => "No phone connected",
                    State::Unauthorized => "Your phone needs to approve this PC",
                    State::Paused => "Sharing is paused",
                    State::AppMissing => "The CatLink app isn\u{2019}t installed",
                    State::AppNotResponding => "The CatLink app isn\u{2019}t responding",
                    State::MultipleDevices(_) => "More than one phone is connected",
                    State::Error(_) => "Something went wrong",
                    _ => "Connecting to your phone\u{2026}",
                };
                (h.to_string(), s.hint())
            }
        };
        ViewModel {
            severity: st.state.severity(),
            state: st.state,
            headline,
            detail,
            history: self.history.iter().copied().collect(),
            open_flows: sh.tcp_active.load(Relaxed) + sh.udp_active.load(Relaxed),
            latency_ms: st.latency_ms,
            total_down: sh.bytes_down.load(Relaxed),
            total_up: sh.bytes_up.load(Relaxed),
            enabled: self.engine.is_enabled(),
            pinned: self.pinned,
            pairing: st.pairing,
        }
    }

    fn apply_flyout_theme(&self) {
        if self.flyout.is_null() {
            return;
        }
        unsafe {
            let dark: i32 = self.theme.dark as i32;
            DwmSetWindowAttribute(self.flyout, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const _ as _, 4);
            let corner: i32 = DWMWCP_ROUND;
            DwmSetWindowAttribute(self.flyout, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &corner as *const _ as _, 4);
            let border = theme::colorref(self.theme.border);
            DwmSetWindowAttribute(self.flyout, DWMWA_BORDER_COLOR as u32, &border as *const _ as _, 4);
        }
    }
}

/// Where to put the flyout: next to the tray, inside the work area of that monitor.
fn flyout_rect(anchor: POINT) -> (i32, i32, i32, i32, f32) {
    unsafe {
        let mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let s = dx as f32 / 96.0;
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        let (work, full) = (mi.rcWork, mi.rcMonitor);
        let w = (flyout::WIDTH * s).round() as i32;
        let h = (flyout::HEIGHT * s).round() as i32;
        let m = (12.0 * s) as i32;
        let clamp_x = |x: i32| x.clamp(work.left + m, work.right - w - m);
        let clamp_y = |y: i32| y.clamp(work.top + m, work.bottom - h - m);
        let (x, y) = if work.bottom < full.bottom {
            (clamp_x(anchor.x - w / 2), work.bottom - h - m)
        } else if work.top > full.top {
            (clamp_x(anchor.x - w / 2), work.top + m)
        } else if work.left > full.left {
            (work.left + m, clamp_y(anchor.y - h / 2))
        } else if work.right < full.right {
            (work.right - w - m, clamp_y(anchor.y - h / 2))
        } else {
            (work.right - w - m, work.bottom - h - m)
        };
        (x, y, w, h, s)
    }
}

/// Whether the flyout was shown so recently that this is still the same click gesture
/// (the second half of a double-click, or the repeated select that Enter sends).
fn just_shown(a: &App) -> bool {
    let dbl = unsafe { GetDoubleClickTime() } as u128;
    a.shown_at.is_some_and(|t| t.elapsed().as_millis() < dbl.max(300))
}

fn show_flyout(anchor: POINT) {
    // A click on the icon while the flyout is open first deactivates (hides) it;
    // skip reopening in that case.
    let skip = with_app(|a| a.hidden_at.is_some_and(|t| t.elapsed().as_millis() < 300)).unwrap_or(false);
    if skip {
        return;
    }
    let Some((hwnd, recent)) = with_app(|a| (a.flyout, just_shown(a))) else { return };
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        if recent {
            // Same gesture: keep it open and focused.
            unsafe { SetForegroundWindow(hwnd) };
        } else {
            // Clicking the icon again closes it (pinned or not).
            close_flyout();
        }
        return;
    }
    open_flyout(hwnd, anchor, false);
}

/// "Show status" from the menu, or catlink.exe started again: show it, never toggle it off.
fn reveal_flyout(anchor: POINT) {
    let Some(hwnd) = with_app(|a| a.flyout) else { return };
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        unsafe { SetForegroundWindow(hwnd) };
    } else {
        open_flyout(hwnd, anchor, false);
    }
}

/// Double-click on the icon: open the flyout pinned, or pin the one the first click opened.
fn pin_flyout(anchor: POINT) {
    let Some(hwnd) = with_app(|a| {
        a.pinned = true;
        a.flyout
    }) else {
        return;
    };
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        unsafe {
            SetForegroundWindow(hwnd);
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    } else {
        open_flyout(hwnd, anchor, true);
    }
}

fn open_flyout(hwnd: HWND, anchor: POINT, pinned: bool) {
    let (x, y, w, h, s) = flyout_rect(anchor);
    with_app(|a| {
        set_flyout_scale(a, s);
        a.hover = Hit::None;
        a.shown_at = Some(Instant::now());
        a.hidden_at = None;
        a.pinned = pinned;
    });
    unsafe {
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn set_flyout_scale(a: &mut App, s: f32) {
    if a.fonts.is_none() || (a.flyout_scale - s).abs() > 0.01 {
        a.fonts = Some(Fonts::new(s));
        a.flyout_scale = s;
    }
}

/// Hide a transient flyout; a pinned one stays (used when the tray menu opens).
fn hide_flyout() {
    if let Some((hwnd, false)) = with_app(|a| (a.flyout, a.pinned)) {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

/// Close the flyout whether it's pinned or not (this also cancels a Wi-Fi pairing).
fn close_flyout() {
    if let Some(hwnd) = with_app(|a| {
        a.pinned = false;
        a.hidden_at = Some(Instant::now());
        a.engine.cancel_pairing();
        a.flyout
    }) {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

/// "Pair over Wi-Fi…": show the QR code in a pinned flyout.
fn start_pairing(anchor: POINT) {
    with_app(|a| a.engine.start_pairing());
    pin_flyout(anchor);
}

/// The left button: Pause/Resume normally, Cancel/Done while pairing.
fn left_button() {
    let pairing = with_app(|a| {
        let p = a.engine.status().pairing.is_some();
        if p {
            a.engine.cancel_pairing();
        }
        p
    })
    .unwrap_or(false);
    if pairing {
        if let Some(f) = with_app(|a| a.flyout) {
            unsafe { InvalidateRect(f, std::ptr::null(), 0) };
        }
    } else {
        toggle_pause();
    }
}

fn toggle_pause() {
    with_app(|a| {
        let on = a.engine.is_enabled();
        a.engine.set_enabled(!on);
        if !on {
            a.history.clear();
        }
    });
    if let Some(f) = with_app(|a| a.flyout) {
        unsafe { InvalidateRect(f, std::ptr::null(), 0) };
    }
}

fn open_log() {
    let p = wide(&catlink_core::logfile::path().display().to_string());
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), w!("open"), p.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

fn quit() {
    log::info!("quit requested");
    let Some((engine, main)) = with_app(|a| {
        a.remove_tray();
        (a.engine.clone(), a.main)
    }) else {
        return;
    };
    hide_flyout();
    engine.shutdown();
    unsafe {
        DestroyWindow(main);
    }
}

fn show_menu(anchor: POINT) {
    let Some((main, st, enabled)) = with_app(|a| (a.main, a.engine.status(), a.engine.is_enabled())) else {
        return;
    };
    let state = st.state.clone();
    unsafe {
        let menu = CreatePopupMenu();
        let header = wide(&format!(
            "CatLink \u{2013} {}{}",
            state.title(),
            if state == State::Connected && st.wireless { " over Wi-Fi" } else { "" }
        ));
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, header.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_SHOW, w!("Show status"));
        let pause = if enabled { w!("Pause sharing") } else { w!("Resume sharing") };
        AppendMenuW(menu, MF_STRING, MENU_PAUSE, pause);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_PAIR, w!("Pair a phone over Wi-Fi\u{2026}"));
        // adb tcpip requires an active USB connection.
        let usb = state == State::Connected && !st.wireless;
        AppendMenuW(menu, MF_STRING | if usb { 0 } else { MF_GRAYED }, MENU_WIFI, w!("Switch this phone to Wi-Fi"));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_LOG, w!("Open log file"));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit CatLink"));
        SetMenuDefaultItem(menu, MENU_SHOW as u32, 0);
        SetForegroundWindow(main);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN, anchor.x, anchor.y, 0, main, std::ptr::null());
        PostMessageW(main, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match cmd as usize {
            MENU_SHOW => reveal_flyout(anchor),
            MENU_PAUSE => toggle_pause(),
            MENU_LOG => open_log(),
            MENU_PAIR => start_pairing(anchor),
            MENU_WIFI => {
                with_app(|a| a.engine.switch_to_wifi());
            }
            MENU_QUIT => quit(),
            _ => {}
        }
    }
}

fn point_from(wparam: WPARAM) -> POINT {
    POINT { x: (wparam & 0xffff) as i16 as i32, y: ((wparam >> 16) & 0xffff) as i16 as i32 }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam & 0xffff) as u32;
            let anchor = point_from(wparam);
            match event {
                // Version 4 icons send WM_LBUTTONUP *and* NIN_SELECT for one click, so only the
                // NIN_ events open the flyout; a double-click pins it.
                NIN_SELECT | NIN_KEYSELECT => show_flyout(anchor),
                WM_LBUTTONDBLCLK => pin_flyout(anchor),
                WM_CONTEXTMENU => {
                    hide_flyout();
                    show_menu(anchor);
                }
                _ => {}
            }
            0
        }
        WM_SHOW_FLYOUT => {
            let mut p = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut p) };
            reveal_flyout(p);
            0
        }
        WM_TIMER if wparam == TIMER_SAMPLE => {
            with_app(|a| a.tick());
            0
        }
        WM_SETTINGCHANGE => {
            with_app(|a| {
                a.theme = Theme::current();
                a.taskbar_dark = theme::taskbar_dark();
                a.icon_key = None;
                a.apply_flyout_theme();
            });
            enable_dark_menus();
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => {
            if let Some(tc) = with_app(|a| a.taskbar_created)
                && msg == tc
            {
                // Explorer restarted: put the icon back.
                with_app(|a| a.add_tray());
                return 0;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn mouse_xy(lparam: LPARAM, s: f32) -> (f32, f32) {
    let x = (lparam & 0xffff) as i16 as f32 / s;
    let y = ((lparam >> 16) & 0xffff) as i16 as f32 / s;
    (x, y)
}

unsafe extern "system" fn flyout_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            APP.with(|a| {
                if let Ok(g) = a.try_borrow()
                    && let Some(app) = g.as_ref()
                    && let Some(fonts) = app.fonts.as_ref()
                {
                    let vm = app.view_model();
                    flyout::paint_flyout(dc, app.flyout_scale, &app.theme, fonts, &vm, app.hover);
                }
            });
            unsafe { EndPaint(hwnd, &ps) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let changed = with_app(|a| {
                let (x, y) = mouse_xy(lparam, a.flyout_scale);
                let hit = flyout::hit_test(x, y, a.history.len(), a.pinned);
                let changed = hit != a.hover;
                a.hover = hit;
                changed
            })
            .unwrap_or(false);
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                TrackMouseEvent(&mut tme);
                if changed {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.hover = Hit::None);
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
            0
        }
        WM_SETCURSOR => {
            let over_button = with_app(|a| matches!(a.hover, Hit::Pause | Hit::Quit | Hit::Close)).unwrap_or(false);
            unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), if over_button { IDC_HAND } else { IDC_ARROW })) };
            1
        }
        WM_LBUTTONUP => {
            let hit = with_app(|a| {
                let (x, y) = mouse_xy(lparam, a.flyout_scale);
                flyout::hit_test(x, y, a.history.len(), a.pinned)
            });
            log::debug!("flyout click: {hit:?}");
            match hit {
                Some(Hit::Pause) => left_button(),
                Some(Hit::Quit) => quit(),
                Some(Hit::Close) => close_flyout(),
                _ => {}
            }
            0
        }
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            close_flyout();
            0
        }
        WM_NCHITTEST => {
            // A pinned window drags by its top area (like a title bar).
            let drag = with_app(|a| {
                let mut p = POINT { x: (lparam & 0xffff) as i16 as i32, y: ((lparam >> 16) & 0xffff) as i16 as i32 };
                a.pinned && unsafe { ScreenToClient(hwnd, &mut p) } != 0 && {
                    flyout::is_drag_area(p.x as f32 / a.flyout_scale, p.y as f32 / a.flyout_scale)
                }
            })
            .unwrap_or(false);
            if drag { HTCAPTION as LRESULT } else { unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } }
        }
        WM_DPICHANGED => {
            // Dragged onto a monitor with a different scale: redraw at the new size.
            let s = (wparam & 0xffff) as f32 / 96.0;
            with_app(|a| set_flyout_scale(a, s));
            let r = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_ACTIVATE => {
            if (wparam & 0xffff) as u32 == WA_INACTIVE {
                let pinned = with_app(|a| {
                    // Focus loss right after opening (taskbar during a double-click) is not recorded.
                    if !a.pinned && !just_shown(a) {
                        a.hidden_at = Some(Instant::now());
                    }
                    a.hover = Hit::None;
                    a.pinned
                })
                .unwrap_or(false);
                unsafe {
                    if pinned {
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    } else {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Create the windows and tray icon, then run the message loop until Quit.
pub fn run(engine: Arc<Engine>) {
    enable_dark_menus();
    unsafe {
        let hinst = instance();
        let app_icon = LoadIconW(hinst, 1 as _);
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(main_proc);
        wc.hInstance = hinst;
        wc.hIcon = app_icon;
        wc.lpszClassName = MAIN_CLASS;
        RegisterClassExW(&wc);

        let mut fc: WNDCLASSEXW = std::mem::zeroed();
        fc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        fc.style = CS_DROPSHADOW;
        fc.lpfnWndProc = Some(flyout_proc);
        fc.hInstance = hinst;
        fc.hIcon = app_icon;
        fc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        fc.lpszClassName = FLYOUT_CLASS;
        RegisterClassExW(&fc);

        let main = CreateWindowExW(0, MAIN_CLASS, w!("CatLink"), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        let flyout = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            FLYOUT_CLASS,
            w!("CatLink"),
            WS_POPUP,
            0,
            0,
            360,
            436,
            main,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );

        let sh = engine.shared.clone();
        let app = App {
            engine,
            main,
            flyout,
            flyout_scale: 1.0,
            fonts: None,
            theme: Theme::current(),
            taskbar_dark: theme::taskbar_dark(),
            hover: Hit::None,
            hidden_at: None,
            shown_at: None,
            pinned: false,
            history: VecDeque::with_capacity(HISTORY + 1),
            last_bytes: (sh.bytes_up.load(Relaxed), sh.bytes_down.load(Relaxed)),
            last_sample: Instant::now(),
            icon: std::ptr::null_mut(),
            icon_key: None,
            tip: "CatLink \u{2013} Starting".into(),
            last_state: State::Starting,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
        };
        APP.with(|a| *a.borrow_mut() = Some(app));
        with_app(|a| {
            a.update_icon(Severity::Warning, 0);
            a.add_tray();
            a.apply_flyout_theme();
        });
        SetTimer(main, TIMER_SAMPLE, 1000, None);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        with_app(|a| {
            if !a.icon.is_null() {
                DestroyIcon(a.icon);
            }
        });
    }
}

/// Simple message box for startup errors (before the tray exists).
pub fn error_box(text: &str) {
    let t = wide(text);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), w!("CatLink"), MB_OK | MB_ICONERROR);
    }
}
