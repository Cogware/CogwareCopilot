// SPDX-License-Identifier: MIT OR Apache-2.0
//! Values that change over time.
//!
//! An animation binds one numeric property of one widget to a curve between
//! two endpoints. [`Animator::tick`] advances every animation and marks whatever
//! moved as dirty, so an animated scene costs exactly the rectangles that are
//! actually changing — the damage tracker never has to be told twice.
//!
//! # Why time is passed in rather than read
//!
//! There is no portable clock. A bare-metal caller reads the system timer, the
//! simulator reads `Instant`, and a test passes whatever number it likes —
//! which is the only reason animation is testable at all. Every function here
//! takes a monotonic microsecond count and none of them knows where it came
//! from.
//!
//! # Why the elapsed time is stored rather than a start instant
//!
//! Storing "started at T" means the first tick after a long stall jumps the
//! animation forward by the whole stall. Storing elapsed time and adding a
//! clamped delta means a stall costs at most one frame of motion, which is
//! what a gauge should do when the machine hiccups: resume, not teleport.

use crate::widget::{NodeId, Tree};

mod ease;
pub use ease::Easing;

/// What happens when an animation reaches its end.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Repeat {
    /// Stop at the end value.
    #[default]
    Once,
    /// Jump back to the start and run again.
    Loop,
    /// Run backwards, then forwards, forever.
    PingPong,
}

impl Repeat {
    /// Parse the name a scene file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "once" => Some(Self::Once),
            "loop" => Some(Self::Loop),
            "ping_pong" | "pingpong" => Some(Self::PingPong),
            _ => None,
        }
    }
}

/// Which property of a widget an animation drives.
///
/// A closed set, like [`Kind`] and for the same reason: the scene format has
/// to name them, and a downstream crate cannot add a widget property it has no
/// way to describe.
///
/// [`Kind`]: crate::widget::Kind
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Property {
    /// A [`Kind::Bar`]'s fill fraction.
    ///
    /// [`Kind::Bar`]: crate::widget::Kind::Bar
    BarValue,
    /// The node's x position, in its parent's coordinates.
    X,
    /// The node's y position.
    Y,
    /// The node's width.
    Width,
    /// The node's height.
    Height,
}

impl Property {
    /// Parse the name a scene file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "value" => Some(Self::BarValue),
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "width" => Some(Self::Width),
            "height" => Some(Self::Height),
            _ => None,
        }
    }
}

/// One animated property of one widget.
#[derive(Clone, Copy, Debug)]
pub struct Animation {
    /// The widget this drives.
    pub node: NodeId,
    /// Which of its properties.
    pub property: Property,
    /// Value at the start of a run.
    pub from: f32,
    /// Value at the end of a run.
    pub to: f32,
    /// How long one run takes, in microseconds. Never zero.
    pub duration_us: u64,
    /// The shape of the motion.
    pub easing: Easing,
    /// What happens at the end.
    pub repeat: Repeat,
    /// How far into the current run we are.
    elapsed_us: u64,
    /// Whether a ping-pong run is currently going backwards.
    reversed: bool,
    /// Whether a `Once` animation has finished.
    done: bool,
}

impl Animation {
    /// A new animation, at the start of its first run.
    ///
    /// A `duration_ms` of zero is raised to one millisecond: a zero-length
    /// animation would divide by zero, and refusing it outright would make a
    /// scene fail to load over something the author plainly meant as "instant".
    #[must_use]
    pub fn new(
        node: NodeId,
        property: Property,
        from: f32,
        to: f32,
        duration_ms: u32,
        easing: Easing,
        repeat: Repeat,
    ) -> Self {
        Self {
            node,
            property,
            from,
            to,
            duration_us: u64::from(duration_ms.max(1)) * 1_000,
            easing,
            repeat,
            elapsed_us: 0,
            reversed: false,
            done: false,
        }
    }

    /// Whether this animation will never change again.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.done
    }

    /// The value this animation currently holds.
    #[must_use]
    pub fn value(&self) -> f32 {
        // `duration_us` is never zero, so this division is always defined.
        let t = self.elapsed_us as f32 / self.duration_us as f32;
        let t = if self.reversed { 1.0 - t } else { t };
        let p = self.easing.apply(t);
        self.from + (self.to - self.from) * p
    }

    /// Advance by `delta_us` and report whether the value changed.
    fn advance(&mut self, delta_us: u64) -> bool {
        if self.done {
            return false;
        }
        let before = self.value();
        self.elapsed_us = self.elapsed_us.saturating_add(delta_us);

        while self.elapsed_us >= self.duration_us {
            match self.repeat {
                Repeat::Once => {
                    self.elapsed_us = self.duration_us;
                    self.done = true;
                    break;
                }
                Repeat::Loop => self.elapsed_us -= self.duration_us,
                Repeat::PingPong => {
                    self.elapsed_us -= self.duration_us;
                    self.reversed = !self.reversed;
                }
            }
        }
        // Comparing values rather than time is what keeps a slow animation
        // from marking a rectangle dirty on every frame while rounding to the
        // same pixel.
        before != self.value()
    }
}

/// Every animation in a scene, advanced together.
#[derive(Clone, Debug, Default)]
pub struct Animator {
    anims: alloc::vec::Vec<Animation>,
    last_us: Option<u64>,
}

/// Longest step a single tick may apply, in microseconds.
///
/// A stall — a slow frame, a debugger breakpoint, a card that took its time —
/// must not teleport every gauge to wherever it would have been. Capping the
/// delta means motion resumes from where it stopped, one frame late.
pub const MAX_STEP_US: u64 = 100_000;

impl Animator {
    /// An empty animator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            anims: alloc::vec::Vec::new(),
            last_us: None,
        }
    }

    /// Hand an animation to the animator to drive.
    pub fn push(&mut self, anim: Animation) {
        self.anims.push(anim);
    }

    /// How many animations are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.anims.len()
    }

    /// Whether there are no animations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anims.is_empty()
    }

    /// Whether every animation has finished.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.anims.iter().all(Animation::is_finished)
    }

    /// Advance every animation to `now_us` and apply the results to `tree`.
    ///
    /// `now_us` is a monotonic microsecond count from any source the caller
    /// likes. The first call establishes the baseline and moves nothing, so a
    /// scene loaded at an arbitrary clock value does not jump on its first
    /// frame.
    pub fn tick(&mut self, tree: &mut Tree, now_us: u64) {
        let delta = match self.last_us {
            None => 0,
            // Saturating, not wrapping: a clock that goes backwards is a bug
            // somewhere else, and freezing is a better response than running
            // every animation through an enormous delta.
            Some(last) => now_us.saturating_sub(last).min(MAX_STEP_US),
        };
        self.last_us = Some(now_us);
        if delta == 0 {
            return;
        }

        for anim in &mut self.anims {
            if anim.advance(delta) {
                apply(tree, anim.node, anim.property, anim.value());
            }
        }
    }
}

/// Write one animated value into the tree, through the setters that mark damage.
fn apply(tree: &mut Tree, node: NodeId, property: Property, value: f32) {
    use crate::widget::Kind;

    let Some(current) = tree.get(node) else {
        return;
    };
    match property {
        Property::BarValue => {
            // Both kinds of bargraph, because they are the same reading shown
            // two ways and a scene should not have to know which one it is
            // driving. Anything else is left alone, and the mismatch shows up
            // as a gauge that does not move.
            let next = match current.kind.clone() {
                Kind::Bar {
                    fill,
                    track,
                    vertical,
                    ..
                } => Some(Kind::Bar {
                    value,
                    fill,
                    track,
                    vertical,
                }),
                Kind::SegBar {
                    segments,
                    gap,
                    fill,
                    track,
                    warn,
                    warn_fill,
                    danger,
                    danger_fill,
                    vertical,
                    profile,
                    height,
                    divisions,
                    div_gap,
                    ..
                } => Some(Kind::SegBar {
                    value,
                    segments,
                    gap,
                    fill,
                    track,
                    warn,
                    warn_fill,
                    danger,
                    danger_fill,
                    vertical,
                    profile,
                    height,
                    divisions,
                    div_gap,
                }),
                _ => None,
            };
            if let Some(k) = next {
                tree.set_kind(node, k);
            }
        }
        Property::X | Property::Y | Property::Width | Property::Height => {
            let r = current.rect;
            let v = value as i32;
            let new = match property {
                Property::X => crate::Rect::new(v, r.top(), r.size.w, r.size.h),
                Property::Y => crate::Rect::new(r.left(), v, r.size.w, r.size.h),
                Property::Width => {
                    crate::Rect::new(r.left(), r.top(), value.max(0.0) as u32, r.size.h)
                }
                Property::Height => {
                    crate::Rect::new(r.left(), r.top(), r.size.w, value.max(0.0) as u32)
                }
                Property::BarValue => unreachable!(),
            };
            tree.set_rect(node, new);
        }
    }
}

/// Advance every playing [`Kind::Anim`] widget in `tree` by `delta_us`.
///
/// Separate from [`Animator::tick`] because the two are different kinds of
/// motion: an [`Animation`] interpolates a number along a curve, while this
/// steps through frames a file has already timed. Sharing a mechanism would
/// mean inventing a curve for something that has none.
///
/// [`Kind::Anim`]: crate::widget::Kind::Anim
pub fn tick_playback(tree: &mut Tree, anims: &crate::asset::AnimTable, delta_us: u64) {
    use crate::widget::Kind;

    // Collected first because advancing one widget borrows the tree mutably,
    // and a scene can hold many players.
    let ids: alloc::vec::Vec<NodeId> = (0..tree.len())
        .filter_map(|i| u32::try_from(i).ok().map(NodeId))
        .filter(|id| {
            matches!(
                tree.get(*id).map(|n| &n.kind),
                Some(Kind::Anim { playing: true, .. })
            )
        })
        .collect();

    for id in ids {
        let Some(node) = tree.get(id) else { continue };
        let Kind::Anim {
            anim,
            frame,
            playing,
            speed,
            elapsed_us,
        } = node.kind
        else {
            continue;
        };
        let Some(a) = anims.get(anim) else { continue };
        if a.frames.is_empty() || speed == 0.0 || !speed.is_finite() {
            continue;
        }

        // Scaling the elapsed time rather than the delay keeps the file's own
        // per-frame timing intact: a GIF whose frames have different delays
        // still plays with those proportions at any speed.
        let scaled = (delta_us as f64 * f64::from(speed.abs())) as u64;
        let mut acc = elapsed_us.saturating_add(scaled);
        let mut idx = frame as usize;
        let backwards = speed < 0.0;

        // Bounded by the frame count so one tick advances at most one full
        // cycle. `.max(1)` below already rules out a zero delay; what this
        // stops is a large accumulated time walking thousands of frames.
        for _ in 0..a.frames.len().max(1) {
            let delay = a.frames.get(idx).map_or(100_000, |f| f.delay_us).max(1);
            if acc < delay {
                break;
            }
            acc -= delay;
            idx = if backwards {
                if idx == 0 {
                    a.frames.len() - 1
                } else {
                    idx - 1
                }
            } else {
                (idx + 1) % a.frames.len()
            };
        }

        if idx as u32 != frame || acc != elapsed_us {
            tree.set_kind(
                id,
                Kind::Anim {
                    anim,
                    frame: idx as u32,
                    playing,
                    speed,
                    elapsed_us: acc,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Kind, Node, ROOT};
    use crate::{Color, Rect};

    const MS: u64 = 1_000;

    fn tree_with_bar() -> (Tree, NodeId) {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let id = t
            .push(
                ROOT,
                Node {
                    rect: Rect::new(0, 0, 10, 10),
                    kind: Kind::Bar {
                        value: 0.0,
                        fill: Color::WHITE,
                        track: Color::BLACK,
                        vertical: false,
                    },
                    visible: true,
                    antialias: None,
                    name: None,
                    children: alloc::vec::Vec::new(),
                    parent: None,
                },
            )
            .unwrap();
        t.clear_damage();
        (t, id)
    }

    fn bar_value(t: &Tree, id: NodeId) -> f32 {
        match t.get(id).unwrap().kind {
            Kind::Bar { value, .. } => value,
            ref k => panic!("expected a bar, got {k:?}"),
        }
    }

    fn anim(id: NodeId, repeat: Repeat) -> Animation {
        Animation::new(
            id,
            Property::BarValue,
            0.0,
            1.0,
            100,
            Easing::Linear,
            repeat,
        )
    }

    #[test]
    fn the_first_tick_establishes_a_baseline_and_moves_nothing() {
        // A scene loaded at an arbitrary clock value must not jump on its
        // first frame by however large that value happens to be.
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Once));
        a.tick(&mut t, 9_999_999_999);
        assert_eq!(bar_value(&t, id), 0.0);
        assert!(t.damage().is_empty());
    }

    #[test]
    fn a_value_advances_and_marks_damage() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Once));
        a.tick(&mut t, 0);
        a.tick(&mut t, 50 * MS);
        assert!((bar_value(&t, id) - 0.5).abs() < 1e-3, "halfway");
        assert!(!t.damage().is_empty(), "a moved gauge must be repainted");
    }

    #[test]
    fn a_once_animation_stops_at_its_end_value() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Once));
        a.tick(&mut t, 0);
        a.tick(&mut t, 100 * MS);
        assert_eq!(bar_value(&t, id), 1.0);
        assert!(a.is_settled());
        // And stays there.
        a.tick(&mut t, 200 * MS);
        assert_eq!(bar_value(&t, id), 1.0);
    }

    #[test]
    fn a_loop_returns_to_the_start() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Loop));
        a.tick(&mut t, 0);
        a.tick(&mut t, 75 * MS);
        assert!(bar_value(&t, id) > 0.5);
        a.tick(&mut t, 155 * MS); // 80ms on, wrapping past the end
        assert!(bar_value(&t, id) < 0.6, "should have wrapped round");
        assert!(!a.is_settled(), "a loop never settles");
    }

    #[test]
    fn ping_pong_reverses_instead_of_jumping() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::PingPong));
        a.tick(&mut t, 0);
        a.tick(&mut t, 90 * MS);
        let near_end = bar_value(&t, id);
        a.tick(&mut t, 180 * MS);
        let after = bar_value(&t, id);
        assert!(
            after < near_end,
            "should be coming back down: {near_end} -> {after}"
        );
    }

    #[test]
    fn a_stall_costs_one_frame_of_motion_not_the_whole_stall() {
        // The reason elapsed time is stored rather than a start instant. A
        // debugger breakpoint must not teleport every gauge.
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(Animation::new(
            id,
            Property::BarValue,
            0.0,
            1.0,
            10_000,
            Easing::Linear,
            Repeat::Loop,
        ));
        a.tick(&mut t, 0);
        a.tick(&mut t, 60 * 1_000_000); // sixty seconds later
        let v = bar_value(&t, id);
        let cap = MAX_STEP_US as f32 / 10_000_000.0;
        assert!(v <= cap + 1e-4, "advanced {v}, more than the {cap} cap");
    }

    #[test]
    fn a_clock_that_goes_backwards_freezes_rather_than_leaping() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Loop));
        a.tick(&mut t, 500 * MS);
        a.tick(&mut t, 550 * MS);
        let before = bar_value(&t, id);
        a.tick(&mut t, 10 * MS);
        assert_eq!(
            bar_value(&t, id),
            before,
            "a backwards clock must not move it"
        );
    }

    #[test]
    fn a_position_animation_moves_the_node_and_dirties_both_places() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(Animation::new(
            id,
            Property::X,
            0.0,
            50.0,
            100,
            Easing::Linear,
            Repeat::Once,
        ));
        a.tick(&mut t, 0);
        a.tick(&mut t, 100 * MS);
        assert_eq!(t.get(id).unwrap().rect.left(), 50);
        let b = t.damage().bounds().unwrap();
        assert!(b.contains_rect(Rect::new(0, 0, 10, 10)), "old position");
        assert!(b.contains_rect(Rect::new(50, 0, 10, 10)), "new position");
    }

    #[test]
    fn driving_a_bar_property_on_a_non_bar_leaves_it_alone() {
        // Silently doing nothing is right here: the mismatch shows up as a
        // widget that does not move, which is findable. Coercing the widget
        // into a bar would be a far stranger surprise.
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let id = t
            .push(
                ROOT,
                Node {
                    rect: Rect::new(0, 0, 10, 10),
                    kind: Kind::Panel {
                        background: Color::WHITE,
                    },
                    visible: true,
                    antialias: None,
                    name: None,
                    children: alloc::vec::Vec::new(),
                    parent: None,
                },
            )
            .unwrap();
        let mut a = Animator::new();
        a.push(anim(id, Repeat::Once));
        a.tick(&mut t, 0);
        a.tick(&mut t, 50 * MS);
        assert!(matches!(t.get(id).unwrap().kind, Kind::Panel { .. }));
    }

    #[test]
    fn a_zero_duration_is_raised_rather_than_dividing_by_zero() {
        let (mut t, id) = tree_with_bar();
        let mut a = Animator::new();
        a.push(Animation::new(
            id,
            Property::BarValue,
            0.0,
            1.0,
            0,
            Easing::Linear,
            Repeat::Once,
        ));
        a.tick(&mut t, 0);
        a.tick(&mut t, 10 * MS);
        assert_eq!(bar_value(&t, id), 1.0);
    }

    #[test]
    fn an_animation_on_a_missing_node_is_ignored() {
        let (mut t, _) = tree_with_bar();
        let mut a = Animator::new();
        a.push(anim(NodeId(999), Repeat::Loop));
        a.tick(&mut t, 0);
        a.tick(&mut t, 50 * MS);
    }

    #[test]
    fn names_parse() {
        assert_eq!(Repeat::parse("loop"), Some(Repeat::Loop));
        assert_eq!(Repeat::parse("ping_pong"), Some(Repeat::PingPong));
        assert_eq!(Repeat::parse("nonsense"), None);
        assert_eq!(Property::parse("value"), Some(Property::BarValue));
        assert_eq!(Property::parse("width"), Some(Property::Width));
        assert_eq!(Property::parse("colour"), None);
    }
}

#[cfg(test)]
mod cast_tests {
    use super::*;
    use crate::widget::{Kind, Node, ROOT};
    use crate::{Color, Rect};

    fn tree_with_panel() -> (Tree, NodeId) {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let id = t
            .push(
                ROOT,
                Node {
                    rect: Rect::new(0, 0, 10, 10),
                    kind: Kind::Panel {
                        background: Color::WHITE,
                    },
                    visible: true,
                    antialias: None,
                    name: None,
                    children: alloc::vec::Vec::new(),
                    parent: None,
                },
            )
            .unwrap();
        t.clear_damage();
        (t, id)
    }

    /// The scene format lets an author write any float as an endpoint, and it
    /// reaches `as i32` and `as u32` here. Rust saturates rather than wrapping,
    /// which is the behaviour this relies on: an absurd endpoint pins the
    /// widget at an extreme instead of teleporting it somewhere arbitrary.
    #[test]
    fn an_absurd_endpoint_saturates_rather_than_wrapping() {
        for (prop, to) in [
            (Property::X, 1.0e20f32),
            (Property::X, -1.0e20),
            (Property::Width, 1.0e20),
        ] {
            let (mut t, id) = tree_with_panel();
            let mut a = Animator::new();
            a.push(Animation::new(
                id,
                prop,
                0.0,
                to,
                10,
                Easing::Linear,
                Repeat::Once,
            ));
            a.tick(&mut t, 0);
            a.tick(&mut t, 100_000);
            let r = t.get(id).unwrap().rect;
            // The direction is the whole point: a wrapping cast would land an
            // enormous positive endpoint on a negative coordinate.
            match prop {
                Property::X if to > 0.0 => assert!(r.left() > 0, "wrapped to {}", r.left()),
                Property::X => assert!(r.left() < 0, "wrapped to {}", r.left()),
                Property::Width => assert!(r.size.w > 0, "wrapped to {}", r.size.w),
                _ => {}
            }
        }
    }

    #[test]
    fn a_nan_endpoint_does_not_move_the_widget_somewhere_arbitrary() {
        // NaN as i32 is 0 in Rust, which is a defined and sane landing spot.
        let (mut t, id) = tree_with_panel();
        let mut a = Animator::new();
        a.push(Animation::new(
            id,
            Property::X,
            0.0,
            f32::NAN,
            10,
            Easing::Linear,
            Repeat::Once,
        ));
        a.tick(&mut t, 0);
        a.tick(&mut t, 100_000);
        let r = t.get(id).unwrap().rect;
        assert_eq!(r.left(), 0);
    }
}
