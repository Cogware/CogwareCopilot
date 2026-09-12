// SPDX-License-Identifier: GPL-3.0-only
//! What a widget draws.
//!
//! A closed enum rather than a trait, for the reasons in the module above.
//! Each variant carries only what drawing it needs; anything an application
//! wants to associate with a node goes in [`super::Node::name`] and lives on
//! the application's side.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Color;

/// Where a label's text sits in its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Against the left edge.
    #[default]
    Left,
    /// Centred between the edges.
    Center,
    /// Against the right edge.
    Right,
}

/// Where a label's text sits between the top and bottom of its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    /// Against the top edge.
    #[default]
    Top,
    /// Centred between the edges.
    Middle,
    /// Against the bottom edge.
    Bottom,
}
/// The most rectangles [`Kind::reading_damage`] can produce.
///
/// A needle is the widest case: a hub plus one band per radial slice.
pub const MAX_READING_RECTS: usize = 8;

/// The drawable kinds of widget.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Kind {
    /// A solid rectangle. Also the grouping element: a transparent panel draws
    /// nothing but still positions its children.
    Panel {
        /// Fill colour. [`Color::TRANSPARENT`] draws nothing.
        background: Color,
    },
    /// A view onto one of the rig's menus.
    ///
    /// The menu itself lives in the rig, not here, so a setting survives a
    /// mode change; this only says where it is drawn and in what colours.
    Menu {
        /// Which of the rig's menus to show, by its `name`.
        menu: String,
        /// Colour of an item that is neither selected nor being edited.
        color: Color,
        /// Colour of the selected item.
        selected: Color,
        /// Colour of the selected item while its value is being changed.
        editing: Color,
        /// Integer magnification of the font, as [`Kind::Label`] uses.
        scale: u8,
    },
    /// A rectangle outline, one pixel wide, drawn just inside the node.
    Frame {
        /// Outline colour.
        color: Color,
    },
    /// A run of text in the current font.
    Label {
        /// The text to draw.
        text: String,
        /// Text colour.
        color: Color,
        /// Integer magnification, 1 for the font's own size.
        scale: u8,
        /// Where the text sits across the node's box.
        align: Align,
        /// Where the text sits down the node's box.
        valign: VAlign,
    },
    /// A still image, by its index in the scene's image table.
    Image {
        /// Index into the scene's image table.
        image: u32,
    },
    /// A playing animation, referenced by the index the scene assigned it.
    Anim {
        /// Index into the scene's animation table.
        anim: u32,
        /// Frame currently shown.
        frame: u32,
        /// Whether time advances the frame.
        playing: bool,
        /// Playback rate. 1.0 is the file's own timing; 0.5 is half speed.
        /// Negative runs backwards.
        speed: f32,
        /// Microseconds accumulated towards the next frame.
        elapsed_us: u64,
    },
    /// A ring filled between two angles.
    ///
    /// Angles are degrees clockwise from twelve o'clock.
    Arc {
        /// Where the sweep begins, degrees clockwise from the top.
        start: i32,
        /// Where a full sweep would end. May be less than `start` to run anticlockwise.
        end: i32,
        /// Filled fraction of the sweep, 0.0 to 1.0.
        value: f32,
        /// Thickness of the ring in pixels.
        thickness: u32,
        /// Colour of the filled part.
        fill: Color,
        /// Colour of the rest of the sweep. Transparent leaves it unpainted.
        track: Color,
    },
    /// A pointer swinging over a sweep.
    ///
    /// Shares its angle convention with [`Kind::Arc`].
    Needle {
        /// Where the sweep begins, degrees clockwise from the top.
        start: i32,
        /// Where the sweep ends. May be less than `start` to run anticlockwise.
        end: i32,
        /// Position along the sweep, 0.0 to 1.0.
        value: f32,
        /// Needle thickness in pixels.
        width: u32,
        /// Needle colour.
        color: Color,
        /// Radius of the disc covering the pivot. 0 draws no hub.
        hub: u32,
    },
    /// Tick marks around a dial's rim.
    Scale {
        /// Where the sweep begins, degrees clockwise from the top.
        start: i32,
        /// Where the sweep ends.
        end: i32,
        /// How many marks, counting both ends.
        ticks: u32,
        /// Every Nth mark, counting from the first, is drawn major. 0 for none.
        major_every: u32,
        /// Minor tick length in pixels, measured inwards from the rim.
        length: u32,
        /// Minor tick thickness.
        width: u32,
        /// Minor tick colour.
        color: Color,
        /// Major tick colour.
        major_color: Color,
    },
    /// A run of connected line segments through listed points.
    ///
    /// Points are fractions of the node's box.
    Line {
        /// Vertices, each `(x, y)` in 0.0..=1.0 from the box's top-left.
        points: Vec<(f32, f32)>,
        /// Stroke thickness in pixels.
        width: u32,
        /// Stroke colour.
        color: Color,
        /// Whether to join the last point back to the first.
        closed: bool,
    },
    /// A filled shape through listed points.
    Polygon {
        /// Vertices, each `(x, y)` in 0.0..=1.0 from the box's top-left.
        points: Vec<(f32, f32)>,
        /// Fill colour.
        color: Color,
    },
    /// A series plotted across the node, optionally filled beneath.
    Chart {
        /// The series, each 0.0 on the bottom edge to 1.0 on the top.
        values: Vec<f32>,
        /// Thickness of the line along the top of the series.
        width: u32,
        /// Colour of that line. Transparent leaves only the fill.
        stroke: Color,
        /// Colour of the area beneath it. Transparent leaves only the line.
        fill: Color,
    },
    /// A number in seven-segment cells.
    SevenSeg {
        /// The characters to show. Digits, a minus and a space are the ones
        /// with a shape; anything else is blank.
        text: String,
        /// A segment that is lit.
        color: Color,
        /// A segment that is not. Transparent draws nothing.
        ghost: Color,
        /// Segment thickness in pixels.
        thickness: u32,
        /// How many cells the readout has, whatever the text is. 0 sizes the
        /// field to the text.
        digits: u32,
    },
    /// A fill that ramps from one colour to another across the widget.
    Gradient {
        /// The colour at the top, or at the left.
        from: Color,
        /// The colour at the bottom, or at the right.
        to: Color,
        /// Whether the ramp runs down rather than across.
        vertical: bool,
    },
    /// Tick marks along an edge.
    Ruler {
        /// How many marks, counting both ends.
        ticks: u32,
        /// Every Nth mark, counting from the first, is drawn major. 0 for none.
        major_every: u32,
        /// Minor tick length in pixels, measured in from the near edge.
        length: u32,
        /// Minor tick thickness.
        width: u32,
        /// Minor tick colour.
        color: Color,
        /// Major tick colour.
        major_color: Color,
        /// Whether the marks run down the left edge rather than across the top.
        vertical: bool,
    },
    /// A bargraph of discrete cells.
    ///
    /// Colour bands are determined by cell position, not the reading.
    SegBar {
        /// How far the reading has got, 0.0 to 1.0.
        value: f32,
        /// How many cells.
        segments: u32,
        /// Pixels of dark between cells. Never against the outside of the box.
        gap: u32,
        /// A lit cell below `warn`.
        fill: Color,
        /// A cell that is not lit. Transparent leaves it unpainted.
        track: Color,
        /// Cells at or past this fraction light in `warn_fill`. Above 1.0
        /// there is no warning band at all.
        warn: f32,
        /// The colour those cells light in.
        warn_fill: Color,
        /// Cells at or past this fraction light in `danger_fill`.
        danger: f32,
        /// The colour those cells light in.
        danger_fill: Color,
        /// Whether cells stack upwards rather than running rightwards.
        vertical: bool,
        /// How far across each cell reaches, 0.0 to 1.0, cut from the base
        /// edge. Empty for a plain bargraph whose cells fill their box.
        profile: Vec<f32>,
        /// How far up its envelope each lit cell reaches, 0.0 to 1.0.
        ///
        /// The second axis, and what makes this a bank rather than a bar.
        /// `value` decides how many cells light along the scale; this decides
        /// how tall the lit ones stand within the shape `profile` cuts for
        /// them. On the instrument being replicated the columns light with
        /// the revs and their height is boost, so the top row is full boost.
        /// 1.0 -- the default -- is every lit cell at its full envelope,
        /// which is a plain segmented bar and what every scene written before
        /// the field existed draws.
        height: f32,
        /// How many pieces each cell is cut into across its width. 0 leaves
        /// the cells solid.
        ///
        /// A vacuum-fluorescent panel's columns are stacks of short dashes,
        /// not solid bars, and the dashes line up right along the bank. The
        /// bands are measured across the whole widget for exactly that reason:
        /// a common grid is what makes it read as one display rather than as a
        /// row of unrelated gauges.
        divisions: u32,
        /// Pixels of dark between those pieces.
        div_gap: u32,
    },
    /// A ruled grid, the way a panel's glass is.
    ///
    /// Not a drawing aid. The instrument being replicated has a fine grid
    /// etched across its face, and at night it is one of the things that makes
    /// the panel look like itself rather than like a screen showing a picture
    /// of it.
    Grid {
        /// Spacing of the vertical lines. 0 leaves that axis unruled.
        pitch_x: u32,
        /// Spacing of the horizontal lines. 0 leaves that axis unruled.
        pitch_y: u32,
        /// Line thickness.
        width: u32,
        /// Line colour.
        color: Color,
    },
    /// A telltale lamp: on, off, or somewhere between.
    ///
    /// Distinct from a [`Kind::Bar`] at full value because a lamp is not a
    /// measurement. It has one colour and a brightness, and an unlit one still
    /// shows faintly -- which is what makes a dark panel readable as a row of
    /// lamps rather than as empty space.
    Led {
        /// The lit colour.
        color: Color,
        /// Brightness, 0.0 dark to 1.0 full. Values between dim the colour.
        level: f32,
        /// How visible an unlit lamp is, 0.0 invisible to 1.0 fully lit.
        ///
        /// A real cluster's unlit telltales still catch the light. Drawing
        /// nothing leaves a hole where the driver expects a symbol, so they
        /// know something is there before it comes on.
        glow: f32,
    },
    /// A rectangle with rounded corners.
    ///
    /// Separate from [`Kind::Panel`] rather than a field on it because the
    /// square case is the overwhelmingly common one and it draws with a single
    /// span per row; branching on a radius that is almost always zero would
    /// put a test in the hottest loop the renderer has.
    RoundRect {
        /// Fill colour.
        background: Color,
        /// Corner radius in pixels, clamped to half the shorter side.
        radius: u32,
    },
    /// A horizontal or vertical bar filled to some fraction, the primitive
    /// every gauge in a cluster is built from.
    Bar {
        /// Filled portion, clamped to 0.0..=1.0 when drawn.
        value: f32,
        /// Colour of the filled part.
        fill: Color,
        /// Colour of the unfilled remainder.
        track: Color,
        /// Whether the bar grows upwards rather than rightwards.
        vertical: bool,
    },
}

/// Which side of a widget a bound gauge lands on.
///
/// Decided by the widget's kind rather than by the scene file, so a binding
/// cannot ask a bar for text or a label for a fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindTarget {
    /// A 0.0..=1.0 fraction: a bar's fill, a needle's position, a lamp's level.
    Reading,
    /// A string: a label or a seven-segment readout.
    Text,
    /// A [`Kind::SegBar`]'s second axis: how tall its lit cells stand.
    Height,
}

impl BindTarget {
    /// The name a scene file's `bind.property` uses.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Reading => "value",
            Self::Text => "text",
            Self::Height => "height",
        }
    }

    /// Parse that name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "value" => Some(Self::Reading),
            "text" => Some(Self::Text),
            "height" => Some(Self::Height),
            _ => None,
        }
    }
}

impl Kind {
    /// The fraction this kind displays, if it has one: `value` of a bar, arc,
    /// needle or segbar, `level` of a led. `None` for everything else.
    #[must_use]
    pub fn reading(&self) -> Option<f32> {
        match self {
            Kind::Bar { value, .. }
            | Kind::Arc { value, .. }
            | Kind::Needle { value, .. }
            | Kind::SegBar { value, .. } => Some(*value),
            Kind::Led { level, .. } => Some(*level),
            _ => None,
        }
    }

    /// The parts of the screen that can differ when a reading moves from
    /// `old` to `new`, written into `out` and counted by the return value.
    ///
    /// Defaults to the widget's whole rectangle, which is always correct. For
    /// the two widgets that occupy a whole dial face while changing only a
    /// sliver of it -- the arc and the needle -- it is the swept wedge
    /// instead, which is the difference between repainting a gauge and
    /// repainting a sliver of one.
    ///
    /// # Why the needle comes back in pieces
    ///
    /// A needle is a spoke. Sweeping it two degrees changes a long thin
    /// triangle from the pivot to the rim, and the *bounding box* of that
    /// triangle is very nearly the quadrant it points into -- 16,772 pixels
    /// on a 480x480 gauge where the arc's own wedge is 310. One box would
    /// hand back most of what the wedge was computed to save, so the sweep is
    /// cut into radial bands and each is bounded on its own. The bands trace
    /// the triangle instead of boxing it.
    ///
    /// Deliberately conservative throughout: every box is padded for the
    /// rounding in the rasteriser's fixed-point trigonometry and for the
    /// stroke's half-width, and any kind this cannot reason about falls back
    /// to the full rectangle.
    #[must_use]
    pub fn reading_damage(
        &self,
        absolute: crate::Rect,
        old: f32,
        new: f32,
        out: &mut [crate::Rect; MAX_READING_RECTS],
    ) -> usize {
        use crate::geom::sector_bounds;
        use crate::trig::TURN;

        let clamp01 = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        let (old, new) = (clamp01(old), clamp01(new));
        let w = absolute.size.w as i32;
        let h = absolute.size.h as i32;
        let radius = w.min(h) / 2;
        let whole = |out: &mut [crate::Rect; MAX_READING_RECTS]| {
            out[0] = absolute;
            1
        };
        if radius <= 0 {
            return whole(out);
        }
        let cx = absolute.left() + w / 2;
        let cy = absolute.top() + h / 2;
        // Degrees clockwise from twelve o'clock into the renderer's brads,
        // which put zero at three. Same conversion as `render::draw`, and it
        // has to stay the same or the wedge misses what was drawn.
        let to_brad = |deg: i32| {
            ((deg as i64 * TURN as i64) / 360 - i64::from(TURN / 4))
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };

        // Padded and never clamped to `absolute`. A stroke is centred on its
        // geometry, so a needle five pixels wide reaches two and a half
        // pixels past the rectangle whose edge its tip sits on. Clamping here
        // left a column of the needle unrepainted at full deflection, which
        // whole-rectangle damage had covered only by accident, because the
        // arc behind it was marking the entire face. Marking more than
        // changed is merely slower; marking less leaves a stale pixel.
        let grow = |r: crate::Rect, pad: i32| {
            crate::Rect::new(
                r.left() - pad,
                r.top() - pad,
                r.size.w + 2 * pad as u32,
                r.size.h + 2 * pad as u32,
            )
        };

        match *self {
            Kind::Arc {
                start,
                end,
                thickness,
                ..
            } => {
                let a0 = to_brad(start);
                let sweep = to_brad(end).saturating_sub(a0);
                let angle = |v: f32| a0.saturating_add((sweep as f32 * v) as i32);
                let inner = (radius - thickness.max(1) as i32).max(0);
                out[0] = grow(
                    sector_bounds(cx, cy, inner, radius, angle(old), angle(new)),
                    2,
                );
                1
            }
            Kind::Needle {
                start,
                end,
                width,
                hub,
                ..
            } => {
                let a0 = to_brad(start);
                let sweep = to_brad(end).saturating_sub(a0);
                let angle = |v: f32| a0.saturating_add((sweep as f32 * v) as i32);
                let (a, b) = (angle(old), angle(new));
                let pad = width.max(1) as i32 / 2 + 2;

                let mut n = 0;
                // The pivot, where the hub sits and every sweep overlaps.
                if hub > 0 {
                    let r = hub as i32 + 1;
                    out[n] = grow(
                        crate::Rect::new(cx - r, cy - r, (r * 2 + 1) as u32, (r * 2 + 1) as u32),
                        pad,
                    );
                    n += 1;
                }
                // The spoke, in bands. Four is enough to follow the triangle
                // closely without the per-rectangle cost of the compositor
                // walking the tree again outweighing the pixels saved.
                const BANDS: i32 = 4;
                for k in 0..BANDS {
                    let lo = radius * k / BANDS;
                    let hi = radius * (k + 1) / BANDS;
                    out[n] = grow(sector_bounds(cx, cy, lo, hi, a, b), pad);
                    n += 1;
                }
                n
            }
            _ => whole(out),
        }
    }

    /// This kind with its reading replaced by `v`, or `None` if it has none.
    ///
    /// A copy rather than a mutation because the tree owns its nodes and marks
    /// damage when one is replaced; see [`super::Tree::set_reading`].
    #[must_use]
    pub fn with_reading(&self, v: f32) -> Option<Kind> {
        let mut k = self.clone();
        match &mut k {
            Kind::Bar { value, .. }
            | Kind::Arc { value, .. }
            | Kind::Needle { value, .. }
            | Kind::SegBar { value, .. } => *value = v,
            Kind::Led { level, .. } => *level = v,
            _ => return None,
        }
        Some(k)
    }

    /// The second axis this kind displays, if it has one. Only a segbar does.
    #[must_use]
    pub fn height(&self) -> Option<f32> {
        match self {
            Kind::SegBar { height, .. } => Some(*height),
            _ => None,
        }
    }

    /// This kind with its second axis replaced, or `None` if it has none.
    #[must_use]
    pub fn with_height(&self, v: f32) -> Option<Kind> {
        let mut k = self.clone();
        match &mut k {
            Kind::SegBar { height, .. } => *height = v,
            _ => return None,
        }
        Some(k)
    }

    /// Whether this kind can take a binding aimed at `target`.
    #[must_use]
    pub fn accepts(&self, target: BindTarget) -> bool {
        match target {
            BindTarget::Reading => self.reading().is_some(),
            BindTarget::Text => self.text().is_some(),
            BindTarget::Height => self.height().is_some(),
        }
    }

    /// The text this kind shows, if it shows any: a label or a readout.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Kind::Label { text, .. } | Kind::SevenSeg { text, .. } => Some(text.as_str()),
            _ => None,
        }
    }

    /// This kind with its text replaced by `t`, or `None` if it shows none.
    #[must_use]
    pub fn with_text(&self, t: &str) -> Option<Kind> {
        let mut k = self.clone();
        match &mut k {
            Kind::Label { text, .. } | Kind::SevenSeg { text, .. } => *text = t.into(),
            _ => return None,
        }
        Some(k)
    }

    /// Where a bound gauge's value would land on this kind, or `None` for a
    /// kind that shows neither a reading nor text.
    #[must_use]
    pub fn bind_target(&self) -> Option<BindTarget> {
        if self.reading().is_some() {
            Some(BindTarget::Reading)
        } else if self.text().is_some() {
            Some(BindTarget::Text)
        } else {
            None
        }
    }
}
