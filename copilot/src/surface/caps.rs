// SPDX-License-Identifier: GPL-3.0-only
//! What a backend can do that the renderer may not otherwise assume.
//!
//! Only [`Caps::RETAINS_CONTENT`] changes what the renderer does:
//! [`crate::render::frame()`] branches on it to choose between repainting the
//! damage and repainting the screen. The other two are declarations for the
//! host to read. [`Caps::NONE`] is the conservative answer to all three.

use core::fmt;
use core::ops::{BitOr, BitOrAssign};

/// The set of things a [`Surface`](super::Surface) can do.
///
/// A bit set rather than a struct of `bool`s so that adding one later is not a
/// breaking change.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Caps(u32);

impl Caps {
    /// No capability at all: the safe assumption about an unknown backend.
    pub const NONE: Self = Self(0);

    /// Pixels already on the surface can be read back.
    ///
    /// Set this when [`Surface::row_mut`] returns rows, or when
    /// [`Surface::blend_span`] genuinely composites. It reports that state
    /// rather than causing it: `blend_span` finds out for itself by asking.
    ///
    /// [`Surface::row_mut`]: super::Surface::row_mut
    /// [`Surface::blend_span`]: super::Surface::blend_span
    pub const READ_BACK: Self = Self(1 << 0);

    /// What was presented last frame is still there at the start of this one.
    ///
    /// It is a claim about the buffer being drawn into, not the one on the
    /// glass: a page flip hands back a buffer two frames old, and claiming
    /// this falsely leaves that frame's pixels wherever nothing moved.
    pub const RETAINS_CONTENT: Self = Self(1 << 1);

    /// A second full-size buffer is available through
    /// [`Surface::with_scratch`].
    ///
    /// Only a cross-fade needs it, and only a backend that has the memory to
    /// spare should claim it. Without it a cross-fade cuts instead.
    ///
    /// [`Surface::with_scratch`]: super::Surface::with_scratch
    pub const SCRATCH: Self = Self(1 << 3);

    /// Drawing is done by hardware, so a whole rectangle is cheaper than the
    /// spans it decomposes into.
    ///
    /// Nothing in the renderer branches on it; it is here so a host can report
    /// which backend it got.
    pub const ACCELERATED: Self = Self(1 << 2);

    /// Whether every capability in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets together.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for Caps {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOrAssign for Caps {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl fmt::Debug for Caps {
    /// Named rather than numeric, because this is a value that turns up in
    /// assertion messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [(Caps, &str); 4] = [
            (Caps::READ_BACK, "READ_BACK"),
            (Caps::RETAINS_CONTENT, "RETAINS_CONTENT"),
            (Caps::ACCELERATED, "ACCELERATED"),
            (Caps::SCRATCH, "SCRATCH"),
        ];
        if self.0 == 0 {
            return f.write_str("NONE");
        }
        let mut first = true;
        for (cap, name) in NAMES {
            if self.contains(cap) {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_contains_nothing_and_is_contained_by_everything() {
        assert!(!Caps::NONE.contains(Caps::READ_BACK));
        assert!(Caps::NONE.contains(Caps::NONE));
        assert!(Caps::READ_BACK.contains(Caps::NONE));
    }

    #[test]
    fn contains_wants_all_of_the_bits_not_any() {
        // The distinction that matters: `frame` asks for RETAINS_CONTENT and
        // must not be satisfied by a backend that only reads back.
        let one = Caps::READ_BACK;
        let both = Caps::READ_BACK | Caps::RETAINS_CONTENT;
        assert!(both.contains(one));
        assert!(!one.contains(both));
    }

    #[test]
    fn union_is_the_or_of_the_bits() {
        let mut c = Caps::NONE;
        c |= Caps::ACCELERATED;
        assert_eq!(c, Caps::ACCELERATED);
        assert_eq!(Caps::READ_BACK | Caps::READ_BACK, Caps::READ_BACK);
    }

    #[test]
    fn the_bits_are_distinct() {
        let all = [
            Caps::READ_BACK,
            Caps::RETAINS_CONTENT,
            Caps::ACCELERATED,
            Caps::SCRATCH,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(!a.contains(*b), "{a:?} and {b:?} share a bit");
            }
        }
    }
}
