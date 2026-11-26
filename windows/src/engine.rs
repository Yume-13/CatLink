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

//! The engine: keeps the adb link to the phone alive, brings the virtual adapter up
//! while the phone is reachable, and publishes a `Status` for the UI.

use crate::relay::Shared;
use crate::{adb, net, proto, relay, wifi};
use anyhow::Result;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct Options {
    pub serial: Option<String>,
    pub adb: Option<PathBuf>,
    pub mtu: u16,
    /// Prefixes to route; empty means "everything".
    pub routes: Vec<String>,
    pub no_routes: bool,
    /// Talk to 127.0.0.1:<port> instead of using adb.
    pub port: Option<u16>,
    pub launch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Starting,
    Paused,
    /// No phone with USB debugging is connected.
    NoDevice,
    /// A phone is connected but hasn't accepted this PC's adb key.
    Unauthorized,
    /// Several phones are connected; one must be selected with --serial.
    MultipleDevices(String),
    /// The CatLink app isn't installed and no catlink.apk was found.
    AppMissing,
    /// The app is installed but its relay isn't answering.
    AppNotResponding,
    Connecting,
    Connected,
    Error(String),
}

impl State {
    pub fn title(&self) -> &'static str {
        match self {
            State::Starting => "Starting",
            State::Paused => "Paused",
            State::NoDevice => "No phone",
            State::Unauthorized => "Waiting for approval",
            State::MultipleDevices(_) => "Several phones",
            State::AppMissing => "App not installed",
            State::AppNotResponding => "App not responding",
            State::Connecting => "Connecting",
            State::Connected => "Connected",
            State::Error(_) => "Error",
        }
    }

    /// One-sentence help for the current state.
    pub fn hint(&self) -> String {
        match self {
            State::Starting | State::Connecting => "Setting up the link to your phone…".into(),
            State::Paused => "This PC is using its own network until you resume.".into(),
            State::NoDevice => "Connect your phone by USB, or pair it over Wi-Fi.".into(),
            State::Unauthorized => "Unlock the phone and tap \u{201c}Allow\u{201d} on the USB debugging prompt.".into(),
            State::MultipleDevices(list) => format!("Several phones are connected ({list}). Unplug the extra ones."),
            State::AppMissing => "Install catlink.apk on the phone, or put it next to catlink.exe.".into(),
            State::AppNotResponding => "Open CatLink on the phone and tap Start.".into(),
            State::Connected => String::new(),
            State::Error(e) => e.clone(),
        }
    }

    pub fn severity(&self) -> Severity {
        match self {
            State::Connected => Severity::Good,
            State::Paused => Severity::Off,
            State::Starting | State::Connecting | State::NoDevice | State::Unauthorized => Severity::Warning,
            State::MultipleDevices(_) | State::AppMissing | State::AppNotResponding | State::Error(_) => Severity::Critical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Good,
    Warning,
    Critical,
    Off,
}

/// What the phone reports about itself (KIND_HELLO).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhoneInfo {
    pub app_version: String,
    pub model: String,
    pub android: String,
    /// "Mobile data", "Wi-Fi", ...
    pub network: String,
    /// Carrier name when on mobile data.
    pub carrier: String,
}

impl PhoneInfo {
    pub fn parse(text: &str) -> PhoneInfo {
        let mut p = PhoneInfo::default();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().to_string();
                match k.trim() {
                    "version" => p.app_version = v,
                    "model" => p.model = v,
                    "android" => p.android = v,
                    "network" => p.network = v,
                    "carrier" => p.carrier = v,
                    _ => {}
                }
            }
        }
        if p.model.is_empty() {
            // A reply without key=value lines is treated as the model name.
            p.model = text.trim().to_string();
        }
        p
    }

    pub fn summary(&self) -> String {
        let mut s = self.model.clone();
        if !self.android.is_empty() {
            s.push_str(&format!(" · Android {}", self.android));
        }
        s
    }

    pub fn via(&self) -> String {
        match (self.network.as_str(), self.carrier.as_str()) {
            ("", _) => String::new(),
            (n, "") => format!("Using the phone\u{2019}s {}", n.to_lowercase().replace("wi-fi", "Wi-Fi")),
            (n, c) => format!("Using the phone\u{2019}s {} · {c}", n.to_lowercase()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    pub state: State,
    pub phone: Option<PhoneInfo>,
    pub serial: Option<String>,
    pub latency_ms: Option<u32>,
    pub connected_since: Option<Instant>,
    /// The phone is reached over Wi-Fi (wireless debugging or `adb tcpip`), not USB.
    pub wireless: bool,
    /// A QR pairing in progress (see [`Engine::start_pairing`]).
    pub pairing: Option<Pairing>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairStage {
    /// The QR code is up; waiting for the phone to scan it.
    Waiting,
    Pairing,
    Connecting,
    /// Paired and connected; holds the phone's name.
    Done(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Pairing {
    /// Text for the QR code that Developer options › Wireless debugging scans.
    pub qr: String,
    pub stage: PairStage,
}

pub struct Engine {
    pub shared: Arc<Shared>,
    status: Mutex<Status>,
    enabled: watch::Sender<bool>,
    shutdown: Notify,
    stopped: Mutex<bool>,
    /// The engine's runtime and adb, so the UI thread can start pairing / switching tasks.
    ctx: Mutex<Option<(tokio::runtime::Handle, adb::Adb)>>,
    pair_task: Mutex<Option<tokio::task::AbortHandle>>,
    /// One-off messages for the UI to show as notifications.
    events: Mutex<Vec<String>>,
}

enum ConnectError {
    NotInstalled,
    NotResponding,
    Multiple(String),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for ConnectError {
    fn from(e: anyhow::Error) -> Self {
        ConnectError::Other(e)
    }
}

struct Link {
    adb: adb::Adb,
    wanted_serial: Option<String>,
    serial: Option<String>,
    port: u16,
    launch: bool,
    /// The current device is reached over Wi-Fi.
    wireless: bool,
    last_wifi_try: Option<Instant>,
}

enum Pick {
    Device(String),
    None,
    Unauthorized,
}

impl Link {
    async fn pick_device(&self) -> Result<Pick, ConnectError> {
        let devices = self.adb.devices().await?;
        let usable = |d: &&adb::Device| match &self.wanted_serial {
            Some(w) => &d.serial == w,
            None => true,
        };
        let all_ready: Vec<_> = devices.iter().filter(usable).filter(|d| d.state == "device").collect();
        // USB devices take precedence. The same phone can also be listed over Wi-Fi, sometimes
        // twice (mDNS name and ip:port), so Wi-Fi entries are merged by hardware serial.
        let usb: Vec<_> = all_ready.iter().copied().filter(|d| !adb::is_wireless(&d.serial)).collect();
        let ready = if !usb.is_empty() || self.wanted_serial.is_some() {
            if self.wanted_serial.is_some() { all_ready } else { usb }
        } else {
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for d in all_ready {
                let hw = self.adb.hw_serial(&d.serial).await.unwrap_or_else(|| d.serial.clone());
                if seen.insert(hw) {
                    out.push(d);
                }
            }
            out
        };
        match ready.len() {
            0 if devices.iter().filter(usable).any(|d| d.state == "unauthorized") => Ok(Pick::Unauthorized),
            0 => Ok(Pick::None),
            1 => Ok(Pick::Device(ready[0].serial.clone())),
            _ => Err(ConnectError::Multiple(
                ready
                    .iter()
                    .map(|d| if d.description.is_empty() { d.serial.clone() } else { d.description.clone() })
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
        }
    }

    /// Reconnect to phones reached over Wi-Fi before (at most every 10 s).
    async fn try_wifi_reconnect(&mut self) -> bool {
        if self.wanted_serial.is_some() || self.last_wifi_try.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
            return false;
        }
        self.last_wifi_try = Some(Instant::now());
        let saved = wifi::load();
        if saved.is_empty() {
            return false;
        }
        let services = if saved.iter().any(|s| matches!(s, wifi::Saved::Mdns(_))) {
            self.adb.mdns_services().await
        } else {
            Vec::new()
        };
        let mut any = false;
        for entry in &saved {
            let addr = match entry {
                wifi::Saved::Mdns(name) => {
                    match services.iter().find(|s| &s.name == name && s.kind.contains("connect")) {
                        Some(s) => s.addr.clone(),
                        None => continue,
                    }
                }
                wifi::Saved::Tcp(addr) => addr.clone(),
            };
            if self.adb.connect(&addr).await.is_ok() {
                log::info!("reconnected to the phone over Wi-Fi at {addr}");
                any = true;
            }
        }
        any
    }

    async fn connect(&mut self) -> Result<Pick, ConnectError> {
        let mut pick = self.pick_device().await?;
        if matches!(pick, Pick::None) && self.try_wifi_reconnect().await {
            pick = self.pick_device().await?;
        }
        let serial = match pick {
            Pick::Device(s) => s,
            other => return Ok(other),
        };
        self.wireless = adb::is_wireless(&serial);
        if self.launch && !self.adb.is_installed(&serial).await? {
            let apk = std::env::current_exe().map_err(anyhow::Error::from)?.with_file_name("catlink.apk");
            if !apk.is_file() {
                return Err(ConnectError::NotInstalled);
            }
            log::info!("installing the CatLink app on {serial}...");
            self.adb.install(&serial, &apk).await?;
        }
        let port = self.adb.forward(&serial).await?;
        self.serial = Some(serial.clone());
        self.port = port;
        if proto::hello(port, self.wireless).await.is_ok() {
            return Ok(Pick::Device(serial));
        }
        if self.launch {
            self.adb.start_app(&serial).await?;
        }
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if proto::hello(port, self.wireless).await.is_ok() {
                return Ok(Pick::Device(serial));
            }
        }
        Err(ConnectError::NotResponding)
    }

    async fn cleanup(&mut self) {
        if let Some(s) = self.serial.take() {
            self.adb.remove_forward(&s, self.port).await;
        }
    }
}

/// The adapter plus the task running the packet stack on it.
struct Tunnel {
    stack: JoinHandle<()>,
    /// Host route that keeps traffic to a Wi-Fi phone off the CatLink adapter.
    pinned: Option<net::PinnedRoute>,
}

impl Engine {
    pub fn new() -> Arc<Engine> {
        let (enabled, _) = watch::channel(true);
        Arc::new(Engine {
            shared: Arc::new(Shared::default()),
            status: Mutex::new(Status {
                state: State::Starting,
                phone: None,
                serial: None,
                latency_ms: None,
                connected_since: None,
                wireless: false,
                pairing: None,
            }),
            enabled,
            shutdown: Notify::new(),
            stopped: Mutex::new(false),
            ctx: Mutex::new(None),
            pair_task: Mutex::new(None),
            events: Mutex::new(Vec::new()),
        })
    }

    /// Messages queued for display since the last call.
    pub fn take_events(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    fn event(&self, text: impl Into<String>) {
        let text = text.into();
        log::info!("{text}");
        self.events.lock().unwrap().push(text);
    }

    fn set_pair_stage(&self, stage: PairStage) {
        if let Some(p) = self.status.lock().unwrap().pairing.as_mut() {
            log::info!("pairing: {stage:?}");
            p.stage = stage;
        }
    }

    /// Show a QR code for Developer options › Wireless debugging › Pair device with QR
    /// code, then pair and connect once the phone scans it (Android 11+).
    pub fn start_pairing(self: &Arc<Self>) {
        let Some((rt, adb)) = self.ctx.lock().unwrap().clone() else { return };
        self.cancel_pairing();
        // adb's QR format: the phone looks for an mDNS pairing service named `name` and
        // pairs with `code` as the password.
        let name = format!("catlink-{:06}", random_u64() % 1_000_000);
        let code: String = (0..10).map(|_| (b'a' + (random_u64() % 26) as u8) as char).collect();
        let qr = format!("WIFI:T:ADB;S:{name};P:{code};;");
        self.status.lock().unwrap().pairing = Some(Pairing { qr, stage: PairStage::Waiting });
        log::info!("pairing: showing QR code for {name}");
        let me = self.clone();
        let task = rt.spawn(async move { me.run_pairing(adb, name, code).await });
        *self.pair_task.lock().unwrap() = Some(task.abort_handle());
    }

    /// Stop a pairing in progress and hide its QR code.
    pub fn cancel_pairing(&self) {
        if let Some(t) = self.pair_task.lock().unwrap().take() {
            t.abort();
        }
        self.status.lock().unwrap().pairing = None;
    }

    async fn run_pairing(&self, adb: adb::Adb, name: String, code: String) {
        let deadline = Instant::now() + Duration::from_secs(300);
        let addr = loop {
            if Instant::now() > deadline {
                self.set_pair_stage(PairStage::Failed("The phone didn\u{2019}t scan the code in time. Try again.".into()));
                return;
            }
            let found = adb.mdns_services().await.into_iter().find(|s| s.name == name && s.kind.contains("pairing"));
            if let Some(s) = found {
                break s.addr;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        self.set_pair_stage(PairStage::Pairing);
        let guid = match adb.pair(&addr, &code).await {
            Ok(g) => g,
            Err(e) => {
                log::warn!("adb pair failed: {e:#}");
                self.set_pair_stage(PairStage::Failed("Pairing failed. Try again.".into()));
                return;
            }
        };
        self.set_pair_stage(PairStage::Connecting);
        // The phone advertises its connect service under the pairing GUID, on the same IP.
        let ip = addr.rsplit_once(':').map(|(ip, _)| ip.to_string()).unwrap_or_default();
        for _ in 0..30 {
            let services = adb.mdns_services().await;
            let svc = services.iter().find(|s| s.kind.contains("connect") && (s.name == guid || s.addr.starts_with(&format!("{ip}:"))));
            if let Some(s) = svc {
                if !guid.is_empty() {
                    wifi::remember(wifi::Saved::Mdns(s.name.clone()));
                }
                // adb may already be connected to a freshly paired phone; connecting again is harmless.
                let _ = adb.connect(&s.addr).await;
                let devices = adb.devices().await.unwrap_or_default();
                let me = devices.iter().find(|d| d.state == "device" && (d.serial.starts_with(&s.name) || d.serial == s.addr));
                if let Some(d) = me {
                    let label = if d.description.is_empty() { "your phone".to_string() } else { d.description.clone() };
                    self.set_pair_stage(PairStage::Done(label));
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        self.set_pair_stage(PairStage::Failed(
            "Paired, but couldn\u{2019}t connect. Check that wireless debugging is still on.".into(),
        ));
    }

    /// While connected over USB: switch the phone to adb over Wi-Fi (`adb tcpip`), which
    /// also works before Android 11. Lasts until the phone reboots.
    pub fn switch_to_wifi(self: &Arc<Self>) {
        let Some((rt, adb)) = self.ctx.lock().unwrap().clone() else { return };
        let st = self.status();
        let Some(serial) = st.serial.filter(|_| !st.wireless) else {
            self.event("Connect the phone by USB first, then switch it to Wi-Fi.");
            return;
        };
        let me = self.clone();
        rt.spawn(async move { me.run_switch(adb, serial).await });
    }

    async fn run_switch(&self, adb: adb::Adb, serial: String) {
        const PORT: u16 = 5555;
        let Some(ip) = adb.wifi_ip(&serial).await else {
            self.event("The phone isn\u{2019}t on Wi-Fi, so it can\u{2019}t switch.");
            return;
        };
        if let Err(e) = adb.tcpip(&serial, PORT).await {
            log::warn!("adb tcpip failed: {e:#}");
            self.event("Couldn\u{2019}t switch the phone to Wi-Fi.");
            return;
        }
        // Wait for adbd to restart on the phone.
        tokio::time::sleep(Duration::from_secs(3)).await;
        let addr = format!("{ip}:{PORT}");
        for _ in 0..5 {
            if adb.connect(&addr).await.is_ok() {
                wifi::remember(wifi::Saved::Tcp(addr.clone()));
                self.event(format!(
                    "The phone is reachable over Wi-Fi at {addr}. You can unplug the cable; this lasts until the phone restarts."
                ));
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        self.event(format!("Couldn\u{2019}t reach the phone at {addr}. Is this PC on the same network?"));
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.borrow()
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.send_replace(on);
    }

    /// Ask `run` to tear everything down and return.
    pub fn shutdown(&self) {
        *self.stopped.lock().unwrap() = true;
        self.shutdown.notify_one();
    }

    fn stopping(&self) -> bool {
        *self.stopped.lock().unwrap()
    }

    fn set_state(&self, state: State) {
        let mut s = self.status.lock().unwrap();
        if s.state != state {
            match &state {
                State::Error(e) => log::error!("{e}"),
                State::Connected => {}
                other => log::info!("{}: {}", other.title(), other.hint()),
            }
            if state != State::Connected {
                s.connected_since = None;
                s.latency_ms = None;
            }
            s.state = state;
        }
    }

    fn set_connected(&self, phone: PhoneInfo, serial: Option<String>, latency: u32, wireless: bool) {
        let mut s = self.status.lock().unwrap();
        if s.state != State::Connected {
            log::info!("connected to {} ({})", phone.summary(), phone.via());
            s.connected_since = Some(Instant::now());
        } else if s.phone.as_ref().map(|p| &p.network) != Some(&phone.network) {
            log::info!("phone network changed: {}", phone.via());
        }
        s.state = State::Connected;
        s.phone = Some(phone);
        s.serial = serial;
        s.latency_ms = Some(latency);
        s.wireless = wireless;
    }

    /// Sleep, but wake early on shutdown or when pause/resume is toggled.
    async fn pause_for(&self, d: Duration, enabled: &mut watch::Receiver<bool>) {
        tokio::select! {
            _ = tokio::time::sleep(d) => {}
            _ = self.shutdown.notified() => {}
            _ = enabled.changed() => {}
        }
    }

    pub async fn run(self: Arc<Self>, opts: Options) -> Result<()> {
        let mut enabled = self.enabled.subscribe();
        let mut link = match opts.port {
            Some(_) => None,
            None => {
                let adb = match adb::Adb::locate(opts.adb.as_deref()) {
                    Ok(a) => a,
                    Err(e) => {
                        self.set_state(State::Error(format!("{e:#}")));
                        return Err(e);
                    }
                };
                log::info!("using {}", adb.path().display());
                *self.ctx.lock().unwrap() = Some((tokio::runtime::Handle::current(), adb.clone()));
                Some(Link {
                    adb,
                    wanted_serial: opts.serial.clone(),
                    serial: None,
                    port: 0,
                    launch: opts.launch,
                    wireless: false,
                    last_wifi_try: None,
                })
            }
        };
        let mut tunnel: Option<Tunnel> = None;
        let mut misses = 0;

        while !self.stopping() {
            if !*enabled.borrow_and_update() {
                self.take_down(&mut tunnel, &mut link).await;
                self.set_state(State::Paused);
                self.pause_for(Duration::from_secs(3600), &mut enabled).await;
                continue;
            }

            let port = self.shared.port.load(Relaxed);
            if port != 0 {
                // Connected: health check, which also refreshes phone info and latency.
                let t = Instant::now();
                let wireless = link.as_ref().is_some_and(|l| l.wireless);
                match proto::hello(port, wireless).await {
                    Ok(info) => {
                        misses = 0;
                        let latency = t.elapsed().as_millis() as u32;
                        // Only report "Connected" once traffic can actually flow.
                        if tunnel.is_none() {
                            // Over Wi-Fi, find the phone's address so it can be kept out of the adapter.
                            let phone_ip = match link.as_ref() {
                                Some(l) if l.wireless => match &l.serial {
                                    Some(s) => l.adb.wifi_ip(s).await,
                                    None => None,
                                },
                                _ => None,
                            };
                            match self.bring_up(&opts, phone_ip).await {
                                Ok(t) => tunnel = Some(t),
                                Err(e) => {
                                    log::error!("{e:#}");
                                    self.set_state(State::Error(adapter_error_text(&e)));
                                    self.shared.port.store(0, Relaxed);
                                    self.pause_for(Duration::from_secs(10), &mut enabled).await;
                                    continue;
                                }
                            }
                        }
                        let serial = link.as_ref().and_then(|l| l.serial.clone());
                        self.set_connected(PhoneInfo::parse(&info), serial, latency, wireless);
                    }
                    Err(e) => {
                        misses += 1;
                        log::debug!("health check failed ({misses}): {e}");
                        if misses >= 2 {
                            log::warn!("lost the phone");
                            self.take_down(&mut tunnel, &mut link).await;
                            self.set_state(State::Connecting);
                            misses = 0;
                            continue;
                        }
                    }
                }
                self.pause_for(Duration::from_secs(2), &mut enabled).await;
                continue;
            }

            // Not connected: (re)establish the link.
            match link.as_mut() {
                None => {
                    // Direct --port mode.
                    let p = opts.port.unwrap_or(0);
                    if proto::hello(p, false).await.is_ok() {
                        self.shared.port.store(p, Relaxed);
                        continue;
                    }
                    self.set_state(State::AppNotResponding);
                }
                Some(l) => {
                    if !matches!(self.status().state, State::NoDevice | State::Unauthorized | State::Error(_)) {
                        self.set_state(State::Connecting);
                    }
                    match l.connect().await {
                        Ok(Pick::Device(_)) => {
                            self.shared.port.store(l.port, Relaxed);
                            continue;
                        }
                        Ok(Pick::None) => self.set_state(State::NoDevice),
                        Ok(Pick::Unauthorized) => self.set_state(State::Unauthorized),
                        Err(ConnectError::NotInstalled) => self.set_state(State::AppMissing),
                        Err(ConnectError::NotResponding) => self.set_state(State::AppNotResponding),
                        Err(ConnectError::Multiple(list)) => self.set_state(State::MultipleDevices(list)),
                        Err(ConnectError::Other(e)) => self.set_state(State::Error(format!("{e:#}"))),
                    }
                }
            }
            self.pause_for(Duration::from_secs(2), &mut enabled).await;
        }

        self.take_down(&mut tunnel, &mut link).await;
        Ok(())
    }

    async fn bring_up(&self, opts: &Options, phone_ip: Option<Ipv4Addr>) -> Result<Tunnel> {
        let mtu = opts.mtu;
        let adapter = tokio::task::block_in_place(|| net::create_adapter(mtu))?;
        log::info!("adapter \"{}\" up (interface {}, {}/24)", net::ADAPTER_NAME, adapter.index, net::LOCAL_IP);
        // Pinned before the adapter routes are added, so the lookup finds the physical network's route.
        let pinned = match phone_ip {
            Some(ip) if !opts.no_routes => tokio::task::block_in_place(|| net::pin_host_route(ip)),
            _ => None,
        };
        if !opts.no_routes {
            let prefixes = if opts.routes.is_empty() { net::default_prefixes() } else { opts.routes.clone() };
            tokio::task::block_in_place(|| net::add_routes(adapter.index, &prefixes))?;
            log::info!("routing {} through the phone", prefixes.join(", "));
        }
        let shared = self.shared.clone();
        let stack = tokio::spawn(async move {
            if let Err(e) = relay::run_stack(adapter.device, mtu, shared).await {
                log::error!("network stack stopped: {e:#}");
            }
        });
        Ok(Tunnel { stack, pinned })
    }

    /// Remove the adapter (Windows falls back to its normal network) and the adb forward.
    async fn take_down(&self, tunnel: &mut Option<Tunnel>, link: &mut Option<Link>) {
        self.shared.port.store(0, Relaxed);
        if let Some(t) = tunnel.take() {
            t.stack.abort();
            if let Some(r) = &t.pinned {
                tokio::task::block_in_place(|| net::unpin_host_route(r));
            }
            let _ = t.stack.await;
            // Let the adapter finish closing before it can be recreated.
            tokio::time::sleep(Duration::from_millis(500)).await;
            log::info!("adapter removed");
        }
        if let Some(l) = link.as_mut() {
            l.cleanup().await;
        }
    }
}

/// A short, actionable message for the UI; the full error goes to the log.
fn adapter_error_text(e: &anyhow::Error) -> String {
    let full = format!("{e:#}");
    if full.contains("wintun.dll not found") {
        "wintun.dll is missing. Put it in the same folder as catlink.exe.".into()
    } else if full.contains("Access is denied") || full.contains("0x00000005") || !crate::elevate::is_elevated() {
        "CatLink needs administrator rights to create its network adapter. Quit and start it again.".into()
    } else {
        "Couldn\u{2019}t set up the network adapter. Open the log file for details.".into()
    }
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

pub fn human_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", human_bytes(bytes_per_sec.max(0.0) as u64))
}

pub fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// Random number from `RandomState`, which std seeds from the OS generator.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0));
    h.finish()
}
