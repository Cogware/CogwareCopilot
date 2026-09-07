// SPDX-License-Identifier: MIT OR Apache-2.0
//! Packing glyph coverage into the CPFN atlas format.
//!
//! The exact inverse of `copilot::font::Font::parse`. The two are in
//! different crates because one runs on a developer's machine and the other
//! on a microcontroller; the round-trip test is what keeps them honest about
//! being the same format.

/// Why an atlas could not be built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteError {
    /// cell_w or cell_h was outside the format's limits.
    BadCell,
    /// No glyphs were supplied.
    Empty,
    /// A glyph bitmap was not cell_w * cell_h entries.
    BadGlyph {
        /// Index of the offending glyph.
        index: usize,
    },
}

/// Pack glyph coverage bitmaps into a CPFN atlas.
///
/// `glyphs` holds one entry per character starting at `first_char`, each of
/// which is `cell_w * cell_h` booleans in row-major order: true means ink.
pub fn write_atlas(
    cell_w: u8,
    cell_h: u8,
    advance: u8,
    line_height: u8,
    first_char: u32,
    glyphs: &[Vec<bool>],
) -> Result<Vec<u8>, WriteError> {
    // Validate cell dimensions against the format's hard limits.
    if !(1..=32).contains(&cell_w) || !(1..=64).contains(&cell_h) {
        return Err(WriteError::BadCell);
    }

    // An atlas with zero glyphs is meaningless.
    if glyphs.is_empty() {
        return Err(WriteError::Empty);
    }

    let expected_len = cell_w as usize * cell_h as usize;

    // Validate all glyph sizes up front so we can pre-allocate the exact buffer.
    for (i, g) in glyphs.iter().enumerate() {
        if g.len() != expected_len {
            return Err(WriteError::BadGlyph { index: i });
        }
    }

    let count = glyphs.len() as u32;
    let row_bytes = (cell_w as usize).div_ceil(8);
    let glyph_data_len = count as usize * row_bytes * cell_h as usize;

    // Header is 18 bytes; total is header + glyph data.
    let total_len = 18 + glyph_data_len;
    let mut out = Vec::with_capacity(total_len);

    // Magic
    out.extend_from_slice(b"CPFN");
    // Version
    out.extend_from_slice(&1u16.to_le_bytes());
    // Cell dimensions
    out.push(cell_w);
    out.push(cell_h);
    // Advance and line height
    out.push(advance);
    out.push(line_height);
    // First character
    out.extend_from_slice(&first_char.to_le_bytes());
    // Glyph count
    out.extend_from_slice(&count.to_le_bytes());

    // Pack each glyph's booleans into bits, MSB-first within each byte.
    //
    // A row is `row_bytes` wide, not one byte: a cell wider than eight pixels
    // spills into the next byte, and `0x80 >> col` would shift past the end of
    // a u8 the moment it did.
    for glyph in glyphs.iter() {
        for row in 0..cell_h as usize {
            for chunk in 0..row_bytes {
                let mut byte = 0u8;
                for bit in 0..8usize {
                    let col = chunk * 8 + bit;
                    if col >= cell_w as usize {
                        // The last byte of a row is padded; a cell 17 wide
                        // leaves seven unused bits, and they must be clear or
                        // the parser reads them as ink.
                        break;
                    }
                    let idx = row * cell_w as usize + col;
                    if glyph.get(idx).copied().unwrap_or(false) {
                        byte |= 0x80u8 >> bit;
                    }
                }
                out.push(byte);
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkerboard, which is asymmetric in both axes -- a solid or striped
    /// pattern would pass even with the bit order or the stride wrong.
    fn checker(w: usize, h: usize) -> Vec<bool> {
        (0..w * h)
            .map(|i| (i % w + i / w).is_multiple_of(2))
            .collect()
    }

    #[test]
    fn the_header_says_what_was_asked_for() {
        let g = vec![checker(5, 7)];
        let out = write_atlas(5, 7, 6, 9, 32, &g).unwrap();
        assert_eq!(&out[..4], b"CPFN");
        assert_eq!(u16::from_le_bytes([out[4], out[5]]), 1);
        assert_eq!((out[6], out[7], out[8], out[9]), (5, 7, 6, 9));
        assert_eq!(u32::from_le_bytes([out[10], out[11], out[12], out[13]]), 32);
        assert_eq!(u32::from_le_bytes([out[14], out[15], out[16], out[17]]), 1);
    }

    #[test]
    fn the_length_matches_the_declared_geometry() {
        let g = vec![checker(9, 3); 4];
        let out = write_atlas(9, 3, 10, 5, 65, &g).unwrap();
        // 9 wide needs two bytes a row.
        assert_eq!(out.len(), 18 + 4 * 2 * 3);
    }

    #[test]
    fn bad_geometry_is_rejected() {
        let g = vec![checker(1, 1)];
        assert_eq!(write_atlas(0, 1, 1, 1, 0, &g), Err(WriteError::BadCell));
        assert_eq!(write_atlas(33, 1, 1, 1, 0, &g), Err(WriteError::BadCell));
        assert_eq!(write_atlas(1, 0, 1, 1, 0, &g), Err(WriteError::BadCell));
        assert_eq!(write_atlas(1, 65, 1, 1, 0, &g), Err(WriteError::BadCell));
    }

    #[test]
    fn an_empty_set_of_glyphs_is_rejected() {
        assert_eq!(write_atlas(5, 7, 6, 9, 32, &[]), Err(WriteError::Empty));
    }

    #[test]
    fn a_wrongly_sized_glyph_names_its_index() {
        let g = vec![checker(5, 7), vec![false; 3], checker(5, 7)];
        assert_eq!(
            write_atlas(5, 7, 6, 9, 32, &g),
            Err(WriteError::BadGlyph { index: 1 })
        );
    }

    #[test]
    fn what_is_written_is_what_copilot_reads_back() {
        // The property that actually matters: the writer here and the parser
        // in copilot are two implementations of one format, in two crates,
        // and nothing but this stops them drifting apart.
        let (w, h) = (5usize, 7usize);
        let glyphs: Vec<Vec<bool>> = (0..3).map(|_| checker(w, h)).collect();
        let bytes = write_atlas(w as u8, h as u8, 6, 9, 65, &glyphs).unwrap();

        let font = copilot::font::Font::parse(&bytes).expect("copilot must read it back");
        assert_eq!((font.cell_w, font.cell_h), (5, 7));
        assert_eq!(font.first_char, 65);
        assert_eq!(font.count, 3);
        for (n, ch) in ['A', 'B', 'C'].iter().enumerate() {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        font.pixel(*ch, x as u32, y as u32),
                        glyphs[n][y * w + x],
                        "glyph {n} pixel {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_wide_cell_spans_multiple_bytes_per_row_correctly() {
        // The stride is ceil(w/8); getting it wrong shears every glyph wider
        // than a byte, which is invisible in a hex dump.
        let (w, h) = (17usize, 2usize);
        let mut g = vec![false; w * h];
        g[16] = true; // last pixel of row 0
        g[w] = true; // first pixel of row 1
        let bytes = write_atlas(w as u8, h as u8, 18, 4, 48, &[g]).unwrap();
        let font = copilot::font::Font::parse(&bytes).unwrap();
        assert!(font.pixel('0', 16, 0));
        assert!(!font.pixel('0', 15, 0));
        assert!(font.pixel('0', 0, 1));
    }
}
