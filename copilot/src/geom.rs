//! Geometry primitives for a `no_std` graphics crate.

/// A point in 2D space with integer coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

/// A non-negative size.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Size {
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    /// Top-left corner.
    pub origin: Point,
    /// Dimensions.
    pub size: Size,
}

impl Rect {
    /// The rectangle at the origin with no area.
    ///
    /// Exists so a fixed-size array of `Rect` can be built in a `const fn`;
    /// `Default` is not usable there.
    pub const ZERO: Rect = Rect {
        origin: Point { x: 0, y: 0 },
        size: Size { w: 0, h: 0 },
    };

    /// Whether `other` lies entirely within `self`.
    ///
    /// An empty `other` is contained by anything non-empty: it covers no
    /// pixels, so there is nothing that could fall outside.
    #[must_use]
    pub fn contains_rect(self, other: Rect) -> bool {
        if self.is_empty() {
            return false;
        }
        if other.is_empty() {
            return true;
        }
        other.left() >= self.left()
            && other.top() >= self.top()
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// Pixel count, saturating rather than wrapping.
    ///
    /// A rect wider than `usize` cannot be drawn, but it can be constructed,
    /// and a wrapped area would make an absurd rectangle look cheap to any
    /// caller comparing sizes.
    #[must_use]
    pub fn area(self) -> usize {
        (self.size.w as usize).saturating_mul(self.size.h as usize)
    }

    /// A rectangle from a corner and a size.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect {
            origin: Point { x, y },
            size: Size { w, h },
        }
    }

    /// X-coordinate of the left edge.
    #[must_use]
    pub fn left(self) -> i32 {
        self.origin.x
    }

    /// Y-coordinate of the top edge.
    #[must_use]
    pub fn top(self) -> i32 {
        self.origin.y
    }

    /// X-coordinate of the right edge (exclusive).
    #[must_use]
    pub fn right(self) -> i32 {
        // i64 intermediate prevents overflow when width alone exceeds i32::MAX
        let r = self.origin.x as i64 + self.size.w as i64;
        r.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    /// Y-coordinate of the bottom edge (exclusive).
    #[must_use]
    pub fn bottom(self) -> i32 {
        let b = self.origin.y as i64 + self.size.h as i64;
        b.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    /// Returns true if the rectangle has zero width or height.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.size.w == 0 || self.size.h == 0
    }

    /// Returns true if the point lies strictly inside the rectangle.
    #[must_use]
    pub fn contains(self, p: Point) -> bool {
        if self.is_empty() {
            return false;
        }
        p.x >= self.left() && p.x < self.right() && p.y >= self.top() && p.y < self.bottom()
    }

    /// The overlap, or `None` when they do not touch.
    #[must_use]
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let left = self.left().max(other.left());
        let top = self.top().max(other.top());
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        // Touching edges produce zero-area overlap, which is not an intersection
        if left >= right || top >= bottom {
            return None;
        }

        let w = (right as i64 - left as i64) as u32;
        let h = (bottom as i64 - top as i64) as u32;
        Some(Rect::new(left, top, w, h))
    }

    /// Returns the smallest rectangle that contains both rectangles.
    #[must_use]
    pub fn union(self, other: Rect) -> Rect {
        // An empty rect contributes nothing to the bounding box
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }

        let left = self.left().min(other.left());
        let top = self.top().min(other.top());
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());

        let w = (right as i64 - left as i64) as u32;
        let h = (bottom as i64 - top as i64) as u32;
        Rect::new(left, top, w, h)
    }

    /// Returns true if the two rectangles have a non-empty intersection.
    #[must_use]
    pub fn intersects(self, other: Rect) -> bool {
        self.intersection(other).is_some()
    }

    /// The same rectangle, moved.
    #[must_use]
    pub fn translate(self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.origin.x.saturating_add(dx),
            self.origin.y.saturating_add(dy),
            self.size.w,
            self.size.h,
        )
    }

    /// Shrinks (or grows, if `d` is negative) all sides by `d`.
    /// The result may be empty if the inset exceeds half the dimension.
    #[must_use]
    pub fn inset(self, d: i32) -> Rect {
        let left = self.left().saturating_add(d);
        let top = self.top().saturating_add(d);
        let right = self.right().saturating_sub(d);
        let bottom = self.bottom().saturating_sub(d);

        if right <= left || bottom <= top {
            return Rect::new(left, top, 0, 0);
        }

        let w = (right as i64 - left as i64) as u32;
        let h = (bottom as i64 - top as i64) as u32;
        Rect::new(left, top, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rect_contains_nothing() {
        let r = Rect::new(0, 0, 0, 5);
        assert!(r.is_empty());
        assert!(!r.contains(Point { x: 0, y: 0 }));
    }

    #[test]
    fn negative_coords() {
        let r = Rect::new(-10, -10, 5, 5);
        assert_eq!(r.left(), -10);
        assert_eq!(r.top(), -10);
        assert_eq!(r.right(), -5);
        assert_eq!(r.bottom(), -5);
        assert!(r.contains(Point { x: -10, y: -10 }));
        assert!(r.contains(Point { x: -6, y: -6 }));
        assert!(!r.contains(Point { x: -5, y: -5 }));
    }

    #[test]
    fn non_overlapping_intersection_is_none() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 20, 10, 10);
        assert!(a.intersection(b).is_none());
        assert!(!a.intersects(b));
    }

    #[test]
    fn touching_edges_do_not_intersect() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(10, 0, 10, 10);
        assert!(a.intersection(b).is_none());
        assert!(!a.intersects(b));
    }

    #[test]
    fn union_with_empty_rect() {
        let a = Rect::new(0, 0, 10, 10);
        let empty = Rect::new(5, 5, 0, 0);
        assert_eq!(a.union(empty), a);
        assert_eq!(empty.union(a), a);
    }

    #[test]
    fn overflow_saturation() {
        // A rect whose right edge would exceed i32::MAX saturates
        let r = Rect::new(i32::MAX - 1, 0, u32::MAX, 1);
        assert_eq!(r.right(), i32::MAX);

        // Translating beyond i32 bounds saturates
        let r = Rect::new(i32::MAX - 1, 0, 1, 1);
        let t = r.translate(10, 0);
        assert_eq!(t.left(), i32::MAX);

        // Insetting further than the rect is wide collapses it. The left edge
        // moves *inward* (towards positive), so it cannot saturate downward —
        // what has to hold is that the result reports itself empty rather than
        // wrapping into a huge rectangle.
        let r = Rect::new(i32::MIN, 0, 1, 1);
        let i = r.inset(100);
        assert!(i.is_empty());
        assert_eq!(i.left(), i32::MIN + 100);
    }
}
