// SPDX-License-Identifier: MIT OR Apache-2.0
//! Turning a scene into pixels.
//!
//! The renderer is deliberately thin: [`damage`] decides *what* to repaint and
//! the rasteriser decides *how*, but neither knows where the pixels end up —
//! that is [`crate::Surface`]'s job.

use crate::asset::{AnimTable, ImageTable};
use crate::font::Font;

/// Everything drawing needs besides the tree itself.
///
/// Bundled because the alternative is a signature that grows by one argument
/// every time the scene format learns a new kind of asset, and every call site
/// in the crate changing with it.
#[derive(Clone, Copy)]
pub struct Resources<'a> {
    /// Still images the scene referenced.
    pub images: &'a ImageTable,
    /// Animations the scene referenced.
    pub anims: &'a AnimTable,
    /// The font labels are drawn in.
    pub font: &'a Font<'a>,
}

pub mod aa;
pub mod compose;
pub mod damage;
pub mod draw;
pub mod gauge;
pub mod gradient;
pub mod grid;
pub mod plot;
pub mod raster;
pub mod scale;
pub mod segbar;
pub mod segment;
pub mod shape;

pub use compose::{compose, compose_all};
pub use damage::Damage;
pub use draw::draw_kind;
pub use gauge::{needle, ruler, scale};
pub use gradient::gradient;
pub use grid::grid;
pub use plot::{chart, polygon, polyline};
pub use raster::{blit, clip, fill_rect, stroke_rect};
pub use scale::scale_nearest;
pub use segbar::seg_bar;
pub use segment::seven_seg;
pub use shape::{circle, disc, line};
