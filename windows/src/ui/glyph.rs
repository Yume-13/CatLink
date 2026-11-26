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

//! Vector drawing shared by the tray icon (runtime) and the exe icon (build.rs).
//! Only depends on tiny-skia so build.rs can `#[path]`-include it.

#![allow(dead_code)]

use tiny_skia::{BlendMode, Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub fn rgb(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

pub fn rgba(hex: u32, alpha: f32) -> Color {
    let mut c = rgb(hex);
    c.set_alpha(alpha);
    c
}

pub fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// Rounded rectangle path (circular corners approximated with cubics).
pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn fill_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if let Some(p) = rrect(x, y, w, h, r) {
        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

pub fn stroke_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
    if let Some(p) = rrect(x, y, w, h, r) {
        let s = Stroke { width, ..Stroke::default() };
        pm.stroke_path(&p, &paint(c), &s, Transform::identity(), None);
    }
}

pub fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

/// Punch a transparent hole (used to separate the status badge from the glyph).
pub fn clear_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32) {
    if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
        let mut pt = paint(Color::BLACK);
        pt.blend_mode = BlendMode::Clear;
        pm.fill_path(&p, &pt, FillRule::Winding, Transform::identity(), None);
    }
}

pub fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

/// Small sitting cat (7 x 8) for the tray and app icons: ears, face, body and a curled tail.
/// `o` marks the eyes.
pub const CAT_TINY: [&str; 8] = [
    "#...#..",
    "#####..",
    "#o#o#..",
    "#####..",
    ".###...",
    "#####.#",
    "#####.#",
    "######.",
];

/// Sitting cat with a curled tail (11 x 11), for the status panel header.
pub const CAT: [&str; 11] = [
    "#.....#....",
    "##...##....",
    "#######....",
    "##o#o##....",
    "#######....",
    ".#####.....",
    ".#####....#",
    "#######...#",
    "#######...#",
    "#######..#.",
    ".########..",
];

/// Draws pixel art with square, unsmoothed pixels of `px` device pixels each, top-left at
/// (`x`, `y`). `#` is `body`; `o` is `eyes`, or a transparent hole when `eyes` is `None`.
pub fn pixel_sprite(pm: &mut Pixmap, sprite: &[&str], x: i32, y: i32, px: i32, body: Color, eyes: Option<Color>) {
    let mut solid = paint(body);
    solid.anti_alias = false;
    let mut hole = paint(Color::BLACK);
    hole.anti_alias = false;
    hole.blend_mode = BlendMode::Clear;
    let eye = eyes.map(|c| {
        let mut p = paint(c);
        p.anti_alias = false;
        p
    });
    for (row, line) in sprite.iter().enumerate() {
        for (col, ch) in line.bytes().enumerate() {
            let p = match (ch, &eye) {
                (b'#', _) => &solid,
                (b'o', Some(e)) => e,
                (b'o', None) => &hole,
                _ => continue,
            };
            let (cx, cy) = (x + col as i32 * px, y + row as i32 * px);
            if let Some(r) = Rect::from_xywh(cx as f32, cy as f32, px as f32, px as f32) {
                pm.fill_rect(r, p, Transform::identity(), None);
            }
        }
    }
}

/// Size in device pixels of one sprite pixel for a tray icon of `size` pixels.
fn tray_px(size: f32) -> i32 {
    ((size / 16.0).floor() as i32).max(1)
}

/// The tray icon: a slim phone on the left, the tiny cat in the top-right corner and
/// the status badge in the bottom-right corner.
pub fn tray_glyph(pm: &mut Pixmap, size: f32, fg: Color, fill: Color, level: f32, badge: Option<Color>) {
    let s = size / 16.0;
    let stroke = (1.25 * s).max(1.0);
    let half = stroke / 2.0;
    let (bx, by, bw, bh) = (1.0 * s, 1.0 * s, 7.0 * s, 14.0 * s);
    stroke_rrect(pm, bx + half, by + half, bw - stroke, bh - stroke, 2.0 * s, stroke, fg);
    let inset = stroke + 0.75 * s;
    let (sx, sy, sw, sh) = (bx + inset, by + inset + 1.0 * s, bw - 2.0 * inset, bh - 2.0 * inset - 1.0 * s);
    let level = level.clamp(0.0, 1.0);
    if level > 0.0 {
        let h = (sh * level).max(1.0 * s);
        fill_rrect(pm, sx, sy + sh - h, sw, h, 0.7 * s, fill);
    }
    let px = tray_px(size);
    let cat_w = CAT_TINY[0].len() as i32 * px;
    pixel_sprite(pm, &CAT_TINY, size as i32 - cat_w, 0, px, fg, None);
    if let Some(b) = badge {
        let (cx, cy, r) = (12.5 * s, 12.5 * s, 3.2 * s);
        clear_circle(pm, cx, cy, r + 1.2 * s);
        fill_circle(pm, cx, cy, r, b);
    }
}

/// Full-color app icon (exe / window / panel header): phone and pixel cat on a blue tile.
pub fn app_icon(size: u32) -> Option<Pixmap> {
    let mut pm = Pixmap::new(size, size)?;
    let s = size as f32;
    fill_rrect(&mut pm, 0.0, 0.0, s, s, s * 0.22, rgb(0x2a78d6));
    // Layout in fractions of the tile (same geometry as ic_launcher_foreground.xml):
    // a phone on the left and the pixel cat sitting beside it.
    let white = rgb(0xffffff);
    let stroke = (s * 0.0556).max(1.0);
    let (x0, y0, x1, y1) = (s * 0.208, s * 0.222, s * 0.486, s * 0.778);
    let h = stroke / 2.0;
    stroke_rrect(&mut pm, x0 + h, y0 + h, x1 - x0 - stroke, y1 - y0 - stroke, s * 0.083, stroke, white);
    let (sx0, sy0, sx1, sy1) = (s * 0.292, s * 0.472, s * 0.403, s * 0.694);
    fill_rrect(&mut pm, sx0, sy0, sx1 - sx0, sy1 - sy0, s * 0.028, rgb(0x7fe0bd));
    // The cat uses whole device pixels; its eyes are drawn in the tile color.
    let px = ((s * 0.0382).round() as i32).max(1);
    let bottom = (s * 0.778).round() as i32;
    let left = (s * 0.528).round() as i32;
    let blue = rgb(0x2a78d6);
    pixel_sprite(&mut pm, &CAT_TINY, left, bottom - CAT_TINY.len() as i32 * px, px, white, Some(blue));
    Some(pm)
}
