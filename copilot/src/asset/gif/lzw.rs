// SPDX-License-Identifier: GPL-3.0-only
//! GIF's variable-width LZW.
//!
//! Written out rather than pulled in because it is the only part of GIF with
//! any real substance, and with no dependency to reach for the alternative to
//! writing it is not having animation at all.
//!
//! # The two traps
//!
//! **The deferred clear.** An encoder is permitted to keep emitting codes at
//! the maximum width after the dictionary is full, without ever sending a
//! clear code. A decoder that grows its width past 12 bits, or that treats a
//! full dictionary as an error, fails on perfectly legal files — and they are
//! common, because several popular encoders do exactly this.
//!
//! **The KwKwK case.** A code may refer to the entry that is only being
//! created by decoding that very code. It happens whenever the encoder sees a
//! sequence like `aXaXa`. The output is the previous sequence plus its own
//! first byte, and a decoder that simply looks the code up finds an empty slot
//! and produces nothing — which corrupts everything after it rather than
//! failing loudly.
//!
//! # Why the dictionary is flat arrays rather than `Vec<Vec<u8>>`
//!
//! Every entry is "some earlier entry, plus one byte", so storing a prefix
//! index and a suffix byte captures it exactly in two arrays of fixed size.
//! That is one allocation instead of four thousand, no pointer chasing, and it
//! is what makes this usable on a machine whose heap is a fixed pool.

use alloc::vec::Vec;

use super::GifError;

/// Largest code width GIF permits.
const MAX_BITS: u8 = 12;
/// Dictionary capacity at [`MAX_BITS`].
const MAX_CODES: usize = 1 << MAX_BITS as usize;

/// Decode one LZW stream into indices.
///
/// `min_code_size` is the byte that precedes the image's sub-blocks, and
/// `data` is those sub-blocks already concatenated. `expected` is how many
/// pixels the frame should contain; decoding stops there even if the stream
/// would go on, so a corrupt file cannot make the output grow without bound.
///
/// # Errors
///
/// [`GifError::BadHeader`] for an impossible code size, [`GifError::Corrupt`]
/// for a code the dictionary cannot explain.
pub fn decode(min_code_size: u8, data: &[u8], expected: usize) -> Result<Vec<u8>, GifError> {
    // 2..=8: one bit is degenerate and more than eight cannot index a palette.
    if !(2..=8).contains(&min_code_size) {
        return Err(GifError::BadHeader);
    }

    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let first_free = end + 1;

    let mut prefix = [0u16; MAX_CODES];
    let mut suffix = [0u8; MAX_CODES];
    // Length is carried so an entry can be emitted back-to-front into a buffer
    // sized exactly, rather than reversed after the fact.
    let mut length = [0u16; MAX_CODES];
    for i in 0..clear as usize {
        suffix[i] = i as u8;
        length[i] = 1;
    }

    let mut out = Vec::with_capacity(expected);
    let mut stack = [0u8; MAX_CODES];

    let mut width = min_code_size + 1;
    let mut next = first_free;
    let mut prev: Option<u16> = None;

    let mut bit = 0usize;
    let total_bits = data.len() * 8;

    while out.len() < expected {
        if bit + width as usize > total_bits {
            // Ran out of input before the frame was full. A truncated GIF is
            // common enough in the wild that the partial frame is worth more
            // than an error; the caller pads whatever is missing.
            break;
        }
        let code = read_code(data, bit, width);
        bit += width as usize;

        if code == clear {
            width = min_code_size + 1;
            next = first_free;
            prev = None;
            continue;
        }
        if code == end {
            break;
        }

        let emit = if (code as usize) < next as usize {
            code
        } else if code == next && prev.is_some() {
            // KwKwK: the code names the entry this very step creates. Its
            // expansion is the previous sequence followed by that sequence's
            // own first byte.
            next
        } else {
            return Err(GifError::Corrupt);
        };

        // Build the new entry *before* expanding, so the KwKwK slot exists.
        if let Some(p) = prev
            && (next as usize) < MAX_CODES
        {
            prefix[next as usize] = p;
            // The new entry is always "previous sequence, plus the first byte
            // of what is being emitted". In the KwKwK case what is emitted IS
            // this entry, whose chain does not exist yet -- so the first byte
            // has to come from the previous sequence, which is the same byte
            // by definition. Reading it from `emit` finds an uninitialised
            // slot and silently corrupts every later entry.
            let head = if emit == next { p } else { emit };
            suffix[next as usize] = first_byte(&prefix, &suffix, &length, head);
            length[next as usize] = length[p as usize].saturating_add(1);
            next += 1;
            // The width grows only while there is room for it. Past 12 bits an
            // encoder simply keeps emitting at the maximum until it feels like
            // sending a clear, and refusing that breaks common files.
            if next as usize == (1usize << width) && width < MAX_BITS {
                width += 1;
            }
        }

        let n = expand(&prefix, &suffix, &length, emit, &mut stack)?;
        out.extend_from_slice(&stack[..n]);
        // `code` is now always a valid entry: either it already was, or the
        // KwKwK branch above just created it.
        prev = Some(code);
    }

    // A short frame is padded with the background index rather than rejected,
    // so one damaged frame does not lose the whole animation.
    out.resize(expected, 0);
    Ok(out)
}

/// Read `width` bits starting at `bit`, least-significant bit first.
///
/// GIF packs codes LSB-first across byte boundaries, which is the opposite of
/// the font atlas and of most image formats; getting it backwards produces
/// plausible-looking noise rather than an error.
fn read_code(data: &[u8], bit: usize, width: u8) -> u16 {
    let mut v = 0u32;
    for i in 0..width as usize {
        let idx = bit + i;
        let byte = data.get(idx / 8).copied().unwrap_or(0);
        let b = (byte >> (idx % 8)) & 1;
        v |= u32::from(b) << i;
    }
    v as u16
}

/// The first byte of the sequence a code expands to.
fn first_byte(prefix: &[u16], suffix: &[u8], length: &[u16], mut code: u16) -> u8 {
    // Bounded by the dictionary size: every prefix points strictly backwards,
    // so this cannot loop, but a corrupt table must not hang the decoder.
    for _ in 0..MAX_CODES {
        if length[code as usize] <= 1 {
            return suffix[code as usize];
        }
        code = prefix[code as usize];
    }
    suffix[code as usize]
}

/// Expand `code` into `stack`, front to back. Returns the byte count.
fn expand(
    prefix: &[u16],
    suffix: &[u8],
    length: &[u16],
    code: u16,
    stack: &mut [u8; MAX_CODES],
) -> Result<usize, GifError> {
    let n = length[code as usize] as usize;
    if n == 0 || n > MAX_CODES {
        return Err(GifError::Corrupt);
    }
    let mut c = code;
    // Written back to front because an entry is defined by its *last* byte;
    // knowing the length up front is what lets it land in place.
    for i in (0..n).rev() {
        stack[i] = suffix[c as usize];
        c = prefix[c as usize];
    }
    Ok(n)
}
