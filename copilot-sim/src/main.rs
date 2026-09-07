// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

//! A window that renders a copilot scene file, and reloads it when it changes.
//!
//! This is the development loop the toolkit is built around: edit the scene in
//! a text editor, save, and watch the window update without restarting. The
//! eventual editor will emit the same files, so anything that renders here
//! renders identically on hardware.
//!
//! # Why it renders the whole frame every time
//!
//! Dirty-rectangle rendering is what makes the toolkit fast on a Pi, but a
//! desktop window is redrawn from a full buffer on every present anyway, so
//! partial repaints would be invisible here and would hide bugs. The simulator
//! deliberately runs [`copilot::render::compose_all`] so that what you see is
//! the tree as the scene describes it, not the tree as the damage tracker
//! believes it to be. Comparing the two is how a damage bug gets caught: see
//! `--damage`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use copilot::anim::Animator;
use copilot::asset::{AnimTable, Image, ImageTable};
use copilot::font::{Font, default_font};
use copilot::render::{Resources, compose, compose_all};
use copilot::widget::Tree;
use copilot::{Color, MemorySurface, PixelFormat, Size, Surface};
use minifb::{Key, Window, WindowOptions};

/// How often the scene file is checked for changes.
const POLL: Duration = Duration::from_millis(250);

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut damage_only = false;
    let mut ppm: Option<PathBuf> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            // One frame to a file and out. Verifying a scene otherwise means
            // opening a window and looking at it, which a machine cannot do
            // and which rules out checking a render from a script at all.
            "--ppm" => match args.next() {
                Some(p) => ppm = Some(PathBuf::from(p)),
                None => {
                    eprintln!("copilot-sim: --ppm needs a path");
                    std::process::exit(2);
                }
            },
            // Renders through the damage tracker instead of repainting
            // everything. Anything that fails to appear is a widget whose
            // change was never marked dirty -- the bug class this mode exists
            // to expose.
            "--damage" => damage_only = true,
            "-h" | "--help" => return usage(),
            other => path = Some(PathBuf::from(other)),
        }
    }

    let Some(path) = path else {
        usage();
        std::process::exit(2);
    };

    let outcome = match ppm {
        Some(out) => shoot(&path, &out),
        None => run(&path, damage_only),
    };
    if let Err(e) = outcome {
        eprintln!("copilot-sim: {e}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!("usage: copilot-sim [--damage] [--ppm <out>] <scene file>");
    eprintln!();
    eprintln!("  --damage      render only what the damage tracker reports dirty,");
    eprintln!("                so a missing repaint shows up as a missing widget");
    eprintln!("  --ppm <out>   render one frame to a binary PPM and exit, for");
    eprintln!("                checking a scene where there is no display");
}

/// Render one frame to a PPM and return.
///
/// Deliberately not the same path as [`run`]: no window, no clock, no reload
/// watch, so it works over a terminal and finishes with an exit status a
/// script can test.
fn shoot(path: &Path, out: &Path) -> Result<(), String> {
    let (tree, images, clips, _anims) = load(path)?;
    let bounds = tree
        .get(copilot::widget::ROOT)
        .ok_or("scene has no root")?
        .rect;
    let (w, h) = (bounds.size.w, bounds.size.h);

    let mut surface = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
    let font: Font<'static> = default_font();
    compose_all(&mut surface, &tree, res(&images, &clips, &font));

    let mut bytes = format!("P6\n{w} {h}\n255\n").into_bytes();
    bytes.reserve((w as usize) * (h as usize) * 3);
    let px = surface.pixels();
    for y in 0..h as usize {
        let row = y * surface.stride();
        for x in 0..w as usize {
            let o = row + x * 4;
            // The surface is BGRX; PPM wants RGB.
            bytes.extend_from_slice(&[px[o + 2], px[o + 1], px[o]]);
        }
    }
    std::fs::write(out, bytes).map_err(|e| format!("cannot write {}: {e}", out.display()))
}

fn run(path: &Path, damage_only: bool) -> Result<(), String> {
    let (mut tree, mut images, mut clips, mut anims) = load(path)?;
    let bounds = tree
        .get(copilot::widget::ROOT)
        .ok_or("scene has no root")?
        .rect;
    let (w, h) = (bounds.size.w, bounds.size.h);

    let mut surface = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
    let mut window = Window::new(
        &format!("copilot — {}", path.display()),
        w as usize,
        h as usize,
        WindowOptions {
            resize: false,
            ..WindowOptions::default()
        },
    )
    .map_err(|e| format!("cannot open a window: {e}"))?;
    window.set_target_fps(60);

    // The monotonic source the core crate refuses to invent for itself.
    let started = Instant::now();
    let mut last_us = 0u64;
    let mut stamp = mtime(path);
    // The first frame has nothing to be incremental against.
    // The built-in atlas until a scene can name its own; parsed once, since
    // its lifetime is borrowed from a const and outlives the loop.
    let font: Font<'static> = default_font();
    compose_all(&mut surface, &tree, res(&images, &clips, &font));
    let mut buffer = vec![0u32; (w * h) as usize];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if let Some(now) = mtime(path)
            && Some(now) != stamp
        {
            stamp = Some(now);
            match load(path) {
                Ok((t, imgs, cl, an)) => {
                    tree = t;
                    images = imgs;
                    clips = cl;
                    anims = an;
                    // A reloaded scene shares nothing with the old one, so the
                    // previous frame's pixels are meaningless.
                    surface.clear(Color::BLACK);
                    compose_all(&mut surface, &tree, res(&images, &clips, &font));
                    eprintln!("reloaded {}", path.display());
                }
                // A half-saved file is normal while someone is editing; keep
                // showing the last good scene rather than exiting.
                Err(e) => eprintln!("{}: {e}", path.display()),
            }
        } else if damage_only {
            compose(
                &mut surface,
                &tree,
                tree.damage(),
                res(&images, &clips, &font),
            );
            tree.clear_damage();
        }

        // Advance animations before drawing, so a frame shows the state the
        // clock says it should rather than the previous one.
        let now_us = started.elapsed().as_micros() as u64;
        anims.tick(&mut tree, now_us);
        copilot::anim::tick_playback(&mut tree, &clips, now_us.saturating_sub(last_us));
        last_us = now_us;
        if !damage_only && !tree.damage().is_empty() {
            compose(
                &mut surface,
                &tree,
                tree.damage(),
                res(&images, &clips, &font),
            );
            tree.clear_damage();
        }

        surface.present(None);
        pack_into(&surface, &mut buffer);
        window
            .update_with_buffer(&buffer, w as usize, h as usize)
            .map_err(|e| format!("cannot present: {e}"))?;
        std::thread::sleep(POLL / 10);
    }
    Ok(())
}

/// Read and build a scene, reporting where it failed rather than just that it did.
///
/// This is the half of the contract the core crate cannot perform: rule 1.4
/// forbids it from opening a file, so the scene lists the images it wants by
/// path and the host resolves them. Paths are taken relative to the scene
/// file, so a scene and its assets can be moved together.
fn load(path: &Path) -> Result<(Tree, ImageTable, AnimTable, Animator), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    let doc = copilot::scene::parse(&text).map_err(|e| format!("{e:?}"))?;
    let scene = copilot::scene::build_scene(&doc).map_err(|e| format!("{e:?}"))?;

    let base = path.parent().unwrap_or(Path::new("."));
    let mut images = ImageTable::new();
    for request in &scene.requests {
        match load_image(&base.join(request)) {
            Some(img) => images.push(img),
            None => {
                // A failed load must still occupy its slot, or every later
                // image shifts down one and widgets draw the wrong picture.
                eprintln!("  missing image: {request}");
                images.push_missing()
            }
        };
    }
    let mut clips = AnimTable::new();
    for request in &scene.anim_requests {
        match std::fs::read(base.join(request))
            .ok()
            .and_then(|b| copilot::asset::gif::decode(&b).ok())
        {
            Some(a) => clips.push(a),
            None => {
                eprintln!("  missing animation: {request}");
                clips.push_missing()
            }
        };
    }
    Ok((scene.tree, images, clips, scene.anims))
}

/// Decode one image file, or `None` if it cannot be read or is not QOI.
fn load_image(path: &Path) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
    let (hdr, pixels) = copilot::asset::qoi::decode(&bytes).ok()?;
    Image::new(hdr.width, hdr.height, pixels)
}

/// Repack BGRX bytes into the 0RGB words minifb wants.
///
/// The surface stores what a real framebuffer would; converting here rather
/// than making the surface store minifb's layout keeps the simulator honest
/// about what the hardware path actually produces.
fn pack_into(surface: &MemorySurface, out: &mut [u32]) {
    let (pixels, _) = surface.pixels().as_chunks::<4>();
    for (word, px) in out.iter_mut().zip(pixels) {
        *word = u32::from(px[2]) << 16 | u32::from(px[1]) << 8 | u32::from(px[0]);
    }
}

/// Bundle what the renderer needs. One place, so a new asset kind is one edit.
fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable, font: &'a Font<'a>) -> Resources<'a> {
    Resources {
        images,
        anims,
        font,
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
