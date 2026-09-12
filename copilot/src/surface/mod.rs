// SPDX-License-Identifier: MIT OR Apache-2.0
//! The boundary between the toolkit and whatever actually owns the pixels.
//!
//! Everything the renderer draws goes through [`Surface`]. Implementing it is
//! the entire integration story: a VideoCore framebuffer, a DRM/KMS dumb
//! buffer, an SPI display's line buffer, a window in the simulator and a GPU
//! texture uploaded by someone else's driver are all just implementations.
//!
//! # Why the trait hands out rows rather than a whole buffer
//!
//! A single `&mut [u32]` over the framebuffer would be the obvious signature,
//! and it is wrong for two common targets. A display behind SPI has no memory
//! to lend — pixels are streamed, and the driver wants them a span at a time.
//! A double-buffered framebuffer may hand back a *different* buffer each flip,
//! so a slice cached across frames is a dangling pointer waiting to happen.
//! [`Surface::fill_span`] and [`Surface::blit_span`] fit both, and a
//! memory-mapped implementation collapses them to a `copy_from_slice`.
//!
//! Implementations that genuinely are flat memory can override
//! [`Surface::row_mut`] to hand the rasteriser the row directly and skip the
//! per-span dispatch.

use crate::{Color, Rect, Size};

/// How a backend expects a pixel to be laid out in memory.
///
/// The renderer composites in [`Color`] and converts once, at the surface
/// boundary, so no drawing code anywhere else in the crate needs to know which
/// of these it is talking to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum PixelFormat {
    /// 32 bits per pixel, byte order B, G, R, X. What the VideoCore hands back
    /// on a Raspberry Pi in the mode digidash asks for.
    Bgrx8888,
    /// 32 bits per pixel, byte order R, G, B, X.
    Rgbx8888,
    /// 16 bits per pixel, 5 red, 6 green, 5 blue, packed little-endian. The
    /// common format for small SPI panels.
    Rgb565,
}

impl PixelFormat {
    /// Bytes one pixel occupies in this format.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Bgrx8888 | Self::Rgbx8888 => 4,
            Self::Rgb565 => 2,
        }
    }
}

/// A destination the renderer can draw into.
///
/// Coordinates passed to these methods are always already clipped to
/// [`Surface::size`] — the renderer clips once, at the top, so an
/// implementation may treat every span it receives as in bounds. That is the
/// contract rule 4.4 exists to protect, and it is what keeps the inner loops
/// free of bounds checks without any `unsafe`.
pub trait Surface {
    /// Dimensions of the drawable area, in pixels.
    fn size(&self) -> Size;

    /// The pixel layout this surface expects.
    fn format(&self) -> PixelFormat;

    /// Fill `count` pixels starting at (`x`, `y`) with a single colour.
    ///
    /// This is the hot path for backgrounds and solid widgets, which is why it
    /// is a span rather than a per-pixel call.
    fn fill_span(&mut self, x: i32, y: i32, count: u32, color: Color);

    /// Copy `src.len()` pixels starting at (`x`, `y`).
    ///
    /// Used for images and for any composited content the renderer has already
    /// blended in [`Color`] space.
    fn blit_span(&mut self, x: i32, y: i32, src: &[Color]);

    /// Borrow row `y` as raw pixels, if this surface is flat memory.
    ///
    /// The default returns `None`, which is always correct — it simply means
    /// the renderer will use the span methods. Overriding it is a pure
    /// optimisation for memory-mapped framebuffers.
    fn row_mut(&mut self, _y: i32) -> Option<&mut [u8]> {
        None
    }

    /// Composite `src` over the pixels starting at (`x`, `y`).
    ///
    /// What an antialiased edge needs and a solid fill does not: the pixel
    /// underneath. The default reads the row back through [`Self::row_mut`],
    /// blends in [`Color`] space and writes it again. A surface that cannot
    /// be read back gets the pixels that are at least half covered written
    /// opaque and the rest left alone -- the aliased picture, which is what
    /// it drew before antialiasing existed, rather than a wrong one.
    fn blend_span(&mut self, x: i32, y: i32, src: &[Color]) {
        let format = self.format();
        let bpp = format.bytes_per_pixel();
        let Ok(start) = usize::try_from(x) else {
            return;
        };
        if let Some(row) = self.row_mut(y) {
            let px = row.chunks_exact_mut(bpp).skip(start);
            for (cell, color) in px.zip(src) {
                let under = Color::unpack(cell, format);
                let (packed, sig) = color.over(under).pack(format);
                cell[..sig].copy_from_slice(&packed[..sig]);
            }
            return;
        }
        for (i, color) in src.iter().enumerate() {
            if color.a >= 128 {
                let solid = Color::rgb(color.r, color.g, color.b);
                self.fill_span(x.saturating_add(i as i32), y, 1, solid);
            }
        }
    }

    /// Announce that `damage` has been redrawn and should reach the display.
    ///
    /// A page-flipping backend flips here; a streaming panel pushes the dirty
    /// window; a windowed simulator presents. Called once per frame after all
    /// drawing, with the union of every dirty rectangle, or `None` when
    /// nothing changed — a backend that must present unconditionally (a
    /// double-buffered flip, say) has to notice that case itself.
    fn present(&mut self, damage: Option<Rect>);
}
