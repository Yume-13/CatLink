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

//! Virtual adapter (Wintun) creation and Windows routing.

use anyhow::{Context, Result, bail};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::Command;
use tun::AbstractDevice;

pub const ADAPTER_NAME: &str = "CatLink";
/// Address of this PC on the virtual link.
pub const LOCAL_IP: Ipv4Addr = Ipv4Addr::new(10, 233, 85, 2);
/// Virtual gateway; also the DNS server Windows is told to use.
pub const GATEWAY_IP: Ipv4Addr = Ipv4Addr::new(10, 233, 85, 1);
pub const NETMASK: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 0);
/// Stable GUID so Windows keeps treating it as the same network between runs.
const ADAPTER_GUID: u128 = 0x6361_746c_696e_6b00_8a3c_52f1_0d7e_4b19;

pub struct Adapter {
    pub device: tun::AsyncDevice,
    pub index: u32,
}

fn wintun_path() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("exe has no parent dir")?;
    let p = dir.join("wintun.dll");
    if !p.is_file() {
        bail!(
            "wintun.dll not found next to {}.\nDownload it from https://www.wintun.net/ (bin\\amd64\\wintun.dll) and put it beside catlink.exe.",
            exe.display()
        );
    }
    Ok(p)
}

pub fn create_adapter(mtu: u16) -> Result<Adapter> {
    let mut cfg = tun::Configuration::default();
    cfg.tun_name(ADAPTER_NAME)
        .address(LOCAL_IP)
        .netmask(NETMASK)
        .mtu(mtu)
        .metric(1)
        .up();
    let wintun = wintun_path()?;
    cfg.platform_config(|c| {
        c.device_guid(ADAPTER_GUID);
        c.wintun_file(wintun.clone());
        c.dns_servers(&[IpAddr::V4(GATEWAY_IP)]);
    });
    let device = tun::create_as_async(&cfg).context("creating the Wintun adapter (are you running as Administrator?)")?;
    let index = device.tun_index().context("getting adapter index")? as u32;
    wait_until_usable(index);
    Ok(Adapter { device, index })
}

/// A new interface address stays "tentative" (duplicate address detection) for a few
/// seconds and Windows drops replies sent to it meanwhile. This point-to-point link has
/// no other hosts, so DAD is disabled; then wait for the address to become usable.
fn wait_until_usable(index: u32) {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, IpDadStatePreferred};
    unsafe {
        let mut luid: NET_LUID_LH = std::mem::zeroed();
        if ConvertInterfaceIndexToLuid(index, &mut luid) != 0 {
            return;
        }
        let mut ifrow: MIB_IPINTERFACE_ROW = std::mem::zeroed();
        InitializeIpInterfaceEntry(&mut ifrow);
        ifrow.Family = AF_INET;
        ifrow.InterfaceLuid = luid;
        if GetIpInterfaceEntry(&mut ifrow) == 0 {
            ifrow.DadTransmits = 0;
            ifrow.SitePrefixLength = 0; // must be 0 for IPv4 or SetIpInterfaceEntry fails
            let r = SetIpInterfaceEntry(&mut ifrow);
            if r != 0 {
                log::debug!("SetIpInterfaceEntry (DadTransmits=0) failed: {r}");
            }
        }
        let start = std::time::Instant::now();
        loop {
            let mut row: MIB_UNICASTIPADDRESS_ROW = std::mem::zeroed();
            InitializeUnicastIpAddressEntry(&mut row);
            row.InterfaceLuid = luid;
            row.Address.si_family = AF_INET;
            row.Address.Ipv4.sin_family = AF_INET;
            row.Address.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(LOCAL_IP.octets());
            if GetUnicastIpAddressEntry(&mut row) == 0 && row.DadState == IpDadStatePreferred {
                log::debug!("adapter address ready after {} ms", start.elapsed().as_millis());
                return;
            }
            if start.elapsed() > std::time::Duration::from_secs(15) {
                log::warn!("adapter address is still not ready (DAD state {}); continuing anyway", row.DadState);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

fn netsh(args: &[&str]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("netsh")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("running netsh")?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stdout);
        let err = String::from_utf8_lossy(&out.stderr);
        // "already exists" when adding and "Element not found" when deleting are not errors.
        if !(msg.contains("already exists") || msg.contains("Element not found") && args.contains(&"delete")) {
            bail!("netsh {} failed: {}{}", args.join(" "), msg.trim(), err.trim());
        }
    }
    Ok(())
}

/// Route the given IPv4 prefixes through the adapter; the routes are removed with the adapter.
pub fn add_routes(index: u32, prefixes: &[String]) -> Result<()> {
    let iface = format!("interface={index}");
    let hop = format!("nexthop={GATEWAY_IP}");
    for p in prefixes {
        let prefix = format!("prefix={p}");
        netsh(&["interface", "ipv4", "add", "route", &prefix, &iface, &hop, "metric=0", "store=active"])
            .with_context(|| format!("adding route {p}"))?;
    }
    Ok(())
}

/// A single 0.0.0.0/0 route through the adapter. With route metric 0 and interface metric 1
/// it takes precedence over the PC's other default routes without modifying them.
pub fn default_prefixes() -> Vec<String> {
    vec!["0.0.0.0/0".into()]
}

/// A host route added for a phone reached over Wi-Fi (see [`pin_host_route`]).
#[derive(Debug)]
pub struct PinnedRoute {
    prefix: String,
    iface: u32,
    hop: std::net::Ipv4Addr,
}

/// Over USB, adb traffic goes to 127.0.0.1 (the adb server) and cannot loop. Over Wi-Fi,
/// adb talks to the phone's IP, which must not be routed into the CatLink adapter. A phone
/// on the local subnet needs nothing, since the subnet route is more specific than the
/// default route. Otherwise a /32 route is pinned through the interface that reaches it now.
/// Must be called before [`add_routes`].
pub fn pin_host_route(ip: std::net::Ipv4Addr) -> Option<PinnedRoute> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_INET};
    let (iface, hop) = unsafe {
        let mut dst: SOCKADDR_INET = std::mem::zeroed();
        dst.si_family = AF_INET;
        dst.Ipv4.sin_family = AF_INET;
        dst.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
        let mut row: MIB_IPFORWARD_ROW2 = std::mem::zeroed();
        let mut src: SOCKADDR_INET = std::mem::zeroed();
        let r = GetBestRoute2(std::ptr::null(), 0, std::ptr::null(), &dst, 0, &mut row, &mut src);
        if r != 0 {
            log::warn!("no route to the phone at {ip} (error {r})");
            return None;
        }
        (row.InterfaceIndex, std::net::Ipv4Addr::from(row.NextHop.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()))
    };
    if hop.is_unspecified() {
        log::info!("phone at {ip} is on the local network (interface {iface})");
        return None;
    }
    let route = PinnedRoute { prefix: format!("{ip}/32"), iface, hop };
    let args = [
        "interface".to_string(),
        "ipv4".into(),
        "add".into(),
        "route".into(),
        format!("prefix={}", route.prefix),
        format!("interface={iface}"),
        format!("nexthop={hop}"),
        "metric=0".into(),
        "store=active".into(),
    ];
    match netsh(&args.iter().map(String::as_str).collect::<Vec<_>>()) {
        Ok(()) => {
            log::info!("keeping the phone at {ip} on interface {iface} via {hop}");
            Some(route)
        }
        Err(e) => {
            log::warn!("could not pin a route to the phone: {e:#}");
            None
        }
    }
}

pub fn unpin_host_route(r: &PinnedRoute) {
    let args = [
        "interface".to_string(),
        "ipv4".into(),
        "delete".into(),
        "route".into(),
        format!("prefix={}", r.prefix),
        format!("interface={}", r.iface),
        format!("nexthop={}", r.hop),
        "store=active".into(),
    ];
    let _ = netsh(&args.iter().map(String::as_str).collect::<Vec<_>>());
}
