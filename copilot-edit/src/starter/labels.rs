// SPDX-License-Identifier: GPL-3.0-only
//! Display names stay separate from the stable scene-format vocabulary.

/// A readable widget name; unknown future types retain their original spelling.
pub(crate) fn widget_title(kind: &str) -> &str {
    match kind {
        "panel" => "Panel / group",
        "pager" => "Swipeable pages",
        "menu" => "Settings menu",
        "frame" => "Rectangle outline",
        "label" => "Text label",
        "image" => "Image",
        "anim" => "Animated image",
        "bar" => "Progress bar",
        "segbar" => "Segmented bar",
        "led" => "Indicator light",
        "roundrect" => "Rounded rectangle",
        "arc" => "Arc gauge",
        "needle" => "Gauge needle",
        "scale" => "Dial tick marks",
        "ruler" => "Straight tick marks",
        "line" => "Line / curve",
        "chart" => "Chart",
        "sevenseg" => "Digital readout",
        "gradient" => "Colour gradient",
        "polygon" => "Filled shape",
        "grid" => "Grid lines",
        "fps" => "Frame-rate readout",
        _ => kind,
    }
}

/// Explain the purpose of a widget without requiring scene-format knowledge.
pub(crate) fn widget_hint(kind: &str) -> &str {
    match kind {
        "panel" => "Group widgets and optionally paint a background behind them.",
        "pager" => "Hold several pages and show one at a time, with swipes or touch buttons.",
        "menu" => "Display a settings menu defined in the rig.",
        "frame" => "Draw a one-pixel rectangular border.",
        "label" => "Show fixed text or a formatted gauge reading.",
        "image" => "Display a QOI image loaded by the scene.",
        "anim" => "Play a GIF animation loaded by the scene.",
        "bar" => "Show a reading as a continuous horizontal or vertical fill.",
        "segbar" => "Show a reading using separate illuminated cells.",
        "led" => "Show an indicator's on/off state, with an optional dim unlit colour.",
        "roundrect" => "Draw a background panel with rounded corners.",
        "arc" => "Show a reading as a filled or segmented ring.",
        "needle" => "Show a reading as a rotating pointer.",
        "scale" => "Place tick marks around a dial.",
        "ruler" => "Place tick marks along a straight edge.",
        "line" => "Draw a line whose points you can edit on the canvas.",
        "chart" => "Draw a series of values or a live gauge history.",
        "sevenseg" => "Show digits using the segments of a digital display.",
        "gradient" => "Fill an area with a blend between two colours.",
        "polygon" => "Draw a filled shape whose corner points you can edit.",
        "grid" => "Draw evenly spaced horizontal and vertical lines.",
        "fps" => "Show the renderer's measured frame rate.",
        _ => "Edit this widget's properties or its scene source.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_add_menu_entry_has_a_human_name_and_an_explanation() {
        for kind in crate::menu::KINDS {
            assert_ne!(widget_title(kind), *kind, "{kind}");
            assert!(!widget_hint(kind).is_empty(), "{kind}");
            let source = format!(
                "{{\"width\":320,\"height\":200,\"root\":{}}}",
                crate::starter::starter_widget(kind),
            );
            let document = copilot::scene::parse(&source).expect("template parses");
            copilot::scene::build_scene(&document).expect("template builds");
        }
    }
}
