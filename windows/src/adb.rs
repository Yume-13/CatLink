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

//! Thin wrapper around the `adb` command-line tool.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub const PACKAGE: &str = "app.catlink";
pub const ACTIVITY: &str = "app.catlink/.MainActivity";
pub const START_ACTION: &str = "app.catlink.action.START";
pub const SOCKET_NAME: &str = "catlink";

#[derive(Clone)]
pub struct Adb {
    exe: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub description: String,
}

impl Adb {
    /// Locate adb.exe: explicit path, next to this exe, PATH, then common SDK locations.
    pub fn locate(explicit: Option<&Path>) -> Result<Adb> {
        if let Some(p) = explicit {
            if p.is_file() {
                return Ok(Adb { exe: p.to_path_buf() });
            }
            bail!("adb not found at {}", p.display());
        }
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(me) = std::env::current_exe() {
            if let Some(dir) = me.parent() {
                candidates.push(dir.join("adb.exe"));
                candidates.push(dir.join("platform-tools").join("adb.exe"));
            }
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                candidates.push(dir.join("adb.exe"));
            }
        }
        for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            if let Some(v) = std::env::var_os(var) {
                candidates.push(PathBuf::from(v).join("platform-tools").join("adb.exe"));
            }
        }
        if let Some(v) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(v);
            candidates.push(base.join("Android").join("Sdk").join("platform-tools").join("adb.exe"));
            candidates.push(
                base.join("Microsoft").join("WinGet").join("Links").join("adb.exe"),
            );
        }
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .map(|exe| Adb { exe })
            .ok_or_else(|| anyhow!("adb.exe not found. Put it next to catlink.exe, add it to PATH, or pass --adb <path>."))
    }

    pub fn path(&self) -> &Path {
        &self.exe
    }

    async fn run(&self, serial: Option<&str>, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new(&self.exe);
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.args(args);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output())
            .await
            .map_err(|_| anyhow!("adb {} timed out", args.join(" ")))?
            .with_context(|| format!("running {}", self.exe.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("adb {} failed: {}{}", args.join(" "), stderr.trim(), stdout.trim());
        }
        Ok(stdout)
    }

    pub async fn devices(&self) -> Result<Vec<Device>> {
        let out = self.run(None, &["devices", "-l"]).await?;
        Ok(out
            .lines()
            .skip_while(|l| !l.starts_with("List of devices"))
            .skip(1)
            .filter_map(|l| {
                let mut parts = l.split_whitespace();
                let serial = parts.next()?.to_string();
                let state = parts.next()?.to_string();
                let description = parts
                    .filter_map(|p| p.strip_prefix("model:"))
                    .next()
                    .unwrap_or("")
                    .replace('_', " ");
                Some(Device { serial, state, description })
            })
            .collect())
    }

    pub async fn is_installed(&self, serial: &str) -> Result<bool> {
        let out = self.run(Some(serial), &["shell", "pm", "path", PACKAGE]).await.unwrap_or_default();
        Ok(out.contains("package:"))
    }

    pub async fn install(&self, serial: &str, apk: &Path) -> Result<()> {
        let apk = apk.to_string_lossy();
        self.run(Some(serial), &["install", "-r", &apk]).await?;
        Ok(())
    }

    /// Launch the phone app and ask it to start its relay service.
    pub async fn start_app(&self, serial: &str) -> Result<()> {
        let out = self
            .run(Some(serial), &["shell", "am", "start", "-n", ACTIVITY, "-a", START_ACTION])
            .await?;
        if out.contains("Error") {
            bail!("could not start the CatLink app on the phone: {}", out.trim());
        }
        Ok(())
    }

    /// Create (or reuse) `tcp:<port> -> localabstract:catlink` and return the local port.
    pub async fn forward(&self, serial: &str) -> Result<u16> {
        let target = format!("localabstract:{SOCKET_NAME}");
        if let Ok(list) = self.run(None, &["forward", "--list"]).await {
            for line in list.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 3 && parts[0] == serial && parts[2] == target {
                    if let Some(port) = parts[1].strip_prefix("tcp:").and_then(|p| p.parse().ok()) {
                        return Ok(port);
                    }
                }
            }
        }
        let out = self.run(Some(serial), &["forward", "tcp:0", &target]).await?;
        out.trim()
            .parse::<u16>()
            .with_context(|| format!("unexpected output from adb forward: {out:?}"))
    }

    pub async fn remove_forward(&self, serial: &str, port: u16) {
        let _ = self.run(Some(serial), &["forward", "--remove", &format!("tcp:{port}")]).await;
    }

    /// Devices advertised on the local network (`adb mdns services`).
    pub async fn mdns_services(&self) -> Vec<MdnsService> {
        let out = self.run(None, &["mdns", "services"]).await.unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                let (name, kind, addr) = (p.next()?, p.next()?, p.next()?);
                kind.starts_with("_adb-tls-").then(|| MdnsService {
                    name: name.to_string(),
                    kind: kind.trim_end_matches('.').to_string(),
                    addr: addr.to_string(),
                })
            })
            .collect()
    }

    /// `adb pair`; returns the phone's pairing GUID (`adb-<serial>-<random>`), which is
    /// also the name it advertises its connect service under.
    pub async fn pair(&self, addr: &str, code: &str) -> Result<String> {
        let out = self.run(None, &["pair", addr, code]).await?;
        if !out.contains("Successfully paired") {
            bail!("{}", out.trim());
        }
        Ok(out.split("guid=").nth(1).map(|g| g.trim().trim_end_matches(']').to_string()).unwrap_or_default())
    }

    /// `adb connect` with a short timeout.
    pub async fn connect(&self, addr: &str) -> Result<()> {
        let out = tokio::time::timeout(std::time::Duration::from_secs(6), self.run(None, &["connect", addr]))
            .await
            .map_err(|_| anyhow!("timed out connecting to {addr}"))??;
        if out.contains("connected to") {
            Ok(())
        } else {
            bail!("{}", out.trim())
        }
    }

    /// Restart adbd on the phone listening on TCP `port` (adb over TCP; also works before Android 11).
    pub async fn tcpip(&self, serial: &str, port: u16) -> Result<()> {
        self.run(Some(serial), &["tcpip", &port.to_string()]).await?;
        Ok(())
    }

    /// The phone's IPv4 address on Wi-Fi, if it has one.
    pub async fn wifi_ip(&self, serial: &str) -> Option<std::net::Ipv4Addr> {
        let out = self.run(Some(serial), &["shell", "ip", "-f", "inet", "addr", "show", "wlan0"]).await.ok()?;
        out.split_whitespace()
            .skip_while(|w| *w != "inet")
            .nth(1)
            .and_then(|a| a.split('/').next())
            .and_then(|a| a.parse().ok())
    }

    /// The phone's hardware serial number, the same over USB and Wi-Fi.
    pub async fn hw_serial(&self, serial: &str) -> Option<String> {
        let out = self.run(Some(serial), &["shell", "getprop", "ro.serialno"]).await.ok()?;
        Some(out.trim().to_string()).filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct MdnsService {
    /// Instance name, e.g. `adb-<serial>-<random>` or the name from a CatLink pairing QR code.
    pub name: String,
    /// `_adb-tls-connect._tcp` or `_adb-tls-pairing._tcp`.
    pub kind: String,
    /// `ip:port`.
    pub addr: String,
}

/// adb serials for network devices: `ip:port` after `adb connect`, or
/// `adb-<serial>-<random>._adb-tls-connect._tcp` when adb connected via mDNS.
pub fn is_wireless(serial: &str) -> bool {
    serial.contains("._adb-tls-connect.") || serial.parse::<std::net::SocketAddr>().is_ok()
}
