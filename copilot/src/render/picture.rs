// SPDX-License-Identifier: GPL-3.0-only
//! Getting a decoded picture onto the surface.
//!
//! Split from [`super::raster`] because a picture has a second rectangle that
//! a primitive does not: where the whole of it would go, which is what fixes
//! the source pixel each destination pixel takes, as well as what may be
//! painted.

use crate::surface::TextureId;
use crate::{Color, Rect, Surface};

use super::raster::clip;

/// Blit `src`, a `width`-pixel-wide RGBA image, with its top-left at `at`.
///
/// Rows are copied whole where the image is fully on screen and sliced where
/// it is not, so a partly off-screen image costs only the pixels that land.
///
/// `src` shorter than `width * height` draws only the rows it can supply,
/// which makes a truncated decode degrade to a partial image rather than to a
/// panic.
pub fn blit<S: Surface + ?Sized>(surface: &mut S, at: Rect, src: &[Color], width: u32) {
    if width == 0 {
        return;
    }
    let Some(r) = clip(surface, at) else {
        return;
    };
    blit_area(surface, at, r, src, width);
}

/// Blit the part of `src` that falls inside `area`, with the whole of it
/// placed at `at`.
///
/// `area` must already be inside both `at` and the surface.
fn blit_area<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    area: Rect,
    src: &[Color],
    width: u32,
) {
    // How far into the source the clip moved us. Both are non-negative because
    // `area` is contained in `at`, so it can only have shrunk from it.
    let skip_x = (area.left() - at.left()) as usize;
    let skip_y = (area.top() - at.top()) as usize;
    let stride = width as usize;

    for (n, y) in (area.top()..area.bottom()).enumerate() {
        let row_start = (skip_y + n) * stride + skip_x;
        let Some(row) = src.get(row_start..) else {
            return;
        };
        let take = (area.size.w as usize).min(row.len());
        if take == 0 {
            return;
        }
        surface.blit_span(area.left(), y, &row[..take]);
    }
}

/// Draw a `src_w` x `src_h` image scaled to fill `dst`, clipped to `clip`.
///
/// Tried in order: the backend draws `id` from memory it already holds, the
/// backend takes the pixels and scales them itself, or the rasteriser
/// resamples and blits.
pub fn image<S: Surface + ?Sized>(
    surface: &mut S,
    id: TextureId,
    dst: Rect,
    clip_to: Rect,
    src: &[Color],
    src_w: u32,
    src_h: u32,
) {
    if src_w == 0 || src_h == 0 || dst.is_empty() {
        return;
    }
    let Some(area) = dst.intersection(clip_to).and_then(|a| clip(surface, a)) else {
        return;
    };

    if surface.draw_texture(id, dst, area) || surface.draw_image(dst, area, src, src_w, src_h) {
        return;
    }

    if src_w == dst.size.w && src_h == dst.size.h {
        blit_area(surface, dst, area, src, src_w);
        return;
    }
    // Scaling to the widget's rectangle is what lets a scene be authored once
    // and shown on a panel of a different size.
    blit_scaled(surface, dst, area, src, src_w, src_h);
}

/// How many destination pixels are resampled before being handed over.
const CHUNK: usize = 64;

/// Draw `src` resampled to `dst`, painting only `area`, without allocating.
///
/// A chunk of a row at a time into a fixed buffer, because building the whole
/// scaled image would allocate in proportion to the widget's area on every
/// frame. The sampling matches
/// [`scale_nearest`](super::scale::scale_nearest) exactly.
fn blit_scaled<S: Surface + ?Sized>(
    surface: &mut S,
    dst: Rect,
    area: Rect,
    src: &[Color],
    src_w: u32,
    src_h: u32,
) {
    let (dw, dh) = (u64::from(dst.size.w), u64::from(dst.size.h));
    if dw == 0 || dh == 0 || src_w == 0 || src_h == 0 {
        return;
    }
    // In u64 throughout: `dx * src_w` leaves u32 for a wide enough widget.
    let (sw, sh) = (u64::from(src_w), u64::from(src_h));
    let (max_sx, max_sy) = (sw - 1, sh - 1);
    let mut buf = [Color::TRANSPARENT; CHUNK];

    for y in area.top()..area.bottom() {
        // Measured from `dst`, never from `area`, so a partial repaint samples
        // exactly what a full one did.
        let dy = u64::from((y - dst.top()).unsigned_abs());
        let sy = (dy * sh / dh).min(max_sy);
        let Ok(row) = usize::try_from(sy * sw) else {
            return;
        };

        let mut x = area.left();
        while x < area.right() {
            let want = ((area.right() - x) as usize).min(CHUNK);
            let mut n = 0;
            while n < want {
                let dx = u64::from((x + n as i32 - dst.left()).unsigned_abs());
                let sx = (dx * sw / dw).min(max_sx);
                // A source shorter than it claims degrades to the rows it had.
                let Some(c) = row.checked_add(sx as usize).and_then(|i| src.get(i)) else {
                    if n > 0 {
                        surface.blit_span(x, y, &buf[..n]);
                    }
                    return;
                };
                buf[n] = *c;
                n += 1;
            }
            surface.blit_span(x, y, &buf[..n]);
            x += n as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::scale::scale_nearest;
    use crate::{MemorySurface, PixelFormat, Size};
    use alloc::vec;
    use alloc::vec::Vec;

    /// A clip rectangle that admits the whole of the fake surfaces below.
    const FULL: Rect = Rect::new(0, 0, 100, 100);

    /// A clip admitting the whole of the 40x40 reference buffers.
    const FULL40: Rect = Rect::new(0, 0, 40, 40);

    /// Records every span it is handed, so a test can assert on what the
    /// rasteriser *emitted* rather than on the pixels that resulted.
    struct Recorder {
        size: Size,
        blits: Vec<(i32, i32, usize)>,
    }

    impl Recorder {
        fn new(w: u32, h: u32) -> Self {
            Self {
                size: Size { w, h },
                blits: Vec::new(),
            }
        }
        /// Panics if anything landed outside the surface, which every backend
        /// is promised.
        fn assert_in_bounds(&self) {
            for &(x, y, n) in &self.blits {
                assert!(x >= 0 && y >= 0 && y < self.size.h as i32);
                assert!(x as i64 + n as i64 <= self.size.w as i64);
            }
        }
    }

    impl Surface for Recorder {
        fn size(&self) -> Size {
            self.size
        }
        fn format(&self) -> PixelFormat {
            PixelFormat::Bgrx8888
        }
        fn fill_span(&mut self, _x: i32, _y: i32, _count: u32, _c: Color) {}
        fn blit_span(&mut self, x: i32, y: i32, src: &[Color]) {
            self.blits.push((x, y, src.len()));
        }
        fn present(&mut self, _damage: Option<Rect>) {}
    }

    /// A backend that takes whichever hooks it was built to take and records
    /// what it was asked for, so a test can check the software path did *not*
    /// also run.
    struct Accel {
        size: Size,
        takes: Take,
        rects: Vec<(Rect, Color)>,
        images: Vec<(Rect, Rect, u32, u32)>,
        textures: Vec<(TextureId, Rect, Rect)>,
        spans: usize,
    }

    /// Which hooks the fake backend claims.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Take {
        Nothing,
        Rects,
        Images,
        Textures,
    }

    impl Accel {
        fn new(takes: Take) -> Self {
            Self {
                size: Size { w: 100, h: 100 },
                takes,
                rects: Vec::new(),
                images: Vec::new(),
                textures: Vec::new(),
                spans: 0,
            }
        }
    }

    impl Surface for Accel {
        fn size(&self) -> Size {
            self.size
        }
        fn format(&self) -> PixelFormat {
            PixelFormat::Bgrx8888
        }
        fn fill_span(&mut self, _x: i32, _y: i32, _n: u32, _c: Color) {
            self.spans += 1;
        }
        fn blit_span(&mut self, _x: i32, _y: i32, _src: &[Color]) {
            self.spans += 1;
        }
        fn draw_rect(&mut self, rect: Rect, color: Color) -> bool {
            self.rects.push((rect, color));
            self.takes == Take::Rects
        }
        fn draw_image(&mut self, dst: Rect, area: Rect, _s: &[Color], w: u32, h: u32) -> bool {
            self.images.push((dst, area, w, h));
            self.takes == Take::Images
        }
        fn draw_texture(&mut self, id: TextureId, dst: Rect, area: Rect) -> bool {
            self.textures.push((id, dst, area));
            self.takes == Take::Textures
        }
        fn present(&mut self, _damage: Option<Rect>) {}
    }

    const ID: TextureId = TextureId::Image { index: 7 };
    /// The same image drawn straight into a buffer, so a test can compare the
    /// scaled path against a reference rather than against hand-written
    /// pixels. Scaling that is subtly wrong looks fine and matches nothing.
    fn reference(dst: Rect, src: &[Color], src_w: u32, src_h: u32) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w: 40, h: 40 }, PixelFormat::Bgrx8888);
        let scaled = scale_nearest(src, src_w, src_h, dst.size.w, dst.size.h);
        blit(&mut s, dst, &scaled, dst.size.w);
        s
    }

    fn ramp(w: u32, h: u32) -> Vec<Color> {
        (0..w * h)
            .map(|i| Color::rgb(i as u8, (i * 7) as u8, (i * 13) as u8))
            .collect()
    }

    #[test]
    fn scaling_without_allocating_lands_the_same_pixels_as_scaling_with() {
        // The change this test exists to protect is a performance one, and a
        // performance change that alters a pixel is a bug. Every ratio: up,
        // down, and neither.
        for (dw, dh) in [(8, 8), (16, 12), (3, 5), (17, 1), (1, 17), (32, 32)] {
            let dst = Rect::new(2, 3, dw, dh);
            let src = ramp(8, 8);
            let want = reference(dst, &src, 8, 8);

            let mut got = MemorySurface::new(Size { w: 40, h: 40 }, PixelFormat::Bgrx8888);
            image(&mut got, ID, dst, FULL40, &src, 8, 8);

            assert_eq!(
                got.pixels(),
                want.pixels(),
                "scaling 8x8 to {dw}x{dh} diverged from the reference"
            );
        }
    }

    #[test]
    fn a_scaled_image_is_still_confined_to_the_damage_rectangle() {
        // The source pixel a destination pixel takes must be fixed by where
        // the whole picture goes, not by the strip of it being painted -- get
        // that wrong and the image slides under its own clip.
        let src = ramp(4, 4);
        let dst = Rect::new(0, 0, 16, 16);

        let full = reference(dst, &src, 4, 4);
        let mut clipped = MemorySurface::new(Size { w: 40, h: 40 }, PixelFormat::Bgrx8888);
        image(&mut clipped, ID, dst, Rect::new(4, 4, 6, 6), &src, 4, 4);

        for y in 0..16usize {
            for x in 0..16usize {
                let inside = (4..10).contains(&x) && (4..10).contains(&y);
                let o = y * clipped.stride() + x * 4;
                let got = &clipped.pixels()[o..o + 4];
                if inside {
                    assert_eq!(got, &full.pixels()[o..o + 4], "at ({x},{y})");
                } else {
                    assert_eq!(got, &[0, 0, 0, 0], "painted outside the clip at ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn a_truncated_source_scales_to_a_partial_picture_not_a_panic() {
        // A half-decoded file is a normal thing to be handed.
        let mut s = MemorySurface::new(Size { w: 40, h: 40 }, PixelFormat::Bgrx8888);
        let short = alloc::vec![Color::WHITE; 5];
        image(&mut s, ID, Rect::new(0, 0, 20, 20), FULL40, &short, 4, 4);
        assert_ne!(s.pixels()[..4], [0, 0, 0, 0], "the rows it had should draw");
    }

    #[test]
    fn an_enormous_upscale_does_not_overflow_the_index_arithmetic() {
        // dx * src_w leaves u32 long before this; the sampling has to be done
        // in u64 for the same reason `scale_nearest` does it.
        let mut s = MemorySurface::new(Size { w: 40, h: 40 }, PixelFormat::Bgrx8888);
        image(
            &mut s,
            ID,
            Rect::new(0, 0, 100_000, 4),
            FULL40,
            &ramp(2, 2),
            2,
            2,
        );
        assert_ne!(s.pixels()[..4], [0, 0, 0, 0]);
    }

    #[test]
    fn a_resident_texture_is_drawn_without_the_pixels_being_touched() {
        let mut s = Accel::new(Take::Textures);
        let src = vec![Color::WHITE; 4];
        image(&mut s, ID, Rect::new(0, 0, 20, 20), FULL, &src, 2, 2);
        assert_eq!(
            s.textures,
            vec![(ID, Rect::new(0, 0, 20, 20), Rect::new(0, 0, 20, 20))]
        );
        assert!(s.images.is_empty(), "no need to offer the pixels as well");
        assert_eq!(s.spans, 0);
    }

    #[test]
    fn a_backend_that_holds_no_texture_is_offered_the_pixels_and_the_scale() {
        // The source size and the destination size both, so a backend with a
        // sampler can do the resampling the rasteriser would otherwise
        // allocate a whole scaled image for.
        let mut s = Accel::new(Take::Images);
        let src = vec![Color::WHITE; 4];
        image(&mut s, ID, Rect::new(0, 0, 20, 20), FULL, &src, 2, 2);
        assert_eq!(s.textures.len(), 1, "the texture was offered first");
        assert_eq!(
            s.images,
            vec![(Rect::new(0, 0, 20, 20), Rect::new(0, 0, 20, 20), 2, 2)]
        );
        assert_eq!(s.spans, 0);
    }

    #[test]
    fn declining_both_leaves_the_rasteriser_to_scale_and_blit() {
        let mut s = Accel::new(Take::Nothing);
        let src = vec![Color::WHITE; 4];
        image(&mut s, ID, Rect::new(0, 0, 20, 20), FULL, &src, 2, 2);
        assert_eq!(s.spans, 20, "one blit per row of the destination");
    }

    #[test]
    fn an_image_is_confined_to_the_damage_rectangle() {
        // It was not, and that is a real bug rather than wasted work: the
        // compositor walks the whole tree once per damage rectangle, so pixels
        // painted outside the current one overwrite a later sibling that has
        // already been drawn correctly for a different rectangle.
        let mut s = Accel::new(Take::Nothing);
        let src = vec![Color::WHITE; 100];
        image(
            &mut s,
            ID,
            Rect::new(0, 0, 10, 10),
            Rect::new(0, 0, 10, 3),
            &src,
            10,
            10,
        );
        assert_eq!(s.spans, 3, "seven rows outside the damage were painted");
    }

    #[test]
    fn an_image_entirely_outside_the_damage_draws_nothing() {
        let mut s = Accel::new(Take::Rects);
        let src = vec![Color::WHITE; 4];
        image(
            &mut s,
            ID,
            Rect::new(0, 0, 2, 2),
            Rect::new(50, 50, 4, 4),
            &src,
            2,
            2,
        );
        assert!(s.textures.is_empty() && s.images.is_empty());
        assert_eq!(s.spans, 0);
    }

    #[test]
    fn a_zero_sized_image_is_never_offered() {
        let mut s = Accel::new(Take::Textures);
        image(&mut s, ID, Rect::new(0, 0, 4, 4), FULL, &[], 0, 4);
        image(&mut s, ID, Rect::new(0, 0, 4, 4), FULL, &[], 4, 0);
        image(
            &mut s,
            ID,
            Rect::new(0, 0, 0, 0),
            FULL,
            &[Color::WHITE],
            1,
            1,
        );
        assert!(s.textures.is_empty());
    }

    #[test]
    fn a_blit_offsets_into_the_source_when_clipped() {
        let mut s = Recorder::new(100, 100);
        let img = vec![Color::WHITE; 10 * 10];
        blit(&mut s, Rect::new(-3, -2, 10, 10), &img, 10);
        // Seven columns and eight rows survive the clip.
        assert_eq!(s.blits.len(), 8);
        assert!(s.blits.iter().all(|&(x, _, n)| x == 0 && n == 7));
        s.assert_in_bounds();
    }

    #[test]
    fn a_truncated_image_draws_the_rows_it_has() {
        // A partial decode should degrade to a partial image, not a panic.
        let mut s = Recorder::new(100, 100);
        let img = vec![Color::WHITE; 25];
        blit(&mut s, Rect::new(0, 0, 10, 10), &img, 10);
        assert_eq!(s.blits.len(), 3);
        s.assert_in_bounds();
    }

    #[test]
    fn a_zero_width_image_draws_nothing() {
        let mut s = Recorder::new(100, 100);
        blit(&mut s, Rect::new(0, 0, 10, 10), &[Color::WHITE], 0);
        assert!(s.blits.is_empty());
    }
}
