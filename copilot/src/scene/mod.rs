// SPDX-License-Identifier: GPL-3.0-only
//! Reading a scene from text.
//!
//! The format is JSON with three additions: `//` and `/* */` comments, and
//! trailing commas. JSON's nesting maps directly onto a widget tree, every
//! editor already highlights it, and a future scene editor emits it trivially
//! — but strict JSON forbids comments, which makes a file nobody can annotate.
//! Accepting a superset costs a few lines in [`lex`] and makes the format
//! genuinely hand-editable, which was the requirement.

/// Binding widgets to gauges on the bus.
pub mod bind;
/// Building a widget tree from a parsed document.
pub mod build;
pub mod edit;
pub mod lex;
pub mod locate;
pub mod parse;
pub mod value;
pub mod write;

pub use bind::{Binding, Shape, unit_by_symbol, units_for};
pub use build::{BuildError, build, build_scene};
pub use edit::{append, insert, remove, replace, set, unset};
pub use locate::{Step, locate};
pub use parse::{ParseError, parse};
pub use value::Value;
pub use write::to_string;
