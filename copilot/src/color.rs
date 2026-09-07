// SPDX-License-Identifier: MIT OR Apache-2.0
//! Colour, and the one place the crate converts to a backend's pixel layout.
//!
//! Everything composites in straight (non-premultiplied) 8-bit RGBA and
//! converts once, when a span reaches the surface. Keeping one internal
//! representation is what lets a widget be written without knowing whether it
//! will end up on a 16-bit SPI panel or a 32-bit framebuffer.
//!
//! # Why not premultiplied alpha
//!
//! Premultiplied is the better choice for a compositor that blends many layers
//! per pixel, because it turns `src over dst` into one multiply-add. This
//! crate blends at most a couple of layers per pixel — a widget over its own
//! background — and scene files are written by hand, where `#ff0000` with 50%
//! alpha meaning "half-transparent red" is far less surprising than the
//! premultiplied `#800000`. Correctness for the person editing the file won a
//! cycle count that does not matter at this depth.

use crate::PixelFormat;

/// A straight-alpha 8-bit-per-channel colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Color {
    /// Red channel, 0-255.
    pub r: u8,
    /// Green channel, 0-255.
    pub g: u8,
    /// Blue channel, 0-255.
    pub b: u8,
    /// Alpha, 0 fully transparent to 255 fully opaque.
    pub a: u8,
}

impl Color {
    /// Fully transparent. Distinct from black: compositing this changes nothing.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// An opaque colour from its three channels.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A colour with an explicit alpha.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Whether compositing this colour can change the destination at all.
    #[must_use]
    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Whether this colour hides whatever is underneath it.
    ///
    /// The renderer uses this to skip reading the destination, which on a
    /// non-cacheable framebuffer is the expensive half of the operation.
    #[must_use]
    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Composite `self` over `dst` using source-over.
    ///
    /// The divide is by 255 rather than 256 so that 255 maps to exactly 1.0;
    /// the `+ 128` and the extra shift are the usual rounding trick that makes
    /// `x * 255 / 255 == x` hold for every input without a division.
    #[must_use]
    pub const fn over(self, dst: Self) -> Self {
        if self.is_opaque() {
            return self;
        }
        if self.is_transparent() {
            return dst;
        }
        let a = self.a as u32;
        let inv = 255 - a;
        Self {
            r: blend(self.r, dst.r, a, inv),
            g: blend(self.g, dst.g, a, inv),
            b: blend(self.b, dst.b, a, inv),
            // Straight alpha: the union of two coverages, not their sum.
            a: (a + (dst.a as u32 * inv + 127) / 255) as u8,
        }
    }

    /// Read a pixel back out of a backend's layout.
    ///
    /// The inverse of [`Self::pack`], for blending an antialiased edge over
    /// what is already on the surface. Alpha comes back opaque: a framebuffer
    /// has no alpha channel, and what is in it is what is on the panel.
    /// `Rgb565` widens each channel by repeating its top bits into the
    /// bottom, so that full white reads back as full white rather than as
    /// 248, 252, 248.
    #[must_use]
    pub fn unpack(bytes: &[u8], format: PixelFormat) -> Self {
        match format {
            PixelFormat::Bgrx8888 => match bytes {
                [b, g, r, ..] => Self::rgb(*r, *g, *b),
                _ => Self::BLACK,
            },
            PixelFormat::Rgbx8888 => match bytes {
                [r, g, b, ..] => Self::rgb(*r, *g, *b),
                _ => Self::BLACK,
            },
            PixelFormat::Rgb565 => match bytes {
                [lo, hi, ..] => {
                    let v = u16::from_le_bytes([*lo, *hi]);
                    let r5 = ((v >> 11) & 0x1f) as u8;
                    let g6 = ((v >> 5) & 0x3f) as u8;
                    let b5 = (v & 0x1f) as u8;
                    Self::rgb(
                        (r5 << 3) | (r5 >> 2),
                        (g6 << 2) | (g6 >> 4),
                        (b5 << 3) | (b5 >> 2),
                    )
                }
                _ => Self::BLACK,
            },
        }
    }

    /// Pack into a backend's layout, little-endian, ready to store.
    ///
    /// Returns the bytes and how many of them are significant, so a caller can
    /// write 2 or 4 without branching on the format again.
    #[must_use]
    pub const fn pack(self, format: PixelFormat) -> ([u8; 4], usize) {
        match format {
            PixelFormat::Bgrx8888 => ([self.b, self.g, self.r, 0xff], 4),
            PixelFormat::Rgbx8888 => ([self.r, self.g, self.b, 0xff], 4),
            PixelFormat::Rgb565 => {
                // 5/6/5 by truncation of the high bits. Dithering belongs in
                // the rasteriser where it can see neighbouring pixels, not
                // here where each conversion is isolated.
                let v = ((self.r as u16 & 0xf8) << 8)
                    | ((self.g as u16 & 0xfc) << 3)
                    | (self.b as u16 >> 3);
                ([v as u8, (v >> 8) as u8, 0, 0], 2)
            }
        }
    }
}

/// One hex digit's value, or `None` if it is not a hex digit.
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse a CSS-style hex colour.
///
/// The `#` is optional, case is ignored, and the three- and four-digit forms
/// double each nibble so `#f00` and `#ff0000` are the same red.
///
/// # Examples
/// ```
/// assert_eq!(copilot::color::parse_hex("#f00"), Some(copilot::Color::rgba(255, 0, 0, 255)));
/// assert_eq!(copilot::color::parse_hex("ff0000"), Some(copilot::Color::rgba(255, 0, 0, 255)));
/// assert_eq!(copilot::color::parse_hex("#ff000080"), Some(copilot::Color::rgba(255, 0, 0, 128)));
/// ```
#[must_use]
pub fn parse_hex(s: &str) -> Option<Color> {
    let bytes = s.as_bytes();
    let start = if bytes.first() == Some(&b'#') { 1 } else { 0 };
    let hex_len = bytes.len() - start;

    // Only 3, 4, 6, or 8 hex digits are valid CSS hex forms.
    let (r, g, b, a) = match hex_len {
        3 => {
            let r = hex_nibble(bytes[start])?;
            let g = hex_nibble(bytes[start + 1])?;
            let b = hex_nibble(bytes[start + 2])?;
            (r * 17, g * 17, b * 17, 255)
        }
        4 => {
            let r = hex_nibble(bytes[start])?;
            let g = hex_nibble(bytes[start + 1])?;
            let b = hex_nibble(bytes[start + 2])?;
            let a = hex_nibble(bytes[start + 3])?;
            (r * 17, g * 17, b * 17, a * 17)
        }
        6 => {
            let r = hex_nibble(bytes[start])? * 16 + hex_nibble(bytes[start + 1])?;
            let g = hex_nibble(bytes[start + 2])? * 16 + hex_nibble(bytes[start + 3])?;
            let b = hex_nibble(bytes[start + 4])? * 16 + hex_nibble(bytes[start + 5])?;
            (r, g, b, 255)
        }
        8 => {
            let r = hex_nibble(bytes[start])? * 16 + hex_nibble(bytes[start + 1])?;
            let g = hex_nibble(bytes[start + 2])? * 16 + hex_nibble(bytes[start + 3])?;
            let b = hex_nibble(bytes[start + 4])? * 16 + hex_nibble(bytes[start + 5])?;
            let a = hex_nibble(bytes[start + 6])? * 16 + hex_nibble(bytes[start + 7])?;
            (r, g, b, a)
        }
        _ => return None,
    };

    Some(Color::rgba(r, g, b, a))
}

const fn blend(src: u8, dst: u8, a: u32, inv: u32) -> u8 {
    ((src as u32 * a + dst as u32 * inv + 127) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_source_replaces_destination() {
        assert_eq!(Color::WHITE.over(Color::BLACK), Color::WHITE);
    }

    #[test]
    fn transparent_source_leaves_destination() {
        assert_eq!(Color::TRANSPARENT.over(Color::WHITE), Color::WHITE);
    }

    #[test]
    fn half_alpha_lands_mid_grey() {
        let got = Color::rgba(255, 255, 255, 128).over(Color::BLACK);
        // 128/255 of full white, rounded: not 127, and not 129.
        assert_eq!(got.r, 128);
    }

    #[test]
    fn round_trip_is_exact_at_the_extremes() {
        // The +127 rounding exists so this holds; a plain >> 8 gives 254 here.
        for v in [0u8, 1, 127, 128, 254, 255] {
            let c = Color::rgba(v, v, v, 255);
            assert_eq!(c.over(Color::BLACK).r, v);
        }
    }

    #[test]
    fn bgrx_swaps_red_and_blue() {
        let (bytes, n) = Color::rgb(1, 2, 3).pack(PixelFormat::Bgrx8888);
        assert_eq!(n, 4);
        assert_eq!(&bytes[..3], &[3, 2, 1]);
    }

    #[test]
    fn rgb565_uses_two_bytes() {
        let (_, n) = Color::WHITE.pack(PixelFormat::Rgb565);
        assert_eq!(n, 2);
    }
}

#[cfg(test)]
mod hex_tests {
    use super::*;

    #[test]
    fn three_digit_form_doubles_each_nibble() {
        assert_eq!(parse_hex("#f00"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(parse_hex("#abc"), Some(Color::rgb(0xaa, 0xbb, 0xcc)));
    }

    #[test]
    fn four_digit_form_carries_alpha() {
        assert_eq!(parse_hex("#f00f"), Some(Color::rgba(255, 0, 0, 255)));
        assert_eq!(parse_hex("#0008"), Some(Color::rgba(0, 0, 0, 0x88)));
    }

    #[test]
    fn six_and_eight_digit_forms() {
        assert_eq!(parse_hex("#102030"), Some(Color::rgb(16, 32, 48)));
        assert_eq!(parse_hex("#10203040"), Some(Color::rgba(16, 32, 48, 64)));
    }

    #[test]
    fn the_hash_is_optional_and_case_is_ignored() {
        assert_eq!(parse_hex("FF00ff"), parse_hex("#ff00FF"));
    }

    #[test]
    fn malformed_input_is_rejected_rather_than_guessed_at() {
        for bad in [
            "",
            "#",
            "#ff",
            "#fffff",
            "#fffffff",
            "#gggggg",
            "#ff 00 ff",
            "123456789",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_lone_hash_does_not_underflow_the_length() {
        // `bytes.len() - start` is the one subtraction here that could wrap.
        assert_eq!(parse_hex("#"), None);
    }

    #[test]
    fn nibble_doubling_reaches_the_extremes_exactly() {
        // 15 * 17 must be 255, not 254: #fff has to be pure white or every
        // shorthand colour in a scene file is imperceptibly wrong.
        assert_eq!(parse_hex("#fff"), Some(Color::WHITE));
        assert_eq!(parse_hex("#000f"), Some(Color::BLACK));
    }
}
