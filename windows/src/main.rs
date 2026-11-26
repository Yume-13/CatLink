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

//! CatLink for Windows (tray app): shares an Android phone's internet connection over adb
//! (USB or Wi-Fi). Every connection is opened by the CatLink app on the phone, so Android
//! treats it as that app's own traffic.
#![windows_subsystem = "windows"]

mod ui;

use catlink_core::engine::{Engine, Options};
use catlink_core::{elevate, logfile};
use clap::Parser;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
use windows_sys::core::w;

#[derive(Parser, Debug, Clone)]
#[command(version, about = "CatLink tray app. See catlink-cli.exe for the console version.")]
struct Args {
    /// adb serial of the phone (only needed when several devices are connected).
    #[arg(short, long)]
    serial: Option<String>,
    /// Path to adb.exe.
    #[arg(long)]
    adb: Option<PathBuf>,
    /// MTU of the virtual adapter.
    #[arg(long, default_value_t = 1500)]
    mtu: u16,
    /// Route only these IPv4 prefixes instead of all traffic. Repeatable.
    #[arg(long = "route", value_name = "CIDR")]
    routes: Vec<String>,
    /// Don't install or launch the phone app automatically.
    #[arg(long)]
    no_launch: bool,
    /// Enable debug logging in the log file.
    #[arg(short, long)]
    verbose: bool,
    #[arg(long, hide = true)]
    no_elevate: bool,
    /// Render the flyout and icons with sample data into this folder, then exit.
    #[arg(long, hide = true, value_name = "DIR")]
    preview: Option<PathBuf>,
}

fn main() {
    let args = match Args::try_parse() {
        Ok(a) => a,
        Err(e) => {
            ui::error_box(&e.to_string());
            return;
        }
    };
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    if let Some(dir) = &args.preview {
        if let Err(e) = ui::preview::render(dir) {
            ui::error_box(&format!("preview failed: {e}"));
        }
        return;
    }

    // Only one tray icon: a second launch just opens the existing flyout.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, w!("Local\\CatLinkTray"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(ui::MAIN_CLASS, std::ptr::null());
            if !existing.is_null() {
                PostMessageW(existing, ui::WM_SHOW_FLYOUT, 0, 0);
            }
            return;
        }
    }

    if !elevate::is_elevated() && !args.no_elevate {
        if elevate::relaunch_elevated(&["--no-elevate".to_string()]).is_err() {
            ui::error_box(
                "CatLink needs administrator rights to create its network adapter.\n\nRun it again and choose Yes on the Windows prompt.",
            );
        }
        return;
    }

    let level = if args.verbose { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    let _ = logfile::init(level);
    log::info!("CatLink {} starting", option_env!("CATLINK_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")));

    let engine = Engine::new();
    let opts = Options {
        serial: args.serial,
        adb: args.adb,
        mtu: args.mtu,
        routes: args.routes,
        no_routes: false,
        port: None,
        launch: !args.no_launch,
    };
    let eng = engine.clone();
    let worker = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
        if let Err(e) = rt.block_on(eng.run(opts)) {
            log::error!("{e:#}");
        }
        rt.shutdown_timeout(Duration::from_secs(2));
    });

    ui::run(engine.clone());

    // On quit, wait briefly for the engine to remove the adapter and adb forward.
    engine.shutdown();
    let start = Instant::now();
    while !worker.is_finished() && start.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(50));
    }
    log::info!("bye");
    log::logger().flush();
    std::process::exit(0);
}
