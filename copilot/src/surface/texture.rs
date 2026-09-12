// SPDX-License-Identifier: GPL-3.0-only
//! Naming the pixels a backend is allowed to keep.
//!
//! A GPU cannot re-upload a gauge face every frame, so the upload happens once
//! and the draw names the result.
//!
//! The name is derived from the scene rather than allocated by the driver: a
//! driver-allocated handle would have to live in the scene tables, which would
//! make [`crate::asset`] depend on whichever backend is linked.

/// Which of a scene's pixels a backend has been asked to hold or draw.
///
/// `#[non_exhaustive]` so that a kind added later is not a breaking change.
/// `Ord` because `core` has no hash map and a bare-metal driver will keep a
/// sorted array.
///
/// # A driver holding textures
///
/// ```
/// use copilot::{Color, PixelFormat, Rect, Size, Surface, TextureId};
///
/// struct Resident { id: TextureId, offset: usize }
///
/// struct Gpu { vram: Vec<Color>, resident: Vec<Resident> }
///
/// impl Surface for Gpu {
///     # fn size(&self) -> Size { Size { w: 480, h: 480 } }
///     # fn format(&self) -> PixelFormat { PixelFormat::Rgb565 }
///     # fn fill_span(&mut self, _x: i32, _y: i32, _n: u32, _c: Color) {}
///     # fn blit_span(&mut self, _x: i32, _y: i32, _s: &[Color]) {}
///     # fn present(&mut self, _damage: Option<Rect>) {}
///     // ... the five required methods ...
///
///     fn upload(&mut self, id: TextureId, pixels: &[Color], _w: u32, _h: u32) -> bool {
///         if self.vram.len() + pixels.len() > 1 << 20 {
///             return false;               // no room is not a failure
///         }
///         let offset = self.vram.len();
///         self.vram.extend_from_slice(pixels);
///         self.resident.push(Resident { id, offset });
///         self.resident.sort_unstable_by_key(|r| r.id);
///         true
///     }
///
///     fn draw_texture(&mut self, id: TextureId, dst: Rect, area: Rect) -> bool {
///         let Ok(i) = self.resident.binary_search_by_key(&id, |r| r.id) else {
///             return false;               // never took it; the rasteriser blits
///         };
///         let _ = (self.resident[i].offset, dst, area);
///         true                            // ... queue a textured quad ...
///     }
///
///     fn release(&mut self, id: TextureId) {
///         self.resident.retain(|r| r.id != id);
///     }
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TextureId {
    /// Entry `index` of the scene's [`ImageTable`](crate::asset::ImageTable).
    Image {
        /// The index a widget's `Kind::Image` refers to.
        index: u32,
    },
    /// One frame of an entry in the scene's [`AnimTable`](crate::asset::AnimTable).
    ///
    /// Per frame rather than per animation, because that is the unit a draw names.
    AnimFrame {
        /// The index a widget's `Kind::Anim` refers to.
        anim: u32,
        /// Which frame of it, counting from zero.
        frame: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn ids_of_different_things_are_different() {
        // A backend keys its uploads on this; a collision draws the wrong
        // picture, which is the hardest kind of bug to see in a car.
        let ids = [
            TextureId::Image { index: 0 },
            TextureId::Image { index: 1 },
            TextureId::AnimFrame { anim: 0, frame: 0 },
            TextureId::AnimFrame { anim: 0, frame: 1 },
            TextureId::AnimFrame { anim: 1, frame: 0 },
        ];
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn an_id_is_ordered_so_a_backend_can_hold_them_in_a_sorted_array() {
        // No hash map in `core`; a backend on bare metal will binary-search a
        // fixed array, which needs a total order.
        let mut ids = alloc::vec![
            TextureId::AnimFrame { anim: 1, frame: 0 },
            TextureId::Image { index: 2 },
            TextureId::AnimFrame { anim: 0, frame: 9 },
        ];
        ids.sort();
        let found = ids.binary_search(&TextureId::Image { index: 2 });
        assert!(found.is_ok());
        assert!(
            ids.binary_search(&TextureId::Image { index: 3 }).is_err(),
            "an id that was never uploaded must not be found"
        );
        let _: Vec<TextureId> = ids;
    }
}
