// SPDX-License-Identifier: GPL-3.0-only
//! Shrinking a frame to the size it is shown at.
//!
//! The preview is a texture drawn at whatever scale fits the panel, and a
//! 2400-pixel cluster fits at about a fifth. Sampled nearest-neighbour at
//! that scale, four rows in five are simply not looked at, and a one-pixel
//! gap between two cells of a bargraph is there or not depending on which
//! row the sampler happened to land on. Half the cells looked joined. The
//! picture people used to judge a gauge was lying about it.
//!
//! So a frame shown smaller than life is shrunk here first, every source
//! pixel counted into the target pixel it falls in, and the GPU is only
//! asked to scale by the fraction left over. A frame shown at life size or
//! larger is left alone: nearest sampling is exactly right there, and is
//! what keeps a zoomed-in pixel a crisp square.

/// Shrink an RGBA image of `w` by `h` to `tw` by `th`, averaging the source
/// pixels each target pixel covers.
///
/// A box filter: each target pixel is the mean of the source pixels whose
/// centres fall in its footprint. Not the best filter there is, but the one
/// whose result a person can predict -- a gap five pixels wide shown at a
/// fifth is one pixel a fifth as bright as its neighbours, never absent.
/// Alpha is ignored and written opaque, since the frame is.
#[must_use]
pub fn shrink(rgba: &[u8], w: usize, h: usize, tw: usize, th: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(tw * th * 4);
    if w == 0 || h == 0 || tw == 0 || th == 0 || rgba.len() < w * h * 4 {
        return out;
    }
    // Source rows and columns per target one, as spans: the first source
    // index each target index covers.
    let edges = |n: usize, tn: usize| -> Vec<usize> { (0..=tn).map(|i| i * n / tn).collect() };
    let xs = edges(w, tw);
    let ys = edges(h, th);
    for ty in 0..th {
        let (y0, y1) = (ys[ty], ys[ty + 1].max(ys[ty] + 1).min(h));
        for tx in 0..tw {
            let (x0, x1) = (xs[tx], xs[tx + 1].max(xs[tx] + 1).min(w));
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for y in y0..y1 {
                let row = &rgba[(y * w + x0) * 4..(y * w + x1) * 4];
                for px in row.as_chunks::<4>().0 {
                    r += u32::from(px[0]);
                    g += u32::from(px[1]);
                    b += u32::from(px[2]);
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let mean = |v: u32| ((v + n / 2) / n) as u8;
            out.extend_from_slice(&[mean(r), mean(g), mean(b), 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` by `h` image, every pixel `f(x, y)` in grey.
    fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let g = f(x, y);
                v.extend_from_slice(&[g, g, g, 255]);
            }
        }
        v
    }

    #[test]
    fn a_thin_dark_line_survives_as_a_dim_one() {
        // Five rows of white with one black row, shown at a fifth: the
        // black row is a fifth of one output pixel, not nothing.
        let src = image(10, 10, |_, y| if y == 4 { 0 } else { 255 });
        let out = shrink(&src, 10, 10, 2, 2);
        assert_eq!(out.len(), 2 * 2 * 4);
        let top_left = out[0];
        assert!(
            top_left > 190 && top_left < 215,
            "got {top_left}, want about 204"
        );
        assert_eq!(out[2 * 4], 255, "the bottom half has no line in it");
    }

    #[test]
    fn a_solid_image_stays_solid() {
        let src = image(7, 5, |_, _| 77);
        let out = shrink(&src, 7, 5, 3, 2);
        assert!(
            out.as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [77, 77, 77, 255])
        );
    }

    #[test]
    fn every_source_pixel_is_counted_once() {
        // Averaging preserves the mean brightness: the total of the output
        // equals the total of the input, scaled by area.
        let src = image(9, 9, |x, y| ((x * 7 + y * 13) % 256) as u8);
        let out = shrink(&src, 9, 9, 3, 3);
        let mean_in: f64 = src
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| f64::from(p[0]))
            .sum::<f64>()
            / 81.0;
        let mean_out: f64 = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| f64::from(p[0]))
            .sum::<f64>()
            / 9.0;
        assert!((mean_in - mean_out).abs() < 1.0, "{mean_in} vs {mean_out}");
    }

    #[test]
    fn degenerate_sizes_give_nothing_rather_than_panicking() {
        assert!(shrink(&[], 0, 0, 1, 1).is_empty());
        assert!(shrink(&image(4, 4, |_, _| 1), 4, 4, 0, 3).is_empty());
        assert!(
            shrink(&[1, 2, 3], 4, 4, 2, 2).is_empty(),
            "too short a buffer"
        );
    }

    #[test]
    fn a_target_larger_than_the_source_still_covers_every_pixel() {
        // Not a use this module is for, but it must not index past the end.
        let src = image(2, 2, |x, _| (x * 200) as u8);
        let out = shrink(&src, 2, 2, 5, 5);
        assert_eq!(out.len(), 5 * 5 * 4);
    }
}
