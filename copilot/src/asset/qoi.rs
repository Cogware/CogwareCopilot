// SPDX-License-Identifier: GPL-3.0-only
//! QOI image decoding.
//!
//! QOI rather than PNG because it is the only lossless format whose decoder is
//! small enough to own outright: no entropy coder, no filters, no zlib. Every
//! read goes through `.get()` and turns a short buffer into
//! [`QoiError::UnexpectedEnd`], because assets can come off a card someone
//! else wrote and a panic on bare metal is a dead dashboard.

use alloc::vec;
use alloc::vec::Vec;

use crate::Color;

/// What a QOI stream says about itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QoiHeader {
    /// Image width in pixels. Never zero in a valid stream.
    pub width: u32,
    /// Image height in pixels. Never zero in a valid stream.
    pub height: u32,
    /// 3 for RGB, 4 for RGBA. Advisory: the decoder always produces RGBA.
    pub channels: u8,
    /// 0 for sRGB with a linear alpha, 1 for all channels linear. Advisory.
    pub colorspace: u8,
}

/// Why a QOI stream could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QoiError {
    /// The first four bytes were not `qoif`.
    BadMagic,
    /// The header parsed but described an impossible image.
    BadHeader,
    /// The stream ended in the middle of a chunk or before the last pixel.
    UnexpectedEnd,
    /// `width * height` does not fit in a `usize` on this target.
    TooLarge,
    /// The eight-byte end marker was missing or wrong.
    BadPadding,
}

/// Largest image this decoder will attempt, in pixels.
///
/// `checked_mul` is not enough on its own: `u32::MAX * u32::MAX` fits in a
/// 64-bit `usize`, so the multiply succeeds and the allocation then aborts on
/// capacity overflow -- a panic a malformed header must not be able to
/// trigger. 256 Mpx is far beyond any panel this crate targets and well inside
/// what `Vec` can express on a 32-bit machine.
const MAX_PIXELS: usize = 1 << 28;

const MAGIC: &[u8; 4] = b"qoif";
const HEADER_LEN: usize = 14;
const PADDING: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;
const MASK2: u8 = 0xc0;

/// Read just the header, without decoding any pixels.
///
/// Useful for laying out a scene before its assets have been decoded.
///
/// # Errors
///
/// [`QoiError::BadMagic`] if the stream is not QOI, [`QoiError::BadHeader`] if
/// it is too short or describes a zero-sized image.
pub fn header(bytes: &[u8]) -> Result<QoiHeader, QoiError> {
    let head = bytes.get(..HEADER_LEN).ok_or(QoiError::BadHeader)?;
    if &head[..4] != MAGIC {
        return Err(QoiError::BadMagic);
    }
    let width = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
    let height = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);
    let channels = head[12];
    let colorspace = head[13];

    // A zero dimension would make `total` zero and the pixel loop terminate
    // immediately, so it would decode "successfully" to nothing. Rejecting it
    // here means a caller never has to special-case an empty image.
    if width == 0 || height == 0 {
        return Err(QoiError::BadHeader);
    }
    if !matches!(channels, 3 | 4) || colorspace > 1 {
        return Err(QoiError::BadHeader);
    }
    Ok(QoiHeader {
        width,
        height,
        channels,
        colorspace,
    })
}

/// Decode a whole QOI stream to [`Color`] pixels, row-major from the top left.
///
/// The result is always four channels regardless of what the header says: a
/// three-channel image decodes with alpha 255 throughout, so callers have one
/// layout to blit rather than two.
///
/// # Errors
///
/// See [`QoiError`]. This function does not panic on any input.
pub fn decode(bytes: &[u8]) -> Result<(QoiHeader, Vec<Color>), QoiError> {
    let hdr = header(bytes)?;

    let w = usize::try_from(hdr.width).map_err(|_| QoiError::TooLarge)?;
    let h = usize::try_from(hdr.height).map_err(|_| QoiError::TooLarge)?;
    let total = w.checked_mul(h).ok_or(QoiError::TooLarge)?;
    if total > MAX_PIXELS {
        return Err(QoiError::TooLarge);
    }

    let mut pixels: Vec<[u8; 4]> = vec![[0, 0, 0, 255]; total];
    // The running array starts all-zero including alpha, which is *not* the
    // initial previous pixel. Seeding it with the previous pixel instead is
    // the classic QOI bug: it makes an OP_INDEX 53 at the head of a stream
    // decode to opaque black rather than transparent.
    let mut running = [[0u8; 4]; 64];
    let mut px = [0u8, 0, 0, 255];

    let body = bytes.get(HEADER_LEN..).ok_or(QoiError::UnexpectedEnd)?;
    let mut i = 0usize;
    let mut n = 0usize;

    while n < total {
        let b0 = *body.get(i).ok_or(QoiError::UnexpectedEnd)?;
        i += 1;

        if b0 == OP_RGB {
            let c = body.get(i..i + 3).ok_or(QoiError::UnexpectedEnd)?;
            // Alpha deliberately carries over: OP_RGB means "same alpha".
            px = [c[0], c[1], c[2], px[3]];
            i += 3;
        } else if b0 == OP_RGBA {
            let c = body.get(i..i + 4).ok_or(QoiError::UnexpectedEnd)?;
            px = [c[0], c[1], c[2], c[3]];
            i += 4;
        } else {
            match b0 & MASK2 {
                OP_INDEX => px = running[(b0 & 0x3f) as usize],
                OP_DIFF => {
                    // Each delta is biased by 2 and wraps: a channel at 255
                    // plus 1 is 0, and that is the format, not an overflow.
                    px[0] = px[0].wrapping_add(((b0 >> 4) & 0x03).wrapping_sub(2));
                    px[1] = px[1].wrapping_add(((b0 >> 2) & 0x03).wrapping_sub(2));
                    px[2] = px[2].wrapping_add((b0 & 0x03).wrapping_sub(2));
                }
                OP_LUMA => {
                    let b1 = *body.get(i).ok_or(QoiError::UnexpectedEnd)?;
                    i += 1;
                    let dg = (b0 & 0x3f).wrapping_sub(32);
                    // dr and db are stored relative to dg, so the green delta
                    // has to be folded back into both.
                    px[0] = px[0]
                        .wrapping_add(dg)
                        .wrapping_add(((b1 >> 4) & 0x0f).wrapping_sub(8));
                    px[1] = px[1].wrapping_add(dg);
                    px[2] = px[2]
                        .wrapping_add(dg)
                        .wrapping_add((b1 & 0x0f).wrapping_sub(8));
                }
                OP_RUN => {
                    let run = (b0 & 0x3f) as usize + 1;
                    // A run may not overrun the image. Clamping instead would
                    // silently accept a corrupt stream and shift every later
                    // pixel, which is far harder to notice than an error.
                    if n + run > total {
                        return Err(QoiError::UnexpectedEnd);
                    }
                    for slot in &mut pixels[n..n + run] {
                        *slot = px;
                    }
                    n += run;
                    running[hash(px)] = px;
                    continue;
                }
                // The tag is two bits and all four values are handled above.
                // Returning an error rather than `unreachable!` keeps it from
                // panicking even if someone edits the constants wrongly later.
                _ => return Err(QoiError::UnexpectedEnd),
            }
        }

        running[hash(px)] = px;
        pixels[n] = px;
        n += 1;
    }

    // The end marker is what distinguishes a complete stream from one that was
    // cut after the last pixel happened to land on a boundary.
    let tail = body.get(i..).ok_or(QoiError::UnexpectedEnd)?;
    if tail.len() < PADDING.len() || tail[..PADDING.len()] != PADDING {
        return Err(QoiError::BadPadding);
    }
    // Converted once, at the end: the decode loop stays on plain byte quads
    // where the wrapping channel arithmetic is expressed most directly, and
    // callers never have to know QOI's channel order.
    Ok((
        hdr,
        pixels
            .into_iter()
            .map(|[r, g, b, a]| Color::rgba(r, g, b, a))
            .collect(),
    ))
}

/// The spec's running-array index.
fn hash(px: [u8; 4]) -> usize {
    let [r, g, b, a] = px;
    ((r as usize)
        .wrapping_mul(3)
        .wrapping_add((g as usize).wrapping_mul(5))
        .wrapping_add((b as usize).wrapping_mul(7))
        .wrapping_add((a as usize).wrapping_mul(11)))
        % 64
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Build a stream: header, the given body bytes, then the end marker.
    fn stream(w: u32, h: u32, ch: u8, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.push(ch);
        v.push(0);
        v.extend_from_slice(body);
        v.extend_from_slice(&PADDING);
        v
    }

    #[test]
    fn rejects_a_foreign_format() {
        let mut s = stream(1, 1, 4, &[OP_RGBA, 1, 2, 3, 4]);
        s[0] = b'p';
        assert_eq!(decode(&s), Err(QoiError::BadMagic));
    }

    #[test]
    fn rejects_a_zero_dimension() {
        assert_eq!(
            decode(&stream(0, 4, 4, &[])).unwrap_err(),
            QoiError::BadHeader
        );
        assert_eq!(
            decode(&stream(4, 0, 4, &[])).unwrap_err(),
            QoiError::BadHeader
        );
    }

    #[test]
    fn rejects_an_impossible_channel_count() {
        assert_eq!(
            decode(&stream(1, 1, 2, &[])).unwrap_err(),
            QoiError::BadHeader
        );
    }

    #[test]
    fn rgba_chunk_round_trips() {
        let (h, px) = decode(&stream(1, 1, 4, &[OP_RGBA, 10, 20, 30, 40])).unwrap();
        assert_eq!((h.width, h.height), (1, 1));
        assert_eq!(px, &[Color::rgba(10, 20, 30, 40)]);
    }

    #[test]
    fn rgb_chunk_carries_alpha_over() {
        // The previous pixel starts opaque, so an OP_RGB must stay opaque --
        // reading alpha from the chunk instead is the obvious wrong guess.
        let (_, px) = decode(&stream(1, 1, 3, &[OP_RGB, 9, 8, 7])).unwrap();
        assert_eq!(px, &[Color::rgb(9, 8, 7)]);
    }

    #[test]
    fn a_run_repeats_the_previous_pixel() {
        let (_, px) = decode(&stream(4, 1, 4, &[OP_RGBA, 1, 2, 3, 4, OP_RUN | 2])).unwrap();
        assert_eq!(px.len(), 4);
        assert!(px.iter().all(|p| *p == Color::rgba(1, 2, 3, 4)));
    }

    #[test]
    fn an_index_chunk_reads_the_running_array() {
        let first = [1u8, 2, 3, 4];
        let idx = hash(first) as u8;
        let want = Color::rgba(1, 2, 3, 4);
        let (_, px) = decode(&stream(
            3,
            1,
            4,
            &[OP_RGBA, 1, 2, 3, 4, OP_RGBA, 9, 9, 9, 9, OP_INDEX | idx],
        ))
        .unwrap();
        assert_eq!(px[2], want);
    }

    #[test]
    fn the_running_array_starts_transparent_not_opaque() {
        // Seeding it with the initial previous pixel is the classic QOI bug:
        // it makes a leading OP_INDEX decode to opaque black instead of
        // transparent. hash([0,0,0,0]) is 0, so index 0 must be all-zero.
        // OP_INDEX with a zero payload is just OP_INDEX; index 0 is the slot
        // that must still hold the all-zero initial entry.
        let (_, px) = decode(&stream(1, 1, 4, &[OP_INDEX])).unwrap();
        assert_eq!(px, &[Color::TRANSPARENT]);
    }

    #[test]
    fn diff_deltas_wrap() {
        // 0 - 2 must wrap to 254, not saturate to 0 and not panic in debug.
        let body = [OP_RGBA, 0, 0, 0, 255, OP_DIFF];
        let (_, px) = decode(&stream(2, 1, 4, &body)).unwrap();
        assert_eq!(px[1], Color::rgb(254, 254, 254));
    }

    #[test]
    fn luma_folds_green_into_red_and_blue() {
        // dg = 0 (biased 32), dr-dg = 0 (biased 8), db-dg = 0 -> no change.
        let body = [OP_RGBA, 100, 100, 100, 255, OP_LUMA | 32, 0x88];
        let (_, px) = decode(&stream(2, 1, 4, &body)).unwrap();
        assert_eq!(px[1], Color::rgb(100, 100, 100));
    }

    #[test]
    fn a_run_may_not_overrun_the_image() {
        // Clamping instead would silently accept a corrupt stream and shift
        // every later pixel, which is far harder to notice than an error.
        let body = [OP_RGBA, 1, 2, 3, 4, OP_RUN | 40];
        assert_eq!(
            decode(&stream(2, 1, 4, &body)).unwrap_err(),
            QoiError::UnexpectedEnd
        );
    }

    #[test]
    fn a_missing_end_marker_is_rejected() {
        let mut s = stream(1, 1, 4, &[OP_RGBA, 1, 2, 3, 4]);
        s.truncate(s.len() - PADDING.len());
        // The pixels decoded and the stream then simply stopped, so the
        // specific complaint is the absent marker, not a short chunk.
        assert_eq!(decode(&s).unwrap_err(), QoiError::BadPadding);
    }

    #[test]
    fn a_wrong_end_marker_is_rejected() {
        let mut s = stream(1, 1, 4, &[OP_RGBA, 1, 2, 3, 4]);
        let n = s.len();
        s[n - 1] = 0;
        assert_eq!(decode(&s).unwrap_err(), QoiError::BadPadding);
    }

    #[test]
    fn an_absurd_size_is_rejected_rather_than_allocating() {
        // 4 billion squared pixels must not be attempted; on a 32-bit target
        // this is the difference between an error and an OOM abort.
        let s = stream(u32::MAX, u32::MAX, 4, &[]);
        assert_eq!(decode(&s).unwrap_err(), QoiError::TooLarge);
    }

    #[test]
    fn every_prefix_is_handled_without_panicking() {
        let good = stream(4, 2, 4, &[OP_RGBA, 1, 2, 3, 4, OP_RUN | 6]);
        for i in 0..=good.len() {
            let _ = decode(&good[..i]);
        }
    }
}
