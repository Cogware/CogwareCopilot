// SPDX-License-Identifier: GPL-3.0-only
//! Rendering a scene into an egui texture.
//!
//! The preview is drawn by `copilot` itself — the same [`compose_all`] the
//! simulator and a real panel call — so what the editor shows is what the
//! target renders, not an approximation of it. There is no second renderer to
//! keep in step, and no rebuild between editing a file and seeing it: the text
//! is re-parsed and re-composed on every change.

use std::path::Path;

use copilot::anim::Animator;
use copilot::asset::{AnimTable, Image, ImageTable};
use copilot::font::{Font, default_font};
use copilot::render::{Resources, compose_all};
use copilot::scene::{Binding, Shape};
use copilot::widget::{ROOT, Tree};
use copilot::{MemorySurface, PixelFormat, Size};

/// A scene loaded and ready to draw, or the reason it could not be.
pub struct Preview {
    /// The widget tree, or `None` while the text does not parse.
    pub tree: Option<Tree>,
    /// Decoded still images.
    pub images: ImageTable,
    /// Decoded animations.
    pub clips: AnimTable,
    /// Declared property animations.
    pub anims: Animator,
    /// Widgets bound to gauges, resolved against the bus spec at load.
    pub bindings: Vec<Binding>,
    /// The CAN node the scene says it is for, if it says.
    pub node: Option<u8>,
    /// The outline of the panel the scene is drawn on.
    pub shape: Shape,
    /// Why the last load failed, for the status bar.
    pub error: Option<String>,
    /// Assets the scene asked for that could not be read.
    pub missing: Vec<String>,
    /// The scene's own antialiasing setting, which the document root draws
    /// with unless it says otherwise.
    pub antialias: bool,
    font: Font<'static>,
    surface: MemorySurface,
    rgba: Vec<u8>,
}

impl Preview {
    /// A preview with no scene loaded.
    ///
    /// The surface is sized when a scene arrives, since only the scene knows
    /// how big it is.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: None,
            images: ImageTable::new(),
            clips: AnimTable::new(),
            anims: Animator::new(),
            bindings: Vec::new(),
            node: None,
            shape: Shape::Rect,
            error: None,
            missing: Vec::new(),
            antialias: false,
            font: default_font(),
            surface: MemorySurface::new(Size { w: 1, h: 1 }, PixelFormat::Bgrx8888),
            rgba: Vec::new(),
        }
    }

    /// Replace the scene from source text, resolving assets relative to `base`.
    ///
    /// A parse failure leaves the previous tree in place and records the
    /// error. That is deliberate: the text is half-written for most of the
    /// time anyone is typing in it, and blanking the preview on every
    /// keystroke would make the editor useless.
    pub fn load(&mut self, text: &str, base: &Path) {
        let doc = match copilot::scene::parse(text) {
            Ok(d) => d,
            Err(e) => {
                self.error = Some(format!("{e:?}"));
                return;
            }
        };
        let scene = match copilot::scene::build_scene(&doc) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("{e:?}"));
                return;
            }
        };

        self.antialias = doc
            .get("antialias")
            .and_then(copilot::scene::Value::as_bool)
            .unwrap_or(false);
        self.missing.clear();
        self.images = ImageTable::new();
        for request in &scene.requests {
            match std::fs::read(base.join(request))
                .ok()
                .and_then(|b| copilot::asset::qoi::decode(&b).ok())
                .and_then(|(h, px)| Image::new(h.width, h.height, px))
            {
                Some(img) => self.images.push(img),
                None => {
                    self.missing.push(request.clone());
                    self.images.push_missing()
                }
            };
        }

        self.clips = AnimTable::new();
        for request in &scene.anim_requests {
            match std::fs::read(base.join(request))
                .ok()
                .and_then(|b| copilot::asset::gif::decode(&b).ok())
            {
                Some(a) => self.clips.push(a),
                None => {
                    self.missing.push(request.clone());
                    self.clips.push_missing()
                }
            };
        }

        let bounds = scene.tree.get(ROOT).map(|n| n.rect);
        if let Some(r) = bounds {
            self.surface = MemorySurface::new(
                Size {
                    w: r.size.w.max(1),
                    h: r.size.h.max(1),
                },
                PixelFormat::Bgrx8888,
            );
        }
        self.tree = Some(scene.tree);
        self.anims = scene.anims;
        self.bindings = scene.bindings;
        self.node = scene.node;
        self.shape = scene.shape;
        self.error = None;
    }

    /// The scene's own size, which is what the preview is scaled from.
    ///
    /// Known as soon as a scene loads, and needed before the frame is drawn:
    /// the view has to be worked out first so the handles can be sized to it.
    #[must_use]
    pub fn scene_size(&self) -> (u32, u32) {
        let s = copilot::Surface::size(&self.surface);
        (s.w, s.h)
    }

    /// Draw a one-pixel selection outline around `id`, after composing.
    ///
    /// Drawn onto the surface rather than as an egui overlay so it scales with
    /// the preview exactly as the scene does. A widget one pixel tall must
    /// show a marquee one pixel tall, or the highlight lies about the geometry
    /// it is meant to be helping you check.
    fn outline(&mut self, id: copilot::widget::NodeId, grab: i32) {
        let Some(tree) = &self.tree else { return };
        let Some(r) = tree.absolute_rect(id) else {
            return;
        };
        // Magenta for the same reason the missing-asset placeholder is: no
        // scene ever chooses it, so it cannot be mistaken for content.
        copilot::render::stroke_rect(&mut self.surface, r, copilot::Color::rgb(255, 0, 255));

        // The eight grab points, drawn at the size they are clickable at, so
        // the target a person aims for is the target they hit.
        let g = grab;
        let (w, h) = (r.size.w as i32, r.size.h as i32);
        for (ax, ay) in [
            (r.left(), r.top()),
            (r.left() + w / 2, r.top()),
            (r.right(), r.top()),
            (r.left(), r.top() + h / 2),
            (r.right(), r.top() + h / 2),
            (r.left(), r.bottom()),
            (r.left() + w / 2, r.bottom()),
            (r.right(), r.bottom()),
        ] {
            let box_ = copilot::Rect::new(ax - g, ay - g, g as u32 * 2, g as u32 * 2);
            copilot::render::fill_rect(&mut self.surface, box_, copilot::Color::rgb(255, 0, 255));
        }
    }

    /// Advance time, then draw. Returns the frame as RGBA for egui.
    pub fn frame(
        &mut self,
        now_us: u64,
        delta_us: u64,
        selected: Option<copilot::widget::NodeId>,
        grab: i32,
    ) -> Option<(usize, usize, &[u8])> {
        let tree = self.tree.as_mut()?;
        self.anims.tick(tree, now_us);
        copilot::anim::tick_playback(tree, &self.clips, delta_us);

        compose_all(
            &mut self.surface,
            tree,
            Resources {
                images: &self.images,
                anims: &self.clips,
                font: &self.font,
                menus: &[],
            },
        );
        tree.clear_damage();
        if let Some(id) = selected {
            self.outline(id, grab);
        }

        let w = self.surface.stride() / 4;
        let h = self.surface.pixels().len() / self.surface.stride().max(1);
        // egui wants RGBA; the surface holds what a real framebuffer would,
        // which is BGRX. Converting here rather than storing egui's layout
        // keeps the preview honest about the hardware path.
        self.rgba.clear();
        self.rgba.reserve(w * h * 4);
        for px in self.surface.pixels().as_chunks::<4>().0 {
            self.rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        Some((w, h, &self.rgba))
    }
}

impl Default for Preview {
    fn default() -> Self {
        Self::new()
    }
}
