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

//! Renders the app icon (.ico with PNG entries) and embeds it as resource #1.

#[path = "src/ui/glyph.rs"]
mod glyph;

use std::io::Write;

fn main() {
    println!("cargo:rerun-if-changed=src/ui/glyph.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 256];
    let pngs: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| glyph::app_icon(s).expect("render icon").encode_png().expect("png"))
        .collect();

    // ICO container: header, directory, then PNG payloads.
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, png) in sizes.iter().zip(&pngs) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bpp
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    let ico_path = out.join("catlink.ico");
    std::fs::write(&ico_path, &ico).unwrap();

    let rc_path = out.join("catlink.rc");
    let mut rc = std::fs::File::create(&rc_path).unwrap();
    let ico_str = ico_path.display().to_string().replace('\\', "\\\\");
    writeln!(rc, "1 ICON \"{ico_str}\"").unwrap();
    drop(rc);
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().unwrap();
}
