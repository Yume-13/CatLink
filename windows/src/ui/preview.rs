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

//! `catlink.exe --preview <dir>`: renders the flyout and tray icons with sample data to
//! PNG files. Requires no phone or admin rights.

use super::flyout::{self, Fonts, Hit, ViewModel};
use super::gdi::Surface;
use super::glyph;
use super::theme::{self, Theme};
use catlink_core::engine::{Severity, State};
use std::path::Path;

fn sample_history() -> Vec<(f64, f64)> {
    (0..60)
        .map(|i| {
            let t = i as f64;
            let burst = if (20..34).contains(&i) { 3.6e6 } else { 0.0 };
            let down = 1.1e6 + 0.7e6 * (t / 4.0).sin().abs() + burst + if i > 50 { 1.8e6 } else { 0.0 };
            let up = 90e3 + 60e3 * (t / 3.0).cos().abs() + if (20..34).contains(&i) { 260e3 } else { 0.0 };
            (down, up)
        })
        .collect()
}

pub fn render(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let scale = 1.5f32;
    let connected = ViewModel {
        state: State::Connected,
        severity: Severity::Good,
        headline: "Example Phone \u{00b7} Android 15".into(),
        detail: "Using the phone\u{2019}s mobile data \u{00b7} Carrier \u{00b7} for 1 h 12 min".into(),
        history: sample_history(),
        open_flows: 23,
        latency_ms: Some(3),
        total_down: 1_288_490_188,
        total_up: 84_934_656,
        enabled: true,
        pinned: false,
        pairing: None,
    };
    let waiting = ViewModel {
        state: State::NoDevice,
        severity: Severity::Warning,
        headline: "No phone connected".into(),
        detail: State::NoDevice.hint(),
        history: vec![],
        open_flows: 0,
        latency_ms: None,
        total_down: 0,
        total_up: 0,
        enabled: true,
        pinned: false,
        pairing: None,
    };
    let paused = ViewModel {
        state: State::Paused,
        severity: Severity::Off,
        headline: "Sharing is paused".into(),
        detail: State::Paused.hint(),
        enabled: false,
        ..waiting_clone(&waiting)
    };
    let error = ViewModel {
        state: State::AppNotResponding,
        severity: Severity::Critical,
        headline: "The CatLink app isn\u{2019}t responding".into(),
        detail: State::AppNotResponding.hint(),
        ..waiting_clone(&waiting)
    };
    let pinned = ViewModel { pinned: true, ..waiting_clone(&connected) };
    let pairing = ViewModel {
        pinned: true,
        headline: "Pair a phone over Wi-Fi".into(),
        detail: "On the phone: Developer options › Wireless debugging › Pair device with QR code.".into(),
        pairing: Some(catlink_core::engine::Pairing {
            qr: "WIFI:T:ADB;S:catlink-482913;P:qhzkwpemtd;;".into(),
            stage: catlink_core::engine::PairStage::Waiting,
        }),
        ..waiting_clone(&waiting)
    };
    let fonts = Fonts::new(scale);
    let w = (flyout::WIDTH * scale).round() as i32;
    let h = (flyout::HEIGHT * scale).round() as i32;
    for (tname, theme) in [("light", Theme::light()), ("dark", Theme::dark())] {
        for (name, vm, hover) in [
            ("connected", &connected, Hit::None),
            ("connected-hover", &connected, Hit::Graph(27)),
            ("nophone", &waiting, Hit::None),
            ("paused", &paused, Hit::Pause),
            ("error", &error, Hit::None),
            ("pinned", &pinned, Hit::Close),
            ("pairing", &pairing, Hit::None),
        ] {
            let surf = Surface::new(w, h).ok_or_else(|| std::io::Error::other("surface"))?;
            flyout::paint_flyout(surf.dc, scale, &theme, &fonts, vm, hover);
            if let Some(png) = surf.to_png() {
                std::fs::write(dir.join(format!("flyout-{name}-{tname}.png")), png)?;
            }
        }
    }
    // Tray icons on dark and light taskbars, all states and a few utilization levels.
    for (tb, bg, fg, fill) in [("dark", 0x202020u32, 0xffffffu32, 0x5aa2f0u32), ("light", 0xf3f3f3, 0x1b1d21, 0x2a78d6)] {
        let cell = 48u32;
        let states = [Some(theme::GOOD), Some(theme::WARNING), Some(theme::CRITICAL), None];
        let mut strip = tiny_skia::Pixmap::new(cell * 6, cell * states.len() as u32).unwrap();
        strip.fill(glyph::rgb(bg));
        for (row, badge) in states.iter().enumerate() {
            for level in 0..6u32 {
                let mut icon = tiny_skia::Pixmap::new(32, 32).unwrap();
                let fgc = if badge.is_none() { glyph::rgba(fg, 0.55) } else { glyph::rgb(fg) };
                glyph::tray_glyph(&mut icon, 32.0, fgc, glyph::rgb(fill), level as f32 / 5.0, badge.map(glyph::rgb));
                strip.draw_pixmap(
                    (level * cell + 8) as i32,
                    (row as u32 * cell + 8) as i32,
                    icon.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
        if let Ok(png) = strip.encode_png() {
            std::fs::write(dir.join(format!("tray-{tb}.png")), png)?;
        }
        // Real tray sizes (100/125/150/200% scaling), blown up 4x with nearest-neighbor.
        let sizes = [16u32, 20, 24, 32];
        let mut real = tiny_skia::Pixmap::new(sizes.iter().map(|s| s * 4 + 16).sum::<u32>() + 16, 32 * 4 + 32).unwrap();
        real.fill(glyph::rgb(bg));
        let mut x = 16;
        for size in sizes {
            let mut icon = tiny_skia::Pixmap::new(size, size).unwrap();
            glyph::tray_glyph(&mut icon, size as f32, glyph::rgb(fg), glyph::rgb(fill), 0.6, Some(glyph::rgb(theme::GOOD)));
            let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
            real.draw_pixmap(0, 0, icon.as_ref(), &paint, tiny_skia::Transform::from_row(4.0, 0.0, 0.0, 4.0, x as f32, 16.0), None);
            x += size * 4 + 16;
        }
        if let Ok(png) = real.encode_png() {
            std::fs::write(dir.join(format!("tray-sizes-{tb}.png")), png)?;
        }
    }
    if let Some(icon) = glyph::app_icon(256)
        && let Ok(png) = icon.encode_png()
    {
        std::fs::write(dir.join("app-icon.png"), png)?;
    }
    // The app icon at the sizes Windows shows it (taskbar, title bar, Explorer), 4x nearest.
    let sizes = [16u32, 24, 32, 48];
    let mut strip = tiny_skia::Pixmap::new(sizes.iter().map(|s| s * 4 + 16).sum::<u32>() + 16, 48 * 4 + 32).unwrap();
    strip.fill(glyph::rgb(0xf3f3f3));
    let mut x = 16;
    for size in sizes {
        if let Some(icon) = glyph::app_icon(size) {
            let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
            strip.draw_pixmap(0, 0, icon.as_ref(), &paint, tiny_skia::Transform::from_row(4.0, 0.0, 0.0, 4.0, x as f32, 16.0), None);
        }
        x += size * 4 + 16;
    }
    if let Ok(png) = strip.encode_png() {
        std::fs::write(dir.join("app-icon-sizes.png"), png)?;
    }
    Ok(())
}

fn waiting_clone(v: &ViewModel) -> ViewModel {
    ViewModel {
        state: v.state.clone(),
        severity: v.severity,
        headline: v.headline.clone(),
        detail: v.detail.clone(),
        history: v.history.clone(),
        open_flows: v.open_flows,
        latency_ms: v.latency_ms,
        total_down: v.total_down,
        total_up: v.total_up,
        enabled: v.enabled,
        pinned: v.pinned,
        pairing: v.pairing.clone(),
    }
}
