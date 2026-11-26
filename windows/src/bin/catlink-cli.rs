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

//! Console version of CatLink: same engine as the tray app, logs to the terminal.
//! Also hosts `--check`, a quick end-to-end test of the phone side.

use anyhow::Result;
use catlink_core::engine::{self, Engine, Options, State};
use catlink_core::{adb, check, elevate, proto};
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

#[derive(Parser, Debug, Clone)]
#[command(name = "catlink-cli", version, about = "Use an Android phone's internet over USB debugging (console version).")]
struct Args {
    /// adb serial of the phone (only needed when several devices are connected).
    #[arg(short, long)]
    serial: Option<String>,
    /// Path to adb.exe (default: next to this exe, PATH, or the Android SDK).
    #[arg(long)]
    adb: Option<PathBuf>,
    /// MTU of the virtual adapter.
    #[arg(long, default_value_t = 1500)]
    mtu: u16,
    /// Route only these IPv4 prefixes (e.g. 1.1.1.0/24) instead of all traffic. Repeatable.
    #[arg(long = "route", value_name = "CIDR")]
    routes: Vec<String>,
    /// Don't add any routes (create the adapter only).
    #[arg(long)]
    no_routes: bool,
    /// Skip adb and talk to a CatLink endpoint already reachable at 127.0.0.1:<PORT>.
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,
    /// Test DNS, TCP, UDP and ping through the phone, then exit (no adapter, no admin rights).
    #[arg(long)]
    check: bool,
    /// Don't install or launch the phone app automatically.
    #[arg(long)]
    no_launch: bool,
    /// More logging (-v debug, -vv trace).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Don't try to relaunch as Administrator.
    #[arg(long, hide = true)]
    no_elevate: bool,
    /// Wait for Enter before closing (used when relaunched in a new elevated window).
    #[arg(long, hide = true)]
    pause_on_exit: bool,
}

fn main() {
    let args = Args::parse();
    let level = match args.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let libs = if args.verbose >= 2 { "" } else { "ipstack=warn,tun=warn," };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(format!("{libs}{level}")))
        .format_target(false)
        .format_timestamp_secs()
        .init();

    if !elevate::is_elevated() && !args.no_elevate && !args.check {
        log::info!("Administrator rights are needed to create the network adapter; asking Windows for them...");
        let extra = ["--no-elevate".to_string(), "--pause-on-exit".to_string()];
        match elevate::relaunch_elevated(&extra) {
            Ok(()) => return,
            Err(e) => {
                log::error!("could not relaunch as Administrator: {e:#}");
                std::process::exit(1);
            }
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let pause = args.pause_on_exit;
    let code = match rt.block_on(run(args)) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("{e:#}");
            1
        }
    };
    if pause {
        eprintln!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}

async fn run_check(args: &Args) -> Result<()> {
    if let Some(p) = args.port {
        return check::run(p).await;
    }
    let adb = adb::Adb::locate(args.adb.as_deref())?;
    let devices = adb.devices().await?;
    let serial = match &args.serial {
        Some(s) => s.clone(),
        None => devices
            .iter()
            .find(|d| d.state == "device")
            .map(|d| d.serial.clone())
            .ok_or_else(|| anyhow::anyhow!("no phone with USB debugging found"))?,
    };
    if !args.no_launch && !adb.is_installed(&serial).await? {
        let apk = std::env::current_exe()?.with_file_name("catlink.apk");
        anyhow::ensure!(apk.is_file(), "the CatLink app is not installed on {serial}");
        log::info!("installing the CatLink app on {serial}...");
        adb.install(&serial, &apk).await?;
    }
    let port = adb.forward(&serial).await?;
    if proto::hello(port, false).await.is_err() && !args.no_launch {
        adb.start_app(&serial).await?;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if proto::hello(port, false).await.is_ok() {
                break;
            }
        }
    }
    let r = check::run(port).await;
    adb.remove_forward(&serial, port).await;
    r
}

async fn run(args: Args) -> Result<()> {
    if args.check {
        return run_check(&args).await;
    }
    let engine = Engine::new();
    let opts = Options {
        serial: args.serial.clone(),
        adb: args.adb.clone(),
        mtu: args.mtu,
        routes: args.routes.clone(),
        no_routes: args.no_routes,
        port: args.port,
        launch: !args.no_launch,
    };
    let task = tokio::spawn(engine.clone().run(opts));
    log::info!("CatLink is running. Press Ctrl+C to stop.");

    let shared = engine.shared.clone();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    let mut last = (0u64, 0u64, Instant::now());
    let mut task = std::pin::pin!(task);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            r = &mut task => {
                return r?;
            }
            _ = tick.tick() => {
                let st = engine.status();
                if st.state != State::Connected {
                    continue;
                }
                let (up, down) = (shared.bytes_up.load(Relaxed), shared.bytes_down.load(Relaxed));
                let secs = last.2.elapsed().as_secs_f64().max(1.0);
                if (up, down) != (last.0, last.1) {
                    log::info!(
                        "{} TCP / {} UDP open · avg ↓ {} ↑ {} · total ↓ {} ↑ {} · latency {} ms",
                        shared.tcp_active.load(Relaxed),
                        shared.udp_active.load(Relaxed),
                        engine::human_rate((down - last.1) as f64 / secs),
                        engine::human_rate((up - last.0) as f64 / secs),
                        engine::human_bytes(down),
                        engine::human_bytes(up),
                        st.latency_ms.unwrap_or(0),
                    );
                }
                last = (up, down, Instant::now());
            }
        }
    }
    log::info!("stopping...");
    engine.shutdown();
    let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
    Ok(())
}
