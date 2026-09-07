// SPDX-License-Identifier: MIT OR Apache-2.0
//! Decoding the images a scene refers to.
//!
//! Every decoder here takes a `&[u8]` the caller has already fetched. The core
//! crate cannot open a path (rule 1.4), so "load this background" is always
//! two steps: the host reads the bytes, this module turns them into pixels.

pub mod gif;
pub mod qoi;
pub mod table;

pub use table::{AnimTable, Image, ImageTable, Scene};
