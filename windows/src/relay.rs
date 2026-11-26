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

//! Turns packets from the virtual adapter into per-flow streams to the phone.

use crate::net::GATEWAY_IP;
use crate::proto::{self, KIND_DNS, KIND_ICMP, KIND_TCP, KIND_UDP};
use ipstack::{IpNumber, IpStack, IpStackConfig, IpStackStream, IpStackTcpStream, IpStackUdpStream, IpStackUnknownTransport, TcpConfig};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering::Relaxed};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};

#[derive(Default)]
pub struct Shared {
    /// Local port of the adb forward, 0 while the phone is unavailable.
    pub port: AtomicU16,
    pub tcp_active: AtomicU64,
    pub udp_active: AtomicU64,
    pub tcp_total: AtomicU64,
    pub failed_total: AtomicU64,
    pub bytes_up: AtomicU64,
    pub bytes_down: AtomicU64,
}

impl Shared {
    fn port(&self) -> Option<u16> {
        match self.port.load(Relaxed) {
            0 => None,
            p => Some(p),
        }
    }
}

struct Gauge<'a>(&'a AtomicU64);
impl<'a> Gauge<'a> {
    fn new(a: &'a AtomicU64) -> Self {
        a.fetch_add(1, Relaxed);
        Gauge(a)
    }
}
impl Drop for Gauge<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Relaxed);
    }
}

/// Stream wrapper that counts bytes for the live statistics.
struct Counted<S> {
    inner: S,
    shared: Arc<Shared>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        let n = buf.filled().len() - before;
        self.shared.bytes_down.fetch_add(n as u64, Relaxed);
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = r {
            self.shared.bytes_up.fetch_add(n as u64, Relaxed);
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Traffic that only makes sense on a real LAN (discovery, broadcasts) is dropped
/// instead of being sent to the phone.
fn is_local_only(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            a.is_multicast()
                || a.is_broadcast()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_loopback()
                || (a.octets()[..3] == GATEWAY_IP.octets()[..3] && a.octets()[3] == 255)
        }
        IpAddr::V6(a) => a.is_multicast() || a.is_unspecified() || a.is_loopback() || a.is_unicast_link_local(),
    }
}

pub async fn run_stack<D>(device: D, mtu: u16, shared: Arc<Shared>) -> anyhow::Result<()>
where
    D: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut cfg = IpStackConfig::default();
    cfg.mtu(mtu)?;
    cfg.udp_timeout(Duration::from_secs(90));
    let mut tcp = TcpConfig::default();
    // Idle timeout after which ipstack closes a TCP session (2 h).
    tcp.timeout = Duration::from_secs(2 * 60 * 60);
    cfg.with_tcp_config(tcp);
    let mut stack = IpStack::new(cfg, device);
    loop {
        match stack.accept().await? {
            IpStackStream::Tcp(t) => {
                tokio::spawn(handle_tcp(t, shared.clone()));
            }
            IpStackStream::Udp(u) => {
                tokio::spawn(handle_udp(u, shared.clone()));
            }
            IpStackStream::UnknownTransport(u) => {
                if u.ip_protocol() == IpNumber::ICMP && u.dst_addr().is_ipv4() {
                    tokio::spawn(handle_ping(u, shared.clone()));
                } else {
                    log::trace!("ignoring IP protocol {:?} to {}", u.ip_protocol(), u.dst_addr());
                }
            }
            IpStackStream::UnknownNetwork(_) => {}
        }
    }
}

async fn handle_tcp(t: IpStackTcpStream, shared: Arc<Shared>) {
    let dst = t.peer_addr();
    let Some(port) = shared.port() else {
        return;
    };
    if is_local_only(dst.ip()) || dst.ip() == IpAddr::V4(GATEWAY_IP) {
        return;
    }
    let _g = Gauge::new(&shared.tcp_active);
    shared.tcp_total.fetch_add(1, Relaxed);
    let phone = match proto::open(port, KIND_TCP, dst).await {
        Ok(s) => s,
        Err(e) => {
            shared.failed_total.fetch_add(1, Relaxed);
            log::debug!("tcp {dst}: {e}");
            return;
        }
    };
    log::debug!("tcp {} -> {dst} open", t.local_addr());
    let phone = Counted { inner: phone, shared: shared.clone() };
    let (pr, mut pw) = tokio::io::split(phone);
    let (tr, mut tw) = tokio::io::split(t);
    let mut pr = BufReader::with_capacity(64 * 1024, pr);
    let mut tr = BufReader::with_capacity(64 * 1024, tr);
    // A PC-side FIN is not forwarded (adb streams have no half-close); receiving
    // continues until the server side finishes.
    let mut up = std::pin::pin!(async {
        let r = tokio::io::copy_buf(&mut tr, &mut pw).await;
        let _ = pw.flush().await;
        r
    });
    let mut down = std::pin::pin!(async {
        let r = tokio::io::copy_buf(&mut pr, &mut tw).await;
        let _ = tw.shutdown().await;
        r
    });
    tokio::select! {
        r = &mut down => {
            log::debug!("tcp {dst}: server side done ({r:?})");
            // Allow the PC app a short time to finish and close too.
            let _ = tokio::time::timeout(Duration::from_secs(10), &mut up).await;
        }
        r = &mut up => {
            log::debug!("tcp {dst}: PC side done ({r:?})");
            if r.is_ok() {
                let _ = tokio::time::timeout(Duration::from_secs(120), &mut down).await;
            }
        }
    }
}

async fn handle_udp(udp: IpStackUdpStream, shared: Arc<Shared>) {
    let dst = udp.peer_addr();
    let Some(port) = shared.port() else {
        return;
    };
    if is_local_only(dst.ip()) {
        return;
    }
    let kind = if dst.ip() == IpAddr::V4(GATEWAY_IP) {
        if dst.port() != 53 {
            return;
        }
        KIND_DNS
    } else {
        KIND_UDP
    };
    let _g = Gauge::new(&shared.udp_active);
    log::trace!("udp {} -> {dst} new flow", udp.local_addr());
    let phone = match proto::open(port, kind, dst).await {
        Ok(s) => s,
        Err(e) => {
            shared.failed_total.fetch_add(1, Relaxed);
            log::debug!("udp {dst}: {e}");
            return;
        }
    };
    let (mut pr, mut pw) = phone.into_split();
    let (mut ur, mut uw) = tokio::io::split(udp);
    let sh = shared.clone();
    let up = async move {
        let mut buf = vec![0u8; 65535];
        loop {
            let n = ur.read(&mut buf).await?;
            if n == 0 {
                return Ok::<_, io::Error>(());
            }
            log::trace!("udp {dst}: {n} bytes up");
            proto::write_frame(&mut pw, &buf[..n]).await?;
            sh.bytes_up.fetch_add(n as u64, Relaxed);
        }
    };
    let down = async {
        let mut buf = Vec::new();
        while let Some(n) = proto::read_frame(&mut pr, &mut buf).await? {
            // One write == one datagram, so never use write_all here.
            log::trace!("udp {dst}: {n} bytes down");
            uw.write(&buf[..n]).await?;
            shared.bytes_down.fetch_add(n as u64, Relaxed);
        }
        Ok::<_, io::Error>(())
    };
    // DNS flows are closed after 15 s; other UDP flows end on the stack's idle timeout.
    let limit = if kind == KIND_DNS { Duration::from_secs(15) } else { Duration::MAX };
    let flow = async {
        tokio::select! {
            r = up => log::trace!("udp {dst} up ended: {r:?}"),
            r = down => log::trace!("udp {dst} down ended: {r:?}"),
        }
    };
    let _ = tokio::time::timeout(limit, flow).await;
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 { u16::from_be_bytes([chunk[0], chunk[1]]) } else { u16::from_be_bytes([chunk[0], 0]) };
        sum += word as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// ICMP echo: the phone sends a real ping (unprivileged ICMP socket) and reports
/// whether a reply came back; the echo reply to the PC is then generated locally.
async fn handle_ping(u: IpStackUnknownTransport, shared: Arc<Shared>) {
    let req = u.payload().to_vec();
    if req.len() < 8 || req[0] != 8 || req[1] != 0 {
        return;
    }
    let Some(port) = shared.port() else {
        return;
    };
    let dst = SocketAddr::new(u.dst_addr(), 0);
    if is_local_only(dst.ip()) {
        return;
    }
    let result = async {
        let mut s = proto::open(port, KIND_ICMP, dst).await?;
        proto::write_frame(&mut s, &req[8..]).await?;
        let mut buf = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), proto::read_frame(&mut s, &mut buf))
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
    .await;
    if let Ok(Some(_)) = result {
        let mut reply = req;
        reply[0] = 0; // echo reply
        reply[2] = 0;
        reply[3] = 0;
        let c = checksum(&reply);
        reply[2..4].copy_from_slice(&c.to_be_bytes());
        let _ = u.send(reply);
    }
}
