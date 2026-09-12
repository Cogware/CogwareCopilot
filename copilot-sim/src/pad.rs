// SPDX-License-Identifier: GPL-3.0-only
//! A five-button pad in its own window, for driving a menu without hardware.
//!
//! The pad is itself a copilot scene composed into a second window, so the
//! only thing this file adds to the simulator is a hit test and a key map.

use copilot::asset::{AnimTable, ImageTable};
use copilot::font::Font;
use copilot::menu::Button;
use copilot::render::{Resources, compose_all};
use copilot::widget::{Align, Kind, Node, ROOT, Tree, VAlign};
use copilot::{Color, MemorySurface, PixelFormat, Rect, Size};
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

/// Side of the pad window, in pixels.
const SIDE: u32 = 210;

/// Side of one button.
const CELL: i32 = 60;

/// Gap between buttons, and from the window edge.
const GAP: i32 = 10;

/// The five buttons and where each sits in the three-by-three grid.
const KEYS: [(Button, i32, i32, &str); 5] = [
    (Button::Up, 1, 0, "^"),
    (Button::Left, 0, 1, "<"),
    (Button::Centre, 1, 1, "OK"),
    (Button::Right, 2, 1, ">"),
    (Button::Down, 1, 2, "v"),
];

/// A window of five buttons.
pub struct Pad {
    window: Window,
    surface: MemorySurface,
    buffer: Vec<u32>,
    /// The button under the pointer, drawn lit so a click is predictable.
    hot: Option<Button>,
    /// Whether the mouse was already down last frame, so one click is one press.
    was_down: bool,
}

impl Pad {
    /// Open the pad window.
    pub fn new() -> Result<Self, String> {
        let window = Window::new(
            "copilot — buttons",
            SIDE as usize,
            SIDE as usize,
            WindowOptions {
                resize: false,
                ..WindowOptions::default()
            },
        )
        .map_err(|e| format!("cannot open the button window: {e}"))?;
        Ok(Self {
            window,
            surface: MemorySurface::new(Size { w: SIDE, h: SIDE }, PixelFormat::Bgrx8888),
            buffer: vec![0; (SIDE * SIDE) as usize],
            hot: None,
            was_down: false,
        })
    }

    /// Whether the pad window is still open.
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// The rectangle of the button at grid position (`col`, `row`).
    fn cell(col: i32, row: i32) -> Rect {
        Rect::new(
            GAP + col * (CELL + GAP),
            GAP + row * (CELL + GAP),
            CELL as u32,
            CELL as u32,
        )
    }

    /// The button a click at (`x`, `y`) lands on.
    fn at(x: f32, y: f32) -> Option<Button> {
        KEYS.iter().find_map(|(b, col, row, _)| {
            let r = Self::cell(*col, *row);
            let (x, y) = (x as i32, y as i32);
            (x >= r.left() && x < r.right() && y >= r.top() && y < r.bottom()).then_some(*b)
        })
    }

    /// The press this frame, from the mouse or the keyboard.
    ///
    /// One press per click and per key stroke: a held button would otherwise
    /// run a menu off its end in a single frame.
    pub fn press(&mut self) -> Option<Button> {
        self.hot = self
            .window
            .get_mouse_pos(MouseMode::Discard)
            .and_then(|(x, y)| Self::at(x, y));

        let down = self.window.get_mouse_down(MouseButton::Left);
        let clicked = down && !self.was_down;
        self.was_down = down;
        if clicked && let Some(b) = self.hot {
            return Some(b);
        }

        self.window
            .get_keys_pressed(minifb::KeyRepeat::No)
            .iter()
            .find_map(|k| match k {
                Key::Up => Some(Button::Up),
                Key::Down => Some(Button::Down),
                Key::Left => Some(Button::Left),
                Key::Right => Some(Button::Right),
                Key::Enter | Key::Space => Some(Button::Centre),
                _ => None,
            })
    }

    /// Repaint the pad and present it.
    pub fn show(&mut self, font: &Font<'_>) -> Result<(), String> {
        let mut tree = Tree::new(Rect::new(0, 0, SIDE, SIDE));
        for (button, col, row, glyph) in KEYS {
            let lit = self.hot == Some(button);
            let face = if lit {
                Color::rgb(70, 80, 95)
            } else {
                Color::rgb(38, 42, 50)
            };
            let cell = Self::cell(col, row);
            let id = tree
                .push(ROOT, node(cell, Kind::Panel { background: face }))
                .ok_or("the pad tree refused a button")?;
            tree.push(
                id,
                node(
                    Rect::new(0, 0, CELL as u32, CELL as u32),
                    Kind::Label {
                        text: glyph.into(),
                        color: Color::WHITE,
                        scale: 3,
                        align: Align::Center,
                        valign: VAlign::Middle,
                    },
                ),
            )
            .ok_or("the pad tree refused a label")?;
        }

        self.surface.clear(Color::rgb(16, 18, 22));
        let (images, anims) = (ImageTable::new(), AnimTable::new());
        compose_all(
            &mut self.surface,
            &tree,
            Resources {
                images: &images,
                anims: &anims,
                font,
                menus: &[],
            },
        );

        let (pixels, _) = self.surface.pixels().as_chunks::<4>();
        for (word, px) in self.buffer.iter_mut().zip(pixels) {
            *word = u32::from(px[2]) << 16 | u32::from(px[1]) << 8 | u32::from(px[0]);
        }
        self.window
            .update_with_buffer(&self.buffer, SIDE as usize, SIDE as usize)
            .map_err(|e| format!("cannot present the buttons: {e}"))
    }
}

/// A node with the fields a pad button never varies.
fn node(rect: Rect, kind: Kind) -> Node {
    Node {
        rect,
        kind,
        visible: true,
        antialias: None,
        name: None,
        children: Vec::new(),
        parent: None,
    }
}
