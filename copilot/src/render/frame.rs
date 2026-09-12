// SPDX-License-Identifier: GPL-3.0-only
//! The backend's lifecycle: what happens when a scene loads, and each frame.
//!
//! Whether repainting only the damage is correct depends on whether the
//! buffer still holds what the last frame drew, and only the backend knows.
//! [`frame()`] asks, via [`Caps::RETAINS_CONTENT`], and picks between
//! [`compose()`] and [`compose_all()`] accordingly.

use crate::surface::TextureId;
use crate::widget::Tree;
use crate::{Caps, Rect, Surface};

use super::{Resources, compose, compose_all};

/// Draw and present one frame, returning whether anything was drawn.
///
/// The whole of a render loop, for a caller that has already advanced its
/// animations and applied whatever readings arrived:
///
/// ```no_run
/// # use copilot::render::{Resources, frame};
/// # use copilot::{Surface, widget::Tree};
/// # fn go<S: Surface>(surface: &mut S, tree: &mut Tree, res: Resources<'_>) {
/// loop {
///     // ... tick animations, apply gauge readings ...
///     frame(surface, tree, res);
/// }
/// # }
/// ```
///
/// A backend that retains its content gets a damage-only repaint, and no
/// present at all when nothing moved; one that does not gets a full repaint
/// every frame, because nothing else would be correct.
pub fn frame<S: Surface + ?Sized>(surface: &mut S, tree: &mut Tree, res: Resources<'_>) -> bool {
    let retains = surface.caps().contains(Caps::RETAINS_CONTENT);

    // Nothing moved and the buffer already holds it, so there is no frame to
    // draw. A backend that must present regardless cannot claim
    // RETAINS_CONTENT, so it never reaches here.
    if retains && tree.damage().is_empty() {
        return false;
    }

    surface.begin_frame(tree.damage());

    let painted = if retains {
        let bounds = tree.damage().bounds();
        compose(surface, tree, tree.damage(), res);
        bounds
    } else {
        let size = surface.size();
        compose_all(surface, tree, res);
        Some(Rect::new(0, 0, size.w, size.h))
    };

    // Cleared before the present, not after: `present` may block, and damage
    // arriving while it does belongs to the next frame.
    tree.clear_damage();
    surface.present(painted);
    true
}

/// Offer every image and animation frame in `res` to the backend to keep.
///
/// Returns how many it took; nothing at all is the normal answer for a
/// software surface. Call it once when a scene loads, never inside a frame.
pub fn upload_assets<S: Surface + ?Sized>(surface: &mut S, res: Resources<'_>) -> u32 {
    let mut taken = 0;
    for index in 0..res.images.len() as u32 {
        let Some(img) = res.images.get(index) else {
            continue;
        };
        if surface.upload(
            TextureId::Image { index },
            &img.pixels,
            img.width,
            img.height,
        ) {
            taken += 1;
        }
    }
    for anim in 0..res.anims.len() as u32 {
        let Some(a) = res.anims.get(anim) else {
            continue;
        };
        for (frame, f) in a.frames.iter().enumerate() {
            let id = TextureId::AnimFrame {
                anim,
                frame: frame as u32,
            };
            if surface.upload(id, &f.pixels, u32::from(a.width), u32::from(a.height)) {
                taken += 1;
            }
        }
    }
    taken
}

/// Release everything [`upload_assets`] offered for `res`.
///
/// Releasing an id the backend never took is not an error.
pub fn release_assets<S: Surface + ?Sized>(surface: &mut S, res: Resources<'_>) {
    for index in 0..res.images.len() as u32 {
        surface.release(TextureId::Image { index });
    }
    for anim in 0..res.anims.len() as u32 {
        let Some(a) = res.anims.get(anim) else {
            continue;
        };
        for frame in 0..a.frames.len() as u32 {
            surface.release(TextureId::AnimFrame { anim, frame });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AnimTable, Image, ImageTable};
    use crate::font::{Font, default_font};
    use crate::widget::{Kind, Node, ROOT};
    use crate::{Color, PixelFormat, Size};
    use alloc::vec::Vec;

    /// A surface that records the lifecycle rather than the pixels: what this
    /// module decides is a sequence of calls, and asserting on pixels would
    /// only let it be inferred.
    struct Backend {
        caps: Caps,
        begun: Vec<usize>,
        presented: Vec<Option<Rect>>,
        spans: usize,
        uploaded: Vec<TextureId>,
        released: Vec<TextureId>,
        /// Ids the backend pretends it had room for.
        accept: bool,
    }

    impl Backend {
        fn new(caps: Caps, accept: bool) -> Self {
            Self {
                caps,
                begun: Vec::new(),
                presented: Vec::new(),
                spans: 0,
                uploaded: Vec::new(),
                released: Vec::new(),
                accept,
            }
        }
    }

    impl Surface for Backend {
        fn size(&self) -> Size {
            Size { w: 20, h: 10 }
        }
        fn format(&self) -> PixelFormat {
            PixelFormat::Bgrx8888
        }
        fn caps(&self) -> Caps {
            self.caps
        }
        fn fill_span(&mut self, _x: i32, _y: i32, _n: u32, _c: Color) {
            self.spans += 1;
        }
        fn blit_span(&mut self, _x: i32, _y: i32, _src: &[Color]) {
            self.spans += 1;
        }
        fn begin_frame(&mut self, damage: &crate::Damage) {
            self.begun.push(damage.rects().len());
        }
        fn upload(&mut self, id: TextureId, _px: &[Color], _w: u32, _h: u32) -> bool {
            if self.accept {
                self.uploaded.push(id);
            }
            self.accept
        }
        fn release(&mut self, id: TextureId) {
            self.released.push(id);
        }
        fn present(&mut self, damage: Option<Rect>) {
            self.presented.push(damage);
        }
    }

    fn font() -> Font<'static> {
        default_font()
    }

    fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable, font: &'a Font<'a>) -> Resources<'a> {
        Resources {
            images,
            anims,
            font,
            menus: &[],
        }
    }

    /// A tree with one 4x4 white panel, and the panel's id so a test can
    /// dirty exactly it.
    fn tree() -> (Tree, crate::widget::NodeId) {
        let mut t = Tree::new(Rect::new(0, 0, 20, 10));
        let id = t
            .push(
                ROOT,
                Node {
                    rect: Rect::new(0, 0, 4, 4),
                    kind: Kind::Panel {
                        background: Color::WHITE,
                    },
                    visible: true,
                    antialias: None,
                    name: None,
                    children: Vec::new(),
                    parent: None,
                },
            )
            .unwrap();
        (t, id)
    }

    fn white() -> Kind {
        Kind::Panel {
            background: Color::WHITE,
        }
    }

    #[test]
    fn a_retaining_backend_paints_only_the_damage_and_presents_its_bounds() {
        let mut s = Backend::new(Caps::RETAINS_CONTENT, false);
        let (mut t, panel) = tree();
        t.clear_damage();
        t.set_kind(panel, white()).unwrap();

        assert!(frame(
            &mut s,
            &mut t,
            res(&ImageTable::new(), &AnimTable::new(), &font())
        ));
        assert_eq!(s.begun, alloc::vec![1], "one damage rectangle was open");
        assert_eq!(s.presented, alloc::vec![Some(Rect::new(0, 0, 4, 4))]);
        assert_eq!(s.spans, 4, "four rows of a 4x4 panel, not the whole screen");
        assert!(t.damage().is_empty(), "damage must be consumed");
    }

    #[test]
    fn a_retaining_backend_with_nothing_to_do_does_not_present() {
        // The cheapest frame is the one that does not happen; a present here
        // would be a flip the display never needed.
        let mut s = Backend::new(Caps::RETAINS_CONTENT, false);
        let (mut t, _) = tree();
        t.clear_damage();
        assert!(!frame(
            &mut s,
            &mut t,
            res(&ImageTable::new(), &AnimTable::new(), &font())
        ));
        assert!(s.begun.is_empty() && s.presented.is_empty());
    }

    #[test]
    fn a_backend_that_keeps_nothing_repaints_everything_every_frame() {
        // The buffer handed back may be two frames old, so damage describes
        // this frame's changes and not that buffer's staleness.
        let mut s = Backend::new(Caps::NONE, false);
        let (mut t, _) = tree();
        t.clear_damage();
        assert!(frame(
            &mut s,
            &mut t,
            res(&ImageTable::new(), &AnimTable::new(), &font())
        ));
        assert_eq!(s.presented, alloc::vec![Some(Rect::new(0, 0, 20, 10))]);
        assert!(
            s.spans > 0,
            "an empty damage set must not stop a non-retaining backend repainting"
        );
    }

    #[test]
    fn every_image_and_frame_is_offered_once_and_released_once() {
        let mut s = Backend::new(Caps::NONE, true);
        let mut images = ImageTable::new();
        images.push(Image::new(2, 2, alloc::vec![Color::WHITE; 4]).unwrap());
        images.push(Image::new(1, 1, alloc::vec![Color::WHITE]).unwrap());
        let anims = AnimTable::new();

        let took = upload_assets(&mut s, res(&images, &anims, &font()));
        assert_eq!(took, 2);
        assert_eq!(
            s.uploaded,
            alloc::vec![TextureId::Image { index: 0 }, TextureId::Image { index: 1 }]
        );

        release_assets(&mut s, res(&images, &anims, &font()));
        assert_eq!(s.released, s.uploaded);
    }

    #[test]
    fn a_backend_that_takes_nothing_is_not_a_failure() {
        let mut s = Backend::new(Caps::NONE, false);
        let mut images = ImageTable::new();
        images.push(Image::new(1, 1, alloc::vec![Color::WHITE]).unwrap());
        assert_eq!(
            upload_assets(&mut s, res(&images, &AnimTable::new(), &font())),
            0
        );
        // And releasing an id it never took must still be safe to do.
        release_assets(&mut s, res(&images, &AnimTable::new(), &font()));
        assert_eq!(s.released.len(), 1);
    }
}
