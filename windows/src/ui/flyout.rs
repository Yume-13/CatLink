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

//! The status flyout that opens from the tray icon: link state, live throughput with a
//! 60-second graph (hover to scrub), and session stats.

use crate::ui::gdi::{Font, Surface, rect};
use crate::ui::glyph::{self, fill_circle, fill_rect, fill_rrect, paint, stroke_rrect};
use crate::ui::theme::Theme;
use catlink_core::engine::{PairStage, Pairing, Severity, State, human_bytes, human_rate};
use tiny_skia::{FillRule, PathBuilder, Pixmap, Stroke, Transform};
use windows_sys::Win32::Graphics::Gdi::*;

pub const WIDTH: f32 = 360.0;
pub const HEIGHT: f32 = 436.0;
pub const HISTORY: usize = 60;

pub struct ViewModel {
    pub state: State,
    pub severity: Severity,
    pub headline: String,
    pub detail: String,
    /// Oldest first, one sample per second: (download, upload) in bytes/s.
    pub history: Vec<(f64, f64)>,
    pub open_flows: u64,
    pub latency_ms: Option<u32>,
    pub total_down: u64,
    pub total_up: u64,
    pub enabled: bool,
    /// Opened with a double-click: stays open, can be dragged, and has a close button.
    pub pinned: bool,
    /// Wi-Fi pairing in progress: the body shows the QR code instead of throughput.
    pub pairing: Option<Pairing>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    None,
    /// Index into history.
    Graph(usize),
    Pause,
    Quit,
    /// The × in the header of a pinned window.
    Close,
}

#[derive(Clone, Copy)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> R {
        R { x, y, w, h }
    }
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

// Layout, in logical pixels (multiplied by the monitor scale when drawing).
const P: f32 = 16.0;
const HEADER: R = R::new(P, 16.0, WIDTH - 2.0 * P, 28.0);
const HEADLINE: R = R::new(P, 54.0, WIDTH - 2.0 * P, 20.0);
const DETAIL: R = R::new(P, 74.0, WIDTH - 2.0 * P, 36.0);
const CARD: R = R::new(P, 116.0, WIDTH - 2.0 * P, 196.0);
const LEGEND_Y: f32 = 128.0;
const VALUES_Y: f32 = 145.0;
const SCALE_Y: f32 = 182.0;
const PLOT: R = R::new(P + 12.0, 202.0, WIDTH - 2.0 * P - 24.0, 82.0);
const XLABELS_Y: f32 = 288.0;
const STATS_Y: f32 = 326.0;
const SEP_Y: f32 = 378.0;
const PAUSE_BTN: R = R::new(P, 390.0, 168.0, 32.0);
const QUIT_BTN: R = R::new(WIDTH - P - 88.0, 390.0, 88.0, 32.0);

const CLOSE_BTN: R = R::new(WIDTH - P - 28.0, HEADER.y, 28.0, 28.0);

/// The part of a pinned window that drags it: everything above the throughput card,
/// except the close button.
pub fn is_drag_area(x: f32, y: f32) -> bool {
    y >= 0.0 && y < CARD.y && !CLOSE_BTN.contains(x, y)
}

pub fn hit_test(x: f32, y: f32, vm_len: usize, pinned: bool) -> Hit {
    if pinned && CLOSE_BTN.contains(x, y) {
        return Hit::Close;
    }
    if PAUSE_BTN.contains(x, y) {
        return Hit::Pause;
    }
    if QUIT_BTN.contains(x, y) {
        return Hit::Quit;
    }
    // Hit area covers the plot plus its labels.
    let area = R::new(PLOT.x - 6.0, PLOT.y - 18.0, PLOT.w + 12.0, PLOT.h + 36.0);
    if vm_len > 0 && area.contains(x, y) {
        let step = PLOT.w / (HISTORY - 1) as f32;
        let slot = ((x - PLOT.x) / step).round().clamp(0.0, (HISTORY - 1) as f32) as usize;
        // History is right-aligned (newest at the right edge).
        let first_slot = HISTORY - vm_len;
        let idx = slot.max(first_slot) - first_slot;
        return Hit::Graph(idx);
    }
    Hit::None
}

/// Graph ceiling: the smallest fixed step (64 KB/s, 128 KB/s, ... 1 GB/s) that is >= `v`.
fn nice_max(v: f64) -> f64 {
    const K: f64 = 1024.0;
    const M: f64 = K * K;
    [64.0 * K, 128.0 * K, 256.0 * K, 512.0 * K, M, 2.0 * M, 5.0 * M, 10.0 * M, 20.0 * M, 50.0 * M, 100.0 * M, 200.0 * M, 500.0 * M, 1024.0 * M]
        .into_iter()
        .find(|&m| m >= v)
        .unwrap_or(v)
}

pub struct Fonts {
    caption: Font,
    caption_semi: Font,
    body: Font,
    body_semi: Font,
    title: Font,
    big: Font,
}

impl Fonts {
    pub fn new(s: f32) -> Fonts {
        let face = "Segoe UI";
        Fonts {
            caption: Font::new(face, 12.0 * s, 400),
            caption_semi: Font::new(face, 12.0 * s, 600),
            body: Font::new(face, 14.0 * s, 400),
            body_semi: Font::new(face, 14.0 * s, 600),
            title: Font::new(face, 15.0 * s, 600),
            big: Font::new(face, 22.0 * s, 600),
        }
    }
}

pub fn paint_flyout(dst: HDC, s: f32, theme: &Theme, fonts: &Fonts, vm: &ViewModel, hover: Hit) {
    let (pw, ph) = ((WIDTH * s).round() as u32, (HEIGHT * s).round() as u32);
    let Some(mut pm) = Pixmap::new(pw, ph) else { return };
    let t = theme;
    pm.fill(t.c(t.bg));

    // ---- shapes (tiny-skia) -------------------------------------------------
    // App tile in the header.
    if let Some(tile) = glyph::app_icon((24.0 * s).round() as u32) {
        pm.draw_pixmap(
            (HEADER.x * s).round() as i32,
            ((HEADER.y + 2.0) * s).round() as i32,
            tile.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    // Status pill: status-colored dot plus text-colored label.
    let status_color = t.status(vm.severity);
    let pill_label = vm.state.title();
    // GDI surface, created here so text can be measured for layout.
    let mut surf = match Surface::new(pw as i32, ph as i32) {
        Some(x) => x,
        None => return,
    };
    let label_w = surf.measure(pill_label, fonts.caption_semi.0) as f32 / s;
    let pill_w = 10.0 + 8.0 + 6.0 + label_w + 12.0;
    let right = if vm.pinned { CLOSE_BTN.x - 6.0 } else { HEADER.x + HEADER.w };
    let pill = R::new(right - pill_w, HEADER.y + 2.0, pill_w, 24.0);
    if vm.pinned {
        if hover == Hit::Close {
            let c = CLOSE_BTN;
            fill_rrect(&mut pm, c.x * s, c.y * s, c.w * s, c.h * s, 6.0 * s, t.c(t.button_hover));
        }
        // The ×, 10 px across.
        let (cx, cy, r) = ((CLOSE_BTN.x + CLOSE_BTN.w / 2.0) * s, (CLOSE_BTN.y + CLOSE_BTN.h / 2.0) * s, 5.0 * s);
        let mut x = PathBuilder::new();
        x.move_to(cx - r, cy - r);
        x.line_to(cx + r, cy + r);
        x.move_to(cx + r, cy - r);
        x.line_to(cx - r, cy + r);
        if let Some(p) = x.finish() {
            let st = Stroke { width: 1.5 * s, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
            pm.stroke_path(&p, &paint(t.c(t.text2)), &st, Transform::identity(), None);
        }
    }
    fill_rrect(&mut pm, pill.x * s, pill.y * s, pill.w * s, pill.h * s, 12.0 * s, t.ca(status_color, if t.dark { 0.22 } else { 0.14 }));
    fill_circle(&mut pm, (pill.x + 14.0) * s, (pill.y + 12.0) * s, 4.0 * s, t.c(status_color));

    // Pixel cat sitting after the title, its feet level with the bottom of the app tile.
    let title_w = surf.measure("CatLink", fonts.title.0) as f32 / s;
    let px = (2.0 * s).round().max(1.0) as i32;
    let cat_h = glyph::CAT.len() as i32 * px;
    glyph::pixel_sprite(
        &mut pm,
        &glyph::CAT,
        ((HEADER.x + 32.0 + title_w + 8.0) * s).round() as i32,
        ((HEADER.y + 26.0) * s).round() as i32 - cat_h,
        px,
        t.c(t.text2),
        Some(t.c(t.up)),
    );

    if let Some(p) = &vm.pairing {
        paint_pairing(pm, surf, dst, s, t, fonts, vm, p, hover, (pill, pill_label, label_w));
        return;
    }

    // Throughput card.
    fill_rrect(&mut pm, CARD.x * s, CARD.y * s, CARD.w * s, CARD.h * s, 8.0 * s, t.c(t.card));
    stroke_rrect(&mut pm, CARD.x * s + 0.5, CARD.y * s + 0.5, CARD.w * s - 1.0, CARD.h * s - 1.0, 8.0 * s, 1.0, t.c(t.border));

    // Legend swatches.
    let col_w = (CARD.w - 24.0) / 2.0;
    let col_x = [CARD.x + 12.0, CARD.x + 12.0 + col_w];
    for (i, c) in [t.down, t.up].into_iter().enumerate() {
        fill_rrect(&mut pm, col_x[i] * s, (LEGEND_Y + 3.0) * s, 10.0 * s, 10.0 * s, 2.0 * s, t.c(c));
    }

    // Graph.
    let n = vm.history.len();
    let peak = vm.history.iter().map(|(d, u)| d.max(*u)).fold(0.0, f64::max);
    let ymax = nice_max(peak);
    for frac in [0.0f32, 0.5, 1.0] {
        if n == 0 && frac != 1.0 {
            continue;
        }
        let y = ((PLOT.y + PLOT.h * frac) * s).round();
        fill_rect(&mut pm, PLOT.x * s, y, PLOT.w * s, 1.0, t.c(if frac == 1.0 { t.border } else { t.grid }));
    }
    let step = PLOT.w / (HISTORY - 1) as f32;
    let point = |i: usize, v: f64| -> (f32, f32) {
        let slot = HISTORY - n + i;
        let x = (PLOT.x + slot as f32 * step) * s;
        let y = (PLOT.y + PLOT.h - (v / ymax).min(1.0) as f32 * PLOT.h) * s;
        (x, y)
    };
    if n >= 2 {
        for (series, color) in [(0usize, t.down), (1usize, t.up)] {
            let val = |i: usize| if series == 0 { vm.history[i].0 } else { vm.history[i].1 };
            // Area fill at 10% opacity; skipped while the series is all zero (empty shape).
            let has_area = (0..n).any(|i| val(i) > 0.0);
            let mut area = PathBuilder::new();
            let (x0, _) = point(0, 0.0);
            area.move_to(x0, (PLOT.y + PLOT.h) * s);
            for i in 0..n {
                let (x, y) = point(i, val(i));
                area.line_to(x, y);
            }
            let (xn, _) = point(n - 1, 0.0);
            area.line_to(xn, (PLOT.y + PLOT.h) * s);
            area.close();
            if has_area && let Some(p) = area.finish() {
                pm.fill_path(&p, &paint(t.ca(color, 0.10)), FillRule::Winding, Transform::identity(), None);
            }
            // 2px line.
            let mut line = PathBuilder::new();
            for i in 0..n {
                let (x, y) = point(i, val(i));
                if i == 0 { line.move_to(x, y) } else { line.line_to(x, y) }
            }
            if let Some(p) = line.finish() {
                let st = Stroke { width: 2.0 * s, line_join: tiny_skia::LineJoin::Round, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
                pm.stroke_path(&p, &paint(t.c(color)), &st, Transform::identity(), None);
            }
        }
    }
    // Hover crosshair + markers (8px dots with a 2px surface ring).
    let shown = match hover {
        Hit::Graph(i) if i < n => Some(i),
        _ => None,
    };
    if let Some(i) = shown {
        let (x, _) = point(i, 0.0);
        fill_rect(&mut pm, x - 0.5 * s, PLOT.y * s, 1.0 * s, PLOT.h * s, t.c(t.muted));
        for (v, color) in [(vm.history[i].0, t.down), (vm.history[i].1, t.up)] {
            let (x, y) = point(i, v);
            fill_circle(&mut pm, x, y, 6.0 * s, t.c(t.card));
            fill_circle(&mut pm, x, y, 4.0 * s, t.c(color));
        }
    }

    // Separator and buttons.
    fill_rect(&mut pm, P * s, (SEP_Y * s).round(), (WIDTH - 2.0 * P) * s, 1.0, t.c(t.border));
    for (r, hit) in [(PAUSE_BTN, Hit::Pause), (QUIT_BTN, Hit::Quit)] {
        let bg = if hover == hit { t.button_hover } else { t.button };
        fill_rrect(&mut pm, r.x * s, r.y * s, r.w * s, r.h * s, 6.0 * s, t.c(bg));
        stroke_rrect(&mut pm, r.x * s + 0.5, r.y * s + 0.5, r.w * s - 1.0, r.h * s - 1.0, 6.0 * s, 1.0, t.c(t.button_border));
    }

    // ---- text (GDI, ClearType) ----------------------------------------------
    surf.load(&pm);
    let single = DT_SINGLELINE | DT_VCENTER;
    let rr = |x: f32, y: f32, w: f32, h: f32| rect(x, y, w, h, s);

    surf.text("CatLink", rr(HEADER.x + 32.0, HEADER.y, 120.0, HEADER.h), fonts.title.0, t.text, single);
    surf.text(pill_label, rr(pill.x + 24.0, pill.y, label_w + 4.0, pill.h), fonts.caption_semi.0, t.text, single);

    surf.text(&vm.headline, rr(HEADLINE.x, HEADLINE.y, HEADLINE.w, HEADLINE.h), fonts.body_semi.0, t.text, single | DT_END_ELLIPSIS);
    surf.text(&vm.detail, rr(DETAIL.x, DETAIL.y, DETAIL.w, DETAIL.h), fonts.caption.0, t.text2, DT_WORDBREAK | DT_END_ELLIPSIS);

    // Values: the hovered second, or the latest one.
    let sample = shown.or(n.checked_sub(1)).map(|i| vm.history[i]);
    let connected = vm.state == State::Connected;
    for (i, label) in ["Download", "Upload"].into_iter().enumerate() {
        surf.text(label, rr(col_x[i] + 16.0, LEGEND_Y, col_w - 16.0, 16.0), fonts.caption.0, t.text2, single);
        let value = match (connected || shown.is_some(), sample) {
            (true, Some((d, u))) => human_rate(if i == 0 { d } else { u }),
            _ => "\u{2014}".to_string(),
        };
        surf.text(&value, rr(col_x[i], VALUES_Y, col_w, 30.0), fonts.big.0, t.text, single);
    }
    if n > 0 {
        surf.text(&human_rate(ymax).replace(".0 ", " "), rr(PLOT.x, SCALE_Y, 120.0, 16.0), fonts.caption.0, t.muted, single);
    } else {
        let msg = if connected { "Waiting for traffic\u{2026}" } else { "Throughput shows here while connected" };
        surf.text(msg, rr(PLOT.x, PLOT.y, PLOT.w, PLOT.h), fonts.caption.0, t.muted, single | DT_CENTER);
    }
    if let Some(i) = shown {
        let ago = n - 1 - i;
        let label = if ago == 0 { "Now".to_string() } else { format!("{ago} s ago") };
        surf.text(&label, rr(PLOT.x + PLOT.w - 120.0, SCALE_Y, 120.0, 16.0), fonts.caption_semi.0, t.text2, single | DT_RIGHT);
    }
    surf.text("60 s ago", rr(PLOT.x, XLABELS_Y, 100.0, 16.0), fonts.caption.0, t.muted, single);
    surf.text("Now", rr(PLOT.x + PLOT.w - 100.0, XLABELS_Y, 100.0, 16.0), fonts.caption.0, t.muted, single | DT_RIGHT);

    // Stats row.
    let stats: [(&str, String, f32); 3] = [
        ("Connections", if connected { format!("{} open", vm.open_flows) } else { "\u{2014}".into() }, 96.0),
        ("Latency", vm.latency_ms.map(|l| format!("{l} ms")).unwrap_or_else(|| "\u{2014}".into()), 80.0),
        (
            "This session",
            if vm.total_down + vm.total_up == 0 {
                "\u{2014}".into()
            } else {
                format!("\u{2193} {}  \u{2191} {}", human_bytes(vm.total_down), human_bytes(vm.total_up))
            },
            WIDTH - 2.0 * P - 176.0,
        ),
    ];
    let mut x = P;
    for (cap, val, w) in stats {
        surf.text(cap, rr(x, STATS_Y, w, 16.0), fonts.caption.0, t.muted, single);
        surf.text(&val, rr(x, STATS_Y + 18.0, w, 20.0), fonts.body_semi.0, t.text, single | DT_END_ELLIPSIS);
        x += w;
    }

    let pause_label = if vm.enabled { "Pause sharing" } else { "Resume sharing" };
    surf.text(pause_label, rr(PAUSE_BTN.x, PAUSE_BTN.y, PAUSE_BTN.w, PAUSE_BTN.h), fonts.body.0, t.text, single | DT_CENTER);
    surf.text("Quit", rr(QUIT_BTN.x, QUIT_BTN.y, QUIT_BTN.w, QUIT_BTN.h), fonts.body.0, t.text, single | DT_CENTER);

    surf.present(dst);
}

const QR_BOX: R = R::new((WIDTH - 196.0) / 2.0, 120.0, 196.0, 196.0);
const PAIR_STAGE_Y: f32 = 324.0;
const PAIR_NOTE_Y: f32 = 344.0;

/// Label for the left button while pairing.
pub fn pairing_button(stage: &PairStage) -> &'static str {
    match stage {
        PairStage::Done(_) => "Done",
        PairStage::Failed(_) => "Close",
        _ => "Cancel",
    }
}

/// The pairing view: a QR code for Developer options › Wireless debugging › Pair device
/// with QR code, the progress below it, and Cancel / Done in place of Pause.
#[allow(clippy::too_many_arguments)]
fn paint_pairing(
    mut pm: Pixmap,
    mut surf: Surface,
    dst: HDC,
    s: f32,
    t: &Theme,
    fonts: &Fonts,
    vm: &ViewModel,
    p: &Pairing,
    hover: Hit,
    (pill, pill_label, label_w): (R, &str, f32),
) {
    // QR code is always dark on white regardless of theme, for camera readability.
    let b = QR_BOX;
    fill_rrect(&mut pm, b.x * s, b.y * s, b.w * s, b.h * s, 10.0 * s, glyph::rgb(0xffffff));
    stroke_rrect(&mut pm, b.x * s + 0.5, b.y * s + 0.5, b.w * s - 1.0, b.h * s - 1.0, 10.0 * s, 1.0, t.c(t.border));
    if let Ok(code) = qrcode::QrCode::with_error_correction_level(p.qr.as_bytes(), qrcode::EcLevel::M) {
        let n = code.width() as i32;
        let quiet = 2;
        // Whole device pixels per module, so the code stays crisp.
        let m = (((b.w - 8.0) * s) as i32 / (n + 2 * quiet)).max(1);
        let size = m * n;
        let x0 = (b.x * s).round() as i32 + ((b.w * s).round() as i32 - size) / 2;
        let y0 = (b.y * s).round() as i32 + ((b.h * s).round() as i32 - size) / 2;
        let mut dark = paint(glyph::rgb(0x111111));
        dark.anti_alias = false;
        for (i, c) in code.to_colors().iter().enumerate() {
            if *c == qrcode::Color::Dark {
                let (col, row) = (i as i32 % n, i as i32 / n);
                if let Some(r) = tiny_skia::Rect::from_xywh((x0 + col * m) as f32, (y0 + row * m) as f32, m as f32, m as f32) {
                    pm.fill_rect(r, &dark, Transform::identity(), None);
                }
            }
        }
    }

    // Separator and buttons.
    fill_rect(&mut pm, P * s, (SEP_Y * s).round(), (WIDTH - 2.0 * P) * s, 1.0, t.c(t.border));
    for (r, hit) in [(PAUSE_BTN, Hit::Pause), (QUIT_BTN, Hit::Quit)] {
        let bg = if hover == hit { t.button_hover } else { t.button };
        fill_rrect(&mut pm, r.x * s, r.y * s, r.w * s, r.h * s, 6.0 * s, t.c(bg));
        stroke_rrect(&mut pm, r.x * s + 0.5, r.y * s + 0.5, r.w * s - 1.0, r.h * s - 1.0, 6.0 * s, 1.0, t.c(t.button_border));
    }

    surf.load(&pm);
    let single = DT_SINGLELINE | DT_VCENTER;
    let rr = |x: f32, y: f32, w: f32, h: f32| rect(x, y, w, h, s);
    surf.text("CatLink", rr(HEADER.x + 32.0, HEADER.y, 120.0, HEADER.h), fonts.title.0, t.text, single);
    surf.text(pill_label, rr(pill.x + 24.0, pill.y, label_w + 4.0, pill.h), fonts.caption_semi.0, t.text, single);
    surf.text(&vm.headline, rr(HEADLINE.x, HEADLINE.y, HEADLINE.w, HEADLINE.h), fonts.body_semi.0, t.text, single | DT_END_ELLIPSIS);
    surf.text(&vm.detail, rr(DETAIL.x, DETAIL.y, DETAIL.w, DETAIL.h), fonts.caption.0, t.text2, DT_WORDBREAK | DT_END_ELLIPSIS);

    let (stage, note) = match &p.stage {
        PairStage::Waiting => (
            "Waiting for the phone to scan the code\u{2026}".to_string(),
            "The phone and this PC need to be on the same network.",
        ),
        PairStage::Pairing => ("Pairing\u{2026}".to_string(), "Keep the phone on this screen for a moment."),
        PairStage::Connecting => ("Connecting over Wi-Fi\u{2026}".to_string(), "Almost done."),
        PairStage::Done(name) => (
            format!("Paired with {name}"),
            "CatLink now uses Wi-Fi whenever the cable isn\u{2019}t plugged in.",
        ),
        PairStage::Failed(why) => (why.clone(), "Make sure wireless debugging is on, then try again."),
    };
    surf.text(&stage, rr(P, PAIR_STAGE_Y, WIDTH - 2.0 * P, 20.0), fonts.body_semi.0, t.text, single | DT_CENTER | DT_END_ELLIPSIS);
    surf.text(note, rr(P, PAIR_NOTE_Y, WIDTH - 2.0 * P, 32.0), fonts.caption.0, t.text2, DT_WORDBREAK | DT_CENTER);

    let label = pairing_button(&p.stage);
    surf.text(label, rr(PAUSE_BTN.x, PAUSE_BTN.y, PAUSE_BTN.w, PAUSE_BTN.h), fonts.body.0, t.text, single | DT_CENTER);
    surf.text("Quit", rr(QUIT_BTN.x, QUIT_BTN.y, QUIT_BTN.w, QUIT_BTN.h), fonts.body.0, t.text, single | DT_CENTER);
    surf.present(dst);
}
