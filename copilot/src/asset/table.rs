// SPDX-License-Identifier: MIT OR Apache-2.0
//! The images a scene refers to, and how they get there.
//!
//! Rule 1.4 says the core crate cannot open a file, so a scene cannot load its
//! own assets. The split is this: a scene file lists the images it wants by
//! path, [`crate::scene::build_scene`] hands that list back as
//! [`Scene::requests`], the host reads whichever bytes it can and decodes them
//! into an [`ImageTable`], and widgets refer to entries by index.
//!
//! # Why widgets hold an index and not the pixels
//!
//! Two widgets showing the same background share one decode, a [`Kind`] stays
//! cheap to clone, and — the reason that actually matters on a Pi — the table
//! can be built once at start-up into memory that is never reallocated, so no
//! frame ever allocates.
//!
//! [`Kind`]: crate::widget::Kind

use alloc::string::String;
use alloc::vec::Vec;

use crate::Color;

/// A decoded image, ready to blit.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// Width in pixels. `pixels.len()` is always `width * height`.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA pixels from the top left.
    pub pixels: Vec<Color>,
}

impl Image {
    /// Build an image, checking that the pixel count matches the dimensions.
    ///
    /// Returns `None` on a mismatch rather than trusting the caller: the
    /// pixels usually come from a decoder fed an untrusted file, and every
    /// blit below assumes this invariant holds.
    #[must_use]
    pub fn new(width: u32, height: u32, pixels: Vec<Color>) -> Option<Self> {
        let want = (width as usize).checked_mul(height as usize)?;
        if want == 0 || pixels.len() != want {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    /// A placeholder for an image the host could not supply.
    ///
    /// A missing asset draws a visible magenta square rather than nothing at
    /// all. Nothing is indistinguishable from a widget that is working
    /// correctly and simply transparent, and a scene author debugging a wrong
    /// path deserves to see which widget is broken.
    #[must_use]
    pub fn missing() -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: alloc::vec![Color::rgb(255, 0, 255)],
        }
    }
}

/// Every image a scene can draw, indexed by the number its widgets use.
#[derive(Clone, Debug, Default)]
pub struct ImageTable {
    images: Vec<Image>,
}

impl ImageTable {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self { images: Vec::new() }
    }

    /// Append an image, returning the index widgets should use for it.
    pub fn push(&mut self, image: Image) -> u32 {
        let id = self.images.len() as u32;
        self.images.push(image);
        id
    }

    /// Append a placeholder, so indices still line up with the scene's list.
    ///
    /// A host that fails to load the second of three images must still call
    /// this, or the third image silently becomes the second and every widget
    /// after it draws the wrong picture.
    pub fn push_missing(&mut self) -> u32 {
        self.push(Image::missing())
    }

    /// Look up an image, or `None` if the index is out of range.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&Image> {
        self.images.get(index as usize)
    }

    /// How many images the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether the table holds no images.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// Every decoded animation a scene can play, indexed as its widgets refer to it.
#[derive(Clone, Debug, Default)]
pub struct AnimTable {
    anims: Vec<crate::asset::gif::Animation>,
}

impl AnimTable {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self { anims: Vec::new() }
    }

    /// Append an animation, returning the index widgets should use.
    pub fn push(&mut self, anim: crate::asset::gif::Animation) -> u32 {
        let id = self.anims.len() as u32;
        self.anims.push(anim);
        id
    }

    /// Append a one-frame placeholder, so indices stay aligned when a load fails.
    ///
    /// Same reasoning as [`ImageTable::push_missing`]: skipping a failure
    /// shifts every later index and widgets play the wrong animation.
    pub fn push_missing(&mut self) -> u32 {
        self.push(crate::asset::gif::Animation {
            width: 1,
            height: 1,
            frames: alloc::vec![crate::asset::gif::Frame {
                pixels: alloc::vec![Color::rgb(255, 0, 255)],
                delay_us: 100_000,
            }],
        })
    }

    /// Look up an animation.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&crate::asset::gif::Animation> {
        self.anims.get(index as usize)
    }

    /// How many animations the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.anims.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anims.is_empty()
    }
}

/// A built scene: the widget tree, plus the assets it still needs.
#[derive(Clone, Debug)]
pub struct Scene {
    /// The widget tree, ready to compose.
    pub tree: crate::widget::Tree,
    /// Animations the scene declared, ready to be ticked.
    pub anims: crate::anim::Animator,
    /// Animation paths the scene asked for, in the order widgets index them.
    pub anim_requests: Vec<String>,
    /// Image paths the scene asked for, in the order widgets index them.
    ///
    /// These are opaque strings, not validated paths — the core crate has no
    /// idea what a path looks like on the host, and deliberately does not.
    pub requests: Vec<String>,
    /// Widgets bound to gauges, in document order. Already resolved against
    /// the gauge table, so a display can subscribe from [`Scene::wanted`] and
    /// feed readings in with [`Scene::apply_gauges`] without a lookup.
    ///
    /// [`Scene::wanted`]: crate::asset::Scene::wanted
    /// [`Scene::apply_gauges`]: crate::asset::Scene::apply_gauges
    pub bindings: Vec<crate::scene::Binding>,
    /// The CAN node address of the display this scene is for, if it says.
    /// `0x00` and `0xFF` never appear here: the bus reserves them.
    pub node: Option<u8>,
    /// The outline of the panel: rectangular, or round like a 480x480 gauge.
    pub shape: crate::scene::Shape,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_must_match_its_dimensions() {
        assert!(Image::new(2, 2, alloc::vec![Color::WHITE; 4]).is_some());
        assert!(Image::new(2, 2, alloc::vec![Color::WHITE; 3]).is_none());
        assert!(Image::new(2, 2, alloc::vec![Color::WHITE; 5]).is_none());
    }

    #[test]
    fn a_zero_dimension_image_is_rejected() {
        assert!(Image::new(0, 4, alloc::vec![]).is_none());
        assert!(Image::new(4, 0, alloc::vec![]).is_none());
    }

    #[test]
    fn an_absurd_size_does_not_overflow() {
        assert!(Image::new(u32::MAX, u32::MAX, alloc::vec![]).is_none());
    }

    #[test]
    fn indices_are_handed_out_in_order() {
        let mut t = ImageTable::new();
        let a = t.push(Image::new(1, 1, alloc::vec![Color::WHITE]).unwrap());
        let b = t.push(Image::new(1, 1, alloc::vec![Color::BLACK]).unwrap());
        assert_eq!((a, b), (0, 1));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn a_placeholder_keeps_later_indices_aligned() {
        // The bug this prevents: skipping a failed load shifts every
        // subsequent image, so widgets quietly draw the wrong picture.
        let mut t = ImageTable::new();
        t.push_missing();
        let second = t.push(Image::new(1, 1, alloc::vec![Color::WHITE]).unwrap());
        assert_eq!(second, 1);
        assert_eq!(t.get(0), Some(&Image::missing()));
    }

    #[test]
    fn an_out_of_range_index_yields_nothing() {
        let t = ImageTable::new();
        assert!(t.get(0).is_none());
        assert!(t.get(u32::MAX).is_none());
    }
}
