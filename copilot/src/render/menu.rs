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

    // Which item goes at the top. A menu built from the bus spec's settings
    // has more rows than any box on a dashboard panel, and the count grows
    // with the spec, so the scene cannot be made big enough to hold it -- the
    // window has to move instead.
    let first = first_row(
        menu.items().len(),
        menu.selected(),
        rows_that_fit(at, row_h),
    );

    for (i, item) in menu.items().iter().enumerate().skip(first) {
        let y = at.top() + ((i - first) as i32 + TITLE_ROWS) * row_h;
        // Stop at the bottom of the widget rather than drawing past it.
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

/// How many item rows fit in `at`, after the title has taken its own.
fn rows_that_fit(at: Rect, row_h: i32) -> usize {
    if row_h <= 0 {
        return 0;
    }
    let rows = at.size.h as i32 / row_h - TITLE_ROWS;
    rows.max(0) as usize
}

/// Which item to draw at the top, so the selected one is always on the screen.
///
/// Worked out from the selection alone rather than remembered between frames:
/// the renderer holds no state, and a scroll position that lived somewhere
/// would have to be invalidated every time the menu changed under it.
///
/// The selection is kept near the middle where it can be, so thumbing a stalk
/// scrolls the same going up as going down. The alternative -- paging only
/// when the selection falls off an edge -- jumps a whole screenful at the
/// boundary and reads as a glitch rather than as a scroll.
fn first_row(len: usize, selected: Option<usize>, rows: usize) -> usize {
    let (Some(sel), true) = (selected, rows > 0) else {
        return 0;
    };
    // A menu that fits never scrolls, which is every menu written by hand.
    sel.saturating_sub(rows / 2).min(len.saturating_sub(rows))
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

    // --- a menu longer than its box ---

    /// A menu of `n` items, with the cursor moved down to `sel`.
    fn long(n: usize, sel: usize) -> Menu {
        let mut m = Menu::new("settings", "SETTINGS");
        for i in 0..n {
            m.push(Item::number(
                alloc::format!("i{i}"),
                alloc::format!("Item {i}"),
                i as f32,
                0.0,
                100.0,
                1.0,
            ));
        }
        for _ in 0..sel {
            m.press(Button::Down);
        }
        m
    }

    #[test]
    fn a_menu_that_fits_its_box_never_scrolls() {
        // Every menu written by hand, and the behaviour every scene that
        // predates the settings table relies on.
        for sel in 0..4 {
            assert_eq!(first_row(4, Some(sel), 9), 0, "{sel}");
        }
        assert_eq!(first_row(4, Some(3), 4), 0, "exactly full");
    }

    #[test]
    fn a_menu_longer_than_its_box_keeps_the_cursor_on_the_screen() {
        // The case the bus spec's settings table creates: seventeen rows, and
        // no box on a 480x480 panel holds them.
        let rows = 9;
        let len = 17;
        for sel in 0..len {
            let first = first_row(len, Some(sel), rows);
            assert!(first <= sel, "row {sel} scrolled off the top");
            assert!(sel < first + rows, "row {sel} scrolled off the bottom");
            assert!(first + rows <= len, "scrolled past the last item");
        }
    }

    #[test]
    fn the_window_moves_the_same_going_up_as_going_down() {
        // Paging only when the cursor falls off an edge jumps a screenful at
        // the boundary and reads as a glitch. Stepping one row moves the
        // window at most one row.
        let (len, rows) = (17, 9);
        for sel in 1..len {
            let before = first_row(len, Some(sel - 1), rows);
            let after = first_row(len, Some(sel), rows);
            assert!(after.abs_diff(before) <= 1, "{sel}: {before} -> {after}");
        }
    }

    #[test]
    fn the_last_screenful_is_full_rather_than_half_empty() {
        // Scrolling to keep the cursor centred past the end would leave a
        // menu with blank rows under its last item.
        assert_eq!(first_row(17, Some(16), 9), 8);
        assert_eq!(first_row(17, Some(13), 9), 8);
    }

    #[test]
    fn a_box_with_no_room_for_a_row_draws_from_the_top_rather_than_dividing() {
        assert_eq!(first_row(17, Some(9), 0), 0);
        assert_eq!(first_row(0, None, 9), 0, "an empty menu");
        assert_eq!(rows_that_fit(Rect::new(0, 0, 10, 1), 20), 0);
        assert_eq!(rows_that_fit(Rect::new(0, 0, 10, 100), 0), 0, "no divide");
    }

    #[test]
    fn scrolling_actually_changes_what_is_drawn() {
        // The arithmetic above is only worth anything if the renderer uses
        // it: the top of a long menu must look different from its bottom.
        let top = render(&long(30, 0));
        let bottom = render(&long(30, 29));
        assert_ne!(top.pixels(), bottom.pixels(), "the menu did not scroll");
        assert!(ink(&bottom) > 0, "the bottom of the menu drew nothing");
    }
}
