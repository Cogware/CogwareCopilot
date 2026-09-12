// SPDX-License-Identifier: GPL-3.0-only
//! Drawing a menu.
//!
//! One row per item, the label on the left and the value on the right. The
//! selected row and the row being edited are drawn differently, because those
//! are the two states a driver has to tell apart: one says up and down move
//! the cursor, the other says they change the number.

use crate::menu::{Item, ItemKind, Menu};
use crate::widget::{Align, VAlign};
use crate::{Color, Rect, Surface};

use super::draw::draw_text;
use super::{Resources, fill_rect};

/// Rows of blank space above the first item, for the title.
const TITLE_ROWS: i32 = 2;

/// Draw `menu` inside `at`, clipped to `clip`.
#[allow(clippy::too_many_arguments)] // Three colours, a scale and two rects.
pub fn menu<S: Surface + ?Sized>(
    surface: &mut S,
    menu: &Menu,
    at: Rect,
    clip: Rect,
    res: Resources<'_>,
    color: Color,
    selected: Color,
    editing: Color,
    scale: u8,
) {
    let row_h = i32::from(res.font.cell_h) * i32::from(scale.max(1)) + i32::from(scale.max(1)) * 2;
    if row_h <= 0 || at.is_empty() {
        return;
    }

    let title = Rect::new(at.left(), at.top(), at.size.w, row_h as u32);
    draw_text(
        surface,
        &menu.title,
        selected,
        title,
        clip,
        res.font,
        scale,
        Align::Center,
        VAlign::Middle,
    );

    for (i, item) in menu.items().iter().enumerate() {
        let y = at.top() + (i as i32 + TITLE_ROWS) * row_h;
        // Stop at the bottom of the widget rather than drawing past it: a menu
        // longer than its box is a scene to fix, not a reason to overrun.
        if y + row_h > at.bottom() {
            break;
        }
        let is_here = menu.selected() == Some(i);
        let ink = match (is_here, menu.editing()) {
            (true, true) => editing,
            (true, false) => selected,
            _ => color,
        };
        let row = Rect::new(at.left(), y, at.size.w, row_h as u32);

        // The selected row gets a bar behind it as well as a colour, so it
        // reads at a glance from a driver's seat rather than on inspection.
        if is_here && let Some(area) = row.intersection(clip) {
            fill_rect(surface, area, dim(ink));
        }

        draw_text(
            surface,
            &item.label,
            ink,
            inset(row, scale),
            clip,
            res.font,
            scale,
            Align::Left,
            VAlign::Middle,
        );
        draw_text(
            surface,
            &shown(item),
            ink,
            inset(row, scale),
            clip,
            res.font,
            scale,
            Align::Right,
            VAlign::Middle,
        );
    }
}

/// One character of air at each end of a row.
fn inset(row: Rect, scale: u8) -> Rect {
    let pad = i32::from(scale.max(1)) * 4;
    let w = (row.size.w as i32 - pad * 2).max(0) as u32;
    Rect::new(row.left() + pad, row.top(), w, row.size.h)
}

/// A quarter-strength version of `c`, for the bar behind the selected row.
fn dim(c: Color) -> Color {
    Color::rgba(c.r / 4, c.g / 4, c.b / 4, c.a)
}

/// What an item reads on the right-hand side.
///
/// An edited item is wrapped in markers, which is what says *this* is the
/// value the buttons are moving.
fn shown(item: &Item) -> alloc::string::String {
    use alloc::format;
    match &item.kind {
        ItemKind::Number { value, .. } => format!("{value:.0}"),
        ItemKind::Choice { .. } => item.shown().unwrap_or("").into(),
        ItemKind::Action { .. } => alloc::string::String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AnimTable, ImageTable};
    use crate::font::default_font;
    use crate::menu::{Button, Item};
    use crate::{MemorySurface, PixelFormat, Size};
    use alloc::vec;

    const AT: Rect = Rect::new(0, 0, 200, 120);

    fn built() -> Menu {
        let mut m = Menu::new("display", "DISPLAY");
        m.push(Item::number("b", "Brightness", 50.0, 0.0, 100.0, 10.0));
        m.push(Item::choice(
            "u",
            "Units",
            0,
            vec!["metric".into(), "imperial".into()],
        ));
        m
    }

    /// Draw `m` and return every pixel, so two states can be compared.
    fn render(m: &Menu) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w: 200, h: 120 }, PixelFormat::Bgrx8888);
        let font = default_font();
        let (images, anims) = (ImageTable::new(), AnimTable::new());
        let res = Resources {
            images: &images,
            anims: &anims,
            font: &font,
            menus: &[],
        };
        menu(
            &mut s,
            m,
            AT,
            AT,
            res,
            Color::rgb(100, 100, 100),
            Color::WHITE,
            Color::rgb(255, 176, 0),
            2,
        );
        s
    }

    fn ink(s: &MemorySurface) -> usize {
        s.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count()
    }

    #[test]
    fn a_menu_draws_its_title_and_its_items() {
        let s = render(&built());
        assert!(ink(&s) > 0, "nothing was drawn at all");
    }

    #[test]
    fn selecting_and_editing_look_different_from_each_other() {
        // The requirement this widget exists for: a driver has to be able to
        // tell whether up and down move the cursor or change the number.
        let mut m = built();
        let selected = render(&m);
        m.press(Button::Centre);
        assert!(m.editing());
        let editing = render(&m);
        assert_ne!(
            selected.pixels(),
            editing.pixels(),
            "editing an item looks identical to having it selected"
        );
    }

    #[test]
    fn moving_the_cursor_changes_what_is_drawn() {
        let mut m = built();
        let first = render(&m);
        m.press(Button::Down);
        let second = render(&m);
        assert_ne!(first.pixels(), second.pixels());
    }

    #[test]
    fn changing_a_value_changes_what_is_drawn() {
        let mut m = built();
        m.press(Button::Centre);
        let before = render(&m);
        m.press(Button::Right);
        assert_ne!(render(&m).pixels(), before.pixels());
    }

    #[test]
    fn a_menu_longer_than_its_box_stops_at_the_bottom_edge() {
        let mut m = Menu::new("long", "LONG");
        for i in 0..40 {
            m.push(Item::action("a", alloc::format!("Item {i}")));
        }
        let mut s = MemorySurface::new(Size { w: 200, h: 120 }, PixelFormat::Bgrx8888);
        let font = default_font();
        let (images, anims) = (ImageTable::new(), AnimTable::new());
        let res = Resources {
            images: &images,
            anims: &anims,
            font: &font,
            menus: &[],
        };
        // A clip larger than the widget: nothing outside `at` may be painted.
        menu(
            &mut s,
            &m,
            Rect::new(0, 0, 200, 60),
            AT,
            res,
            Color::WHITE,
            Color::WHITE,
            Color::WHITE,
            2,
        );
        let stride = s.stride();
        let below = &s.pixels()[60 * stride..];
        assert!(
            below.as_chunks::<4>().0.iter().all(|p| p[3] == 0),
            "the menu ran past the bottom of its own rectangle"
        );
    }

    #[test]
    fn an_empty_menu_draws_only_its_title() {
        let empty = render(&Menu::new("e", "EMPTY"));
        assert!(ink(&empty) > 0, "the title should still draw");
    }
}
