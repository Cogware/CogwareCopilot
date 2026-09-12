// SPDX-License-Identifier: GPL-3.0-only
//! One line of a menu, and the value behind it.

use alloc::string::String;
use alloc::vec::Vec;

/// What an item currently holds.
///
/// Returned by [`Menu::value`](super::Menu::value), which is how a host reads
/// a setting without knowing how it was authored.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    /// A bounded number, in whatever unit the item's label says.
    Number(f32),
    /// Which option of a [`ItemKind::Choice`] is picked, by index.
    Choice(u32),
    /// An action, and whether it is waiting to be taken.
    Action(bool),
}

/// What kind of thing an item changes.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// A number the driver moves between `min` and `max`.
    Number {
        /// The current value, always within `min..=max`.
        value: f32,
        /// The lowest the driver can go.
        min: f32,
        /// The highest the driver can go.
        max: f32,
        /// How far one press moves it.
        step: f32,
    },
    /// One of a fixed list, by index.
    Choice {
        /// Which option is picked, always a valid index into `options`.
        index: u32,
        /// The options, in the order they are offered.
        options: Vec<String>,
    },
    /// Something that happens once when the driver presses centre.
    Action {
        /// Set by a press and cleared by
        /// [`Menu::take_action`](super::Menu::take_action).
        fired: bool,
    },
}

/// One line of a menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What a host looks the item up by, and what the scene file names it.
    pub name: String,
    /// What the driver reads on the screen.
    pub label: String,
    /// What it changes.
    pub kind: ItemKind,
}

impl Item {
    /// A bounded number, clamped into its own range.
    #[must_use]
    pub fn number(
        name: impl Into<String>,
        label: impl Into<String>,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    ) -> Self {
        // Ordered here so that a file naming max below min is a range of one
        // value rather than one no press can ever move.
        let (min, max) = if max < min { (max, min) } else { (min, max) };
        Self {
            name: name.into(),
            label: label.into(),
            kind: ItemKind::Number {
                value: clamp(value, min, max),
                min,
                max,
                step: if step > 0.0 { step } else { 1.0 },
            },
        }
    }

    /// A choice from `options`, starting at `index`.
    ///
    /// An `index` past the end starts at the first option, because a scene
    /// that names a missing default should still open.
    #[must_use]
    pub fn choice(
        name: impl Into<String>,
        label: impl Into<String>,
        index: u32,
        options: Vec<String>,
    ) -> Self {
        let index = if (index as usize) < options.len() {
            index
        } else {
            0
        };
        Self {
            name: name.into(),
            label: label.into(),
            kind: ItemKind::Choice { index, options },
        }
    }

    /// An action, not yet fired.
    #[must_use]
    pub fn action(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: ItemKind::Action { fired: false },
        }
    }

    /// What the item currently holds.
    #[must_use]
    pub fn value(&self) -> Value {
        match &self.kind {
            ItemKind::Number { value, .. } => Value::Number(*value),
            ItemKind::Choice { index, .. } => Value::Choice(*index),
            ItemKind::Action { fired } => Value::Action(*fired),
        }
    }

    /// What the driver reads to the right of the label, if anything.
    #[must_use]
    pub fn shown(&self) -> Option<&str> {
        match &self.kind {
            ItemKind::Choice { index, options } => options.get(*index as usize).map(String::as_str),
            _ => None,
        }
    }

    /// Move a number or a choice by `dir` steps, returning whether it moved.
    pub(super) fn step(&mut self, dir: i32) -> bool {
        match &mut self.kind {
            ItemKind::Number {
                value,
                min,
                max,
                step,
            } => {
                let next = clamp(*value + *step * dir as f32, *min, *max);
                let moved = next != *value;
                *value = next;
                moved
            }
            ItemKind::Choice { index, options } => {
                if options.is_empty() {
                    return false;
                }
                let last = options.len() as i32 - 1;
                let next = (*index as i32 + dir).clamp(0, last) as u32;
                let moved = next != *index;
                *index = next;
                moved
            }
            // An action has nothing to step through; centre is its only press.
            ItemKind::Action { .. } => false,
        }
    }

    /// Fire an action, returning whether this item was one.
    pub(super) fn fire(&mut self) -> bool {
        if let ItemKind::Action { fired } = &mut self.kind {
            *fired = true;
            return true;
        }
        false
    }

    /// Take a pending fire, returning whether one was waiting.
    pub(super) fn take_fired(&mut self) -> bool {
        if let ItemKind::Action { fired } = &mut self.kind {
            return core::mem::replace(fired, false);
        }
        false
    }
}

/// Clamp with NaN going to `min`, which `f32::clamp` panics on instead.
fn clamp(v: f32, min: f32, max: f32) -> f32 {
    if v.is_nan() {
        return min;
    }
    v.clamp(min, max)
}
