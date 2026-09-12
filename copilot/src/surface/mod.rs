// SPDX-License-Identifier: GPL-3.0-only
//! The boundary between the toolkit and whatever actually owns the pixels.
//!
//! Everything the renderer draws goes through [`Surface`]. Implementing it is
//! the entire integration story: a VideoCore framebuffer, a DRM/KMS dumb
//! buffer, an SPI display's line buffer, a window in the simulator and a GPU
//! texture uploaded by someone else's driver are all just implementations.
//!
//! # Required and optional
//!
//! [`Surface::size`], [`Surface::format`], [`Surface::fill_span`],
//! [`Surface::blit_span`] and [`Surface::present`] are the whole of a working
//! backend. Every other method has a default, and the `draw_*` hooks default
//! to declining, so a driver accelerates the cases its hardware has and the
//! software rasteriser draws the rest.
//!
//! The call order is [`Surface::upload`] when a scene loads, then per frame
//! [`Surface::begin_frame`], the drawing methods, [`Surface::present`].
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
//!
//! # Writing a backend
//!
//! `docs/backends.md` is the long form.
//!
//! ```
//! use copilot::{Caps, Color, PixelFormat, Rect, Size, Surface};
//!
//! struct Blitter;
//!
//! impl Surface for Blitter {
//!     fn size(&self) -> Size { Size { w: 480, h: 480 } }
//!     fn format(&self) -> PixelFormat { PixelFormat::Rgb565 }
//!     fn fill_span(&mut self, _x: i32, _y: i32, _n: u32, _c: Color) { /* one row */ }
//!     fn blit_span(&mut self, _x: i32, _y: i32, _src: &[Color]) { /* one row */ }
//!     fn present(&mut self, _damage: Option<Rect>) { /* flip */ }
//!
//!     // Everything below here is optional.
//!     fn caps(&self) -> Caps { Caps::ACCELERATED | Caps::RETAINS_CONTENT }
//!     fn draw_rect(&mut self, rect: Rect, _color: Color) -> bool {
//!         // Hardware that only fills narrow rectangles declines the rest,
//!         // and the renderer emits spans for them instead.
//!         rect.size.w <= 2048
//!     }
//! }
//! ```

use crate::render::Damage;
use crate::{Color, Rect, Size};

pub mod caps;
pub mod primitive;
pub mod texture;

pub use caps::Caps;
pub use primitive::Primitive;
pub use texture::TextureId;

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
/// implementation may treat every span it receives as in bounds, which is what
/// keeps the inner loops free of bounds checks without any `unsafe`.
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

    /// What this backend can do beyond the four methods above.
    ///
    /// The default, [`Caps::NONE`], is the conservative answer to all of them.
    fn caps(&self) -> Caps {
        Caps::NONE
    }

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

    /// Open a frame that is about to repaint `damage`, paired with
    /// [`Self::present`].
    ///
    /// Wait for the previous flip here rather than in `present`, so the
    /// renderer composes during the gap instead of blocking after it.
    fn begin_frame(&mut self, _damage: &Damage) {}

    /// Draw a solid rectangle, returning whether the backend did.
    ///
    /// `rect` is already clipped to the damage rectangle and the surface.
    /// `false`, the default, sends it to the software rasteriser.
    fn draw_rect(&mut self, _rect: Rect, _color: Color) -> bool {
        false
    }

    /// Draw a `src_w` x `src_h` image scaled to `dst`, painting only `area`.
    ///
    /// `dst` fixes the scale and `area`, already clipped, bounds the painting.
    /// Sampling is nearest-neighbour; `src` is row-major and may be short.
    fn draw_image(
        &mut self,
        _dst: Rect,
        _area: Rect,
        _src: &[Color],
        _src_w: u32,
        _src_h: u32,
    ) -> bool {
        false
    }

    /// Fill `dst` with a ramp from `from` to `to`, painting only `area`.
    ///
    /// `vertical` runs it top to bottom, otherwise left to right. The ramp
    /// spans `dst`, so a partial repaint produces the same pixels as a full one.
    fn draw_gradient(
        &mut self,
        _dst: Rect,
        _area: Rect,
        _from: Color,
        _to: Color,
        _vertical: bool,
    ) -> bool {
        false
    }

    /// Draw a curved or diagonal shape, returning whether the backend did.
    ///
    /// `area` is already clipped; the shape's own coordinates are not. Decline
    /// when `antialias` is set and the hardware cannot blend, so that the
    /// software path draws what the scene asked for.
    fn draw_primitive(
        &mut self,
        _prim: Primitive,
        _area: Rect,
        _color: Color,
        _antialias: bool,
    ) -> bool {
        false
    }

    /// Offer `id`'s pixels for the backend to keep, returning whether it did.
    ///
    /// Called when a scene loads, never during a frame. `true` promises that a
    /// later [`Self::draw_texture`] with this `id` draws these pixels.
    fn upload(&mut self, _id: TextureId, _pixels: &[Color], _width: u32, _height: u32) -> bool {
        false
    }

    /// Draw resident texture `id` scaled to `dst`, painting only `area`.
    ///
    /// The rectangles mean exactly what they do in [`Self::draw_image`]. A
    /// backend must answer `false` for an `id` it never took, and the renderer
    /// then falls back to scaling and blitting the pixels itself.
    fn draw_texture(&mut self, _id: TextureId, _dst: Rect, _area: Rect) -> bool {
        false
    }

    /// Draw into a second full-size buffer, then composite it over this
    /// surface at `alpha`, returning whether the backend did.
    ///
    /// Only [`crate::render::transition()`]'s cross-fade uses this, and only a
    /// backend claiming [`Caps::SCRATCH`] is asked. A board with no memory for
    /// a second buffer leaves both alone and gets a cut instead of a fade,
    /// which is the whole of what it costs to not implement this.
    ///
    /// One method rather than a borrow and a blend, so that a backend cannot
    /// implement half of the pair.
    fn with_scratch(&mut self, _alpha: u8, _draw: &mut dyn FnMut(&mut dyn Surface)) -> bool {
        false
    }

    /// Drop any memory held for `id`.
    ///
    /// Called when a scene is replaced. An `id` that was never taken is not an
    /// error to release; the default does nothing at all.
    fn release(&mut self, _id: TextureId) {}

    /// Announce that `damage` has been redrawn and should reach the display.
    ///
    /// A page-flipping backend flips here; a streaming panel pushes the dirty
    /// window; a windowed simulator presents. Called once per frame after all
    /// drawing, with the union of every dirty rectangle, or `None` when
    /// nothing changed — a backend that must present unconditionally (a
    /// double-buffered flip, say) has to notice that case itself.
    ///
    /// This closes the frame [`Self::begin_frame`] opened.
    fn present(&mut self, damage: Option<Rect>);
}
