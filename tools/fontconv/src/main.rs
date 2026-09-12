// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Convert a TTF into the bitmap atlas `copilot::font` reads.
//!
//! ```text
//! fontconv --size 16 --range 32-126 DejaVuSansMono.ttf out.cpfn
//! ```
//!
//! A build tool rather than a runtime loader, so the target does a shift and a
//! mask and `copilot` keeps an empty dependency table. The cell is sized from
//! the glyphs actually requested rather than from the font's declared ascent
//! and descent, which reserve space the glyphs do not use — several wasted
//! rows per cell on a small panel.

mod pack;

use std::path::PathBuf;
use std::process::ExitCode;

use ab_glyph::{Font, FontVec, ScaleFont};

fn main() -> ExitCode {
    match run() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fontconv: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    input: PathBuf,
    output: PathBuf,
    size: f32,
    first: u32,
    last: u32,
    threshold: f32,
}

fn parse_args() -> Result<Args, String> {
    let mut size = 16.0f32;
    let mut range = (32u32, 126u32);
    let mut threshold = 0.5f32;
    let mut positional: Vec<PathBuf> = Vec::new();
    let mut it = std::env::args().skip(1);

    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                size = it
                    .next()
                    .ok_or("--size needs a value")?
                    .parse()
                    .map_err(|_| "--size must be a number")?;
            }
            "--range" => {
                let v = it.next().ok_or("--range needs a value, e.g. 32-126")?;
                let (a, b) = v.split_once('-').ok_or("--range must look like 32-126")?;
                range = (
                    a.parse().map_err(|_| "--range start must be a number")?,
                    b.parse().map_err(|_| "--range end must be a number")?,
                );
            }
            // Coverage below this becomes background. Lower keeps more of a
            // thin stroke at small sizes, at the cost of a fuzzier edge.
            "--threshold" => {
                threshold = it
                    .next()
                    .ok_or("--threshold needs a value")?
                    .parse()
                    .map_err(|_| "--threshold must be a number")?;
            }
            "-h" | "--help" => return Err(usage()),
            other => positional.push(PathBuf::from(other)),
        }
    }

    if positional.len() != 2 {
        return Err(usage());
    }
    if range.0 > range.1 {
        return Err("--range start is after its end".into());
    }
    if !(0.0..=1.0).contains(&threshold) {
        return Err("--threshold must be between 0 and 1".into());
    }
    Ok(Args {
        input: positional[0].clone(),
        output: positional[1].clone(),
        size,
        first: range.0,
        last: range.1,
        threshold,
    })
}

fn usage() -> String {
    "usage: fontconv [--size N] [--range A-B] [--threshold F] <in.ttf> <out.cpfn>\n\
     \n  --size       pixel size to rasterise at (default 16)\
     \n  --range      inclusive codepoint range (default 32-126, printable ASCII)\
     \n  --threshold  coverage above which a pixel is ink, 0..1 (default 0.5)"
        .into()
}

fn run() -> Result<String, String> {
    let args = parse_args()?;
    let bytes = std::fs::read(&args.input)
        .map_err(|e| format!("cannot read {}: {e}", args.input.display()))?;
    let font = FontVec::try_from_vec(bytes).map_err(|_| "not a font this build can read")?;
    let scaled = font.as_scaled(args.size);

    let chars: Vec<char> = (args.first..=args.last)
        .filter_map(char::from_u32)
        .collect();
    if chars.is_empty() {
        return Err("the requested range contains no characters".into());
    }

    // First pass: measure. The cell has to hold every glyph in the range, and
    // that is not knowable until all of them have been outlined.
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut any = false;
    for ch in &chars {
        let g = scaled.scaled_glyph(*ch);
        if let Some(outline) = font.outline_glyph(g) {
            let b = outline.px_bounds();
            min_x = min_x.min(b.min.x.floor() as i32);
            min_y = min_y.min(b.min.y.floor() as i32);
            max_x = max_x.max(b.max.x.ceil() as i32);
            max_y = max_y.max(b.max.y.ceil() as i32);
            any = true;
        }
    }
    if !any {
        return Err("no glyph in the range has an outline".into());
    }

    let cell_w = (max_x - min_x).clamp(1, 32) as u8;
    let cell_h = (max_y - min_y).clamp(1, 64) as u8;

    // Second pass: rasterise into the cell that was just measured.
    let mut glyphs: Vec<Vec<bool>> = Vec::with_capacity(chars.len());
    let mut clipped = 0usize;
    for ch in &chars {
        let mut cell = vec![false; cell_w as usize * cell_h as usize];
        let g = scaled.scaled_glyph(*ch);
        if let Some(outline) = font.outline_glyph(g) {
            let b = outline.px_bounds();
            let ox = b.min.x.floor() as i32 - min_x;
            let oy = b.min.y.floor() as i32 - min_y;
            let mut lost = false;
            outline.draw(|gx, gy, coverage| {
                let x = ox + gx as i32;
                let y = oy + gy as i32;
                if x < 0 || y < 0 || x >= cell_w as i32 || y >= cell_h as i32 {
                    // A glyph that will not fit is reported rather than
                    // silently cropped: a clipped descender is the kind of
                    // thing nobody notices until it ships.
                    if coverage >= args.threshold {
                        lost = true;
                    }
                    return;
                }
                if coverage >= args.threshold
                    && let Some(slot) = cell.get_mut(y as usize * cell_w as usize + x as usize)
                {
                    *slot = true;
                }
            });
            if lost {
                clipped += 1;
            }
        }
        glyphs.push(cell);
    }

    // The advance a monospace face declares, falling back to the cell width
    // plus a pixel of tracking when the font does not say.
    let advance = {
        let a = scaled.h_advance(font.glyph_id('0')).round() as i32;
        if a > 0 { a } else { i32::from(cell_w) + 1 }
    }
    .clamp(1, 255) as u8;
    let line_height = (i32::from(cell_h) + 2).clamp(1, 255) as u8;

    let atlas = pack::write_atlas(cell_w, cell_h, advance, line_height, args.first, &glyphs)
        .map_err(|e| format!("cannot pack the atlas: {e:?}"))?;
    std::fs::write(&args.output, &atlas)
        .map_err(|e| format!("cannot write {}: {e}", args.output.display()))?;

    let warn = if clipped > 0 {
        format!("  ({clipped} glyphs did not fit the cell and were cropped)")
    } else {
        String::new()
    };
    Ok(format!(
        "{}: {} glyphs, {}x{} cell, advance {}, {} bytes{warn}",
        args.output.display(),
        glyphs.len(),
        cell_w,
        cell_h,
        advance,
        atlas.len(),
    ))
}
