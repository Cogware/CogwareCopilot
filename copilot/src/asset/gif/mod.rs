// SPDX-License-Identifier: MIT OR Apache-2.0
//! GIF decoding.
//!
//! GIF rather than a video codec because it is the one animated format small
//! enough to own outright: a palette, a block structure, and LZW. Everything
//! after that is bookkeeping.

pub mod lzw;

use alloc::vec::Vec;

use crate::Color;

/// Why a GIF could not be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GifError {
    /// The signature was not GIF87a or GIF89a.
    BadMagic,
    /// The file ended inside a structure.
    UnexpectedEnd,
    /// A header field held an impossible value.
    BadHeader,
    /// A feature this decoder does not implement.
    Unsupported,
    /// The compressed data could not be decoded.
    Corrupt,
}

/// A GIF's logical screen, and its global palette if it has one.
#[derive(Clone, Debug)]
pub struct Screen {
    /// Logical screen width in pixels.
    pub width: u16,
    /// Logical screen height in pixels.
    pub height: u16,
    /// Index into the global palette used for pixels no frame covers.
    pub background: u8,
    /// The global colour table, empty if the file has none.
    pub palette: Vec<crate::Color>,
}

/// Read the header, logical screen descriptor and global colour table.
///
/// Returns the parsed screen and the byte offset just past everything it
/// consumed, so the caller can continue from there.
pub fn screen(bytes: &[u8]) -> Result<(Screen, usize), GifError> {
    let magic = bytes.get(0..6).ok_or(GifError::UnexpectedEnd)?;
    if magic != b"GIF87a" && magic != b"GIF89a" {
        return Err(GifError::BadMagic);
    }

    let lsd_start = 6;
    let lsd = bytes
        .get(lsd_start..lsd_start + 7)
        .ok_or(GifError::UnexpectedEnd)?;

    let width = u16::from_le_bytes([lsd[0], lsd[1]]);
    let height = u16::from_le_bytes([lsd[2], lsd[3]]);

    // Zero dimensions are invalid per the GIF spec.
    if width == 0 || height == 0 {
        return Err(GifError::BadHeader);
    }

    let packed = lsd[4];
    let has_gct = packed & 0x80 != 0;
    let n = packed & 0x07;
    let background = lsd[5];

    let mut offset = lsd_start + 7;

    let palette = if has_gct {
        let entry_count = 1usize << (n + 1);
        let table_len = entry_count * 3;
        let table = bytes
            .get(offset..offset + table_len)
            .ok_or(GifError::UnexpectedEnd)?;

        let mut palette = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let base = i * 3;
            let r = table[base];
            let g = table[base + 1];
            let b = table[base + 2];
            palette.push(crate::Color::rgb(r, g, b));
        }
        offset += table_len;
        palette
    } else {
        Vec::new()
    };

    Ok((
        Screen {
            width,
            height,
            background,
            palette,
        },
        offset,
    ))
}

/// One frame of an animation, already composited onto the logical screen.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Full-screen RGBA pixels, row-major from the top left.
    pub pixels: Vec<Color>,
    /// How long to show this frame, in microseconds.
    pub delay_us: u64,
}

/// A decoded animation.
#[derive(Clone, Debug)]
pub struct Animation {
    /// Logical screen width.
    pub width: u16,
    /// Logical screen height.
    pub height: u16,
    /// Frames in playback order. Never empty on success.
    pub frames: Vec<Frame>,
}

/// What to do with a frame's area before the next one is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Disposal {
    /// Leave it; the next frame draws on top.
    Keep,
    /// Clear it to the background.
    Background,
    /// Restore whatever was there before this frame.
    Previous,
}

/// Longest animation this decoder will assemble, in frames.
///
/// A GIF can declare an unbounded loop of frames and each one costs a full
/// screen of RGBA. The cap is what stops a malicious file from exhausting a
/// fixed heap; 512 frames at 20 fps is twenty-five seconds, far more than any
/// dashboard ornament needs.
pub const MAX_FRAMES: usize = 512;

/// Decode a whole GIF into composited frames.
///
/// Every frame is returned full-screen with disposal already applied, so a
/// caller can show frame *n* without having replayed frames 0..n. That costs
/// memory and buys the thing that actually matters here: seeking to a frame is
/// an index, not a replay, and a widget can be handed one frame per tick with
/// no decoder state between them.
///
/// # Errors
///
/// See [`GifError`]. This function does not panic on any input.
pub fn decode(bytes: &[u8]) -> Result<Animation, GifError> {
    let (scr, mut at) = screen(bytes)?;
    let px = (scr.width as usize)
        .checked_mul(scr.height as usize)
        .ok_or(GifError::BadHeader)?;

    // The canvas frames are composited onto, and the copy kept for
    // Disposal::Previous.
    let mut canvas = alloc::vec![Color::TRANSPARENT; px];
    let mut saved = canvas.clone();
    let mut frames: Vec<Frame> = Vec::new();

    // Values from the most recent Graphic Control Extension. They apply to the
    // next image descriptor and are reset after it, which is the part of the
    // spec most decoders get wrong by making them sticky.
    let mut delay_cs = 0u16;
    let mut transparent: Option<u8> = None;
    let mut disposal = Disposal::Keep;

    while frames.len() < MAX_FRAMES {
        let block = *bytes.get(at).ok_or(GifError::UnexpectedEnd)?;
        at += 1;
        match block {
            // Trailer.
            0x3b => break,
            // Extension.
            0x21 => {
                let label = *bytes.get(at).ok_or(GifError::UnexpectedEnd)?;
                at += 1;
                if label == 0xf9 {
                    let size = *bytes.get(at).ok_or(GifError::UnexpectedEnd)? as usize;
                    let body = bytes
                        .get(at + 1..at + 1 + size)
                        .ok_or(GifError::UnexpectedEnd)?;
                    if size >= 4 {
                        let packed = body[0];
                        disposal = match (packed >> 2) & 0x07 {
                            2 => Disposal::Background,
                            3 => Disposal::Previous,
                            _ => Disposal::Keep,
                        };
                        delay_cs = u16::from_le_bytes([body[1], body[2]]);
                        transparent = (packed & 1 != 0).then_some(body[3]);
                    }
                    at += 1 + size;
                }
                at = skip_blocks(bytes, at)?;
            }
            // Image descriptor.
            0x2c => {
                at = image(
                    bytes,
                    at,
                    &scr,
                    &mut canvas,
                    &mut saved,
                    &mut frames,
                    delay_cs,
                    transparent,
                    disposal,
                )?;
                // Control values apply to one image only.
                delay_cs = 0;
                transparent = None;
                disposal = Disposal::Keep;
            }
            _ => return Err(GifError::Unsupported),
        }
    }

    if frames.is_empty() {
        return Err(GifError::UnexpectedEnd);
    }
    Ok(Animation {
        width: scr.width,
        height: scr.height,
        frames,
    })
}

/// Walk a chain of length-prefixed sub-blocks, returning the offset past it.
fn skip_blocks(bytes: &[u8], mut at: usize) -> Result<usize, GifError> {
    loop {
        let n = *bytes.get(at).ok_or(GifError::UnexpectedEnd)? as usize;
        at += 1;
        if n == 0 {
            return Ok(at);
        }
        at = at.checked_add(n).ok_or(GifError::UnexpectedEnd)?;
        if at > bytes.len() {
            return Err(GifError::UnexpectedEnd);
        }
    }
}

/// Concatenate a chain of sub-blocks, returning the data and the offset past it.
fn gather(bytes: &[u8], mut at: usize) -> Result<(Vec<u8>, usize), GifError> {
    let mut out = Vec::new();
    loop {
        let n = *bytes.get(at).ok_or(GifError::UnexpectedEnd)? as usize;
        at += 1;
        if n == 0 {
            return Ok((out, at));
        }
        let chunk = bytes.get(at..at + n).ok_or(GifError::UnexpectedEnd)?;
        out.extend_from_slice(chunk);
        at += n;
    }
}

#[allow(clippy::too_many_arguments)] // Every one is a distinct GIF concept;
// bundling them into a struct would only move the argument list somewhere else.
fn image(
    bytes: &[u8],
    mut at: usize,
    scr: &Screen,
    canvas: &mut [Color],
    saved: &mut Vec<Color>,
    frames: &mut Vec<Frame>,
    delay_cs: u16,
    transparent: Option<u8>,
    disposal: Disposal,
) -> Result<usize, GifError> {
    let d = bytes.get(at..at + 9).ok_or(GifError::UnexpectedEnd)?;
    let fx = u16::from_le_bytes([d[0], d[1]]) as usize;
    let fy = u16::from_le_bytes([d[2], d[3]]) as usize;
    let fw = u16::from_le_bytes([d[4], d[5]]) as usize;
    let fh = u16::from_le_bytes([d[6], d[7]]) as usize;
    let packed = d[8];
    at += 9;

    let local = packed & 0x80 != 0;
    let interlaced = packed & 0x40 != 0;
    let palette = if local {
        let n = 2usize << (packed & 0x07);
        let raw = bytes.get(at..at + n * 3).ok_or(GifError::UnexpectedEnd)?;
        at += n * 3;
        raw.as_chunks::<3>()
            .0
            .iter()
            .map(|c| Color::rgb(c[0], c[1], c[2]))
            .collect()
    } else {
        scr.palette.clone()
    };
    if palette.is_empty() {
        return Err(GifError::BadHeader);
    }

    let min_code_size = *bytes.get(at).ok_or(GifError::UnexpectedEnd)?;
    at += 1;
    let (data, next) = gather(bytes, at)?;

    let count = fw.checked_mul(fh).ok_or(GifError::BadHeader)?;
    let indices = lzw::decode(min_code_size, &data, count)?;

    if disposal == Disposal::Previous {
        saved.clear();
        saved.extend_from_slice(canvas);
    }

    let sw = scr.width as usize;
    for (i, &idx) in indices.iter().enumerate() {
        // An interlaced frame's rows arrive in four passes; without this the
        // image decodes correctly but appears shredded into bands.
        let row = if interlaced {
            deinterlace(i / fw.max(1), fh)
        } else {
            i / fw.max(1)
        };
        let col = i % fw.max(1);
        let (x, y) = (fx + col, fy + row);
        if x >= sw || y >= scr.height as usize {
            continue;
        }
        if Some(idx) == transparent {
            // Transparent means "leave what is underneath", not "write
            // nothing" -- the distinction only shows up on the second frame.
            continue;
        }
        if let Some(c) = palette.get(idx as usize) {
            canvas[y * sw + x] = *c;
        }
    }

    frames.push(Frame {
        pixels: canvas.to_vec(),
        // A zero or absent delay means "as fast as sensible"; browsers settled
        // on 100ms and so does this, or an animation runs at frame rate.
        delay_us: if delay_cs == 0 {
            100_000
        } else {
            u64::from(delay_cs) * 10_000
        },
    });

    match disposal {
        Disposal::Keep => {}
        Disposal::Background => {
            for y in fy..(fy + fh).min(scr.height as usize) {
                for x in fx..(fx + fw).min(sw) {
                    canvas[y * sw + x] = Color::TRANSPARENT;
                }
            }
        }
        Disposal::Previous => canvas.copy_from_slice(saved),
    }

    Ok(next)
}

/// Map an interlaced row index to its position on the screen.
fn deinterlace(row: usize, height: usize) -> usize {
    let p1 = height.div_ceil(8);
    let p2 = height.div_ceil(8).max(1);
    let g2 = if height > 4 {
        (height - 4).div_ceil(8)
    } else {
        0
    };
    let g3 = if height > 2 {
        (height - 2).div_ceil(4)
    } else {
        0
    };
    let _ = p2;
    if row < p1 {
        row * 8
    } else if row < p1 + g2 {
        4 + (row - p1) * 8
    } else if row < p1 + g2 + g3 {
        2 + (row - p1 - g2) * 4
    } else {
        1 + (row - p1 - g2 - g3) * 2
    }
}
