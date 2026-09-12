// SPDX-License-Identifier: GPL-3.0-only
//! A frame does not allocate, enforced rather than asserted in prose.
//!
//! A bare-metal cluster runs a fixed heap, so a renderer that allocates per
//! frame fragments it and fails hours later. Everywhere else that is a
//! comment; here it fails a build.
//!
//! It needs its own test binary to install a global allocator, and it is one
//! test function rather than three because the counter is a global and
//! `cargo test` runs a binary's functions in parallel.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use copilot::asset::{AnimTable, ImageTable};
use copilot::font::default_font;
use copilot::render::{Resources, compose, compose_all};
use copilot::widget::ROOT;
use copilot::{MemorySurface, PixelFormat, Size};

/// Counts allocations, but only while armed: loading a scene is supposed to
/// allocate, and the question here is only ever about the frame.
struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to the system allocator unchanged, and the
// only additional work is an atomic increment on a counter, which allocates
// nothing and cannot unwind.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `layout` is passed through untouched from a caller that has
        // already satisfied the trait's requirements.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above; `ptr` came from `System.alloc` with this `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Run `f` with the counter armed, and report how many allocations it made.
fn while_drawing(f: impl FnOnce()) -> usize {
    ALLOCS.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    f();
    ARMED.store(false, Ordering::Relaxed);
    ALLOCS.load(Ordering::Relaxed)
}

/// Everything a frame needs, built before the counter is armed.
fn scene(text: &str) -> (copilot::widget::Tree, MemorySurface) {
    let doc = copilot::scene::parse(text).expect("scene must parse");
    let tree = copilot::scene::build(&doc).expect("scene must build");
    let bounds = tree.get(ROOT).unwrap().rect;
    let surface = MemorySurface::new(
        Size {
            w: bounds.size.w,
            h: bounds.size.h,
        },
        PixelFormat::Bgrx8888,
    );
    (tree, surface)
}

#[test]
fn a_frame_allocates_nothing() {
    a_round_gauge();
    a_scaled_image();
    a_damage_driven_frame();
}

fn a_round_gauge() {
    // Arcs, a rim scale, a needle, a hub and a seven-segment readout: the
    // coverage sampler, the span emitters and the glyph path all at once.
    let (tree, mut surface) = scene(include_str!("../../examples/gauge-left-normal.scene"));
    let font = default_font();
    let (images, anims) = (ImageTable::new(), AnimTable::new());
    let res = Resources {
        images: &images,
        anims: &anims,
        font: &font,
        menus: &[],
    };
    // Once before arming: the first frame is where any lazily-built table
    // would appear, and it is not the frame under test.
    compose_all(&mut surface, &tree, res);

    let n = while_drawing(|| compose_all(&mut surface, &tree, res));
    assert_eq!(n, 0, "composing a dial allocated {n} times");
}

fn a_scaled_image() {
    // The case that used to fail: an image widget whose rectangle is not the
    // image's own size built a whole resampled copy on every frame, sized by
    // the widget's area.
    let (tree, mut surface) = scene(include_str!("../../examples/image.scene"));
    let font = default_font();
    let mut images = ImageTable::new();
    let (hdr, pixels) = copilot::asset::qoi::decode(include_bytes!("../../examples/checker.qoi"))
        .expect("the checker must decode");
    images.push(
        copilot::asset::Image::new(hdr.width, hdr.height, pixels).expect("dimensions must agree"),
    );
    let anims = AnimTable::new();
    let res = Resources {
        images: &images,
        anims: &anims,
        font: &font,
        menus: &[],
    };
    compose_all(&mut surface, &tree, res);

    let n = while_drawing(|| compose_all(&mut surface, &tree, res));
    assert_eq!(n, 0, "composing a scaled image allocated {n} times");
}

fn a_damage_driven_frame() {
    // The path a display actually runs: damage, compose, present.
    let (mut tree, mut surface) = scene(include_str!("../../examples/gauge-left-normal.scene"));
    let font = default_font();
    let (images, anims) = (ImageTable::new(), AnimTable::new());
    let res = Resources {
        images: &images,
        anims: &anims,
        font: &font,
        menus: &[],
    };
    compose_all(&mut surface, &tree, res);
    tree.clear_damage();

    let needle = tree.find("needle").expect("the dial has a needle");
    tree.set_reading(needle, 0.5)
        .expect("the needle takes a reading");

    let n = while_drawing(|| {
        compose(&mut surface, &tree, tree.damage(), res);
    });
    assert_eq!(n, 0, "a damage-driven frame allocated {n} times");
}
