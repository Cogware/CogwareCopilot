// SPDX-License-Identifier: GPL-3.0-only
//! The shapes that are cheap in hardware and expensive in software.
//!
//! A ring, a hub and a needle have no rectangular decomposition, so software
//! draws them by sampling coverage inside every pixel they touch where
//! hardware draws each as one primitive.
//!
//! Coordinates are floats because these shapes do not land on pixel
//! boundaries: a needle passes through every angle as it sweeps.

/// A shape offered whole to a backend, in pixels.
///
/// Colour, antialiasing and the clip rectangle are separate arguments to
/// [`Surface::draw_primitive`]; only the geometry differs between variants.
/// `#[non_exhaustive]`, so match with a `_ => false` arm.
///
/// [`Surface::draw_primitive`]: super::Surface::draw_primitive
#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub enum Primitive {
    /// A straight line with round ends: a needle, a chart's trace, a tick.
    Line {
        /// One end, in pixels.
        a: (f32, f32),
        /// The other end, in pixels.
        b: (f32, f32),
        /// How wide the line is across, not half of it.
        width: f32,
    },
    /// A filled circle: the hub a needle turns on, a plotted point.
    ///
    /// Filled rather than outlined; an outline is an [`Arc`](Self::Arc) whose
    /// radii differ by the line width.
    Disc {
        /// Where the middle of it sits, in pixels.
        centre: (f32, f32),
        /// How far the fill reaches from `centre`, in pixels.
        radius: f32,
    },
    /// The part of a ring lying within a sweep of angle: every dial there is.
    Arc {
        /// Where the middle of the ring sits, in pixels.
        centre: (f32, f32),
        /// The radius of the hole, in pixels. Zero is a filled wedge.
        ///
        /// Already clamped to at most `outer` by the caller.
        inner: f32,
        /// The radius of the outside edge, in pixels.
        outer: f32,
        /// Where the sweep begins, in brads clockwise from three o'clock.
        ///
        /// Brads because the crate's trigonometry is a table indexed in them:
        /// a full turn is [`crate::trig::TURN`], which is 4096.
        start: i32,
        /// How far the sweep runs from `start`, in brads, and which way.
        ///
        /// Signed: negative sweeps anticlockwise. A magnitude of
        /// [`crate::trig::TURN`] or more is the whole ring.
        sweep: i32,
    },
}

#[cfg(test)]
mod tests {
    use crate::render::aa::{arc, disc, line};
    use crate::surface::Primitive;
    use crate::{Color, PixelFormat, Rect, Size, Surface};
    use alloc::vec::Vec;

    /// A backend that takes whichever primitives it was built to take and
    /// records every one it was offered, so a test can check the coverage
    /// sampler did *not* also run.
    struct Accel {
        takes: Option<&'static str>,
        offered: Vec<(Primitive, Rect, Color, bool)>,
        spans: usize,
    }

    impl Accel {
        fn new(takes: Option<&'static str>) -> Self {
            Self {
                takes,
                offered: Vec::new(),
                spans: 0,
            }
        }
        fn kind(prim: &Primitive) -> &'static str {
            match prim {
                Primitive::Line { .. } => "line",
                Primitive::Disc { .. } => "disc",
                // No wildcard: `non_exhaustive` binds other crates, not this
                // one, so a shape added here fails to compile until this
                // helper knows about it -- which is the reminder we want.
                Primitive::Arc { .. } => "arc",
            }
        }
    }

    impl Surface for Accel {
        fn size(&self) -> Size {
            Size { w: 64, h: 64 }
        }
        fn format(&self) -> PixelFormat {
            PixelFormat::Bgrx8888
        }
        fn fill_span(&mut self, _x: i32, _y: i32, _n: u32, _c: Color) {
            self.spans += 1;
        }
        fn blit_span(&mut self, _x: i32, _y: i32, _s: &[Color]) {
            self.spans += 1;
        }
        fn blend_span(&mut self, _x: i32, _y: i32, _s: &[Color]) {
            self.spans += 1;
        }
        fn draw_primitive(&mut self, p: Primitive, area: Rect, c: Color, aa: bool) -> bool {
            self.offered.push((p, area, c, aa));
            self.takes == Some(Self::kind(&p))
        }
        fn present(&mut self, _damage: Option<Rect>) {}
    }

    const AREA: Rect = Rect::new(0, 0, 64, 64);

    #[test]
    fn a_taken_arc_costs_one_primitive_and_no_spans() {
        // The whole point of the hook: a 480x480 dial is thousands of
        // coverage-sampled rows in software and one draw call here.
        let mut s = Accel::new(Some("arc"));
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            20.0,
            30.0,
            0,
            2048,
            Color::WHITE,
            true,
        );
        assert_eq!(s.offered.len(), 1);
        assert_eq!(s.spans, 0, "the sampler ran as well");
    }

    #[test]
    fn a_declined_arc_falls_back_to_the_sampler() {
        // The half-finished driver: it sees the ring and cannot draw it.
        let mut s = Accel::new(None);
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            20.0,
            30.0,
            0,
            2048,
            Color::WHITE,
            true,
        );
        assert_eq!(s.offered.len(), 1);
        assert!(s.spans > 0, "declining must not lose the ring");
    }

    #[test]
    fn a_ring_is_offered_with_its_hole_already_clamped() {
        // A backend must never have to reason about a ring whose hole is
        // bigger than the ring itself.
        let mut s = Accel::new(Some("arc"));
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            50.0,
            30.0,
            0,
            1024,
            Color::WHITE,
            false,
        );
        match s.offered[0].0 {
            Primitive::Arc { inner, outer, .. } => assert_eq!((inner, outer), (30.0, 30.0)),
            ref other => panic!("expected an arc, got {other:?}"),
        }
    }

    #[test]
    fn the_sweep_reaches_the_backend_signed_and_in_brads() {
        // A driver converting to its own angles needs the sign to know which
        // way round the dial goes; losing it draws the complement.
        let mut s = Accel::new(Some("arc"));
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            0.0,
            20.0,
            512,
            -1024,
            Color::WHITE,
            false,
        );
        match s.offered[0].0 {
            Primitive::Arc { start, sweep, .. } => assert_eq!((start, sweep), (512, -1024)),
            ref other => panic!("expected an arc, got {other:?}"),
        }
    }

    #[test]
    fn a_line_is_offered_with_its_full_width_not_its_half() {
        let mut s = Accel::new(Some("line"));
        line(
            &mut s,
            AREA,
            (1.0, 1.0),
            (20.0, 9.0),
            3.0,
            Color::WHITE,
            true,
        );
        match s.offered[0].0 {
            Primitive::Line { a, b, width } => {
                assert_eq!((a, b), ((1.0, 1.0), (20.0, 9.0)));
                assert_eq!(width, 3.0);
            }
            ref other => panic!("expected a line, got {other:?}"),
        }
        assert_eq!(s.spans, 0);
    }

    #[test]
    fn a_disc_is_offered_and_the_antialias_flag_travels_with_it() {
        // A backend with no blending declines rather than drawing the hard
        // edge, so it has to be told which was asked for.
        let mut s = Accel::new(Some("disc"));
        disc(&mut s, AREA, (32.0, 32.0), 8.0, Color::WHITE, false);
        disc(&mut s, AREA, (32.0, 32.0), 8.0, Color::WHITE, true);
        assert_eq!(s.offered.len(), 2);
        assert!(!s.offered[0].3 && s.offered[1].3);
    }

    #[test]
    fn a_backend_taking_only_arcs_still_gets_its_needles_drawn() {
        // Declining per variant is the point: hardware accelerates a case.
        let mut s = Accel::new(Some("arc"));
        line(
            &mut s,
            AREA,
            (1.0, 1.0),
            (20.0, 9.0),
            3.0,
            Color::WHITE,
            true,
        );
        assert!(s.spans > 0, "the declined line must still be rasterised");
    }

    #[test]
    fn nothing_invisible_is_ever_offered() {
        // Waking the hardware for a shape that cannot change a pixel is the
        // cost these early returns exist to avoid.
        let mut s = Accel::new(Some("arc"));
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            0.0,
            20.0,
            0,
            0,
            Color::WHITE,
            true,
        );
        arc(
            &mut s,
            AREA,
            (32.0, 32.0),
            0.0,
            20.0,
            0,
            512,
            Color::TRANSPARENT,
            true,
        );
        disc(&mut s, AREA, (32.0, 32.0), 0.0, Color::WHITE, true);
        line(
            &mut s,
            AREA,
            (1.0, 1.0),
            (9.0, 9.0),
            2.0,
            Color::TRANSPARENT,
            true,
        );
        assert!(s.offered.is_empty());
    }

    #[test]
    fn a_shape_entirely_off_the_clip_is_never_offered() {
        let mut s = Accel::new(Some("disc"));
        disc(
            &mut s,
            Rect::new(80, 80, 4, 4),
            (2.0, 2.0),
            1.0,
            Color::WHITE,
            true,
        );
        assert!(s.offered.is_empty());
    }

    #[test]
    fn the_area_offered_is_clipped_to_the_surface_not_just_the_damage() {
        // What a backend is handed always lands on the surface.
        let mut s = Accel::new(Some("arc"));
        arc(
            &mut s,
            Rect::new(-10, -10, 200, 200),
            (32.0, 32.0),
            0.0,
            20.0,
            0,
            1024,
            Color::WHITE,
            true,
        );
        assert_eq!(s.offered[0].1, Rect::new(0, 0, 64, 64));
    }
}
