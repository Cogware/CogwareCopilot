// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end: the example scene file, through every stage, to pixels.
//!
//! This is the v0.1 milestone expressed as a test. It runs headless, so it
//! catches a regression in CI that the simulator would only reveal to someone
//! looking at a window.

use copilot::asset::{AnimTable, ImageTable};
use copilot::font::default_font;
use copilot::render::{Resources, compose_all};
use copilot::widget::ROOT;
use copilot::{Color, MemorySurface, PixelFormat, Size, Surface};

/// The same file the simulator opens, compiled in so the test cannot drift
/// from the example anyone is told to run.
const SCENE: &str = include_str!("../../examples/cluster.scene");

fn render() -> MemorySurface {
    let doc = copilot::scene::parse(SCENE).expect("the example scene must parse");
    let tree = copilot::scene::build(&doc).expect("the example scene must build");
    let bounds = tree.get(ROOT).unwrap().rect;
    let mut s = MemorySurface::new(
        Size {
            w: bounds.size.w,
            h: bounds.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    compose_all(&mut s, &tree, res(&ImageTable::new(), &AnimTable::new()));
    s
}

/// Pixel at (x, y) as (b, g, r, x).
fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 4] {
    let o = y * s.stride() + x * 4;
    s.pixels()[o..o + 4].try_into().unwrap()
}

/// The render resources a test needs, with the built-in font.
fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable) -> Resources<'a> {
    Resources {
        images,
        anims,
        font: FONT.get_or_init(default_font),
    }
}

static FONT: std::sync::OnceLock<copilot::font::Font<'static>> = std::sync::OnceLock::new();

fn bgr(c: Color) -> [u8; 4] {
    [c.b, c.g, c.r, 255]
}

#[test]
fn the_example_scene_parses_and_builds() {
    let doc = copilot::scene::parse(SCENE).expect("parse");
    let tree = copilot::scene::build(&doc).expect("build");
    // Root, plus every node the file declares.
    assert!(
        tree.len() > 10,
        "expected a populated tree, got {}",
        tree.len()
    );
    for name in [
        "root", "fuel", "tacho", "rpm", "speed", "coolant", "oil", "volts",
    ] {
        assert!(tree.find(name).is_some(), "{name} missing from the tree");
    }
}

#[test]
fn the_backdrop_covers_the_whole_frame() {
    let s = render();
    let bg = bgr(Color::rgb(0x0d, 0x11, 0x17));
    assert_eq!(px(&s, 0, 0), bg);
    assert_eq!(px(&s, 799, 479), bg);
}

#[test]
fn the_fuel_bar_is_filled_from_the_bottom() {
    // value 0.62 of a 352-tall bar starting at y=64: 218 pixels, so the fill
    // runs from y=198 to y=415. A bar that filled downward would put the
    // boundary at the other end, which is the bug this pins.
    let s = render();
    let fill = bgr(Color::rgb(0x3f, 0xb9, 0x50));
    let track = bgr(Color::rgb(0x16, 0x1b, 0x22));
    assert_eq!(px(&s, 70, 410), fill, "near the bottom should be filled");
    assert_eq!(px(&s, 70, 70), track, "near the top should be track");
}

#[test]
fn a_horizontal_bar_is_filled_from_the_left() {
    let s = render();
    let fill = bgr(Color::rgb(0x58, 0xa6, 0xff));
    // rpm sits at 150,70 and is 500 wide at 0.45 -> 225 filled.
    assert_eq!(px(&s, 200, 100), fill);
    assert_ne!(px(&s, 600, 100), fill, "past 45% should not be filled");
}

#[test]
fn a_child_is_drawn_inside_its_parent() {
    // The fuel bar is a child of the frame at 40,60 and is itself at 4,4, so
    // it must land at 44,64 -- not at 4,4.
    let s = render();
    let track = bgr(Color::rgb(0x16, 0x1b, 0x22));
    assert_eq!(
        px(&s, 50, 70),
        track,
        "the bar should be offset by its parent"
    );
}

#[test]
fn a_widget_hanging_off_the_edge_is_clipped_not_wrapped() {
    // The last frame runs to x=900 on an 800-wide surface. If clipping were
    // done by wrapping rather than by intersection, its right edge would
    // reappear on the left of some row.
    let s = render();
    let edge = bgr(Color::rgb(0x8b, 0x94, 0x9e));
    assert_eq!(px(&s, 799, 430), edge, "the frame should reach the edge");
    for y in 420..480 {
        assert_ne!(px(&s, 0, y), edge, "wrapped onto the left edge at row {y}");
    }
}

#[test]
fn rendering_is_deterministic() {
    // Two renders of the same scene must be byte-identical, or the damage
    // tracker has nothing stable to compare against.
    assert_eq!(render().pixels(), render().pixels());
}

#[test]
fn presenting_does_not_disturb_the_pixels() {
    let mut a = render();
    let before = a.pixels().to_vec();
    a.present(None);
    assert_eq!(a.pixels(), &before[..]);
}

// --- the asset pipeline ---

const IMAGE_SCENE: &str = include_str!("../../examples/image.scene");
const CHECKER: &[u8] = include_bytes!("../../examples/checker.qoi");

/// Stand in for the host: decode the scene's requests into a table.
fn with_images() -> (copilot::widget::Tree, ImageTable) {
    let doc = copilot::scene::parse(IMAGE_SCENE).expect("parse");
    let scene = copilot::scene::build_scene(&doc).expect("build");
    assert_eq!(
        scene.requests,
        ["checker.qoi"],
        "the scene must ask for its asset"
    );

    let mut table = ImageTable::new();
    let (hdr, pixels) = copilot::asset::qoi::decode(CHECKER).expect("decode");
    table.push(copilot::asset::Image::new(hdr.width, hdr.height, pixels).unwrap());
    (scene.tree, table)
}

#[test]
fn a_qoi_file_decodes_to_the_pattern_that_was_encoded() {
    let (hdr, pixels) = copilot::asset::qoi::decode(CHECKER).expect("decode");
    assert_eq!((hdr.width, hdr.height), (8, 8));
    assert_eq!(pixels.len(), 64);
    assert_eq!(pixels[0], Color::rgb(255, 0, 0), "top left is red");
    assert_eq!(pixels[2], Color::rgb(0, 0, 255), "the next square is blue");
}

#[test]
fn an_image_widget_draws_the_decoded_file() {
    let (tree, table) = with_images();
    let mut s = MemorySurface::new(Size { w: 320, h: 160 }, PixelFormat::Bgrx8888);
    compose_all(&mut s, &tree, res(&table, &AnimTable::new()));
    // The 8x8 copy sits at 16,16 and its first square is red.
    assert_eq!(px(&s, 16, 16), bgr(Color::rgb(255, 0, 0)));
    assert_eq!(px(&s, 18, 16), bgr(Color::rgb(0, 0, 255)));
}

#[test]
fn a_scaled_image_keeps_its_pattern() {
    // 8x8 blown up to 128x128 is 16x per source pixel; the checker must stay a
    // checker rather than smearing, which is why scaling is nearest-neighbour.
    let (tree, table) = with_images();
    let mut s = MemorySurface::new(Size { w: 320, h: 160 }, PixelFormat::Bgrx8888);
    compose_all(&mut s, &tree, res(&table, &AnimTable::new()));
    assert_eq!(px(&s, 48, 16), bgr(Color::rgb(255, 0, 0)));
    assert_eq!(px(&s, 48 + 32, 16), bgr(Color::rgb(0, 0, 255)));
}

#[test]
fn a_missing_image_draws_a_visible_placeholder() {
    // Silence here is the worst outcome: it is indistinguishable from a widget
    // that is working correctly and simply transparent.
    let (tree, table) = with_images();
    let mut s = MemorySurface::new(Size { w: 320, h: 160 }, PixelFormat::Bgrx8888);
    compose_all(&mut s, &tree, res(&table, &AnimTable::new()));
    assert_eq!(px(&s, 200, 16), bgr(Color::rgb(255, 0, 255)));
}

#[test]
fn the_speed_label_now_renders_ink() {
    // Labels were a documented no-op until the font landed; this is the test
    // that flips from "deliberately blank" to "must draw something".
    let doc = copilot::scene::parse(SCENE).expect("parse");
    let tree = copilot::scene::build(&doc).expect("build");
    let bounds = tree.get(ROOT).unwrap().rect;
    let mut s = MemorySurface::new(
        Size {
            w: bounds.size.w,
            h: bounds.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    compose_all(&mut s, &tree, res(&ImageTable::new(), &AnimTable::new()));

    // The panel sits at 140,200 and the label at 20,40 inside it, so the text
    // starts at 160,240 -- two 5x7 glyphs at a 6px advance.
    let text = bgr(Color::rgb(0xf0, 0xf6, 0xfc));
    let ink: usize = (160..175)
        .flat_map(|x| (238..250).map(move |y| (x, y)))
        .filter(|&(x, y)| px(&s, x, y) == text)
        .count();
    assert!(ink > 20, "the speed label drew {ink} pixels of ink");

    // And nothing outside the label's own box.
    assert_ne!(px(&s, 200, 240), text, "ink escaped to the right");
}

// --- animation ---

#[test]
fn the_cluster_scene_declares_its_animations() {
    let doc = copilot::scene::parse(SCENE).expect("parse");
    let scene = copilot::scene::build_scene(&doc).expect("build");
    assert_eq!(scene.anims.len(), 3, "fuel, rpm and oil are animated");
    assert!(!scene.anims.is_settled(), "looping animations never settle");
}

#[test]
fn ticking_the_clock_changes_what_is_drawn() {
    // The end-to-end proof that time reaches pixels: two renders of the same
    // scene at different clock values must differ.
    let doc = copilot::scene::parse(SCENE).expect("parse");
    let mut scene = copilot::scene::build_scene(&doc).expect("build");
    let bounds = scene.tree.get(ROOT).unwrap().rect;
    let size = Size {
        w: bounds.size.w,
        h: bounds.size.h,
    };

    let mut early = MemorySurface::new(size, PixelFormat::Bgrx8888);
    scene.anims.tick(&mut scene.tree, 0);
    compose_all(
        &mut early,
        &scene.tree,
        res(&ImageTable::new(), &AnimTable::new()),
    );

    // Several capped steps, since one tick may advance at most MAX_STEP_US.
    for i in 1..=12 {
        scene
            .anims
            .tick(&mut scene.tree, i * copilot::anim::MAX_STEP_US);
    }
    let mut later = MemorySurface::new(size, PixelFormat::Bgrx8888);
    compose_all(
        &mut later,
        &scene.tree,
        res(&ImageTable::new(), &AnimTable::new()),
    );

    assert_ne!(early.pixels(), later.pixels(), "nothing moved");
}

#[test]
fn an_animated_scene_reports_damage_so_a_partial_repaint_still_works() {
    // If ticking did not mark damage, the simulator's --damage mode would show
    // a frozen frame while compose_all showed motion -- the exact disagreement
    // that mode exists to surface.
    let doc = copilot::scene::parse(SCENE).expect("parse");
    let mut scene = copilot::scene::build_scene(&doc).expect("build");
    scene.anims.tick(&mut scene.tree, 0);
    scene.tree.clear_damage();
    scene
        .anims
        .tick(&mut scene.tree, copilot::anim::MAX_STEP_US);
    assert!(
        !scene.tree.damage().is_empty(),
        "a moving gauge marked nothing"
    );
}

// --- GIF ---

const SPIN: &[u8] = include_bytes!("../../examples/spin.gif");

#[test]
fn a_gif_decodes_to_the_frames_that_were_encoded() {
    // The encoder that produced this file is a separate Python implementation,
    // including its own LZW compressor. A decoder tested only against bytes it
    // helped produce can agree with itself about a wrong reading of the spec.
    let a = copilot::asset::gif::decode(SPIN).expect("decode");
    assert_eq!((a.width, a.height), (8, 8));
    assert_eq!(a.frames.len(), 3);
    for f in &a.frames {
        assert_eq!(f.pixels.len(), 64);
        assert_eq!(f.delay_us, 100_000, "10 centiseconds");
    }
}

#[test]
fn lzw_round_trips_a_solid_frame() {
    let a = copilot::asset::gif::decode(SPIN).expect("decode");
    assert!(
        a.frames[0]
            .pixels
            .iter()
            .all(|p| *p == Color::rgb(255, 0, 0)),
        "frame 0 should be solid red"
    );
    assert!(
        a.frames[1]
            .pixels
            .iter()
            .all(|p| *p == Color::rgb(0, 255, 0)),
        "frame 1 should be solid green"
    );
}

#[test]
fn lzw_round_trips_a_patterned_frame() {
    // A frame with structure exercises the dictionary; a solid fill would pass
    // even with the run-length case working and nothing else.
    let a = copilot::asset::gif::decode(SPIN).expect("decode");
    let f = &a.frames[2];
    assert_eq!(f.pixels[0], Color::rgb(0, 0, 255), "the blue corner");
    assert_eq!(f.pixels[3], Color::rgb(0, 0, 255));
    assert_eq!(f.pixels[4], Color::BLACK, "past the corner");
    assert_eq!(f.pixels[63], Color::BLACK, "far corner");
}

#[test]
fn a_truncated_gif_does_not_panic() {
    for i in 0..SPIN.len() {
        let _ = copilot::asset::gif::decode(&SPIN[..i]);
    }
}

#[test]
fn a_gif_with_a_broken_header_is_rejected() {
    let mut bad = SPIN.to_vec();
    bad[0] = b'X';
    assert!(copilot::asset::gif::decode(&bad).is_err());
}

// --- animation playback ---

/// A scene playing the three-frame test GIF.
fn anim_scene() -> (copilot::widget::Tree, AnimTable, copilot::widget::NodeId) {
    let src = r#"{
        "width": 16, "height": 16,
        "anims": ["spin.gif"],
        "root": { "type":"panel", "rect":[0,0,16,16], "children":[
          { "type":"anim", "rect":[0,0,8,8], "anim":0, "name":"spinner" }
        ]}
    }"#;
    let doc = copilot::scene::parse(src).expect("parse");
    let scene = copilot::scene::build_scene(&doc).expect("build");
    assert_eq!(scene.anim_requests, ["spin.gif"]);

    let mut table = AnimTable::new();
    table.push(copilot::asset::gif::decode(SPIN).expect("decode"));
    let id = scene.tree.find("spinner").expect("named node");
    (scene.tree, table, id)
}

fn shown_frame(t: &copilot::widget::Tree, id: copilot::widget::NodeId) -> u32 {
    match t.get(id).unwrap().kind {
        copilot::widget::Kind::Anim { frame, .. } => frame,
        ref k => panic!("expected an anim, got {k:?}"),
    }
}

#[test]
fn an_animation_advances_by_its_own_frame_delays() {
    let (mut tree, table, id) = anim_scene();
    assert_eq!(shown_frame(&tree, id), 0);
    // Each frame is 100ms; half of one is not enough to step.
    copilot::anim::tick_playback(&mut tree, &table, 50_000);
    assert_eq!(shown_frame(&tree, id), 0, "stepped too early");
    copilot::anim::tick_playback(&mut tree, &table, 60_000);
    assert_eq!(shown_frame(&tree, id), 1);
}

#[test]
fn playback_wraps_at_the_end() {
    let (mut tree, table, id) = anim_scene();
    for _ in 0..3 {
        copilot::anim::tick_playback(&mut tree, &table, 100_000);
    }
    assert_eq!(shown_frame(&tree, id), 0, "three frames should wrap to 0");
}

#[test]
fn pausing_stops_the_frame_advancing() {
    let (mut tree, table, id) = anim_scene();
    tree.set_playing(id, false).expect("is an anim");
    copilot::anim::tick_playback(&mut tree, &table, 5_000_000);
    assert_eq!(shown_frame(&tree, id), 0, "a paused animation moved");
}

#[test]
fn seeking_lands_on_the_frame_asked_for() {
    let (mut tree, table, id) = anim_scene();
    tree.seek(id, 2).expect("is an anim");
    assert_eq!(shown_frame(&tree, id), 2);
    // And the leftover time is cleared, so it does not immediately step off.
    copilot::anim::tick_playback(&mut tree, &table, 50_000);
    assert_eq!(shown_frame(&tree, id), 2);
}

#[test]
fn negative_speed_runs_backwards() {
    let (mut tree, table, id) = anim_scene();
    tree.set_speed(id, -1.0).expect("is an anim");
    copilot::anim::tick_playback(&mut tree, &table, 100_000);
    assert_eq!(shown_frame(&tree, id), 2, "should have wrapped backwards");
}

#[test]
fn half_speed_takes_twice_as_long() {
    let (mut tree, table, id) = anim_scene();
    tree.set_speed(id, 0.5).expect("is an anim");
    copilot::anim::tick_playback(&mut tree, &table, 100_000);
    assert_eq!(shown_frame(&tree, id), 0, "half speed stepped too soon");
    copilot::anim::tick_playback(&mut tree, &table, 100_000);
    assert_eq!(shown_frame(&tree, id), 1);
}

#[test]
fn zero_speed_is_a_pause_and_does_not_spin() {
    let (mut tree, table, id) = anim_scene();
    tree.set_speed(id, 0.0).expect("is an anim");
    copilot::anim::tick_playback(&mut tree, &table, 10_000_000);
    assert_eq!(shown_frame(&tree, id), 0);
}

#[test]
fn the_control_api_refuses_widgets_that_are_not_animations() {
    // A caller driving a scene it did not author must be able to tell
    // "paused it" from "there is nothing there to pause".
    let (mut tree, _, _) = anim_scene();
    let root_child = tree.get(copilot::widget::ROOT).unwrap().children[0];
    assert!(tree.set_playing(root_child, false).is_none());
    assert!(tree.seek(root_child, 1).is_none());
    assert!(tree.set_speed(root_child, 2.0).is_none());
}

#[test]
fn playback_marks_damage_so_a_partial_repaint_follows_it() {
    let (mut tree, table, _) = anim_scene();
    tree.clear_damage();
    copilot::anim::tick_playback(&mut tree, &table, 100_000);
    assert!(!tree.damage().is_empty(), "a stepped frame marked nothing");
}

#[test]
fn an_animation_widget_draws_its_current_frame() {
    let (mut tree, table, id) = anim_scene();
    let mut s = MemorySurface::new(Size { w: 16, h: 16 }, PixelFormat::Bgrx8888);
    compose_all(&mut s, &tree, res(&ImageTable::new(), &table));
    assert_eq!(px(&s, 0, 0), bgr(Color::rgb(255, 0, 0)), "frame 0 is red");

    tree.seek(id, 1).unwrap();
    compose_all(&mut s, &tree, res(&ImageTable::new(), &table));
    assert_eq!(px(&s, 0, 0), bgr(Color::rgb(0, 255, 0)), "frame 1 is green");
}

/// The Z31 wired to the bus: the same panel with each instrument bound to
/// the gauge it reads, for the cluster at node 0x01.
const Z31_NORMAL: &str = include_str!("../../examples/z31-normal.scene");

#[test]
fn the_wired_z31_binds_every_instrument_it_labels() {
    let doc = copilot::scene::parse(Z31_NORMAL).expect("z31-normal must parse");
    let scene = copilot::scene::build_scene(&doc).expect("z31-normal must build");
    assert_eq!(scene.node, Some(1), "the main cluster is node 0x01");

    // Which gauges, deliberately not pinned: this is a panel somebody is still
    // drawing, and a test that names its instruments turns rebinding one into
    // a failing build. What has to hold is that the wiring is *sound*,
    // whatever it has been wired to.
    let bound: Vec<&str> = scene.bindings.iter().map(|b| b.gauge.name).collect();
    assert!(
        bound.len() >= 6,
        "a cluster this size reads more: {bound:?}"
    );

    // The subscription the display derives: one id per gauge, in bus order,
    // asked for once however many widgets show it.
    let wanted = scene.wanted();
    let mut distinct = bound.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        wanted.len(),
        distinct.len(),
        "one subscription per gauge, however many widgets show it"
    );
    assert!(wanted.windows(2).all(|w| w[0] < w[1]), "{wanted:?}");
    for id in &wanted {
        assert!(
            copilot::cogware_can::gauge_by_id(u16::from(*id)).is_some(),
            "0x{id:02X} is not a gauge"
        );
    }
}

/// The example rig: every display in every mode parses, builds, binds against
/// the real gauge table, needs no asset, and sits on the node the rig says.
///
/// Read from disk rather than compiled in, because the point is that the
/// files name each other and a rig is only as good as the set on the card.
#[test]
fn the_example_rig_and_all_its_scenes_agree() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let text = std::fs::read_to_string(dir.join("z31.rig")).expect("z31.rig");
    let rig = copilot::rig::parse(&text).expect("the rig parses");
    assert_eq!(rig.modes, ["normal", "sport", "track"]);
    let nodes: Vec<u8> = rig.displays.iter().map(|d| d.node).collect();
    assert_eq!(nodes, [1, 2, 3], "gateway at 0, then the three displays");

    for d in &rig.displays {
        for (m, file) in d.scenes.iter().enumerate() {
            let text =
                std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
            let doc = copilot::scene::parse(&text).unwrap_or_else(|e| panic!("{file}: {e:?}"));
            let scene =
                copilot::scene::build_scene(&doc).unwrap_or_else(|e| panic!("{file}: {e:?}"));
            assert_eq!(rig.check(d.node, m, &scene), Ok(()), "{file}");
            assert!(
                !scene.bindings.is_empty(),
                "{file} binds nothing to the bus"
            );
            assert!(
                scene.requests.is_empty(),
                "{file} wants an asset the rig does not ship"
            );
            if d.node != 1 {
                assert_eq!(scene.shape, copilot::scene::Shape::Round, "{file}");
                assert_eq!(scene.tree.get(ROOT).unwrap().rect.size.w, 480, "{file}");
            }
        }
    }
}

/// The loop a display on the bus actually runs, as the README states it:
/// subscribe to what the scene binds, tick the animations, lay the live
/// readings over the top, compose. Here to prove it type-checks -- the
/// animator and the tree are two fields of one `Scene` and are borrowed
/// together -- and that live data wins over an authored animation.
#[test]
fn a_display_ticks_animations_then_lays_the_bus_over_them() {
    use cogware_can::{ACCEL_ENRICH, clear_all};

    let src = r#"{"width":40,"height":10,"root":{"type":"panel","rect":[0,0,40,10],
        "children":[
          {"type":"bar","rect":[0,0,40,10],"name":"demo","value":0.0,
           "animate":{"property":"value","from":0.0,"to":1.0,"duration_ms":1000},
           "bind":{"gauge":"ACCEL_ENRICH","min":0,"max":200}}]}}"#;
    let doc = copilot::scene::parse(src).expect("parse");
    let mut scene = copilot::scene::build_scene(&doc).expect("build");

    // What the display would hand `Subscription::new`.
    assert_eq!(scene.wanted(), vec![0x2E], "ACCEL_ENRICH's id");

    let id = scene.tree.find("demo").expect("the bar");
    clear_all();

    // Frame by frame, as a display runs it. The first tick only sets the
    // clock's baseline, and no tick may step further than `MAX_STEP_US`, so
    // half of a one-second animation is the baseline plus five 100ms frames.
    let mut now_us = 0;
    let frame = |scene: &mut copilot::asset::Scene, now: &mut u64| {
        scene.anims.tick(&mut scene.tree, *now);
        let fed = scene.apply_gauges();
        *now += copilot::anim::MAX_STEP_US;
        fed
    };
    for _ in 0..6 {
        // No reading yet: the animation has the widget to itself, which is
        // what lets a bound scene still demonstrate itself in the simulator.
        assert_eq!(frame(&mut scene, &mut now_us), 0);
    }
    let animated = scene.tree.get(id).unwrap().kind.reading().unwrap();
    assert!((animated - 0.5).abs() < 1e-3, "got {animated}");

    // Once the bus speaks, it wins wherever it has something to say.
    ACCEL_ENRICH.set(500); // 50.0 %, scale 10, over a 0..200 range
    assert_eq!(frame(&mut scene, &mut now_us), 1);
    let live = scene.tree.get(id).unwrap().kind.reading().unwrap();
    assert!((live - 0.25).abs() < 1e-3, "got {live}");

    let b = scene.tree.get(ROOT).unwrap().rect;
    let mut s = MemorySurface::new(
        Size {
            w: b.size.w,
            h: b.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    compose_all(
        &mut s,
        &scene.tree,
        res(&ImageTable::new(), &AnimTable::new()),
    );
    ACCEL_ENRICH.clear();
}

/// The Z31 replica that ships on the SD card, rendered headless.
///
/// The scene lives in this tree, beside the other examples, because it is
/// written entirely in copilot widgets and this is where the renderer that
/// draws it is tested. digidash copies it to the boot partition; a scene the
/// board is meant to boot into must not be able to stop parsing unnoticed.
const Z31: &str = include_str!("../../examples/z31.scene");

#[test]
fn the_z31_replica_builds_and_draws() {
    let doc = copilot::scene::parse(Z31).expect("z31 scene must parse");
    let scene = copilot::scene::build_scene(&doc).expect("z31 scene must build");
    assert!(
        scene.tree.len() > 30,
        "expected a full panel, got {}",
        scene.tree.len()
    );
    assert_eq!(
        scene.anims.len(),
        1,
        "the tank is the only reading that moves on its own"
    );

    // Every part of the panel this scene draws, by the name it gives it. A
    // replica that quietly loses its oil gauge still renders, which is the
    // whole reason to name them here.
    //
    // The lamps along the top are absent on purpose: on the real car they sit
    // above the display rather than on it.
    for name in [
        "cluster",
        "lens",
        "turn_left",
        "turn_right",
        "speed_box",
        "speed",
        "cruise",
        "tacho",
        "rpm_digits",
        "rpm",
        "power_curve",
        "coolant",
        "coolant_scale",
        "oil",
        "oil_scale",
        "volts",
        "volts_scale",
        "fuel",
        "fuel_scale",
        "oil_lamp",
        "batt_lamp",
    ] {
        assert!(scene.tree.find(name).is_some(), "{name} missing");
    }

    let b = scene.tree.get(ROOT).unwrap().rect;
    let mut s = MemorySurface::new(
        Size {
            w: b.size.w,
            h: b.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    compose_all(
        &mut s,
        &scene.tree,
        res(&ImageTable::new(), &AnimTable::new()),
    );

    // Something was actually drawn across the whole panel, not just a corner.
    let backdrop = bgr(Color::rgb(0x07, 0x09, 0x0c));
    let lit = (0..b.size.h as usize)
        .step_by(7)
        .flat_map(|y| (0..b.size.w as usize).step_by(7).map(move |x| (x, y)))
        .filter(|&(x, y)| px(&s, x, y) != backdrop)
        .count();
    assert!(
        lit > 2000,
        "only {lit} sampled pixels differ from the backdrop"
    );
}

// --- partial repaint ---

/// Build the Z31 tree and a surface the right size for it.
fn z31() -> (copilot::widget::Tree, MemorySurface) {
    let doc = copilot::scene::parse(Z31).expect("z31 scene must parse");
    let tree = copilot::scene::build(&doc).expect("z31 scene must build");
    let bounds = tree.get(ROOT).unwrap().rect;
    let s = MemorySurface::new(
        Size {
            w: bounds.size.w,
            h: bounds.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    (tree, s)
}

#[test]
fn a_partial_repaint_of_every_widget_matches_a_full_one() {
    // The bug this exists to catch: a widget that draws correctly when the
    // clip is the whole screen but wrongly when it is a rectangle around
    // itself. It shows up as debris left on screen after something moves,
    // which is exactly what nobody notices in a still screenshot.
    let (tree, mut full) = z31();
    compose_all(&mut full, &tree, res(&ImageTable::new(), &AnimTable::new()));

    for i in 0..tree.len() as u32 {
        let Some(node) = tree.get(copilot::widget::NodeId(i)) else {
            continue;
        };
        let rect = node.rect;
        if rect.is_empty() {
            continue;
        }

        // Start from the finished frame, scrub one widget's box to a colour
        // the scene never uses, and let the damage path put it back. Rendering
        // afresh rather than copying the buffer: `rendering_is_deterministic`
        // already holds, and the surface exposes no way to write bytes
        // wholesale.
        let mut partial = MemorySurface::new(full.size(), PixelFormat::Bgrx8888);
        compose_all(
            &mut partial,
            &tree,
            res(&ImageTable::new(), &AnimTable::new()),
        );
        for y in rect.top()..rect.bottom() {
            partial.fill_span(rect.left(), y, rect.size.w, Color::rgb(0xff, 0x00, 0xff));
        }

        let mut damage = copilot::render::Damage::default();
        damage.add(rect);
        copilot::render::compose(
            &mut partial,
            &tree,
            &damage,
            res(&ImageTable::new(), &AnimTable::new()),
        );

        assert_eq!(
            partial.pixels(),
            full.pixels(),
            "repainting just {:?} did not reproduce the full frame",
            node.name
        );
    }
}
