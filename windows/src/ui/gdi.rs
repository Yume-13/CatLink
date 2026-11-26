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

//! Small GDI helpers: an offscreen 32-bit surface that tiny-skia draws into and GDI
//! writes ClearType text onto, fonts, and HICON creation.

use crate::ui::theme::colorref;
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn bitmap_info(w: i32, h: i32) -> BITMAPINFO {
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    bmi
}

pub struct Surface {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub w: i32,
    pub h: i32,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Option<Surface> {
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return None;
            }
            let bmi = bitmap_info(w, h);
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bmp);
            SetBkMode(dc, TRANSPARENT as _);
            Some(Surface { dc, bmp, old, bits: bits as *mut u8, w, h })
        }
    }

    /// Copy a (fully opaque) tiny-skia pixmap of the same size into the surface.
    pub fn load(&mut self, pm: &Pixmap) {
        let n = (self.w * self.h) as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, n * 4) };
        for (i, p) in pm.pixels().iter().take(n).enumerate() {
            dst[i * 4] = p.blue();
            dst[i * 4 + 1] = p.green();
            dst[i * 4 + 2] = p.red();
            dst[i * 4 + 3] = 255;
        }
    }

    pub fn text(&self, s: &str, r: RECT, font: HFONT, color: u32, flags: u32) {
        let mut w: Vec<u16> = s.encode_utf16().collect();
        let mut r = r;
        unsafe {
            SelectObject(self.dc, font);
            SetTextColor(self.dc, colorref(color));
            DrawTextW(self.dc, w.as_mut_ptr(), w.len() as i32, &mut r, flags | DT_NOPREFIX);
        }
    }

    pub fn measure(&self, s: &str, font: HFONT) -> i32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(self.dc, font);
            GetTextExtentPoint32W(self.dc, w.as_ptr(), w.len() as i32, &mut size);
        }
        size.cx
    }

    /// Snapshot as PNG (used by `--preview`).
    pub fn to_png(&self) -> Option<Vec<u8>> {
        let n = (self.w * self.h) as usize;
        let src = unsafe { std::slice::from_raw_parts(self.bits, n * 4) };
        let mut pm = Pixmap::new(self.w as u32, self.h as u32)?;
        for (i, px) in pm.data_mut().chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[src[i * 4 + 2], src[i * 4 + 1], src[i * 4], 255]);
        }
        pm.encode_png().ok()
    }

    pub fn present(&self, dst: HDC) {
        unsafe {
            BitBlt(dst, 0, 0, self.w, self.h, self.dc, 0, 0, SRCCOPY);
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

pub struct Font(pub HFONT);

impl Font {
    pub fn new(face: &str, px: f32, weight: i32) -> Font {
        let face = wide(face);
        let h = unsafe {
            CreateFontW(
                -(px.round() as i32),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                face.as_ptr(),
            )
        };
        Font(h)
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}

/// Build an HICON from a pixmap (premultiplied RGBA -> straight BGRA).
pub fn hicon_from_pixmap(pm: &Pixmap) -> HICON {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    unsafe {
        let bmi = bitmap_info(w, h);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(std::ptr::null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if color.is_null() {
            return std::ptr::null_mut();
        }
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for (i, p) in pm.pixels().iter().enumerate() {
            let c = p.demultiply();
            dst[i * 4] = c.blue();
            dst[i * 4 + 1] = c.green();
            dst[i * 4 + 2] = c.red();
            dst[i * 4 + 3] = c.alpha();
        }
        let mask_bits = vec![0u8; ((w + 15) / 16 * 2 * h) as usize];
        let mask = CreateBitmap(w, h, 1, 1, mask_bits.as_ptr() as *const _);
        let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii);
        DeleteObject(mask);
        DeleteObject(color);
        icon
    }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32, s: f32) -> RECT {
    RECT {
        left: (x * s).round() as i32,
        top: (y * s).round() as i32,
        right: ((x + w) * s).round() as i32,
        bottom: ((y + h) * s).round() as i32,
    }
}
