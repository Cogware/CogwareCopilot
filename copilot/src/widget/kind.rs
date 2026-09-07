// SPDX-License-Identifier: MIT OR Apache-2.0
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
///
/// Without this a scene file has to place text by hand, computing the pixel
/// width of a string from the font's cell size and halving it. That
/// arithmetic is wrong the moment the text or the scale changes, and it is
/// wrong silently.
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
///
/// A separate enum from [`Align`] rather than one with six variants, because a
/// label needs one of each and a single enum would let a scene ask for "left"
/// on the axis that has no left.
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
        ///
        /// Integer rather than a point size because the font is a bitmap: a
        /// glyph doubled is four crisp pixels per pixel, while a glyph scaled
        /// by 1.7 is a smear. A panel that wants a 90-pixel speedometer reads
        /// far better built from a clean 5x7 at twelve times than from an
        /// interpolated one.
        scale: u8,
        /// Where the text sits across the node's box.
        align: Align,
        /// Where the text sits down the node's box.
        valign: VAlign,
    },
    /// A still image, by its index in the scene's image table.
    ///
    /// An index rather than the pixels, so that two widgets showing the same
    /// background share one decode, and so a `Kind` stays cheap to clone.
    Image {
        /// Index into the scene's image table.
        image: u32,
    },
    /// A playing animation, referenced by the index the scene assigned it.
    ///
    /// The current frame lives here, in the retained tree, rather than in the
    /// decoder. That is what makes playback controllable: pausing is a field,
    /// seeking is an assignment, and a scene reloaded from disk resumes where
    /// the widget says it was rather than at frame zero.
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
    /// A ring filled between two angles: the round gauge every cluster has.
    ///
    /// Angles are degrees clockwise from twelve o'clock, which is how a person
    /// describes a dial. The renderer converts once; the scene file never sees
    /// the brads the trigonometry actually uses.
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
    /// A pointer swinging over a sweep: the analogue gauge's moving part.
    ///
    /// Shares its angle convention with [`Kind::Arc`] so that a needle, a ring
    /// and a [`Kind::Scale`] stacked in one node all agree where a value sits.
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
    ///
    /// Separate from [`Kind::Needle`] rather than a field on it because the
    /// ticks never change once the scene is loaded while the needle moves every
    /// frame; keeping them apart lets the renderer repaint only the pointer.
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
    /// Points are fractions of the node's box rather than pixels, so the same
    /// curve serves a 320-pixel panel and a 1080-pixel one. A cluster designed
    /// once should not need rewriting for the next screen it lands on.
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
    ///
    /// The filled counterpart to [`Kind::Line`], and what a telltale symbol
    /// actually is: an arrow, a warning triangle and a chevron are all three
    /// points and four points, and a scene that can say so needs no built-in
    /// icon set to be kept up to date with what people want to draw.
    Polygon {
        /// Vertices, each `(x, y)` in 0.0..=1.0 from the box's top-left.
        points: Vec<(f32, f32)>,
        /// Fill colour.
        color: Color,
    },
    /// A series plotted across the node, optionally filled beneath.
    ///
    /// Distinct from [`Kind::Line`] because the x positions are implied by the
    /// count rather than given: a rolling window of readings can be pushed and
    /// popped without the scene restating where each one sits.
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
    /// A number in seven-segment cells, the way an instrument shows one.
    ///
    /// Not a [`Kind::Label`] in a squarish font. A magnified bitmap glyph is a
    /// picture of a digit; this is the digit, drawn from the seven bars a real
    /// display has. That is what lets it show its *unlit* segments, which is
    /// most of what makes a panel read as a panel rather than as text.
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
        ///
        /// A soldered display does not gain and lose digits as the number
        /// does. Without this, "9" and "188" are drawn at two different cell
        /// widths in the same box, and a speedometer visibly rearranges
        /// itself every time it crosses a hundred.
        digits: u32,
    },
    /// A fill that ramps from one colour to another across the widget.
    ///
    /// Separate from [`Kind::Panel`] rather than a field on it, for the reason
    /// [`Kind::RoundRect`] is separate: the flat case is the overwhelmingly
    /// common one and it draws with a single span per row.
    Gradient {
        /// The colour at the top, or at the left.
        from: Color,
        /// The colour at the bottom, or at the right.
        to: Color,
        /// Whether the ramp runs down rather than across.
        vertical: bool,
    },
    /// Tick marks along an edge: the straight counterpart to [`Kind::Scale`].
    ///
    /// A bar gauge had no way to be graduated, so a reading could be seen to
    /// move without being read off. Same fields as the round one, minus the
    /// angles it has no use for.
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
    /// A bargraph of discrete cells, the way a vacuum-fluorescent panel shows
    /// a reading.
    ///
    /// Not a refinement of [`Kind::Bar`] but a different instrument. A solid
    /// bar says "about this much"; a row of cells says "this many", and a
    /// driver counts them without looking away from the road.
    ///
    /// The colour bands come from where a cell *sits*, not from the reading,
    /// so a red cell is red whenever it is lit. That is what makes a redline a
    /// redline rather than a colour the whole gauge turns at the last moment.
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
        ///
        /// Bands run from the top of the scale down, which is what a
        /// tachometer, a temperature gauge and a boost gauge all want. A tank
        /// is the exception -- low is the bad end -- and reads better as a
        /// telltale beside the gauge than as an inverted band inside it.
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
        ///
        /// This is what a printed lens does to a real tachometer: the cells
        /// are cut to a power curve, so the lit ones trace the engine's torque
        /// rather than forming a level block.
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
