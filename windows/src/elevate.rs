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

//! UAC helpers: creating a Wintun adapter and adding routes needs Administrator rights.

use anyhow::{Result, bail};
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub fn is_elevated() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Start this exe again through UAC ("Run as administrator") with the current arguments plus `extra`.
pub fn relaunch_elevated(extra: &[String]) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut params: Vec<String> = std::env::args().skip(1).map(|a| quote(&a)).collect();
    params.extend(extra.iter().map(|a| quote(a)));
    let params = params.join(" ");
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let params_w = wide(params.as_ref());
    let cwd = std::env::current_dir().ok().map(|d| wide(d.as_os_str()));
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params_w.as_ptr(),
            cwd.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    if (r as isize) <= 32 {
        bail!("ShellExecute failed (code {}); the UAC prompt may have been declined", r as isize);
    }

    Ok(())
}
