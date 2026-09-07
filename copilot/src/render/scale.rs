// SPDX-License-Identifier: MIT OR Apache-2.0
//! Resampling an image to a widget's size.
//!
//! Nearest-neighbour only. Bilinear would look better on a photograph and
//! worse on the thing this actually draws -- an icon or a gauge face with
//! hard edges, where interpolation turns a crisp one-pixel line into a grey
//! smear. A scene that wants smooth scaling should ship the image at the
//! size it will be drawn.

use crate::Color;

/// Resample `src` (a `src_w` x `src_h` image) to `dst_w` x `dst_h` using
/// nearest-neighbour sampling.
pub fn scale_nearest(
    src: &[Color],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> alloc::vec::Vec<Color> {
    // Guard against degenerate dimensions or insufficient source data.
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return alloc::vec::Vec::new();
    }
    let required = (src_w as usize) * (src_h as usize);
    if src.len() < required {
        return alloc::vec::Vec::new();
    }

    let out_len = (dst_w as usize) * (dst_h as usize);
    let mut out = alloc::vec::Vec::with_capacity(out_len);

    // Precompute the max source indices to clamp against, avoiding
    // repeated subtraction and potential underflow on edge cases.
    let max_sx = (src_w - 1) as u64;
    let max_sy = (src_h - 1) as u64;
    let src_w_u64 = src_w as u64;
    let dst_w_u64 = dst_w as u64;
    let dst_h_u64 = dst_h as u64;

    for dy in 0..dst_h {
        let sy = ((dy as u64) * src_h as u64 / dst_h_u64).min(max_sy);
        for dx in 0..dst_w {
            let sx = ((dx as u64) * src_w as u64 / dst_w_u64).min(max_sx);
            let idx = (sy * src_w_u64 + sx) as usize;
            // Use .get() to guarantee no panic even if index math is off.
            let color = src.get(idx).copied().unwrap_or(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            });
            out.push(color);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn ramp(w: u32, h: u32) -> Vec<Color> {
        (0..w * h).map(|i| Color::rgb(i as u8, 0, 0)).collect()
    }

    #[test]
    fn identity_reproduces_the_source_exactly() {
        // The common case: an image already the right size must not be
        // resampled into something subtly different.
        let src = ramp(4, 3);
        assert_eq!(scale_nearest(&src, 4, 3, 4, 3), src);
    }

    #[test]
    fn the_output_is_always_the_requested_size() {
        for (dw, dh) in [(1, 1), (7, 3), (100, 1), (1, 100)] {
            let got = scale_nearest(&ramp(4, 4), 4, 4, dw, dh);
            assert_eq!(got.len(), (dw * dh) as usize, "for {dw}x{dh}");
        }
    }

    #[test]
    fn doubling_repeats_each_pixel() {
        let src = vec![Color::rgb(1, 0, 0), Color::rgb(2, 0, 0)];
        let got = scale_nearest(&src, 2, 1, 4, 1);
        assert_eq!(got[0].r, 1);
        assert_eq!(got[1].r, 1);
        assert_eq!(got[2].r, 2);
        assert_eq!(got[3].r, 2);
    }

    #[test]
    fn halving_samples_every_other_pixel() {
        let src = ramp(4, 1);
        let got = scale_nearest(&src, 4, 1, 2, 1);
        assert_eq!(got[0].r, 0);
        assert_eq!(got[1].r, 2);
    }

    #[test]
    fn the_corners_of_the_source_survive() {
        // A sampling bug that is off by one loses the last row or column, and
        // on a gauge face that is the tick mark at full scale.
        let src = ramp(3, 3);
        let got = scale_nearest(&src, 3, 3, 9, 9);
        assert_eq!(got[0].r, 0, "top left");
        assert_eq!(got[80].r, 8, "bottom right");
    }

    #[test]
    fn a_zero_dimension_yields_nothing() {
        assert!(scale_nearest(&ramp(2, 2), 0, 2, 2, 2).is_empty());
        assert!(scale_nearest(&ramp(2, 2), 2, 0, 2, 2).is_empty());
        assert!(scale_nearest(&ramp(2, 2), 2, 2, 0, 2).is_empty());
        assert!(scale_nearest(&ramp(2, 2), 2, 2, 2, 0).is_empty());
    }

    #[test]
    fn a_short_source_is_rejected_rather_than_read_past() {
        assert!(scale_nearest(&ramp(2, 2), 4, 4, 2, 2).is_empty());
    }

    #[test]
    fn an_enormous_target_does_not_overflow_the_index_maths() {
        // dx * src_w overflows u32 well before this; the spec says compute in
        // u64 for exactly this reason.
        let got = scale_nearest(&ramp(2, 1), 2, 1, 100_000, 1);
        assert_eq!(got.len(), 100_000);
        assert_eq!(got[99_999].r, 1);
    }
}
