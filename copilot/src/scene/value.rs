// SPDX-License-Identifier: GPL-3.0-only
//! The parsed form of a scene document.
//!
//! [`Value`] is a plain tree, not a typed scene: widgets are built from it in
//! a second pass. Keeping the two apart means a syntax error and a schema
//! error are reported by different code with different messages, and a scene
//! file can carry keys this version does not understand without failing to
//! parse — which is what makes an editor and a runtime able to disagree about
//! their versions without breaking.

use alloc::string::String;
use alloc::vec::Vec;

/// Where a value came from in the source text.
///
/// Byte offsets rather than line and column, because the consumer is an editor
/// splicing text: it wants `&src[span.start..span.end]`, not a position it has
/// to re-count the file to resolve. Absent when a `Value` was built in memory
/// rather than parsed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset just past the last character.
    pub end: usize,
}

impl Span {
    /// Whether this span covers no text, which is what an in-memory value has.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

/// A node in a parsed scene document.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// The literal `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number that was written without a fraction or exponent.
    ///
    /// Kept separate from [`Value::Float`] because coordinates are integers
    /// and silently routing them through `f64` costs exactness above 2^53 for
    /// no benefit to a layout that is measured in pixels.
    Int(i64),
    /// A number written with a fraction or exponent, or too large for `i64`.
    Float(f64),
    /// A string, with escapes already decoded.
    Str(String),
    /// An ordered list.
    Array(Vec<Value>),
    /// Key/value pairs, in document order.
    ///
    /// A `Vec` rather than a map: scene objects have a handful of keys, order
    /// is worth preserving for diagnostics, and a hash map would mean writing
    /// a hasher this crate does not otherwise need.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The value for `key`, if this is an object containing it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// This value as a string slice, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// This value as an integer.
    ///
    /// A whole-numbered [`Value::Float`] converts, because `4` and `4.0` mean
    /// the same thing to someone hand-editing a coordinate and refusing one of
    /// them would be a needlessly sharp edge.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Int(i) => Some(i),
            // `f64::trunc` lives in `std`, not `core`, so whole-numberedness
            // is tested by round-tripping instead. The `as` cast saturates
            // rather than wrapping, so an out-of-range float fails the
            // comparison and is rejected rather than silently clamped.
            Self::Float(f) if f.is_finite() && (f as i64) as f64 == f => Some(f as i64),
            _ => None,
        }
    }

    /// This value as a float, accepting an integer.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Int(i) => Some(i as f64),
            Self::Float(f) => Some(f),
            _ => None,
        }
    }

    /// This value as a boolean, if it is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }
}
