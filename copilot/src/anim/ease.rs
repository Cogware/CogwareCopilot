// SPDX-License-Identifier: GPL-3.0-only
//! Easing curves.
//!
//! All polynomial. `core` has no `sin`, `powf` or `sqrt`, so the usual
//! sinusoidal and exponential easings are simply not available without
//! pulling in libm — and for a gauge sweeping between two readings the
//! quadratic and cubic curves are indistinguishable from them anyway.

/// How a value moves between its endpoints over an animation's run.
///
/// Each variant encodes a deterministic mapping from normalised time to
/// normalised progress. The mapping is always a pure polynomial (or a
/// piecewise polynomial) in `t`, so no transcendental or library calls are
/// required — this keeps the module usable in `#![no_std]` environments
/// where `core` provides no `sin`, `cos`, `powf`, or `sqrt`.
///
/// The `#[non_exhaustive]` marker lets future revisions add new curves
/// without breaking downstream `match` statements.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Easing {
    /// Constant rate: progress tracks time one-for-one.
    ///
    /// This is the default because it is the only variant that is both
    /// identity and the neutral element for composition — useful when a
    /// scene file omits an easing and the author expects "no easing".
    #[default]
    Linear,

    /// Slow to start, quadratic: progress accelerates from rest.
    ///
    /// The `t*t` shape gives zero derivative at `t = 0`, so an object
    /// begins at rest and speeds up — the natural feel for a launch.
    InQuad,

    /// Slow to stop, quadratic: progress decelerates into the end.
    ///
    /// The `1 - (1-t)^2` shape gives zero derivative at `t = 1`, so an
    /// object arrives gently rather than slamming into the target.
    OutQuad,

    /// Slow at both ends, quadratic: ease-in then ease-out.
    ///
    /// The piecewise form keeps the curve `C^1` at the midpoint while
    /// remaining a low-degree polynomial on each side, which is cheap
    /// to evaluate on embedded targets.
    InOutQuad,

    /// Slow to start, cubic — more pronounced than [`Self::InQuad`].
    ///
    /// The `t^3` shape has a flatter start than `t^2`, giving a longer
    /// "held" feel before motion becomes visible.
    InCubic,

    /// Slow to stop, cubic.
    ///
    /// The `1 - (1-t)^3` shape has a flatter end than [`Self::OutQuad`], so
    /// the deceleration feels more deliberate.
    OutCubic,

    /// Slow at both ends, cubic.
    ///
    /// The piecewise cubic is the standard "smooth" easing in most
    /// animation toolkits; it is `C^1` at the midpoint and has zero
    /// slope at both endpoints.
    InOutCubic,

    /// Jumps to the end value immediately and stays there.
    ///
    /// This is not a smooth curve but a hard cut: for any `t < 1.0`
    /// the progress is `0.0`, and at `t == 1.0` it snaps to `1.0`.
    /// Useful for toggles and discrete state changes that should not
    /// animate.
    Step,
}

impl Easing {
    /// Map a normalised time `t` to a normalised progress.
    ///
    /// `t` is clamped to `0.0..=1.0` and NaN becomes `0.0`, so the result is
    /// always in range.
    pub fn apply(self, t: f32) -> f32 {
        // NaN has to be caught before the clamp, not by it: `f32::clamp`
        // propagates NaN rather than pinning it to a bound, and a NaN here
        // would interpolate to a NaN value and make a gauge vanish.
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };

        match self {
            Easing::Linear => t,
            Easing::InQuad => t * t,
            Easing::OutQuad => {
                let u = 1.0 - t;
                1.0 - u * u
            }
            Easing::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - u * u / 2.0
                }
            }
            Easing::InCubic => t * t * t,
            Easing::OutCubic => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Easing::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - u * u * u / 2.0
                }
            }
            Easing::Step => {
                if t < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }

    /// Parse the name a scene file uses.
    ///
    /// Accepts exactly: `"linear"`, `"in_quad"`, `"out_quad"`,
    /// `"in_out_quad"`, `"in_cubic"`, `"out_cubic"`, `"in_out_cubic"`,
    /// `"step"`. Returns `None` for anything else, including empty
    /// strings and names with different casing or whitespace.
    ///
    /// The match is exhaustive over the known set; any unrecognised
    /// string falls through to `None` so that a malformed scene file
    /// degrades gracefully rather than panicking.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "linear" => Some(Easing::Linear),
            "in_quad" => Some(Easing::InQuad),
            "out_quad" => Some(Easing::OutQuad),
            "in_out_quad" => Some(Easing::InOutQuad),
            "in_cubic" => Some(Easing::InCubic),
            "out_cubic" => Some(Easing::OutCubic),
            "in_out_cubic" => Some(Easing::InOutCubic),
            "step" => Some(Easing::Step),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Easing] = &[
        Easing::Linear,
        Easing::InQuad,
        Easing::OutQuad,
        Easing::InOutQuad,
        Easing::InCubic,
        Easing::OutCubic,
        Easing::InOutCubic,
        Easing::Step,
    ];

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        // An easing that overshoots its endpoints turns a bar's clamp into the
        // thing that defines its extremes, which hides the bug until some
        // other widget without a clamp uses the same curve.
        for e in ALL {
            assert_eq!(e.apply(0.0), 0.0, "{e:?} at t=0");
            assert_eq!(e.apply(1.0), 1.0, "{e:?} at t=1");
        }
    }

    #[test]
    fn every_curve_stays_within_its_endpoints() {
        for e in ALL {
            for i in 0..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&v), "{e:?} left the unit range: {v}");
            }
        }
    }

    #[test]
    fn every_curve_is_monotonic() {
        for e in ALL {
            let mut prev = e.apply(0.0);
            for i in 1..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!(v >= prev, "{e:?} went backwards at {i}: {prev} -> {v}");
                prev = v;
            }
        }
    }

    #[test]
    fn time_outside_the_unit_range_is_clamped() {
        for e in ALL {
            assert_eq!(e.apply(-5.0), 0.0, "{e:?} below zero");
            assert_eq!(e.apply(5.0), 1.0, "{e:?} above one");
        }
    }

    #[test]
    fn nan_time_is_treated_as_the_start() {
        // NaN fails every comparison, so a naive clamp leaves it NaN and the
        // interpolated value becomes NaN too -- a gauge that vanishes.
        for e in ALL {
            assert_eq!(e.apply(f32::NAN), 0.0, "{e:?} on NaN");
        }
    }

    #[test]
    fn linear_is_the_identity() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((Easing::Linear.apply(t) - t).abs() < 1e-6);
        }
    }

    #[test]
    fn in_curves_start_slower_than_out_curves() {
        // The whole point of having both. At a quarter through, an ease-in has
        // covered less ground than an ease-out.
        assert!(Easing::InQuad.apply(0.25) < Easing::OutQuad.apply(0.25));
        assert!(Easing::InCubic.apply(0.25) < Easing::OutCubic.apply(0.25));
    }

    #[test]
    fn in_out_curves_are_symmetric_about_the_midpoint() {
        for e in [Easing::InOutQuad, Easing::InOutCubic] {
            assert!((e.apply(0.5) - 0.5).abs() < 1e-5, "{e:?} midpoint");
            for i in 0..=50 {
                let t = i as f32 / 100.0;
                let a = e.apply(t);
                let b = 1.0 - e.apply(1.0 - t);
                assert!((a - b).abs() < 1e-4, "{e:?} asymmetric at {t}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn step_holds_the_start_until_the_very_end() {
        assert_eq!(Easing::Step.apply(0.99), 0.0);
        assert_eq!(Easing::Step.apply(1.0), 1.0);
    }

    #[test]
    fn names_round_trip_and_nonsense_is_rejected() {
        for (name, want) in [
            ("linear", Easing::Linear),
            ("in_quad", Easing::InQuad),
            ("out_quad", Easing::OutQuad),
            ("in_out_quad", Easing::InOutQuad),
            ("in_cubic", Easing::InCubic),
            ("out_cubic", Easing::OutCubic),
            ("in_out_cubic", Easing::InOutCubic),
            ("step", Easing::Step),
        ] {
            assert_eq!(Easing::parse(name), Some(want), "{name}");
        }
        for bad in ["", "Linear", "ease", "in-quad", "bounce"] {
            assert_eq!(Easing::parse(bad), None, "{bad:?} should not parse");
        }
    }
}
