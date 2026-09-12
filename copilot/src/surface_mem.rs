// SPDX-License-Identifier: GPL-3.0-only
//! A [`Surface`] over an ordinary heap buffer.
//!
//! This is the reference implementation of the trait and the one the tests and
//! the simulator draw into. It is also the worked example for anyone writing a
//! real backend: flat memory, so [`Surface::row_mut`] is implemented and the
//! renderer can bypass the per-span dispatch entirely.

use crate::{Caps, Color, PixelFormat, Rect, Size, Surface};
use alloc::vec;
use alloc::vec::Vec;

/// A Surface backed by an ordinary heap buffer.
pub struct MemorySurface {
    buf: Vec<u8>,
    size: Size,
    format: PixelFormat,
    /// A second buffer of the same size, when one was asked for.
    scratch: Option<Vec<u8>>,
}

impl MemorySurface {
    /// A zeroed surface of the given size and format.
    pub fn new(size: Size, format: PixelFormat) -> Self {
        let bpp = format.bytes_per_pixel();
        let len = size.w as usize * size.h as usize * bpp;
        Self {
            buf: vec![0u8; len],
            size,
            format,
            scratch: None,
        }
    }

    /// The same surface with a second buffer, so a cross-fade can blend.
    ///
    /// Opt-in because it doubles the memory, and only a cross-fade uses it.
    #[must_use]
    pub fn with_scratch_buffer(mut self) -> Self {
        self.scratch = Some(vec![0u8; self.buf.len()]);
        self
    }

    /// The whole buffer, as the backend's own byte layout.
    pub fn pixels(&self) -> &[u8] {
        &self.buf
    }

    /// Bytes per row.
    #[must_use]
    pub fn stride(&self) -> usize {
        self.size.w as usize * self.format.bytes_per_pixel()
    }

    /// Overwrite every pixel with `color`.
    ///
    /// Not part of [`Surface`]: a real backend clears by drawing, and offering
    /// a trait method for it would invite callers to clear a framebuffer that
    /// is about to be fully repainted anyway -- which on a non-cacheable
    /// mapping is the most expensive thing they could do. It exists here
    /// because a host-side buffer genuinely does need resetting when the scene
    /// it was showing is replaced.
    pub fn clear(&mut self, color: Color) {
        let bpp = self.format.bytes_per_pixel();
        let (packed, sig) = color.pack(self.format);
        for px in self.buf.chunks_exact_mut(bpp) {
            px[..sig].copy_from_slice(&packed[..sig]);
        }
    }

    /// Byte offset and pixel count of the on-surface part of a span.
    ///
    /// Returns `None` when none of it lands. The renderer has already clipped,
    /// so this is belt and braces -- but `Surface` is a public trait anyone may
    /// call directly, and no caller may be able to make this panic.
    fn span_range(&self, x: i32, y: i32, count: usize) -> Option<(usize, usize)> {
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        let (w, h) = (self.size.w as usize, self.size.h as usize);
        if x >= w || y >= h {
            return None;
        }
        let len = count.min(w - x);
        if len == 0 {
            return None;
        }
        Some((y * self.stride() + x * self.format.bytes_per_pixel(), len))
    }
}

impl Surface for MemorySurface {
    fn size(&self) -> Size {
        self.size
    }

    fn format(&self) -> PixelFormat {
        self.format
    }

    /// Both of the ones a plain buffer earns, and neither of them by
    /// accident: the pixels can be read back because they are right there,
    /// and they survive a present because nothing happens on one.
    fn caps(&self) -> Caps {
        let base = Caps::READ_BACK | Caps::RETAINS_CONTENT;
        if self.scratch.is_some() {
            base | Caps::SCRATCH
        } else {
            base
        }
    }

    fn with_scratch(&mut self, alpha: u8, draw: &mut dyn FnMut(&mut dyn Surface)) -> bool {
        let Some(mut buf) = self.scratch.take() else {
            return false;
        };
        // Drawn into a surface of its own so the renderer sees an ordinary
        // target, then blended back a pixel at a time.
        let mut into = Self {
            buf: core::mem::take(&mut buf),
            size: self.size,
            format: self.format,
            scratch: None,
        };
        into.clear(Color::TRANSPARENT);
        draw(&mut into);

        let bpp = self.format.bytes_per_pixel();
        let format = self.format;
        for (front, over) in self
            .buf
            .chunks_exact_mut(bpp)
            .zip(into.buf.chunks_exact(bpp))
        {
            let under = Color::unpack(front, format);
            let mut top = Color::unpack(over, format);
            top.a = alpha;
            let (packed, sig) = top.over(under).pack(format);
            front[..sig].copy_from_slice(&packed[..sig]);
        }
        self.scratch = Some(into.buf);
        true
    }

    fn fill_span(&mut self, x: i32, y: i32, count: u32, color: Color) {
        let bpp = self.format.bytes_per_pixel();
        let (packed, sig) = color.pack(self.format);

        // A negative coordinate cast to `usize` becomes an enormous positive
        // number that happens to fail the bounds test, which is luck rather
        // than logic -- and with overflow-checks on, a later addition to it
        // would panic. Rejecting it here makes the rest of the function honest.
        let Some((start, len)) = self.span_range(x, y, count as usize) else {
            return;
        };
        let Some(row) = self.buf.get_mut(start..start + len * bpp) else {
            return;
        };
        // One pass over the row rather than a bounds check per pixel: this is
        // the hot path for every solid background the renderer draws.
        for px in row.chunks_exact_mut(bpp) {
            px[..sig].copy_from_slice(&packed[..sig]);
        }
    }

    fn blit_span(&mut self, x: i32, y: i32, src: &[Color]) {
        let bpp = self.format.bytes_per_pixel();
        let format = self.format;
        let Some((start, len)) = self.span_range(x, y, src.len()) else {
            return;
        };
        let Some(row) = self.buf.get_mut(start..start + len * bpp) else {
            return;
        };
        for (px, color) in row.chunks_exact_mut(bpp).zip(src) {
            let (packed, sig) = color.pack(format);
            px[..sig].copy_from_slice(&packed[..sig]);
        }
    }

    fn row_mut(&mut self, y: i32) -> Option<&mut [u8]> {
        let y = usize::try_from(y).ok()?;
        if y >= self.size.h as usize {
            return None;
        }
        let stride = self.stride();
        let start = y * stride;
        let end = start + stride;
        self.buf.get_mut(start..end)
    }

    fn present(&mut self, _damage: Option<Rect>) {
        // No-op: this is a plain memory buffer with no display backend to flip.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;
    use crate::{Rect, Size};

    fn surf(w: u32, h: u32) -> MemorySurface {
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888)
    }

    #[test]
    fn it_advertises_what_a_plain_buffer_can_actually_do() {
        // `frame` reads RETAINS_CONTENT to decide whether repainting only the
        // damage is even correct, so getting this wrong here would show up as
        // fragments of an older frame rather than as a failing assertion.
        let c = surf(1, 1).caps();
        assert!(c.contains(Caps::READ_BACK | Caps::RETAINS_CONTENT));
        assert!(!c.contains(Caps::ACCELERATED));
    }

    #[test]
    fn a_new_surface_is_zeroed_and_correctly_sized() {
        let s = surf(4, 3);
        assert_eq!(s.stride(), 16);
        assert_eq!(s.pixels().len(), 48);
        assert!(s.pixels().iter().all(|b| *b == 0));
    }

    #[test]
    fn a_fill_lands_in_bgrx_order() {
        let mut s = surf(2, 1);
        s.fill_span(0, 0, 2, Color::rgb(1, 2, 3));
        // B, G, R, X -- red and blue swapped relative to the constructor.
        assert_eq!(&s.pixels()[..4], &[3, 2, 1, 255]);
        assert_eq!(&s.pixels()[4..8], &[3, 2, 1, 255]);
    }

    #[test]
    fn a_span_is_clamped_to_the_row_it_starts_on() {
        // Running off the right edge must not wrap onto the next row, which is
        // the classic stride bug and looks like a diagonal smear.
        let mut s = surf(2, 2);
        s.fill_span(1, 0, 99, Color::WHITE);
        assert_eq!(&s.pixels()[4..8], &[255, 255, 255, 255]);
        assert_eq!(&s.pixels()[8..16], &[0; 8], "row 1 was touched");
    }

    #[test]
    fn negative_coordinates_are_ignored_not_wrapped() {
        // `-1 as usize` is enormous; relying on that to fail a bounds check is
        // luck, and under overflow-checks the next addition would panic.
        let mut s = surf(4, 4);
        s.fill_span(-1, 0, 4, Color::WHITE);
        s.fill_span(0, -1, 4, Color::WHITE);
        s.blit_span(-5, 0, &[Color::WHITE; 4]);
        assert!(s.pixels().iter().all(|b| *b == 0));
        assert!(s.row_mut(-1).is_none());
    }

    #[test]
    fn an_out_of_range_row_yields_nothing() {
        let mut s = surf(4, 4);
        assert!(s.row_mut(4).is_none());
        assert!(s.row_mut(0).is_some());
    }

    #[test]
    fn a_blit_writes_each_source_pixel_once() {
        let mut s = surf(3, 1);
        s.blit_span(0, 0, &[Color::rgb(9, 0, 0), Color::rgb(0, 9, 0)]);
        assert_eq!(&s.pixels()[..4], &[0, 0, 9, 255]);
        assert_eq!(&s.pixels()[4..8], &[0, 9, 0, 255]);
        assert_eq!(&s.pixels()[8..12], &[0, 0, 0, 0], "third pixel untouched");
    }

    #[test]
    fn rgb565_writes_two_bytes_per_pixel() {
        let mut s = MemorySurface::new(Size { w: 2, h: 1 }, PixelFormat::Rgb565);
        assert_eq!(s.stride(), 4);
        s.fill_span(0, 0, 2, Color::WHITE);
        assert_eq!(s.pixels(), &[0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn the_rasteriser_draws_through_it_end_to_end() {
        // The integration that matters: clip, span, pack, store.
        let mut s = surf(4, 4);
        render::fill_rect(&mut s, Rect::new(-1, -1, 3, 3), Color::WHITE);
        // A 2x2 block at the origin survives the clip.
        assert_eq!(&s.pixels()[0..8], &[255; 8]);
        assert_eq!(&s.pixels()[8..16], &[0; 8], "column 2 should be clear");
        assert_eq!(&s.pixels()[32..40], &[0; 8], "row 2 should be clear");
    }
}
