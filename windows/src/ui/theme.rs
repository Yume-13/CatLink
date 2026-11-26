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

//! Colors for the flyout and tray icon, following the Windows light/dark setting.
//! Series colors: blue = download, aqua = upload.

use crate::ui::glyph::{rgb, rgba};
use catlink_core::engine::Severity;
use tiny_skia::Color;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: u32,
    pub card: u32,
    pub border: u32,
    pub text: u32,
    pub text2: u32,
    pub muted: u32,
    pub grid: u32,
    pub down: u32,
    pub up: u32,
    pub button: u32,
    pub button_hover: u32,
    pub button_border: u32,
}

pub const GOOD: u32 = 0x0ca30c;
pub const WARNING: u32 = 0xfab219;
pub const CRITICAL: u32 = 0xd03b3b;

impl Theme {
    pub fn current() -> Theme {
        if apps_dark() { Theme::dark() } else { Theme::light() }
    }

    pub fn light() -> Theme {
        Theme {
            dark: false,
            bg: 0xf3f4f6,
            card: 0xffffff,
            border: 0xdfe1e5,
            text: 0x17191c,
            text2: 0x4b5058,
            muted: 0x6b7079,
            grid: 0xe8eaed,
            down: 0x2a78d6,
            up: 0x1baf7a,
            button: 0xffffff,
            button_hover: 0xeef0f3,
            button_border: 0xd5d8dd,
        }
    }

    pub fn dark() -> Theme {
        Theme {
            dark: true,
            bg: 0x1f2023,
            card: 0x2b2d31,
            border: 0x3a3d42,
            text: 0xf2f3f5,
            text2: 0xc4c7cc,
            muted: 0x9a9ea6,
            grid: 0x3a3d42,
            down: 0x3987e5,
            up: 0x199e70,
            button: 0x34373c,
            button_hover: 0x3f4248,
            button_border: 0x464a50,
        }
    }

    pub fn status(&self, s: Severity) -> u32 {
        match s {
            Severity::Good => GOOD,
            Severity::Warning => WARNING,
            Severity::Critical => CRITICAL,
            Severity::Off => self.muted,
        }
    }

    pub fn c(&self, hex: u32) -> Color {
        rgb(hex)
    }

    pub fn ca(&self, hex: u32, a: f32) -> Color {
        rgba(hex, a)
    }
}

/// GDI wants 0x00BBGGRR.
pub fn colorref(hex: u32) -> u32 {
    ((hex & 0xff) << 16) | (hex & 0xff00) | ((hex >> 16) & 0xff)
}

fn read_personalize(name: &str) -> Option<u32> {
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
    let val: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data: u32 = 0;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    (r == 0).then_some(data)
}

/// Apps (flyout, menus) use dark mode.
pub fn apps_dark() -> bool {
    read_personalize("AppsUseLightTheme") == Some(0)
}

/// Taskbar/tray uses dark mode (Windows default is dark taskbar even in light app mode).
pub fn taskbar_dark() -> bool {
    read_personalize("SystemUsesLightTheme") != Some(1)
}
