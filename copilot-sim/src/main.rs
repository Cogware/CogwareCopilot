// SPDX-License-Identifier: GPL-3.0-only
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

//! A window that renders a copilot scene file, and reloads it when it changes.
//!
//! This is the development loop the toolkit is built around: edit the scene in
//! a text editor, save, and watch the window update without restarting. The
//! eventual editor will emit the same files, so anything that renders here
//! renders identically on hardware.
//!
//! It repaints the whole tree every frame, so what you see is the scene as
//! written rather than as the damage tracker believes it to be. `--damage`
//! switches to the damage-driven path a display actually runs, and comparing
//! the two is how a missing repaint gets caught.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

mod pad;

use copilot::anim::Animator;
use copilot::asset::{AnimTable, Image, ImageTable};
use copilot::font::{Font, default_font};
use copilot::menu::Menu;
use copilot::render::{Resources, compose_all, frame, transition};
use copilot::rig::Rig;
use copilot::widget::Tree;
use copilot::widget::{Kind, NodeId};
use copilot::{Color, MemorySurface, PixelFormat, Size, Surface};
use minifb::{Key, Window, WindowOptions};

/// How often the scene file is checked for changes.
const POLL: Duration = Duration::from_millis(250);

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut damage_only = false;
    let mut ppm: Option<PathBuf> = None;
    let mut rig: Option<PathBuf> = None;

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
            // Loads the rig's menus and opens a second window of buttons, so
            // a menu widget can be driven without hardware.
            "--rig" => match args.next() {
                Some(p) => rig = Some(PathBuf::from(p)),
                None => {
                    eprintln!("copilot-sim: --rig needs a path");
                    std::process::exit(2);
                }
            },
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
        Some(out) => shoot(&path, &out, rig.as_deref()),
        None => run(&path, damage_only, rig.as_deref()),
    };
    if let Err(e) = outcome {
        eprintln!("copilot-sim: {e}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!("usage: copilot-sim [--damage] [--rig <file>] [--ppm <out>] <scene file>");
    eprintln!();
    eprintln!("  --damage      render only what the damage tracker reports dirty,");
    eprintln!("                so a missing repaint shows up as a missing widget");
    eprintln!("  --rig <file>  load that rig's menus and open a button pad window,");
    eprintln!("                so a menu widget in the scene can be driven, and");
    eprintln!("                cycle the display's modes with TAB");
    eprintln!("  --ppm <out>   render one frame to a binary PPM and exit, for");
    eprintln!("                checking a scene where there is no display");
}

/// Render one frame to a PPM and return.
///
/// Deliberately not the same path as [`run`]: no window, no clock, no reload
/// watch, so it works over a terminal and finishes with an exit status a
/// script can test.
fn shoot(path: &Path, out: &Path, rig_path: Option<&Path>) -> Result<(), String> {
    let Loaded {
        tree,
        images,
        clips,
        ..
    } = load(path)?;
    let bounds = tree
        .get(copilot::widget::ROOT)
        .ok_or("scene has no root")?
        .rect;
    let (w, h) = (bounds.size.w, bounds.size.h);

    let mut surface = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
    let font: Font<'static> = default_font();
    let menus = load_menus(rig_path)?;
    compose_all(&mut surface, &tree, res(&images, &clips, &font, &menus));

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

fn run(path: &Path, damage_only: bool, rig_path: Option<&Path>) -> Result<(), String> {
    let Loaded {
        mut tree,
        mut images,
        mut clips,
        mut anims,
        node,
    } = load(path)?;
    let bounds = tree
        .get(copilot::widget::ROOT)
        .ok_or("scene has no root")?
        .rect;
    let (w, h) = (bounds.size.w, bounds.size.h);

    // A desktop has the memory, so the simulator always lends a second buffer
    // and a cross-fade previews here even when the board it is for cannot.
    let mut surface =
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888).with_scratch_buffer();
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

    let mut menus = load_menus(rig_path)?;
    let mut pad = match rig_path {
        Some(_) => Some(pad::Pad::new()?),
        None => None,
    };
    let mut driven = menu_widget(&tree);

    // A mode change loads the next scene for this display and runs the rig's
    // transition over it. Without a rig, or a scene that names no node, there
    // is nothing to switch between and TAB does nothing.
    let rig = match rig_path {
        Some(rp) => {
            let text = std::fs::read_to_string(rp).map_err(|e| format!("{}: {e}", rp.display()))?;
            Some(copilot::rig::parse(&text).map_err(|e| format!("{}: {e:?}", rp.display()))?)
        }
        None => None,
    };
    let rig_dir = rig_path
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut mode = 0usize;
    let mut swap: Option<Swap> = None;

    // The monotonic source the core crate refuses to invent for itself.
    let started = Instant::now();
    let mut last_us = 0u64;
    let mut stamp = mtime(path);
    // The first frame has nothing to be incremental against.
    // The built-in atlas until a scene can name its own; parsed once, since
    // its lifetime is borrowed from a const and outlives the loop.
    let font: Font<'static> = default_font();
    compose_all(&mut surface, &tree, res(&images, &clips, &font, &menus));
    let mut buffer = vec![0u32; (w * h) as usize];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if let Some(now) = mtime(path)
            && Some(now) != stamp
        {
            stamp = Some(now);
            match load(path) {
                Ok(l) => {
                    tree = l.tree;
                    images = l.images;
                    clips = l.clips;
                    anims = l.anims;
                    // A reloaded scene shares nothing with the old one, so the
                    // previous frame's pixels are meaningless.
                    surface.clear(Color::BLACK);
                    compose_all(&mut surface, &tree, res(&images, &clips, &font, &menus));
                    // It marks itself wholly dirty when built and has just
                    // been painted in full.
                    tree.clear_damage();
                    driven = menu_widget(&tree);
                    eprintln!("reloaded {}", path.display());
                }
                // A half-saved file is normal while someone is editing; keep
                // showing the last good scene rather than exiting.
                Err(e) => eprintln!("{}: {e}", path.display()),
            }
        }

        if window.is_key_pressed(Key::Tab, minifb::KeyRepeat::No)
            && swap.is_none()
            && let (Some(r), Some(n)) = (rig.as_ref(), node)
        {
            match next_mode(r, n, mode, &rig_dir) {
                Ok(Some((next, loaded))) => {
                    eprintln!("mode {} -> {}", r.modes[mode], r.modes[next]);
                    mode = next;
                    swap = Some(Swap {
                        into: loaded,
                        started: Instant::now(),
                    });
                }
                Ok(None) => {}
                Err(e) => eprintln!("{e}"),
            }
        }

        if let Some(s) = swap.as_ref() {
            let r = rig.as_ref().expect("a swap only starts with a rig");
            let ms = r.transition_ms.max(1) as f32;
            let t = s.started.elapsed().as_millis() as f32 / ms;
            if t >= 1.0 {
                // Finished: the incoming scene becomes the scene, and the
                // next frame is an ordinary one again.
                let s = swap.take().expect("checked just above");
                tree = s.into.tree;
                images = s.into.images;
                clips = s.into.clips;
                anims = s.into.anims;
                driven = menu_widget(&tree);
                compose_all(&mut surface, &tree, res(&images, &clips, &font, &menus));
            } else {
                transition(
                    &mut surface,
                    &tree,
                    res(&images, &clips, &font, &menus),
                    &s.into.tree,
                    res(&s.into.images, &s.into.clips, &font, &menus),
                    r.transition,
                    t,
                );
            }
        }

        if let Some(p) = pad.as_mut() {
            if !p.is_open() {
                pad = None;
            } else if let Some(button) = p.press()
                && let Some((id, name)) = driven.as_ref()
                && let Some(m) = menus.iter_mut().find(|m| &m.name == name)
                && m.press(button)
            {
                // The menu lives outside the tree, so the widget showing it
                // has to be told that what it draws has moved.
                tree.touch(*id);
            }
        }

        // Advance animations before drawing, so a frame shows the state the
        // clock says it should rather than the previous one.
        let now_us = started.elapsed().as_micros() as u64;
        anims.tick(&mut tree, now_us);
        copilot::anim::tick_playback(&mut tree, &clips, now_us.saturating_sub(last_us));
        last_us = now_us;

        if swap.is_some() {
            // The transition already painted the whole surface this frame.
        } else if damage_only {
            // The hardware path: ask the backend whether the last frame
            // survived, repaint what the tracker says moved, present.
            frame(&mut surface, &mut tree, res(&images, &clips, &font, &menus));
        } else {
            // The whole tree, every frame, so what you see is the scene as
            // written rather than as the damage tracker believes it to be.
            compose_all(&mut surface, &tree, res(&images, &clips, &font, &menus));
            tree.clear_damage();
            surface.present(None);
        }

        if let Some(p) = pad.as_mut() {
            p.show(&font)?;
        }
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
/// This is the half of the contract the core crate cannot perform: it cannot
/// open a file, so the scene lists the images it wants by path and the host
/// resolves them. Paths are taken relative to the scene
/// file, so a scene and its assets can be moved together.
/// A scene and everything it needs to draw.
struct Loaded {
    tree: Tree,
    images: ImageTable,
    clips: AnimTable,
    anims: Animator,
    /// The CAN node the scene says it is for, which is how a rig finds the
    /// display it belongs to.
    node: Option<u8>,
}

fn load(path: &Path) -> Result<Loaded, String> {
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
    Ok(Loaded {
        tree: scene.tree,
        images,
        clips,
        anims: scene.anims,
        node: scene.node,
    })
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

/// A mode change in progress: the scene coming in, and when it started.
struct Swap {
    into: Loaded,
    started: Instant,
}

/// Load the scene for the mode after `mode` on the display at `node`.
///
/// `None` when the rig has only one mode, so TAB on a single-mode rig is not
/// an error.
fn next_mode(
    rig: &Rig,
    node: u8,
    mode: usize,
    dir: &Path,
) -> Result<Option<(usize, Loaded)>, String> {
    if rig.modes.len() < 2 {
        return Ok(None);
    }
    let next = (mode + 1) % rig.modes.len();
    let path = rig
        .scene_path(node, next)
        .ok_or_else(|| format!("no scene for node {node:#04x} in mode {}", rig.modes[next]))?;
    let loaded = load(&dir.join(path))?;
    Ok(Some((next, loaded)))
}

/// The rig's menus, or none when no rig was named.
///
/// The rig owns them; a scene only says where one is drawn. Without `--rig` a
/// menu widget draws its placeholder, which is the same thing it does on a
/// display whose rig forgot the menu.
fn load_menus(rig_path: Option<&Path>) -> Result<Vec<Menu>, String> {
    let Some(rp) = rig_path else {
        return Ok(Vec::new());
    };
    let text = std::fs::read_to_string(rp).map_err(|e| format!("{}: {e}", rp.display()))?;
    let parsed = copilot::rig::parse(&text).map_err(|e| format!("{}: {e:?}", rp.display()))?;
    Ok(parsed.menus)
}

/// The first `menu` widget in `tree`, and which menu it shows.
///
/// The first rather than all of them: a scene with two menu widgets is showing
/// one menu in two places, and a pad drives what the author put first.
fn menu_widget(tree: &Tree) -> Option<(NodeId, String)> {
    (0..tree.len() as u32).find_map(|i| {
        let id = NodeId(i);
        match &tree.get(id)?.kind {
            Kind::Menu { menu, .. } => Some((id, menu.clone())),
            _ => None,
        }
    })
}

/// Bundle what the renderer needs. One place, so a new asset kind is one edit.
fn res<'a>(
    images: &'a ImageTable,
    anims: &'a AnimTable,
    font: &'a Font<'a>,
    menus: &'a [Menu],
) -> Resources<'a> {
    Resources {
        images,
        anims,
        font,
        menus,
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository's `examples` directory, wherever the crate is built.
    fn examples() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the crate sits inside the workspace")
            .join("examples")
    }

    fn rig() -> Rig {
        let text = std::fs::read_to_string(examples().join("z31.rig")).expect("the rig must read");
        copilot::rig::parse(&text).expect("the rig must parse")
    }

    #[test]
    fn tab_walks_the_modes_in_order_and_wraps() {
        let r = rig();
        let dir = examples();
        let mut mode = 0;
        let mut seen = Vec::new();
        for _ in 0..r.modes.len() {
            let (next, _) = next_mode(&r, 0x02, mode, &dir)
                .expect("every mode must have a scene")
                .expect("a three-mode rig has a next mode");
            mode = next;
            seen.push(r.modes[mode].clone());
        }
        assert_eq!(seen, ["sport", "track", "normal"], "modes must cycle");
    }

    #[test]
    fn the_scene_it_loads_is_the_one_the_rig_names() {
        let r = rig();
        let (next, loaded) = next_mode(&r, 0x02, 0, &examples()).unwrap().unwrap();
        assert_eq!(r.scene_path(0x02, next), Some("gauge-left-sport.scene"));
        assert_eq!(
            loaded.node,
            Some(0x02),
            "the scene must agree about its node"
        );
    }

    #[test]
    fn a_single_mode_rig_has_nowhere_to_switch_to() {
        // TAB on a rig with one mode is not an error, it just does nothing.
        let text = r#"{ "modes": ["only"], "displays": [
            { "name": "x", "node": "0x02", "scenes": { "only": "gauge-left-normal.scene" } }] }"#;
        let r = copilot::rig::parse(text).unwrap();
        assert!(next_mode(&r, 0x02, 0, &examples()).unwrap().is_none());
    }

    #[test]
    fn a_node_the_rig_does_not_have_is_reported_rather_than_ignored() {
        let r = rig();
        assert!(next_mode(&r, 0x7f, 0, &examples()).is_err());
    }
}
