// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fixed-cell bitmap fonts.
//!
//! Bitmap rather than outline, for the reason LVGL made the same choice:
//! rasterising a glyph outline needs floating point, a scanline fill and a
//! cache, and a dashboard draws the same dozen digits forever. A cell lookup
//! is a shift and a mask.
//!
//! Fixed cell rather than proportional in this version. Every glyph occupies
//! the same box, so a glyph's address is a multiplication rather than a table
//! lookup, and a column of numbers lines up without anyone thinking about it
//! -- which is what a speedometer wants. A per-glyph width table can be added
//! behind a header flag when proportional text is worth its cost.

//! Fixed-cell bitmap font atlas parser.

/// Why a font atlas could not be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontError {
    /// The first four bytes were not `CPFN`.
    BadMagic,
    /// The version field named a format this build cannot read.
    BadVersion,
    /// A header field held an impossible value.
    BadHeader,
    /// The glyph data was shorter than the header said.
    Truncated,
}

/// A fixed-cell bitmap font.
#[derive(Clone, Debug)]
pub struct Font<'a> {
    /// Width of one glyph cell in pixels.
    pub cell_w: u8,
    /// Height of one glyph cell in pixels.
    pub cell_h: u8,
    /// Pixels to advance after drawing a glyph.
    pub advance: u8,
    /// Pixels between baselines.
    pub line_height: u8,
    /// Unicode scalar of glyph index 0.
    pub first_char: u32,
    /// Number of glyphs.
    pub count: u32,
    /// Raw glyph bitmaps, borrowed from the input.
    glyphs: &'a [u8],
}

impl<'a> Font<'a> {
    /// Parse an atlas from bytes, borrowing the glyph data.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FontError> {
        if bytes.len() < 18 {
            return Err(FontError::Truncated);
        }

        if bytes[0..4] != *b"CPFN" {
            return Err(FontError::BadMagic);
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != 1 {
            return Err(FontError::BadVersion);
        }

        let cell_w = bytes[6];
        let cell_h = bytes[7];
        let advance = bytes[8];
        let line_height = bytes[9];

        if !(1..=32).contains(&cell_w) {
            return Err(FontError::BadHeader);
        }
        if !(1..=64).contains(&cell_h) {
            return Err(FontError::BadHeader);
        }

        let first_char = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let count = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);

        if count < 1 {
            return Err(FontError::BadHeader);
        }

        let row_bytes = (cell_w as usize).div_ceil(8);
        let bytes_per_glyph = row_bytes * cell_h as usize;
        let total_glyph_bytes = bytes_per_glyph * count as usize;

        let glyph_start = 18;
        if bytes.len() < glyph_start + total_glyph_bytes {
            return Err(FontError::Truncated);
        }

        let glyphs = &bytes[glyph_start..glyph_start + total_glyph_bytes];

        Ok(Font {
            cell_w,
            cell_h,
            advance,
            line_height,
            first_char,
            count,
            glyphs,
        })
    }

    /// Bytes per glyph row.
    pub fn row_bytes(&self) -> usize {
        (self.cell_w as usize).div_ceil(8)
    }

    /// Whether the pixel at (x, y) of the glyph for `ch` is ink.
    ///
    /// Returns false for any character outside the font, and for any (x, y)
    /// outside the cell, so callers never need to range check.
    pub fn pixel(&self, ch: char, x: u32, y: u32) -> bool {
        let ch_code = ch as u32;

        if ch_code < self.first_char {
            return false;
        }
        let index = ch_code - self.first_char;
        if index >= self.count {
            return false;
        }

        if x >= self.cell_w as u32 || y >= self.cell_h as u32 {
            return false;
        }

        let row_bytes = self.row_bytes();
        let byte_offset = index as usize * row_bytes * self.cell_h as usize
            + y as usize * row_bytes
            + (x / 8) as usize;

        let byte = match self.glyphs.get(byte_offset) {
            Some(b) => *b,
            None => return false,
        };

        let bit = 7 - (x % 8);
        (byte >> bit) & 1 != 0
    }
}

/// The font compiled into the crate.
///
/// A 5x7 cell covering printable ASCII, authored for this project so the crate
/// carries no third-party font data and no licence obligations beyond its own.
/// It exists so a label draws something the moment a scene asks for one; a real
/// panel is expected to ship its own atlas built by `fontconv`.
pub const DEFAULT: &[u8] = include_bytes!("default.cpfn");

/// The built-in font, parsed.
///
/// # Panics
///
/// Never in practice: the atlas is compiled in and covered by a test. The
/// `expect` is the enforcement of that, not a hope.
#[must_use]
pub fn default_font() -> Font<'static> {
    Font::parse(DEFAULT).expect("the compiled-in atlas must parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_atlas_parses() {
        let f = default_font();
        assert_eq!((f.cell_w, f.cell_h), (5, 7));
        assert_eq!(f.first_char, 32);
        assert_eq!(f.count, 95, "printable ASCII, space through tilde");
    }

    #[test]
    fn a_space_is_blank_and_a_digit_is_not() {
        let f = default_font();
        let ink =
            |ch| (0..f.cell_h as u32).any(|y| (0..f.cell_w as u32).any(|x| f.pixel(ch, x, y)));
        assert!(!ink(' '), "space must be empty");
        for ch in "0123456789".chars() {
            assert!(ink(ch), "{ch} has no ink");
        }
    }

    #[test]
    fn the_glyph_for_one_has_its_stem_where_it_was_drawn() {
        // Pins the bit order: bit 7 of a row byte is the LEFTMOST pixel. Get
        // that backwards and every glyph is mirrored, which is obvious on a
        // screen and invisible in a hex dump.
        let f = default_font();
        assert!(f.pixel('1', 2, 0), "top of the stem");
        assert!(f.pixel('1', 1, 1), "the flag on the left");
        assert!(!f.pixel('1', 4, 0), "nothing on the right");
    }

    #[test]
    fn characters_outside_the_font_are_blank_rather_than_wrapping() {
        // A char below first_char would make the index subtraction underflow;
        // one above count would read another glyph's bits.
        let f = default_font();
        assert!(!f.pixel('\u{1}', 0, 0));
        assert!(!f.pixel('\u{2028}', 0, 0));
        assert!(!f.pixel('~', 99, 0), "out of cell horizontally");
        assert!(!f.pixel('~', 0, 99), "out of cell vertically");
    }

    #[test]
    fn a_truncated_atlas_is_rejected() {
        for n in [0, 4, 17, DEFAULT.len() - 1] {
            assert!(Font::parse(&DEFAULT[..n]).is_err(), "{n} bytes should fail");
        }
    }

    #[test]
    fn a_foreign_or_future_atlas_is_rejected() {
        let mut bad = DEFAULT.to_vec();
        bad[0] = b'X';
        assert_eq!(Font::parse(&bad).unwrap_err(), FontError::BadMagic);

        let mut bad = DEFAULT.to_vec();
        bad[4] = 99;
        assert_eq!(Font::parse(&bad).unwrap_err(), FontError::BadVersion);
    }

    #[test]
    fn an_impossible_cell_size_is_rejected() {
        let mut bad = DEFAULT.to_vec();
        bad[6] = 0;
        assert_eq!(Font::parse(&bad).unwrap_err(), FontError::BadHeader);
        let mut bad = DEFAULT.to_vec();
        bad[7] = 200;
        assert!(Font::parse(&bad).is_err());
    }
}
