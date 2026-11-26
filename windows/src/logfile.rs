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

//! Minimal file logger for the tray app (which has no console).

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

struct FileLogger {
    file: Mutex<File>,
    level: log::LevelFilter,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        let t = m.target();
        let max = if t.starts_with("tiny_skia") {
            // Drawing warnings (e.g. an empty path) repeat every frame.
            log::LevelFilter::Error
        } else if t.starts_with("ipstack") || t.starts_with("tun") {
            log::LevelFilter::Warn
        } else {
            self.level
        };
        m.level() <= max
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
            if let Ok(mut f) = self.file.lock() {
                let _ = writeln!(f, "{h:02}:{m:02}:{s:02} UTC {:5} {}", r.level(), r.args());
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("CatLink").join("catlink.log")
}

/// Log to %LOCALAPPDATA%\CatLink\catlink.log (truncated at each start).
pub fn init(level: log::LevelFilter) -> std::io::Result<PathBuf> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = File::create(&p)?;
    let _ = log::set_boxed_logger(Box::new(FileLogger { file: Mutex::new(file), level }));
    log::set_max_level(level);
    Ok(p)
}
