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

//! Phones previously reached over Wi-Fi, used for automatic reconnection.
//! Kept in `%LOCALAPPDATA%\CatLink\wifi-devices.txt`, one per line.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// Paired with wireless debugging (Android 11+). The phone advertises itself over
    /// mDNS under this name (`adb-<serial>-<random>`), on a port that changes.
    Mdns(String),
    /// Switched over with `adb tcpip`: a fixed `ip:port`, valid until the phone reboots.
    Tcp(String),
}

fn path() -> PathBuf {
    crate::logfile::path().with_file_name("wifi-devices.txt")
}

pub fn load() -> Vec<Saved> {
    std::fs::read_to_string(path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| match l.trim().split_once(' ')? {
            ("mdns", v) => Some(Saved::Mdns(v.trim().to_string())),
            ("tcp", v) => Some(Saved::Tcp(v.trim().to_string())),
            _ => None,
        })
        .collect()
}

pub fn remember(entry: Saved) {
    let mut all = load();
    if all.contains(&entry) {
        return;
    }
    all.push(entry);
    let text: String = all
        .iter()
        .map(|e| match e {
            Saved::Mdns(n) => format!("mdns {n}\n"),
            Saved::Tcp(a) => format!("tcp {a}\n"),
        })
        .collect();
    if let Err(e) = std::fs::write(path(), text) {
        log::warn!("could not save {}: {e}", path().display());
    }
}
