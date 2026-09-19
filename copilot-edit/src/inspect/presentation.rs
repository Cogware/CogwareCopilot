// SPDX-License-Identifier: GPL-3.0-only
//! Display names, units and guidance for scene fields.

use copilot::widget::Kind;

/// A field's visible name, explanation and numeric suffix.
pub(crate) fn field(kind: &Kind, key: &str) -> (&'static str, &'static str, &'static str) {
    match key {
        "background" => (
            "Background colour",
            "The colour behind this widget and its children.",
            "",
        ),
        "color" => (
            "Colour",
            "The main drawing colour. Alpha controls opacity.",
            "",
        ),
        "selected" => ("Selected item", "Colour of the highlighted menu item.", ""),
        "editing" => (
            "Item being edited",
            "Colour while a menu item's value is being changed.",
            "",
        ),
        "menu" => (
            "Menu name",
            "The exact name of a menu defined in the rig.",
            "",
        ),
        "text" => (
            "Displayed text",
            "Text drawn by the widget. A live reading or frame-rate measurement can replace it.",
            "",
        ),
        "suffix" => (
            "After the number",
            "Text after the frame rate; include a leading space if wanted.",
            "",
        ),
        "scale" => (
            "Text size",
            "Font magnification: 1 is the original size; whole numbers from 1 to 255.",
            "×",
        ),
        "align" => (
            "Horizontal alignment",
            "Where the text sits across its rectangle.",
            "",
        ),
        "valign" => (
            "Vertical alignment",
            "Where the text sits down its rectangle.",
            "",
        ),
        "image" => (
            "Image asset index",
            "Index in the scene's images list. The first asset is 0; paths stay in the scene text.",
            "",
        ),
        "anim" => (
            "Animation asset index",
            "Index in the scene's animations list. The first asset is 0; paths stay in the scene text.",
            "",
        ),
        "frame" => (
            "Animation frame",
            "Frame index, starting at 0. Pause playback to keep a frame on screen.",
            "",
        ),
        "playing" => (
            "Play animation",
            "Advance frames using the animation's timing.",
            "",
        ),
        "speed" => (
            "Playback speed",
            "1× is normal, 0.5× is half speed, 2× is double; negative plays backwards.",
            "×",
        ),
        "value" => (
            "Reading",
            "Fraction of the scale: 0 is empty, 1 is full. A data binding replaces this value.",
            "",
        ),
        "fill" => (
            "Fill colour",
            "The lit part of a gauge, or the area below a chart.",
            "",
        ),
        "fill_to" => (
            "Fill at full scale",
            "End colour of the lit-cell gradient.",
            "",
        ),
        "track" => (
            "Unlit track colour",
            "Colour of the unfilled part of the gauge.",
            "",
        ),
        "vertical" => (
            "Vertical orientation",
            "Use the height rather than the width of the rectangle.",
            "",
        ),
        "start" => (
            "Start angle",
            "Degrees clockwise from twelve o'clock. Negative angles are allowed.",
            "°",
        ),
        "end" => (
            "End angle",
            "Degrees clockwise from twelve o'clock; below the start runs anticlockwise.",
            "°",
        ),
        "thickness" => (
            "Thickness",
            "Thickness of the ring or digit strokes, in scene pixels.",
            " px",
        ),
        "width" => (
            "Stroke width",
            "Width of the line, needle or tick, in scene pixels.",
            " px",
        ),
        "hub" => (
            "Centre hub radius",
            "Radius of the circle over the needle pivot; 0 hides it.",
            " px",
        ),
        "segments" => (
            "Segments",
            "Number of cells. On an arc, 0 draws a continuous ring.",
            "",
        ),
        "gap" if matches!(kind, Kind::Arc { .. }) => (
            "Segment gap",
            "Angular space between arc segments, in degrees.",
            "°",
        ),
        "gap" => (
            "Segment gap",
            "Space between neighbouring cells, in scene pixels.",
            " px",
        ),
        "cursor" => (
            "Reading marker colour",
            "Picking a colour enables the marker at the current reading.",
            "",
        ),
        "cursor_size" if matches!(kind, Kind::Arc { .. }) => (
            "Marker width",
            "Width of the arc marker in degrees; 0 uses the default.",
            "°",
        ),
        "cursor_size" => (
            "Marker cells",
            "Number of cells covered by the marker, counting back from the reading. 0 and 1 both mark one cell.",
            " cells",
        ),
        "lights" => (
            "Lit cells",
            "Choose accumulated fill, every cell, or only the current cell.",
            "",
        ),
        "ticks" => (
            "Tick count",
            "Number of tick marks along the scale, including its ends.",
            "",
        ),
        "major_every" => (
            "Major tick interval",
            "Make every Nth tick a major tick. 0 draws no major ticks.",
            "",
        ),
        "length" => (
            "Tick length",
            "Length of a minor tick in scene pixels.",
            " px",
        ),
        "major_color" => (
            "Major tick colour",
            "Colour of the longer, major tick marks.",
            "",
        ),
        "ghost" => (
            "Unlit digit colour",
            "Colour of digit strokes that are not lit.",
            "",
        ),
        "digits" => (
            "Digit cells",
            "Reserved character cells; 0 sizes to the text.",
            "",
        ),
        "from" => ("Gradient start", "Colour at the start of the gradient.", ""),
        "to" => ("Gradient end", "Colour at the end of the gradient.", ""),
        "track_shape" => (
            "Unlit cell shape",
            "How much of an unlit cell is painted.",
            "",
        ),
        "height" => (
            "Cell height",
            "Fraction of the cell envelope: 0 to 1. A height binding replaces it.",
            "",
        ),
        "divisions" => (
            "Cell divisions",
            "Split each cell into pieces across its other axis. 0 keeps a solid cell.",
            "",
        ),
        "div_gap" => (
            "Division gap",
            "Space between the pieces of a cell, in scene pixels.",
            " px",
        ),
        "pitch_x" => (
            "Horizontal spacing",
            "Distance between vertical grid lines, in scene pixels.",
            " px",
        ),
        "pitch_y" => (
            "Vertical spacing",
            "Distance between horizontal grid lines, in scene pixels.",
            " px",
        ),
        "level" => (
            "Brightness",
            "Lamp brightness: 0 is off and 1 is fully lit. A binding replaces it.",
            "",
        ),
        "glow" => (
            "Glow strength",
            "Halo strength around the lamp, from 0 to 1.",
            "",
        ),
        "off" => (
            "Unlit lamp colour",
            "Colour shown when the lamp is off.",
            "",
        ),
        "radius" => (
            "Corner radius",
            "Rounded corner radius, in scene pixels.",
            " px",
        ),
        "closed" => (
            "Close the line",
            "Join the last point back to the first.",
            "",
        ),
        "stroke" => ("Line colour", "Colour of the chart's plotted line.", ""),
        "color_to" => ("Line end colour", "Gradient colour at the last point.", ""),
        "history" => (
            "History length",
            "Retained samples. 0 uses the authored values instead of recording readings.",
            " samples",
        ),
        "period_ms" => (
            "Sample interval",
            "Time between chart samples; 0 records every update.",
            " ms",
        ),
        "warn" => (
            "Warning starts at",
            "Fraction of the scale. Above 1 disables this threshold; negative values are allowed.",
            "",
        ),
        "warn_stroke" => (
            "Warning line colour",
            "Chart line colour at or above the warning threshold.",
            "",
        ),
        "warn_fill" => (
            "Warning fill colour",
            "Chart area colour at or above the warning threshold.",
            "",
        ),
        "danger" => (
            "Danger starts at",
            "Fraction of the scale. Above 1 disables this threshold; negative values are allowed.",
            "",
        ),
        "danger_stroke" => (
            "Danger line colour",
            "Chart line colour at or above the danger threshold.",
            "",
        ),
        "danger_fill" => (
            "Danger fill colour",
            "Chart area colour at or above the danger threshold.",
            "",
        ),
        "fade_to" => (
            "Older samples colour",
            "Colour the chart line fades towards for older samples.",
            "",
        ),
        "newest" => (
            "Newest sample at",
            "Which edge of the chart receives new samples.",
            "",
        ),
        "page" => (
            "Initial page index",
            "Child page to show first, starting at 0. Page order is the child order in Widgets.",
            "",
        ),
        "axis" => (
            "Swipe direction",
            "Direction in which pages are arranged and swiped.",
            "",
        ),
        "wrap" => (
            "Loop through pages",
            "Continue from the last page to the first, and vice versa.",
            "",
        ),
        "swipe" => (
            "Allow swipe gestures",
            "Let a finger swipe between pages.",
            "",
        ),
        _ => ("Scene value", "Edit the scene value.", ""),
    }
}

/// A coherent section for a field; the scene's field order is not changed.
pub(crate) fn group(key: &str) -> &'static str {
    match key {
        "start" | "end" | "thickness" | "width" | "hub" | "segments" | "gap" | "cursor_size"
        | "ticks" | "major_every" | "length" | "vertical" | "height" | "divisions" | "div_gap"
        | "pitch_x" | "pitch_y" | "radius" | "closed" => "Shape and layout",
        "background" | "color" | "fill" | "fill_to" | "track" | "cursor" | "lights"
        | "major_color" | "ghost" | "from" | "to" | "track_shape" | "glow" | "off" | "stroke"
        | "color_to" | "fade_to" | "selected" | "editing" => "Appearance",
        "warn" | "warn_stroke" | "warn_fill" | "danger" | "danger_stroke" | "danger_fill" => {
            "Chart thresholds"
        }
        _ => "Content and behaviour",
    }
}

/// A displayed choice; serialisation still uses the original word.
pub(crate) fn choice(word: &str) -> &str {
    match word {
        "center" => "Centre",
        "left" => "Left",
        "right" => "Right",
        "top" => "Top",
        "middle" => "Middle",
        "bottom" => "Bottom",
        "horizontal" => "Horizontal",
        "vertical" => "Vertical",
        "lit" => "Match the lit cell",
        "envelope" => "Full cell profile",
        "box" => "Full rectangle",
        "bar" => "Fill up to the reading",
        "all" => "Light every cell",
        "one" => "Only the current cell",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_units_match_the_kind_instead_of_looking_like_pixels() {
        let source = r#"{"width":10,"height":10,"root":{"type":"segbar","rect":[0,0,10,10]}}"#;
        let tree = copilot::scene::build(&copilot::scene::parse(source).unwrap()).unwrap();
        let node = tree.get(copilot::widget::ROOT).unwrap().children[0];
        let kind = &tree.get(node).unwrap().kind;
        assert_eq!(field(kind, "cursor_size").2, " cells");
        assert!(field(kind, "divisions").1.contains("other axis"));
    }
}
