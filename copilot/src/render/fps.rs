// SPDX-License-Identifier: GPL-3.0-only
//! Measuring the rate the toolkit is drawing at, and putting it in the scene.
//!
//! A frame rate used to be a host's own business: it counted its frames and
//! painted the figure over the top of whatever the scene had drawn, in a
//! corner it picked. That works and it is ugly — the number sits outside the
//! layout, it cannot be moved without changing firmware, and every host that
//! wants one writes it again.
//!
//! So the rate is a widget ([`crate::widget::Kind::Fps`]) placed wherever the
//! scene likes, and this keeps it current. The host's only part is the clock,
//! because this crate does not have one: it calls [`Meter::frame`] once a
//! frame with a millisecond count from whatever the board's timer is.
//!
//! # Measuring something from inside it
//!
//! Drawing the rate costs frames, so a rate widget changes the number it is
//! reporting. That cannot be abolished, only made small, and what makes it
//! small is the window: the text is rewritten when the *displayed* figure
//! changes, which is about twice a second, rather than every frame. In
//! between, the widget is unchanged, marks no damage, and costs the
//! compositor nothing at all.
//!
//! The figure is therefore an average over the window and not an instant
//! rate. That is also the more useful number: an instant rate taken between
//! two frames swings through every value and is unreadable.

use alloc::format;

use crate::widget::{Kind, NodeId, Tree};

/// How much wall time a rate is averaged over, in milliseconds.
///
/// Long enough that the figure is readable rather than flickering through
/// every value, short enough that a scene which suddenly costs more says so
/// while somebody is still looking at it.
pub const WINDOW_MS: u64 = 500;

/// Counts frames and works out a rate over a window.
///
/// Holds no clock of its own: this crate runs on boards whose timers have
/// nothing in common, so the caller brings the time.
#[derive(Clone, Copy, Debug)]
pub struct Meter {
    window_ms: u64,
    /// When the window now being counted began, or `None` before the first
    /// frame — which is what stops the first window being measured from a
    /// zero that was never a real instant.
    opened_ms: Option<u64>,
    frames: u32,
    rate: u16,
}

impl Default for Meter {
    fn default() -> Self {
        Self::new(WINDOW_MS)
    }
}

impl Meter {
    /// A meter averaging over `window_ms`.
    ///
    /// A window of zero would divide by it, so it is taken as one millisecond:
    /// a caller asking for no averaging at all gets the shortest there is
    /// rather than a panic.
    #[must_use]
    pub const fn new(window_ms: u64) -> Self {
        Self {
            window_ms: if window_ms == 0 { 1 } else { window_ms },
            opened_ms: None,
            frames: 0,
            rate: 0,
        }
    }

    /// Count a frame drawn at `now_ms`.
    ///
    /// Returns the new rate on the frame that closes a window, and `None`
    /// otherwise — so a caller can do the work of publishing it only when
    /// there is something new to publish.
    pub fn frame(&mut self, now_ms: u64) -> Option<u16> {
        let opened = *self.opened_ms.get_or_insert(now_ms);
        // A clock that has gone backwards is not worth a rate: start again
        // from here rather than report an enormous one.
        if now_ms < opened {
            self.opened_ms = Some(now_ms);
            self.frames = 1;
            return None;
        }
        let elapsed = now_ms - opened;
        if elapsed < self.window_ms {
            self.frames = self.frames.saturating_add(1);
            return None;
        }
        // Rounded rather than truncated, so a steady sixty reads as sixty
        // instead of fifty-nine.
        let per_second = (u64::from(self.frames) * 1_000 + elapsed / 2) / elapsed;
        self.rate = u16::try_from(per_second).unwrap_or(u16::MAX);
        // This frame opens the next window rather than closing this one. It
        // happened at the instant the window ended, and counting it in both
        // would report one frame more than were drawn, every window.
        self.opened_ms = Some(now_ms);
        self.frames = 1;
        Some(self.rate)
    }

    /// The rate last worked out. Zero until the first window closes.
    #[must_use]
    pub const fn rate(&self) -> u16 {
        self.rate
    }
}

/// Write `rate` into every [`Kind::Fps`] widget in `tree`.
///
/// Marks a widget dirty only where the text it would now show differs from
/// the text it is showing, so a rate that has not moved costs no repaint.
///
/// Returns how many widgets were updated, which is zero for the ordinary
/// scene that has no rate in it — a host can call this unconditionally and
/// pay nothing for a scene that never asked.
pub fn show(tree: &mut Tree, rate: u16) -> usize {
    let mut changed = 0;
    for index in 0..tree.len() {
        let id = NodeId(index as u32);
        // The text is built before anything is written, and the node is let
        // go of first: the suffix is the node's and `set_text` wants the tree.
        let Some(text) = tree.get(id).and_then(|node| match &node.kind {
            Kind::Fps { suffix, .. } => Some(format!("{rate}{suffix}")),
            _ => None,
        }) else {
            continue;
        };
        if tree.get(id).and_then(|n| n.kind.text()) == Some(text.as_str()) {
            continue;
        }
        if tree.set_text(id, &text).is_some() {
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Align, Node, VAlign, ROOT};
    use crate::{Color, Rect};
    use alloc::string::{String, ToString};

    fn meter() -> Meter {
        Meter::new(1_000)
    }

    #[test]
    fn nothing_is_reported_until_a_window_has_closed() {
        let mut m = meter();
        assert_eq!(m.frame(0), None);
        assert_eq!(m.frame(500), None);
        assert_eq!(m.rate(), 0);
    }

    #[test]
    fn a_steady_rate_reads_as_itself() {
        let mut m = meter();
        // Sixty frames spread across the second, the last of them closing it.
        let mut last = None;
        for i in 0..=60u64 {
            last = m.frame(i * 1_000 / 60);
        }
        assert_eq!(last, Some(60), "a steady sixty should read as sixty");
    }

    #[test]
    fn the_rate_is_rounded_rather_than_truncated() {
        // A window as long as the span, so the whole run is one measurement.
        let mut m = Meter::new(1_500);
        // Twenty-nine frames in a second and a half is 19.33 a second.
        let mut last = None;
        for i in 0..=29u64 {
            last = m.frame(i * 1_500 / 29);
        }
        assert_eq!(last, Some(19));
    }

    #[test]
    fn a_rate_just_over_a_half_rounds_up() {
        // Three frames in two seconds is 1.5 a second, which rounds to 2.
        let mut m = Meter::new(2_000);
        let mut last = None;
        for i in 0..=3u64 {
            last = m.frame(i * 2_000 / 3);
        }
        assert_eq!(last, Some(2));
    }

    #[test]
    fn each_window_is_measured_on_its_own() {
        let mut m = meter();
        for i in 0..=10u64 {
            m.frame(i * 100);
        }
        assert_eq!(m.rate(), 10);
        // A slower second after a faster one reports the slower one, not an
        // average of the two.
        for i in 1..=2u64 {
            m.frame(1_000 + i * 500);
        }
        assert_eq!(m.rate(), 2);
    }

    #[test]
    fn a_clock_that_goes_backwards_starts_again_instead_of_reporting_a_fiction() {
        let mut m = meter();
        m.frame(5_000);
        assert_eq!(m.frame(10), None);
        assert_eq!(m.rate(), 0, "no rate should have been invented");
        // And it goes on working from the new reading: four frames counted
        // across the second that follows.
        for i in 1..=4u64 {
            m.frame(10 + i * 250);
        }
        assert_eq!(m.rate(), 4);
    }

    #[test]
    fn a_window_of_zero_is_taken_as_the_shortest_there_is() {
        let mut m = Meter::new(0);
        m.frame(0);
        // Would divide by zero if the window were taken at face value.
        assert!(m.frame(1).is_some());
    }

    fn node(kind: Kind) -> Node {
        Node {
            rect: Rect::new(0, 0, 10, 10),
            kind,
            visible: true,
            antialias: None,
            name: None,
            children: alloc::vec::Vec::new(),
            parent: None,
        }
    }

    fn fps_node(suffix: &str) -> Node {
        node(Kind::Fps {
            text: String::from("--"),
            color: Color::WHITE,
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
            suffix: suffix.to_string(),
        })
    }

    #[test]
    fn the_widget_is_written_with_the_rate_and_its_suffix() {
        let mut tree = Tree::new(Rect::new(0, 0, 64, 64));
        let id = tree.push(ROOT, fps_node(" fps")).expect("root takes a child");
        assert_eq!(show(&mut tree, 58), 1);
        assert_eq!(tree.get(id).and_then(|n| n.kind.text()), Some("58 fps"));
    }

    #[test]
    fn a_rate_that_has_not_moved_costs_no_repaint() {
        let mut tree = Tree::new(Rect::new(0, 0, 64, 64));
        tree.push(ROOT, fps_node(" fps")).expect("root takes a child");
        assert_eq!(show(&mut tree, 58), 1);
        // The point of the window: between changes this must do nothing, or
        // the widget joins the damage set on every frame and the rate it is
        // reporting is a rate it caused.
        assert_eq!(show(&mut tree, 58), 0);
        assert_eq!(show(&mut tree, 59), 1);
    }

    #[test]
    fn an_empty_suffix_leaves_the_bare_figure() {
        let mut tree = Tree::new(Rect::new(0, 0, 64, 64));
        let id = tree.push(ROOT, fps_node("")).expect("root takes a child");
        show(&mut tree, 120);
        assert_eq!(tree.get(id).and_then(|n| n.kind.text()), Some("120"));
    }

    #[test]
    fn a_scene_with_no_rate_in_it_is_untouched() {
        let mut tree = Tree::new(Rect::new(0, 0, 64, 64));
        tree.push(
            ROOT,
            node(Kind::Panel {
                background: Color::WHITE,
            }),
        )
        .expect("root takes a child");
        assert_eq!(show(&mut tree, 60), 0);
    }
}
