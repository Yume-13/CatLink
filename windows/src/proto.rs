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

//! CatLink wire protocol (one TCP stream per flow, carried by `adb forward`).
//!
//! Every stream opened by the PC starts with a request header:
//!
//! ```text
//! "CTLK"  u8 version  u8 kind  u8 family(4|6)  addr[4|16]  u16 port (big-endian)
//! ```
//!
//! The phone answers with one status byte (0 = OK). After that:
//! * `KIND_TCP`  – raw bytes in both directions.
//! * `KIND_UDP`, `KIND_DNS`, `KIND_ICMP` – frames of `u16 length` + payload, both directions.
//! * `KIND_HELLO` – the phone sends one frame with a UTF-8 description, then closes.
//!   The header's port field carries flags: bit 0 = the PC reaches the phone over
//!   Wi-Fi (wireless debugging or `adb tcpip`; the phone may then send traffic over mobile data).

use std::io;
use std::net::{IpAddr, SocketAddr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub const MAGIC: &[u8; 4] = b"CTLK";
pub const VERSION: u8 = 1;

pub const KIND_HELLO: u8 = 0;
pub const KIND_TCP: u8 = 1;
pub const KIND_UDP: u8 = 2;
pub const KIND_DNS: u8 = 3;
pub const KIND_ICMP: u8 = 4;

pub fn status_text(code: u8) -> &'static str {
    match code {
        0 => "ok",
        1 => "failed",
        2 => "connection refused",
        3 => "network unreachable",
        4 => "timed out",
        5 => "bad request",
        6 => "unsupported protocol version",
        _ => "unknown error",
    }
}

/// Open a new stream to the phone and send the request header.
/// Returns the stream once the phone reports success.
pub async fn open(port: u16, kind: u8, dst: SocketAddr) -> io::Result<TcpStream> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await?;
    s.set_nodelay(true)?;
    let mut hdr = Vec::with_capacity(28);
    hdr.extend_from_slice(MAGIC);
    hdr.push(VERSION);
    hdr.push(kind);
    match dst.ip() {
        IpAddr::V4(a) => {
            hdr.push(4);
            hdr.extend_from_slice(&a.octets());
        }
        IpAddr::V6(a) => {
            hdr.push(6);
            hdr.extend_from_slice(&a.octets());
        }
    }
    hdr.extend_from_slice(&dst.port().to_be_bytes());
    s.write_all(&hdr).await?;
    let status = s.read_u8().await.map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            io::Error::new(io::ErrorKind::NotConnected, "phone closed the stream (is the CatLink app running?)")
        } else {
            e
        }
    })?;
    if status != 0 {
        let kind = match status {
            2 => io::ErrorKind::ConnectionRefused,
            4 => io::ErrorKind::TimedOut,
            _ => io::ErrorKind::Other,
        };
        return Err(io::Error::new(kind, status_text(status)));
    }
    Ok(s)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> io::Result<()> {
    let len = u16::try_from(data.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut buf = Vec::with_capacity(2 + data.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(data);
    w.write_all(&buf).await
}

/// Reads one frame into `buf`. Returns `Ok(None)` on clean EOF.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<usize>> {
    let mut len = [0u8; 2];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u16::from_be_bytes(len) as usize;
    buf.resize(len, 0);
    r.read_exact(&mut buf[..len]).await?;
    Ok(Some(len))
}

/// Ask the phone for its description; used as a liveness probe. `wireless` tells the
/// phone how the PC is connected.
pub async fn hello(port: u16, wireless: bool) -> io::Result<String> {
    let dst = SocketAddr::from(([0, 0, 0, 0], u16::from(wireless)));
    let mut s = tokio::time::timeout(std::time::Duration::from_secs(3), open(port, KIND_HELLO, dst))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no answer from phone"))??;
    let mut buf = Vec::new();
    match tokio::time::timeout(std::time::Duration::from_secs(3), read_frame(&mut s, &mut buf)).await {
        Ok(Ok(Some(n))) => Ok(String::from_utf8_lossy(&buf[..n]).into_owned()),
        Ok(Ok(None)) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty hello")),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "no answer from phone")),
    }
}
