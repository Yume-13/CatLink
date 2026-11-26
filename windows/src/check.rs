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

//! `catlink-cli --check`: exercises every relay type on the phone without creating the
//! adapter (no Administrator rights needed).

use crate::proto::{self, KIND_DNS, KIND_ICMP, KIND_TCP, KIND_UDP};
use anyhow::{Context, Result, bail};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn dns_query(name: &str, id: u16) -> Vec<u8> {
    let mut q = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, 1 question
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // A, IN
    q
}

/// First A record in a DNS answer.
fn first_a(resp: &[u8]) -> Option<Ipv4Addr> {
    let qd = u16::from_be_bytes([*resp.get(4)?, *resp.get(5)?]);
    let an = u16::from_be_bytes([*resp.get(6)?, *resp.get(7)?]);
    let mut i = 12;
    let skip_name = |mut i: usize| -> Option<usize> {
        loop {
            let l = *resp.get(i)? as usize;
            if l == 0 {
                return Some(i + 1);
            }
            if l & 0xc0 == 0xc0 {
                return Some(i + 2);
            }
            i += l + 1;
        }
    };
    for _ in 0..qd {
        i = skip_name(i)? + 4;
    }
    for _ in 0..an {
        i = skip_name(i)?;
        let typ = u16::from_be_bytes([*resp.get(i)?, *resp.get(i + 1)?]);
        let len = u16::from_be_bytes([*resp.get(i + 8)?, *resp.get(i + 9)?]) as usize;
        i += 10;
        if typ == 1 && len == 4 {
            return Some(Ipv4Addr::new(resp[i], resp[i + 1], resp[i + 2], resp[i + 3]));
        }
        i += len;
    }
    None
}

async fn with_timeout<T>(secs: u64, f: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(secs), f).await.context("timed out")?
}

pub async fn run(port: u16) -> Result<()> {
    let mut failures = 0;

    let info = crate::engine::PhoneInfo::parse(&proto::hello(port, false).await?);
    println!("phone          {} ({})", info.summary(), info.via());

    // DNS through the phone's resolver.
    let t = Instant::now();
    let dns = with_timeout(8, async {
        let mut s = proto::open(port, KIND_DNS, SocketAddr::from(([10, 233, 85, 1], 53))).await?;
        proto::write_frame(&mut s, &dns_query("example.com", 0x4354)).await?;
        let mut buf = Vec::new();
        proto::read_frame(&mut s, &mut buf).await?.context("no answer")?;
        first_a(&buf).context("answer had no A record")
    })
    .await;
    let target = match dns {
        Ok(ip) => {
            println!("dns            ok   example.com = {ip} ({} ms)", t.elapsed().as_millis());
            ip
        }
        Err(e) => {
            failures += 1;
            println!("dns            FAIL {e:#}");
            Ipv4Addr::new(1, 1, 1, 1)
        }
    };

    // TCP: plain HTTP request.
    let t = Instant::now();
    let tcp = with_timeout(15, async {
        let mut s = proto::open(port, KIND_TCP, SocketAddr::from((target, 80))).await?;
        s.write_all(b"HEAD / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n").await?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await?;
        let line = String::from_utf8_lossy(&buf).lines().next().unwrap_or("").to_string();
        if !line.starts_with("HTTP/") {
            bail!("unexpected reply {line:?}");
        }
        Ok(line)
    })
    .await;
    match tcp {
        Ok(line) => println!("tcp            ok   {target}:80 -> {line} ({} ms)", t.elapsed().as_millis()),
        Err(e) => {
            failures += 1;
            println!("tcp            FAIL {e:#}");
        }
    }

    // UDP: raw DNS query to a public resolver.
    let t = Instant::now();
    let udp = with_timeout(8, async {
        let mut s = proto::open(port, KIND_UDP, SocketAddr::from(([1, 1, 1, 1], 53))).await?;
        proto::write_frame(&mut s, &dns_query("example.com", 0x5544)).await?;
        let mut buf = Vec::new();
        proto::read_frame(&mut s, &mut buf).await?.context("no reply")?;
        first_a(&buf).context("reply had no A record")
    })
    .await;
    match udp {
        Ok(ip) => println!("udp            ok   1.1.1.1:53 answered {ip} ({} ms)", t.elapsed().as_millis()),
        Err(e) => {
            failures += 1;
            println!("udp            FAIL {e:#}");
        }
    }

    // ICMP echo from the phone.
    let t = Instant::now();
    let ping = with_timeout(8, async {
        let mut s = proto::open(port, KIND_ICMP, SocketAddr::from(([1, 1, 1, 1], 0))).await?;
        proto::write_frame(&mut s, b"catlink-check").await?;
        let mut buf = Vec::new();
        proto::read_frame(&mut s, &mut buf).await?.context("no reply")?;
        Ok(())
    })
    .await;
    match ping {
        Ok(()) => println!("ping           ok   1.1.1.1 ({} ms)", t.elapsed().as_millis()),
        Err(e) => {
            failures += 1;
            println!("ping           FAIL {e:#}");
        }
    }

    if failures > 0 {
        bail!("{failures} check(s) failed");
    }
    println!("\nAll checks passed.");
    Ok(())
}
