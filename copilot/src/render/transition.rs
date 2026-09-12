// SPDX-License-Identifier: GPL-3.0-only
//! Moving from one scene to another over time.
//!
//! A mode change swaps whole scenes, and swapping them between one frame and
//! the next is abrupt. Every transition here is a clip and an offset over two
//! ordinary tree walks, so none of them needs a second buffer to composite in
//! — which is what makes them affordable on a target that has no spare
//! framebuffer to give.

use crate::widget::Tree;
use crate::{Caps, Rect, Surface};

use super::{Resources, compose_all, compose_moved};

/// Which edge the incoming scene arrives from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// From the left edge, moving right.
    Left,
    /// From the right edge, moving left.
    Right,
    /// From the top edge, moving down.
    Up,
    /// From the bottom edge, moving up.
    Down,
}

/// How one scene gives way to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Transition {
    /// Straight swap, with no intermediate frame.
    #[default]
    Cut,
    /// Both scenes move together, the new one pushing the old off.
    Slide(Edge),
    /// Neither scene moves; the new one is uncovered from `Edge` across.
    Wipe(Edge),
    /// The old scene fades out as the new one fades in.
    ///
    /// The only transition needing a second full-size buffer, because both
    /// scenes have to exist at once to be blended. A backend that does not
    /// claim [`Caps::SCRATCH`] gets a [`Cut`](Self::Cut) instead.
    CrossFade,
}

impl Transition {
    /// The name a rig file uses, as `"slide-left"` or `"cut"`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let (kind, edge) = match name.split_once('-') {
            Some((k, e)) => (k, Some(e)),
            None => (name, None),
        };
        let edge = match edge {
            Some("left") => Some(Edge::Left),
            Some("right") => Some(Edge::Right),
            Some("up") => Some(Edge::Up),
            Some("down") => Some(Edge::Down),
            Some(_) => return None,
            None => None,
        };
        match (kind, edge) {
            ("cut", None) => Some(Self::Cut),
            ("crossfade", None) => Some(Self::CrossFade),

            ("slide", Some(e)) => Some(Self::Slide(e)),
            ("wipe", Some(e)) => Some(Self::Wipe(e)),
            _ => None,
        }
    }
}

/// Draw the frame `t` of the way from `from` to `to`.
///
/// `t` is clamped to `0.0..=1.0`, and NaN is treated as zero. Both trees are
/// repainted in full, because during a transition everything on the screen is
/// moving and damage has nothing left to save.
///
/// The two are drawn to cover the surface between them, so a scene whose root
/// does not fill its display leaves whatever was underneath showing through,
/// exactly as it does in an ordinary frame.
///
/// Each tree brings its own [`Resources`], because two scenes have their own
/// image and animation tables and an index means nothing across them.
#[allow(clippy::too_many_arguments)] // Two trees, each with its own resources.
pub fn transition<S: Surface + ?Sized>(
    surface: &mut S,
    from: &Tree,
    from_res: Resources<'_>,
    to: &Tree,
    to_res: Resources<'_>,
    kind: Transition,
    t: f32,
) {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let size = surface.size();
    let (w, h) = (size.w as i32, size.h as i32);
    let full = Rect::new(0, 0, size.w, size.h);

    match kind {
        // A cut has no intermediate state; it is the old scene until it is
        // the new one.
        Transition::Cut => {
            if t >= 1.0 {
                compose_all(surface, to, to_res);
            } else {
                compose_all(surface, from, from_res);
            }
        }
        Transition::Slide(edge) => {
            let (dx, dy) = match edge {
                Edge::Left => (moved(w, t), 0),
                Edge::Right => (-moved(w, t), 0),
                Edge::Up => (0, moved(h, t)),
                Edge::Down => (0, -moved(h, t)),
            };
            // The outgoing scene is pushed by the full extent so the pair
            // always covers the surface with no seam between them.
            compose_moved(surface, from, from_res, dx, dy, full);
            let (bx, by) = match edge {
                Edge::Left => (dx - w, 0),
                Edge::Right => (dx + w, 0),
                Edge::Up => (0, dy - h),
                Edge::Down => (0, dy + h),
            };
            compose_moved(surface, to, to_res, bx, by, full);
        }
        Transition::CrossFade => {
            // A board with no second buffer cuts rather than fading, which is
            // the whole of what not implementing `with_scratch` costs.
            if !surface.caps().contains(Caps::SCRATCH) {
                if t >= 1.0 {
                    compose_all(surface, to, to_res);
                } else {
                    compose_all(surface, from, from_res);
                }
                return;
            }
            // The incoming scene goes to the front buffer and the outgoing one
            // is blended back over it, so the pair sums to one whole picture at
            // every `t`.
            compose_all(surface, to, to_res);
            let alpha = ((1.0 - t) * 255.0) as u8;
            let mut paint_from = |s: &mut dyn Surface| compose_all(s, from, from_res);
            if !surface.with_scratch(alpha, &mut paint_from) {
                // Claimed the capability and then declined: the incoming scene
                // is already drawn, which is a cut one frame early rather than
                // a half-blended frame.
            }
        }
        Transition::Wipe(edge) => {
            let (front, back) = wipe_bands(edge, w, h, t);
            if !back.is_empty() {
                compose_moved(surface, from, from_res, 0, 0, back);
            }
            if !front.is_empty() {
                compose_moved(surface, to, to_res, 0, 0, front);
            }
        }
    }
}

/// How far a scene has travelled across `extent` at `t`.
fn moved(extent: i32, t: f32) -> i32 {
    (extent as f32 * t) as i32
}

/// The band the new scene occupies, and the band the old one keeps.
fn wipe_bands(edge: Edge, w: i32, h: i32, t: f32) -> (Rect, Rect) {
    let across = moved(w, t);
    let down = moved(h, t);
    match edge {
        Edge::Left => (
            Rect::new(0, 0, across as u32, h as u32),
            Rect::new(across, 0, (w - across) as u32, h as u32),
        ),
        Edge::Right => (
            Rect::new(w - across, 0, across as u32, h as u32),
            Rect::new(0, 0, (w - across) as u32, h as u32),
        ),
        Edge::Up => (
            Rect::new(0, 0, w as u32, down as u32),
            Rect::new(0, down, w as u32, (h - down) as u32),
        ),
        Edge::Down => (
            Rect::new(0, h - down, w as u32, down as u32),
            Rect::new(0, 0, w as u32, (h - down) as u32),
        ),
    }
}

/// Fixtures shared by both test modules below.
#[cfg(test)]
mod tests_support {
    use super::*;
    use crate::asset::{AnimTable, ImageTable};
    use crate::font::default_font;
    use crate::widget::{Kind, Node, ROOT};
    use crate::{Color, MemorySurface};
    use alloc::vec::Vec;

    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);

    /// A tree that is one solid colour over `size`.
    pub fn solid(c: Color, w: u32, h: u32) -> Tree {
        let mut t = Tree::new(Rect::new(0, 0, w, h));
        t.push(
            ROOT,
            Node {
                rect: Rect::new(0, 0, w, h),
                kind: Kind::Panel { background: c },
                visible: true,
                antialias: None,
                name: None,
                children: Vec::new(),
                parent: None,
            },
        )
        .unwrap();
        t
    }

    /// Run a transition from red to blue into an existing surface.
    pub fn run_into(s: &mut MemorySurface, kind: Transition, t: f32) {
        let size = s.size();
        let font = default_font();
        let (images, anims) = (ImageTable::new(), AnimTable::new());
        let res = Resources {
            images: &images,
            anims: &anims,
            font: &font,
            menus: &[],
        };
        transition(
            s,
            &solid(RED, size.w, size.h),
            res,
            &solid(BLUE, size.w, size.h),
            res,
            kind,
            t,
        );
    }

    pub fn is_red_at(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        (s.pixels()[o + 2], s.pixels()[o + 1], s.pixels()[o]) == (255, 0, 0)
    }

    pub fn is_blue_at(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        (s.pixels()[o + 2], s.pixels()[o + 1], s.pixels()[o]) == (0, 0, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AnimTable, ImageTable};
    use crate::font::{Font, default_font};
    use crate::widget::{Kind, Node, ROOT};
    use crate::{Color, MemorySurface, PixelFormat, Size};
    use alloc::vec::Vec;

    const W: u32 = 20;
    const H: u32 = 10;

    /// A tree that is one solid colour over the whole display.
    fn solid(c: Color) -> Tree {
        let mut t = Tree::new(Rect::new(0, 0, W, H));
        t.push(
            ROOT,
            Node {
                rect: Rect::new(0, 0, W, H),
                kind: Kind::Panel { background: c },
                visible: true,
                antialias: None,
                name: None,
                children: Vec::new(),
                parent: None,
            },
        )
        .unwrap();
        t
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn run(kind: Transition, t: f32) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w: W, h: H }, PixelFormat::Bgrx8888);
        let font: Font<'static> = default_font();
        let (images, anims) = (ImageTable::new(), AnimTable::new());
        let res = Resources {
            images: &images,
            anims: &anims,
            font: &font,
            menus: &[],
        };
        transition(&mut s, &solid(RED), res, &solid(BLUE), res, kind, t);
        s
    }

    /// The colour at (x, y) as (r, g, b).
    fn at(s: &MemorySurface, x: usize, y: usize) -> (u8, u8, u8) {
        let o = y * s.stride() + x * 4;
        (s.pixels()[o + 2], s.pixels()[o + 1], s.pixels()[o])
    }

    fn is_red(s: &MemorySurface, x: usize, y: usize) -> bool {
        at(s, x, y) == (255, 0, 0)
    }

    fn is_blue(s: &MemorySurface, x: usize, y: usize) -> bool {
        at(s, x, y) == (0, 0, 255)
    }

    /// Whether every pixel belongs to one scene or the other.
    fn covered(s: &MemorySurface) -> bool {
        (0..H as usize).all(|y| (0..W as usize).all(|x| is_red(s, x, y) || is_blue(s, x, y)))
    }

    #[test]
    fn every_transition_starts_on_the_old_scene_and_ends_on_the_new() {
        for kind in [
            Transition::Cut,
            Transition::Slide(Edge::Left),
            Transition::Slide(Edge::Right),
            Transition::Slide(Edge::Up),
            Transition::Slide(Edge::Down),
            Transition::Wipe(Edge::Left),
            Transition::Wipe(Edge::Right),
            Transition::Wipe(Edge::Up),
            Transition::Wipe(Edge::Down),
        ] {
            let start = run(kind, 0.0);
            assert!(
                is_red(&start, 0, 0) && is_red(&start, 19, 9),
                "{kind:?} at 0"
            );
            let end = run(kind, 1.0);
            assert!(is_blue(&end, 0, 0) && is_blue(&end, 19, 9), "{kind:?} at 1");
        }
    }

    #[test]
    fn a_transition_in_progress_leaves_no_gap_between_the_two_scenes() {
        // The bug a seam looks like: one column of whatever was on the screen
        // before, showing between the outgoing and incoming scenes.
        for kind in [
            Transition::Slide(Edge::Left),
            Transition::Slide(Edge::Right),
            Transition::Slide(Edge::Up),
            Transition::Slide(Edge::Down),
            Transition::Wipe(Edge::Left),
            Transition::Wipe(Edge::Right),
            Transition::Wipe(Edge::Up),
            Transition::Wipe(Edge::Down),
        ] {
            for step in 1..10 {
                let s = run(kind, step as f32 / 10.0);
                assert!(covered(&s), "{kind:?} left a gap at t={step}/10");
            }
        }
    }

    #[test]
    fn a_wipe_uncovers_the_new_scene_from_the_edge_it_names() {
        let s = run(Transition::Wipe(Edge::Left), 0.5);
        assert!(is_blue(&s, 0, 5), "the left edge should be the new scene");
        assert!(is_red(&s, 19, 5), "the right edge should still be the old");

        let s = run(Transition::Wipe(Edge::Up), 0.5);
        assert!(is_blue(&s, 10, 0), "the top should be the new scene");
        assert!(is_red(&s, 10, 9), "the bottom should still be the old");
    }

    #[test]
    fn a_slide_brings_the_new_scene_in_from_the_edge_it_names() {
        let s = run(Transition::Slide(Edge::Left), 0.5);
        assert!(is_blue(&s, 0, 5), "the new scene enters from the left");
        assert!(is_red(&s, 19, 5), "the old scene is still leaving right");

        let s = run(Transition::Slide(Edge::Down), 0.5);
        assert!(is_blue(&s, 10, 9), "the new scene enters from the bottom");
        assert!(is_red(&s, 10, 0));
    }

    #[test]
    fn a_cut_shows_the_old_scene_until_it_is_finished() {
        assert!(is_red(&run(Transition::Cut, 0.99), 10, 5));
        assert!(is_blue(&run(Transition::Cut, 1.0), 10, 5));
    }

    #[test]
    fn a_progress_outside_the_range_is_clamped_rather_than_extrapolated() {
        // A caller with a stalled clock must not slide a scene off-screen.
        assert!(is_red(&run(Transition::Slide(Edge::Left), -5.0), 10, 5));
        assert!(is_blue(&run(Transition::Slide(Edge::Left), 5.0), 10, 5));
        assert!(is_red(&run(Transition::Wipe(Edge::Left), f32::NAN), 10, 5));
    }

    #[test]
    fn the_names_a_rig_file_uses_round_trip() {
        assert_eq!(Transition::parse("cut"), Some(Transition::Cut));
        assert_eq!(
            Transition::parse("slide-left"),
            Some(Transition::Slide(Edge::Left))
        );
        assert_eq!(
            Transition::parse("wipe-down"),
            Some(Transition::Wipe(Edge::Down))
        );
        assert_eq!(Transition::parse("slide"), None, "a slide needs an edge");
        assert_eq!(Transition::parse("cut-left"), None, "a cut has no edge");
        assert_eq!(Transition::parse("fade"), None);
        assert_eq!(Transition::parse("wipe-sideways"), None);
    }
}

#[cfg(test)]
mod crossfade_tests {
    use super::tests_support::*;
    use super::*;
    use crate::{Color, MemorySurface, PixelFormat, Size};

    /// Halfway through a fade, is this pixel a blend of the two?
    fn blended(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        let (b, r) = (s.pixels()[o], s.pixels()[o + 2]);
        b > 20 && r > 20
    }

    #[test]
    fn a_board_with_a_second_buffer_actually_blends() {
        let mut s =
            MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888).with_scratch_buffer();
        run_into(&mut s, Transition::CrossFade, 0.5);
        assert!(
            blended(&s, 10, 5),
            "halfway through a fade neither scene should be pure"
        );
    }

    #[test]
    fn a_board_without_one_cuts_instead_of_fading() {
        // The whole of what not implementing `with_scratch` costs.
        let mut s = MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888);
        run_into(&mut s, Transition::CrossFade, 0.5);
        assert!(!blended(&s, 10, 5), "it blended without a second buffer");
        assert!(
            is_red_at(&s, 10, 5),
            "a cut shows the old scene until the end"
        );
    }

    #[test]
    fn a_fade_still_starts_on_the_old_scene_and_ends_on_the_new() {
        for scratch in [false, true] {
            let mut s = MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888);
            if scratch {
                s = s.with_scratch_buffer();
            }
            run_into(&mut s, Transition::CrossFade, 0.0);
            assert!(is_red_at(&s, 10, 5), "scratch={scratch} at t=0");

            let mut s = MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888);
            if scratch {
                s = s.with_scratch_buffer();
            }
            run_into(&mut s, Transition::CrossFade, 1.0);
            assert!(is_blue_at(&s, 10, 5), "scratch={scratch} at t=1");
        }
    }

    #[test]
    fn the_scratch_buffer_survives_being_used_twice() {
        // It is lent out and handed back, so a second frame must still fade.
        let mut s =
            MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888).with_scratch_buffer();
        run_into(&mut s, Transition::CrossFade, 0.5);
        run_into(&mut s, Transition::CrossFade, 0.5);
        assert!(blended(&s, 10, 5));
        assert!(
            s.caps().contains(Caps::SCRATCH),
            "the buffer was not returned"
        );
    }

    #[test]
    fn a_rig_can_name_a_cross_fade() {
        assert_eq!(Transition::parse("crossfade"), Some(Transition::CrossFade));
        assert_eq!(Transition::parse("crossfade-left"), None);
    }

    #[test]
    fn a_transparent_pixel_in_the_outgoing_scene_does_not_darken_the_new_one() {
        // The scratch starts transparent, so a scene that does not cover its
        // display must not blend black over what is underneath.
        let mut s =
            MemorySurface::new(Size { w: 20, h: 10 }, PixelFormat::Bgrx8888).with_scratch_buffer();
        let _ = Color::TRANSPARENT;
        run_into(&mut s, Transition::CrossFade, 0.999);
        assert!(is_blue_at(&s, 10, 5) || blended(&s, 10, 5));
    }
}
