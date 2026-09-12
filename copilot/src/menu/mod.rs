// SPDX-License-Identifier: GPL-3.0-only
//! Menus: values a driver changes on the screen and a host acts on.
//!
//! A [`Menu`] owns its items, which one is selected and whether that one is
//! being edited; it owns no hardware and applies nothing. The host pushes
//! [`Button`] presses in and reads values back out by name, exactly as it
//! already feeds gauge readings in and reads [`crate::asset::Scene::wanted`].

use alloc::string::String;
use alloc::vec::Vec;

mod item;

pub use item::{Item, ItemKind, Value};

/// A button on whatever the driver actually presses.
///
/// Five, because that is the pad every cluster stalk and steering control
/// reduces to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Button {
    /// Move the selection towards the first item.
    Up,
    /// Move the selection towards the last item.
    Down,
    /// Decrease the selected value while editing.
    Left,
    /// Increase the selected value while editing.
    Right,
    /// Start editing the selected item, fire it, or finish editing.
    Centre,
}

/// A list of items the driver moves through, one of which is selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    /// What a scene's `menu` widget names to show this one.
    pub name: String,
    /// What a person reads at the top of it.
    pub title: String,
    items: Vec<Item>,
    selected: usize,
    editing: bool,
}

impl Menu {
    /// An empty menu, looked up by `name` and headed with `title`.
    #[must_use]
    pub fn new(name: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            items: Vec::new(),
            selected: 0,
            editing: false,
        }
    }

    /// Append an item, returning its index.
    pub fn push(&mut self, item: Item) -> usize {
        self.items.push(item);
        self.items.len() - 1
    }

    /// The items, in the order they are drawn.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Which item the cursor is on, or `None` when the menu is empty.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        (!self.items.is_empty()).then_some(self.selected)
    }

    /// Whether the cursor is inside the selected item's value rather than on
    /// the item.
    ///
    /// The renderer needs both states: one says up and down move the cursor,
    /// the other says they change a number.
    #[must_use]
    pub const fn editing(&self) -> bool {
        self.editing
    }

    /// The item named `name`, if the menu has one.
    #[must_use]
    pub fn item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.name == name)
    }

    /// The current value of the item named `name`.
    ///
    /// This is the whole of the host-facing API: a consuming crate asks for
    /// `"display brightness"` and applies the answer wherever it belongs.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<Value> {
        self.item(name).map(Item::value)
    }

    /// Take the pending fire of the action named `name`, clearing it.
    ///
    /// Returns `false` for an item that is not an action, so a host draining
    /// every name it knows cannot accidentally fire a number.
    pub fn take_action(&mut self, name: &str) -> bool {
        self.items
            .iter_mut()
            .find(|i| i.name == name)
            .is_some_and(Item::take_fired)
    }

    /// Apply a button press, returning whether anything changed.
    ///
    /// `false` means the press did nothing at all -- a menu with no items, or
    /// a value already at its limit -- and a caller can use it to decide
    /// whether the menu needs repainting.
    pub fn press(&mut self, button: Button) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.selected = self.selected.min(self.items.len() - 1);
        match (button, self.editing) {
            (Button::Up, false) => self.move_to(self.selected.checked_sub(1)),
            (Button::Down, false) => self.move_to(Some(self.selected + 1)),
            (Button::Centre, false) => self.activate(),
            (Button::Centre, true) => {
                self.editing = false;
                true
            }
            (Button::Left | Button::Down, true) => self.items[self.selected].step(-1),
            (Button::Right | Button::Up, true) => self.items[self.selected].step(1),
            // Left and right do nothing until centre has opened the item: a
            // driver scrolling past brightness must not be able to change it
            // by brushing sideways.
            (Button::Left | Button::Right, false) => false,
        }
    }

    /// Move the cursor, refusing to fall off either end.
    fn move_to(&mut self, to: Option<usize>) -> bool {
        let Some(to) = to.filter(|i| *i < self.items.len()) else {
            return false;
        };
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Fire an action, or begin editing anything else.
    fn activate(&mut self) -> bool {
        let item = &mut self.items[self.selected];
        if item.fire() {
            return true;
        }
        self.editing = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn menu() -> Menu {
        let mut m = Menu::new("display", "Display");
        m.push(Item::number(
            "display brightness",
            "Brightness",
            50.0,
            0.0,
            100.0,
            10.0,
        ));
        m.push(Item::choice(
            "units",
            "Units",
            0,
            vec!["metric".into(), "imperial".into()],
        ));
        m.push(Item::action("reset trip", "Reset trip"));
        m
    }

    #[test]
    fn a_host_reads_a_value_by_the_name_the_scene_gave_it() {
        let m = menu();
        assert_eq!(m.value("display brightness"), Some(Value::Number(50.0)));
        assert_eq!(m.value("units"), Some(Value::Choice(0)));
        assert_eq!(m.value("no such thing"), None);
    }

    #[test]
    fn a_value_only_moves_once_centre_has_opened_it() {
        // A driver scrolling past brightness must not change it sideways.
        let mut m = menu();
        assert!(!m.press(Button::Right));
        assert_eq!(m.value("display brightness"), Some(Value::Number(50.0)));

        assert!(m.press(Button::Centre));
        assert!(m.editing());
        assert!(m.press(Button::Right));
        assert_eq!(m.value("display brightness"), Some(Value::Number(60.0)));
    }

    #[test]
    fn centre_closes_the_item_it_opened() {
        let mut m = menu();
        m.press(Button::Centre);
        assert!(m.editing());
        assert!(m.press(Button::Centre));
        assert!(!m.editing());
    }

    #[test]
    fn up_and_down_move_the_cursor_and_stop_at_the_ends() {
        let mut m = menu();
        assert!(!m.press(Button::Up), "already at the top");
        assert_eq!(m.selected(), Some(0));
        assert!(m.press(Button::Down));
        assert!(m.press(Button::Down));
        assert_eq!(m.selected(), Some(2));
        assert!(!m.press(Button::Down), "already at the bottom");
    }

    #[test]
    fn a_number_stops_at_its_limits() {
        let mut m = menu();
        m.press(Button::Centre);
        for _ in 0..20 {
            m.press(Button::Right);
        }
        assert_eq!(m.value("display brightness"), Some(Value::Number(100.0)));
        assert!(!m.press(Button::Right), "at the top, nothing changed");
        for _ in 0..20 {
            m.press(Button::Left);
        }
        assert_eq!(m.value("display brightness"), Some(Value::Number(0.0)));
    }

    #[test]
    fn a_choice_walks_its_options_and_does_not_wrap() {
        // Wrapping would make a long list a lottery to land on.
        let mut m = menu();
        m.press(Button::Down);
        m.press(Button::Centre);
        assert!(m.press(Button::Right));
        assert_eq!(m.value("units"), Some(Value::Choice(1)));
        assert!(!m.press(Button::Right));
        assert_eq!(m.item("units").and_then(Item::shown), Some("imperial"));
    }

    #[test]
    fn an_action_fires_on_centre_and_is_taken_once() {
        let mut m = menu();
        m.press(Button::Down);
        m.press(Button::Down);
        assert!(m.press(Button::Centre));
        assert!(!m.editing(), "an action has nothing to edit");
        assert_eq!(m.value("reset trip"), Some(Value::Action(true)));
        assert!(m.take_action("reset trip"));
        assert!(!m.take_action("reset trip"), "it fires once per press");
    }

    #[test]
    fn taking_a_number_as_an_action_does_nothing() {
        // A host draining every name it knows must not fire a setting.
        let mut m = menu();
        assert!(!m.take_action("display brightness"));
        assert_eq!(m.value("display brightness"), Some(Value::Number(50.0)));
    }

    #[test]
    fn an_empty_menu_answers_every_press_with_nothing() {
        let mut m = Menu::new("empty", "Empty");
        for b in [
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
            Button::Centre,
        ] {
            assert!(!m.press(b));
        }
        assert_eq!(m.selected(), None);
    }

    #[test]
    fn a_number_authored_out_of_range_is_clamped_into_it() {
        let i = Item::number("x", "X", 500.0, 0.0, 100.0, 1.0);
        assert_eq!(i.value(), Value::Number(100.0));
        let n = Item::number("x", "X", f32::NAN, 0.0, 100.0, 1.0);
        assert_eq!(n.value(), Value::Number(0.0));
    }

    #[test]
    fn a_reversed_range_becomes_a_range_rather_than_a_dead_item() {
        let mut i = Item::number("x", "X", 5.0, 10.0, 0.0, 1.0);
        assert_eq!(i.value(), Value::Number(5.0));
        assert!(i.step(1));
    }

    #[test]
    fn a_choice_defaulting_past_its_options_opens_on_the_first() {
        let i = Item::choice("x", "X", 9, vec!["a".into(), "b".into()]);
        assert_eq!(i.value(), Value::Choice(0));
    }

    #[test]
    fn a_choice_with_no_options_cannot_be_stepped() {
        let mut i = Item::choice("x", "X", 0, Vec::new());
        assert!(!i.step(1));
        assert_eq!(i.shown(), None);
    }

    #[test]
    fn a_zero_step_still_moves_the_number() {
        // A file with "step": 0 would otherwise author an item no press moves.
        let mut i = Item::number("x", "X", 0.0, 0.0, 10.0, 0.0);
        assert!(i.step(1));
        assert_eq!(i.value(), Value::Number(1.0));
    }
}
