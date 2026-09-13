// SPDX-License-Identifier: GPL-3.0-only
//! Turning a touch panel's stream of points into gestures.
//!
//! The crate owns no input device for the same reason it owns no pixels: a
//! resistive panel on SPI, a capacitive controller on I2C and a simulator's
//! mouse produce the same three facts -- a finger went down, moved, came up --
//! and nothing above that line should have to know which one it was talking
//! to. A host reads its panel and pushes [`Touch`] in, exactly as it already
//! pushes time into [`crate::anim::Animator::tick`] and readings into
//! [`crate::asset::Scene::apply_gauges`].
//!
//! ```ignore
//! let mut touch = Recognizer::new();                 // once
//! if let Some(g) = touch.feed(panel.read(now_us)) {  // per sample
//!     if let Some(event) = scene.touch(g) { /* act on it */ }
//! }
//! ```
//!
//! # Why a recogniser at all
//!
//! A panel reports positions, and the difference between a tap and a swipe is
//! entirely in what happened between the down and the up. Every consumer would
//! otherwise write the same state machine, and each would pick its own
//! threshold for "did the finger move"; a scene that works on the cluster and
//! not on the round gauge beside it is exactly the failure the shared
//! implementation rules out.
//!
//! # Why there is no long press
//!
//! A hold has to be detected while nothing is happening, which means the
//! recogniser needs a clock poll of its own on top of the samples. Nothing in
//! the format asks for one yet, and an API that is fed only when the panel has
//! something to say is much harder to use wrongly. If a hold is wanted later
//! it arrives as a `tick(now_us)` beside `feed`, and no existing caller
//! changes.

use crate::Point;

/// Where a touch is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The finger has just landed.
    Down,
    /// It is still down and has moved.
    Move,
    /// It has lifted.
    Up,
    /// The panel, or the host, has abandoned this touch: a palm rejected, a
    /// window losing focus, a controller resetting. Distinct from `Up`
    /// because an abandoned touch must never fire whatever it was over.
    Cancel,
}

/// One sample from a touch panel.
///
/// Single touch only. Nothing on a dashboard is pinched or rotated, and a
/// second finger on a cluster is a passenger leaning on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch {
    /// Where the finger is, in screen coordinates.
    pub point: Point,
    /// What just happened to it.
    pub phase: Phase,
    /// The host's monotonic microsecond count, as [`crate::anim`] uses.
    pub at_us: u64,
}

impl Touch {
    /// A sample, for a host building one per panel read.
    #[must_use]
    pub const fn new(point: Point, phase: Phase, at_us: u64) -> Self {
        Self {
            point,
            phase,
            at_us,
        }
    }
}

/// Which way a swipe went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Towards smaller x.
    Left,
    /// Towards larger x.
    Right,
    /// Towards smaller y.
    Up,
    /// Towards larger y.
    Down,
}

impl Dir {
    /// The axis this direction runs along.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Dir::Left | Dir::Right => Axis::Horizontal,
            Dir::Up | Dir::Down => Axis::Vertical,
        }
    }

    /// Whether a swipe this way brings the *next* page on: left and up do,
    /// right and down go back.
    ///
    /// The finger drags the page it is on out of the way, and the next one
    /// follows it in -- which is why this is the opposite of the direction a
    /// page of text is read in, and why it reads the same on a horizontal
    /// pager and a vertical one. Anyone who has used a phone expects nothing
    /// else, and a cluster that paged the other way would feel broken however
    /// well it was argued for.
    #[must_use]
    pub const fn advances(self) -> bool {
        matches!(self, Dir::Left | Dir::Up)
    }

    /// The name a scene file uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Dir::Left => "left",
            Dir::Right => "right",
            Dir::Up => "up",
            Dir::Down => "down",
        }
    }
}

/// Which way a thing is laid out, and therefore which swipes move it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    /// Left and right.
    #[default]
    Horizontal,
    /// Up and down.
    Vertical,
}

impl Axis {
    /// Parse the name a scene file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "horizontal" => Some(Self::Horizontal),
            "vertical" => Some(Self::Vertical),
            _ => None,
        }
    }

    /// The name a scene file uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// What the recogniser made of a touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// A finger has landed here.
    ///
    /// Emitted before anyone knows what the touch will turn out to be, so a
    /// widget can light under the finger. Nothing should *act* on a press:
    /// the finger has not committed to anything yet and may still slide off.
    Press(Point),
    /// A finger landed and lifted without going anywhere.
    Tap(Point),
    /// A finger travelled far enough, in one direction, to mean it.
    Swipe {
        /// Where it landed. This is what decides *what* was swiped, because
        /// it is the only point the finger was certainly over.
        from: Point,
        /// Where it lifted.
        to: Point,
        /// The dominant direction of travel.
        dir: Dir,
    },
    /// The finger is off the panel. Always the last gesture of a touch,
    /// including after a `Tap` or a `Swipe`, so a pressed widget has exactly
    /// one place to let go.
    Release(Point),
}

/// How far a finger may stray, and how long it may linger, before a touch
/// stops being a tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Pixels of travel a tap is allowed. Below this the finger is still
    /// "in the same place" as far as a person is concerned.
    ///
    /// A finger is a blunt instrument: nobody lands on a 480x480 panel and
    /// lifts off the same pixel, and a zero slop would make taps unreliable
    /// rather than precise.
    pub slop: u32,
    /// Pixels of travel a swipe needs. Comfortably above `slop`, so a sloppy
    /// tap can never be mistaken for a short swipe.
    pub swipe: u32,
    /// Microseconds after which a touch that has not moved is no longer a
    /// tap. A finger resting on the panel is not pressing a button.
    pub hold_us: u64,
}

impl Default for Limits {
    /// Thresholds for a panel a person prods with a finger while driving.
    ///
    /// Eight pixels of slop is about a millimetre on a 480x480 round gauge;
    /// forty for a swipe is far enough that it cannot be reached by the
    /// wobble of a hand on a moving car. Both are in pixels rather than
    /// millimetres because the crate is never told how big the panel is.
    fn default() -> Self {
        Self {
            slop: 8,
            swipe: 40,
            hold_us: 700_000,
        }
    }
}

/// The state of one finger, between its down and its up.
#[derive(Clone, Copy, Debug)]
struct Down {
    from: Point,
    at_us: u64,
    /// Whether the finger has ever been further than `slop` from `from`.
    ///
    /// Sticky: a finger that swings out and comes back has still moved, and
    /// treating the return trip as a tap would fire whatever it landed on.
    strayed: bool,
}

/// A touch panel's samples in, gestures out.
///
/// Holds one finger's worth of state and no allocation, so a display can own
/// one in a static.
#[derive(Clone, Copy, Debug, Default)]
pub struct Recognizer {
    limits: Limits,
    down: Option<Down>,
}

impl Recognizer {
    /// A recogniser with the default [`Limits`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            limits: Limits::default(),
            down: None,
        }
    }

    /// A recogniser with thresholds of your own, for a panel whose pixels are
    /// a different size from the one the defaults were chosen on.
    #[must_use]
    pub const fn with_limits(limits: Limits) -> Self {
        Self { limits, down: None }
    }

    /// The thresholds in force.
    #[must_use]
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// Whether a finger is currently down.
    #[must_use]
    pub const fn is_down(&self) -> bool {
        self.down.is_some()
    }

    /// Forget the touch in progress without emitting anything.
    ///
    /// For a host that has lost the panel rather than the finger -- a window
    /// closing, a mode change tearing the scene down. A host that still has a
    /// scene to notify should feed [`Phase::Cancel`] instead, so the press
    /// gets its matching release.
    pub fn forget(&mut self) {
        self.down = None;
    }

    /// Feed one sample, and get back what it meant.
    ///
    /// At most one gesture per sample. A tap and its release are two samples
    /// apart in the panel's stream but arrive from one `Up`, so the `Up` that
    /// completes a tap returns the [`Gesture::Tap`] and the release is
    /// implied -- see [`Gesture::Release`] for when it is emitted instead.
    pub fn feed(&mut self, touch: Touch) -> Option<Gesture> {
        match touch.phase {
            Phase::Down => {
                // A second down with no up between is a panel glitch or a
                // second finger. Take the new one: the alternative is a
                // recogniser stuck on a finger that is no longer there.
                self.down = Some(Down {
                    from: touch.point,
                    at_us: touch.at_us,
                    strayed: false,
                });
                Some(Gesture::Press(touch.point))
            }
            Phase::Move => {
                let down = self.down.as_mut()?;
                if !down.strayed && travel(down.from, touch.point) > self.limits.slop {
                    down.strayed = true;
                }
                None
            }
            Phase::Up => {
                let down = self.down.take()?;
                let strayed = down.strayed || travel(down.from, touch.point) > self.limits.slop;
                // A swipe is judged on the whole journey, a tap on whether
                // there was one at all.
                if let Some(dir) = direction(down.from, touch.point, self.limits.swipe) {
                    return Some(Gesture::Swipe {
                        from: down.from,
                        to: touch.point,
                        dir,
                    });
                }
                // `saturating_sub`, because a host whose clock jumped
                // backwards should get a tap, not a touch that can never be
                // one again.
                let held = touch.at_us.saturating_sub(down.at_us);
                if !strayed && held < self.limits.hold_us {
                    Some(Gesture::Tap(down.from))
                } else {
                    Some(Gesture::Release(touch.point))
                }
            }
            Phase::Cancel => {
                // Released, never fired. Whatever was lit goes out and
                // nothing happens, which is the whole point of a cancel.
                self.down.take()?;
                Some(Gesture::Release(touch.point))
            }
        }
    }
}

/// How far apart two points are, as the larger of the two axis distances.
///
/// Chebyshev rather than Euclidean: it needs no square root, which on a target
/// without an FPU is worth more than the difference between a circular
/// threshold and a square one that nobody can feel.
fn travel(a: Point, b: Point) -> u32 {
    let dx = b.x.saturating_sub(a.x).unsigned_abs();
    let dy = b.y.saturating_sub(a.y).unsigned_abs();
    dx.max(dy)
}

/// The direction from `a` to `b`, if it went at least `min` pixels along one
/// axis and that axis clearly won.
///
/// The dominant axis has to beat the other outright, so a diagonal drag is not
/// a swipe at all rather than a coin toss between two pages.
fn direction(a: Point, b: Point, min: u32) -> Option<Dir> {
    let dx = b.x.saturating_sub(a.x);
    let dy = b.y.saturating_sub(a.y);
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    if ax >= min && ax > ay {
        Some(if dx > 0 { Dir::Right } else { Dir::Left })
    } else if ay >= min && ay > ax {
        Some(if dy > 0 { Dir::Down } else { Dir::Up })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    /// Drive a whole touch through a fresh recogniser and collect what it said.
    fn gestures(samples: &[Touch]) -> alloc::vec::Vec<Gesture> {
        let mut r = Recognizer::new();
        samples.iter().filter_map(|t| r.feed(*t)).collect()
    }

    fn down(x: i32, y: i32, us: u64) -> Touch {
        Touch::new(at(x, y), Phase::Down, us)
    }
    fn moved(x: i32, y: i32, us: u64) -> Touch {
        Touch::new(at(x, y), Phase::Move, us)
    }
    fn up(x: i32, y: i32, us: u64) -> Touch {
        Touch::new(at(x, y), Phase::Up, us)
    }

    #[test]
    fn a_finger_landing_presses_before_anything_is_known() {
        let mut r = Recognizer::new();
        assert_eq!(r.feed(down(10, 10, 0)), Some(Gesture::Press(at(10, 10))));
        assert!(r.is_down());
    }

    #[test]
    fn down_and_up_in_one_place_is_a_tap() {
        assert_eq!(
            gestures(&[down(10, 10, 0), up(10, 10, 1000)]),
            [Gesture::Press(at(10, 10)), Gesture::Tap(at(10, 10))]
        );
    }

    #[test]
    fn a_tap_reports_where_the_finger_landed_not_where_it_lifted() {
        // The landing point is the one the finger was certainly over; by the
        // time it lifts it may have slid a few pixels onto the next widget.
        let g = gestures(&[down(10, 10, 0), up(14, 13, 1000)]);
        assert_eq!(g[1], Gesture::Tap(at(10, 10)));
    }

    #[test]
    fn a_small_wobble_is_still_a_tap() {
        // Nobody lands and lifts on the same pixel. A zero-slop recogniser
        // would make a dashboard button feel broken.
        let g = gestures(&[
            down(100, 100, 0),
            moved(103, 98, 100),
            moved(100, 102, 200),
            up(101, 101, 300),
        ]);
        assert_eq!(g.last(), Some(&Gesture::Tap(at(100, 100))));
    }

    #[test]
    fn a_finger_that_strayed_and_came_back_is_not_a_tap() {
        // Sticky straying: the finger left the widget, so lifting back over
        // it must not fire it.
        let g = gestures(&[down(100, 100, 0), moved(100, 130, 100), up(101, 101, 200)]);
        assert_eq!(g.last(), Some(&Gesture::Release(at(101, 101))));
    }

    #[test]
    fn a_finger_resting_too_long_releases_rather_than_taps() {
        let g = gestures(&[down(10, 10, 0), up(10, 10, 5_000_000)]);
        assert_eq!(g.last(), Some(&Gesture::Release(at(10, 10))));
    }

    #[test]
    fn a_long_travel_is_a_swipe_in_the_direction_it_went() {
        for (dx, dy, want) in [
            (200, 0, Dir::Right),
            (-200, 0, Dir::Left),
            (0, 200, Dir::Down),
            (0, -200, Dir::Up),
        ] {
            let g = gestures(&[down(240, 240, 0), up(240 + dx, 240 + dy, 200_000)]);
            assert_eq!(
                g.last(),
                Some(&Gesture::Swipe {
                    from: at(240, 240),
                    to: at(240 + dx, 240 + dy),
                    dir: want,
                }),
                "{dx},{dy}"
            );
        }
    }

    #[test]
    fn a_slow_swipe_is_still_a_swipe() {
        // The hold limit ends taps, not swipes: a deliberate, unhurried drag
        // across the panel is exactly how someone drives with one hand.
        let g = gestures(&[down(400, 100, 0), up(100, 100, 9_000_000)]);
        assert!(matches!(g.last(), Some(Gesture::Swipe { .. })));
    }

    #[test]
    fn a_diagonal_drag_is_not_a_swipe_at_all() {
        // Neither axis wins, so the recogniser refuses rather than tossing a
        // coin between two pages.
        let g = gestures(&[down(0, 0, 0), up(200, 200, 100_000)]);
        assert_eq!(g.last(), Some(&Gesture::Release(at(200, 200))));
    }

    #[test]
    fn a_drag_shorter_than_the_swipe_limit_is_neither() {
        // Past the slop so it is not a tap, short of the swipe limit so it is
        // not a swipe. Both refusals are the point.
        let g = gestures(&[down(100, 100, 0), up(125, 100, 100_000)]);
        assert_eq!(g.last(), Some(&Gesture::Release(at(125, 100))));
    }

    #[test]
    fn a_cancelled_touch_releases_and_fires_nothing() {
        let mut r = Recognizer::new();
        r.feed(down(10, 10, 0));
        assert_eq!(
            r.feed(Touch::new(at(10, 10), Phase::Cancel, 1000)),
            Some(Gesture::Release(at(10, 10)))
        );
        assert!(!r.is_down());
    }

    #[test]
    fn samples_with_no_finger_down_are_ignored() {
        // A panel that reports a move or an up out of nowhere must not make
        // the recogniser invent a gesture.
        let mut r = Recognizer::new();
        assert_eq!(r.feed(moved(10, 10, 0)), None);
        assert_eq!(r.feed(up(10, 10, 0)), None);
        assert_eq!(r.feed(Touch::new(at(1, 1), Phase::Cancel, 0)), None);
    }

    #[test]
    fn a_second_down_takes_over_from_the_first() {
        // A panel glitch, or a second finger. Staying on a finger that is no
        // longer there is worse than following the one that is.
        let mut r = Recognizer::new();
        r.feed(down(10, 10, 0));
        r.feed(down(300, 300, 1000));
        assert_eq!(r.feed(up(300, 300, 2000)), Some(Gesture::Tap(at(300, 300))));
    }

    #[test]
    fn forgetting_a_touch_emits_nothing_and_ends_it() {
        let mut r = Recognizer::new();
        r.feed(down(10, 10, 0));
        r.forget();
        assert!(!r.is_down());
        assert_eq!(r.feed(up(10, 10, 1000)), None);
    }

    #[test]
    fn a_clock_that_jumps_backwards_still_taps() {
        // `saturating_sub` on the hold: a host whose timer wrapped must not
        // be left unable to tap anything ever again.
        let mut r = Recognizer::new();
        r.feed(down(10, 10, 9_000_000));
        assert_eq!(r.feed(up(10, 10, 5)), Some(Gesture::Tap(at(10, 10))));
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        let mut r = Recognizer::new();
        r.feed(down(i32::MIN, i32::MIN, 0));
        let _ = r.feed(up(i32::MAX, i32::MAX, 1000));
    }

    #[test]
    fn tighter_limits_can_be_asked_for() {
        let mut r = Recognizer::with_limits(Limits {
            slop: 0,
            swipe: 4,
            hold_us: u64::MAX,
        });
        assert_eq!(r.limits().swipe, 4);
        r.feed(down(0, 0, 0));
        assert!(matches!(r.feed(up(5, 0, 10)), Some(Gesture::Swipe { .. })));
    }

    #[test]
    fn the_default_limits_cannot_confuse_a_sloppy_tap_for_a_swipe() {
        // The one invariant between the two thresholds. If slop ever reaches
        // the swipe limit, a shaky finger starts changing pages.
        let l = Limits::default();
        assert!(l.slop < l.swipe, "a tap's slop must stay under a swipe");
    }

    #[test]
    fn a_direction_knows_its_axis_and_which_way_it_pages() {
        assert_eq!(Dir::Left.axis(), Axis::Horizontal);
        assert_eq!(Dir::Down.axis(), Axis::Vertical);
        // The drag convention: the finger pulls the current page away and
        // the next one follows it in.
        assert!(Dir::Left.advances() && Dir::Up.advances());
        assert!(!Dir::Right.advances() && !Dir::Down.advances());
    }

    #[test]
    fn an_axis_round_trips_through_its_name() {
        for a in [Axis::Horizontal, Axis::Vertical] {
            assert_eq!(Axis::parse(a.name()), Some(a));
        }
        assert_eq!(Axis::parse("sideways"), None);
    }
}
