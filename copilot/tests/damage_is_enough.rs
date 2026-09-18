// SPDX-License-Identifier: GPL-3.0-only
//! Damage has to be enough to repaint what actually changed.
//!
//! The pipeline every display here runs is: mark what moved, then repaint only
//! that. It is only correct if the second step reproduces what a full repaint
//! would have drawn -- so this renders a widget incrementally and whole, and
//! compares every pixel.

use copilot::asset::{AnimTable, ImageTable};
use copilot::font::default_font;
use copilot::render::{Resources, compose, compose_all};
use copilot::widget::{Kind, Lights, Node, ROOT, Tree};
use copilot::{Color, MemorySurface, PixelFormat, Rect, Size};

const W: u32 = 120;
const H: u32 = 120;

fn surface() -> MemorySurface {
    MemorySurface::new(Size { w: W, h: H }, PixelFormat::Bgrx8888)
}

fn res<'a>(
    i: &'a ImageTable,
    a: &'a AnimTable,
    f: &'a copilot::font::Font<'a>,
) -> Resources<'a> {
    Resources { images: i, anims: a, font: f, menus: &[] }
}

/// A dark panel with one segmented dial on it, as the gauge scene has.
fn dial(segments: u32) -> (Tree, copilot::widget::NodeId) {
    let mut tree = Tree::new(Rect::new(0, 0, W, H));
    let panel = tree
        .push(
            ROOT,
            Node {
                rect: Rect::new(0, 0, W, H),
                kind: Kind::Panel { background: Color::rgb(5, 7, 10) },
                visible: true,
                antialias: None,
                name: None,
                children: alloc_vec(),
                parent: None,
            },
        )
        .expect("panel");
    let arc = tree
        .push(
            panel,
            Node {
                rect: Rect::new(10, 10, 100, 100),
                kind: Kind::Arc {
                    start: 225,
                    end: 495,
                    value: 0.0,
                    thickness: 10,
                    fill: Color::rgb(0, 200, 57),
                    track: Color::rgb(0, 62, 11),
                    zones: Vec::new(),
                    segments,
                    gap: 2,
                    cursor: Some(Color::rgb(207, 251, 0)),
                    cursor_size: 0,
                    lights: Lights::Bar,
                },
                visible: true,
                antialias: Some(true),
                name: None,
                children: alloc_vec(),
                parent: None,
            },
        )
        .expect("arc");
    (tree, arc)
}

fn alloc_vec() -> Vec<copilot::widget::NodeId> {
    Vec::new()
}

/// Walk a reading across the dial, repainting only what damage names, and
/// check the glass matches a full repaint at every step.
fn walk(segments: u32) {
    let (images, anims, font) = (ImageTable::new(), AnimTable::new(), default_font());
    let r = res(&images, &anims, &font);

    let (mut tree, arc) = dial(segments);
    let mut incremental = surface();
    tree.set_reading(arc, 0.0);
    compose_all(&mut incremental, &tree, r);
    tree.clear_damage();

    // Small steps, because the bug being hunted is a segment whose state
    // changed without the damage covering it -- which happens at a boundary,
    // and a big jump would step over the boundaries rather than onto them.
    let steps = 200;
    for i in 1..=steps {
        let value = i as f32 / steps as f32;
        tree.set_reading(arc, value);
        compose(&mut incremental, &tree, tree.damage(), r);
        tree.clear_damage();

        let mut whole = surface();
        compose_all(&mut whole, &tree, r);

        for y in 0..H as usize {
            for x in 0..W as usize {
                let o = y * incremental.stride() + x * 4;
                assert_eq!(
                    incremental.pixels()[o..o + 4],
                    whole.pixels()[o..o + 4],
                    "{segments} segments, reading {value}: pixel {x},{y} was left \
                     showing the old dial"
                );
            }
        }
    }
}

#[test]
fn a_segmented_dial_repaints_every_segment_whose_state_changed() {
    walk(30);
}

/// The same walk for any kind that carries a reading.
fn walk_kind(what: &str, kind: Kind) {
    let (images, anims, font) = (ImageTable::new(), AnimTable::new(), default_font());
    let r = res(&images, &anims, &font);

    let mut tree = Tree::new(Rect::new(0, 0, W, H));
    let panel = tree
        .push(
            ROOT,
            Node {
                rect: Rect::new(0, 0, W, H),
                kind: Kind::Panel { background: Color::rgb(5, 7, 10) },
                visible: true,
                antialias: None,
                name: None,
                children: Vec::new(),
                parent: None,
            },
        )
        .expect("panel");
    let id = tree
        .push(
            panel,
            Node {
                rect: Rect::new(10, 10, 100, 100),
                kind,
                visible: true,
                antialias: Some(true),
                name: None,
                children: Vec::new(),
                parent: None,
            },
        )
        .expect("widget");

    let mut incremental = surface();
    tree.set_reading(id, 0.0);
    compose_all(&mut incremental, &tree, r);
    tree.clear_damage();

    let steps = 200;
    for i in 1..=steps {
        let value = i as f32 / steps as f32;
        tree.set_reading(id, value);
        compose(&mut incremental, &tree, tree.damage(), r);
        tree.clear_damage();

        let mut whole = surface();
        compose_all(&mut whole, &tree, r);
        for y in 0..H as usize {
            for x in 0..W as usize {
                let o = y * incremental.stride() + x * 4;
                assert_eq!(
                    incremental.pixels()[o..o + 4],
                    whole.pixels()[o..o + 4],
                    "{what} at {value}: pixel {x},{y} was left showing the old reading"
                );
            }
        }
    }
}

#[test]
fn a_needle_repaints_everywhere_it_has_swept() {
    walk_kind(
        "needle",
        Kind::Needle {
            start: 225,
            end: 495,
            value: 0.0,
            width: 6,
            hub: 8,
            color: Color::rgb(220, 60, 60),
        },
    );
}

#[test]
fn a_bank_of_cells_repaints_every_cell_that_changed() {
    walk_kind(
        "segbar",
        Kind::SegBar {
            value: 0.0,
            segments: 16,
            gap: 2,
            fill: Color::rgb(0, 200, 57),
            fill_to: Color::rgb(0, 200, 57),
            track: Color::rgb(0, 62, 11),
            track_shape: copilot::widget::TrackShape::Lit,
            zones: Vec::new(),
            cursor: Some(Color::rgb(207, 251, 0)),
            cursor_size: 0,
            lights: Lights::Bar,
            vertical: false,
            profile: Vec::new(),
            height: 1.0,
            divisions: 0,
            div_gap: 0,
        },
    );
}

#[test]
fn a_smooth_dial_repaints_everything_that_moved() {
    walk(0);
}
