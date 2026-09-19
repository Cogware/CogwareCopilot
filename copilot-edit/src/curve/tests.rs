// SPDX-License-Identifier: GPL-3.0-only
//! Coordinate conversion and point ordering for the supported curve kinds.

use super::*;

#[test]
fn adding_near_a_long_segments_end_does_not_pick_the_next_short_leg() {
    let points = [(0.0, 0.0), (1.0, 0.0), (1.0, 0.1)];
    assert_eq!(insert_at(&points, (0.9, 0.01), Shape::Points), 1);
    assert_eq!(
        insert_at(&[(0.5, 0.5), (0.5, 0.5)], (0.0, 0.0), Shape::Points),
        1
    );
}

#[test]
fn an_envelope_round_trips_through_the_screen_and_back() {
    // A horizontal bank's profile is cut from the bottom, so a value of
    // 1.0 is at the top of the box and 0.0 at the bottom.
    let kind = Kind::SegBar {
        value: 0.0,
        segments: 4,
        gap: 1,
        fill: copilot::Color::WHITE,
        fill_to: copilot::Color::WHITE,
        track: copilot::Color::BLACK,
        track_shape: copilot::widget::TrackShape::Lit,
        cursor: None,
        cursor_size: 0,
        lights: copilot::widget::Lights::Bar,
        zones: Vec::new(),
        vertical: false,
        profile: vec![0.25, 1.0],
        height: 1.0,
        divisions: 0,
        div_gap: 1,
    };
    let shape = Shape::Envelope { vertical: false };
    let pts = handles(&kind, shape);
    assert_eq!(pts.len(), 2);
    assert!((pts[0].1 - 0.75).abs() < 1e-6, "0.25 up is 0.75 down");
    assert!(pts[0].0 < pts[1].0, "the handles run along the bank");
    assert_eq!(to_scene(&pts, shape), "[0.25, 1]");
}

#[test]
fn a_vertical_bank_measures_across_the_other_way() {
    let shape = Shape::Envelope { vertical: true };
    // Value along x, position up the screen: the first cell is at the
    // bottom, which is the largest y.
    let pts = vec![(0.5, 0.75), (1.0, 0.25)];
    assert_eq!(to_scene(&pts, shape), "[0.5, 1]");
}

#[test]
fn points_are_written_as_pairs_and_a_series_as_heights() {
    assert_eq!(
        to_scene(&[(0.0, 1.0), (0.5, 0.0)], Shape::Points),
        "[[0, 1], [0.5, 0]]"
    );
    assert_eq!(
        to_scene(&[(0.0, 0.75), (1.0, 0.0)], Shape::Series),
        "[0.25, 1]"
    );
}

#[test]
fn a_new_point_lands_in_order_along_the_curve() {
    let row = vec![(0.0, 0.5), (0.5, 0.5), (1.0, 0.5)];
    let e = Shape::Envelope { vertical: false };
    assert_eq!(insert_at(&row, (0.25, 0.1), e), 1);
    assert_eq!(insert_at(&row, (0.75, 0.1), e), 2);
    assert_eq!(insert_at(&row, (2.0, 0.1), e), 3, "past the end goes last");
    assert_eq!(insert_at(&[], (0.5, 0.5), e), 0);
}

#[test]
fn a_new_point_on_a_shape_lands_on_the_leg_it_was_aimed_at() {
    // A triangle: a click near the middle of the second leg belongs
    // between its two ends, not at the end of the row.
    let tri = vec![(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)];
    assert_eq!(insert_at(&tri, (0.75, 0.5), Shape::Points), 2);
    assert_eq!(insert_at(&tri, (0.25, 0.5), Shape::Points), 1);
}

#[test]
fn every_curve_bearing_kind_offers_one_and_others_do_not() {
    let line = Kind::Line {
        points: vec![],
        width: 1,
        color: copilot::Color::WHITE,
        color_to: copilot::Color::WHITE,
        closed: false,
    };
    assert_eq!(Shape::of(&line).map(|(_, f)| f), Some("points"));
    assert_eq!(
        Shape::of(&Kind::Panel {
            background: copilot::Color::WHITE
        }),
        None
    );
}
