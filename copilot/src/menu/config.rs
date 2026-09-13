// SPDX-License-Identifier: GPL-3.0-only
//! A menu of the display's own settings, built from the bus spec.
//!
//! [`cogware_can::config`] describes every setting a display holds well enough
//! to draw a form from: its kind, its unit, its range, its default and a line
//! saying what it is for. That description exists so that this crate does not
//! have to hardcode one, and this module is where it is taken up -- a rig says
//! `{"name": "settings", "from": "config"}` and gets a menu of the lot, which
//! grows a row the day the spec grows a setting and needs no edit here.
//!
//! ```ignore
//! let mut m = Menu::from_config("settings", "Display");   // build it
//! m.press(Button::Centre);                                // driver edits it
//! m.press(Button::Right);
//! m.apply_config();                                       // write it back
//! ```
//!
//! # Why values are written back rather than through
//!
//! A [`Setting`](cogware_can::config::Setting) is live: writing one takes
//! effect the next time the firmware reads it. Writing on every press would
//! mean a driver scrolling the brightness swept the backlight through every
//! value on the way, and an editor previewing a menu would reconfigure the
//! machine it is running on. [`Menu::apply_config`] is one call at the point
//! the host decides the change is meant, and [`Menu::load_config`] is the
//! other direction, for a menu opening on what the settings actually hold.
//!
//! # Physical units, not counts
//!
//! The spec stores fixed point -- brightness is 0..=1000 because percent has
//! a scale of ten -- and a menu shows what a person reads, so an item's
//! number is `counts / unit.scale`. The conversion is the only arithmetic
//! here and it is undone exactly on the way back.

use alloc::string::String;
use alloc::vec::Vec;

use cogware_can::config::{ALL_SETTINGS, Kind as SettingKind, Setting};

use super::{Item, ItemKind, Menu};

/// The name a rig's `"from"` field uses to ask for this menu.
pub const SOURCE: &str = "config";

/// What one press of a number should move it by, in physical units.
///
/// A hundred presses end to end. A step of one count would make `bus_timeout`
/// -- ten million of them -- a setting nobody could reach the end of, and a
/// step of a whole unit would make a range of two useless.
fn step_for(min: f32, max: f32) -> f32 {
    let span = (max - min).abs();
    if span <= 0.0 {
        return 1.0;
    }
    // Never finer than a hundredth: past that the number on the screen stops
    // changing in a way anyone can see it change.
    (span / 100.0).max(0.01)
}

/// A setting's fixed-point counts as the number a person reads.
fn physical(setting: &Setting, counts: i32) -> f32 {
    let scale = setting.unit.scale.max(1);
    counts as f32 / scale as f32
}

/// The inverse: what a person typed, back into the counts the spec stores.
fn counts(setting: &Setting, value: f32) -> i32 {
    let scale = setting.unit.scale.max(1) as f32;
    // Rounded, not truncated: 80.0 percent through a scale of ten must be
    // 800 and not 799, however the float landed. `f32::round` is `std`, and
    // this crate has `core`, so it is done by hand -- adding a half towards
    // whichever side the number is on is exactly what `round` does.
    let raw = value * scale;
    let rounded = if raw < 0.0 { raw - 0.5 } else { raw + 0.5 } as i32;
    rounded.clamp(setting.min, setting.max)
}

/// One menu row for one setting, holding the value the setting holds now.
///
/// The item's `name` is the setting's `key`, which is what lets
/// [`Menu::apply_config`] find its way back without a table of its own -- and
/// what a host reads a value by, exactly as it does for a hand-written menu.
#[must_use]
pub fn item_for(setting: &'static Setting) -> Item {
    // The label is the key made readable: the spec's `doc` is a whole
    // sentence, which belongs beside the item rather than in place of its
    // name, and there is no third string to use.
    let label = label_for(setting.key);
    match setting.kind {
        SettingKind::Toggle => Item::choice(
            setting.key,
            label,
            u32::from(setting.is_on()),
            // A toggle reads better as two words than as a 0 and a 1, and the
            // menu already knows how to walk a list of options.
            alloc::vec!["off".into(), "on".into()],
        ),
        SettingKind::Choice => Item::choice(
            setting.key,
            label,
            u32::try_from(setting.get().max(0)).unwrap_or(0),
            setting.choices.iter().map(|c| (*c).into()).collect(),
        ),
        SettingKind::Number => {
            let (min, max) = (
                physical(setting, setting.min),
                physical(setting, setting.max),
            );
            Item::number(
                setting.key,
                label,
                physical(setting, setting.get()),
                min,
                max,
                step_for(min, max),
            )
        }
    }
}

/// `shift_light_rpm` as `Shift light rpm`.
///
/// The spec's keys are the stable identifiers a blob travels under and are
/// spelt for a machine; a driver reading the screen is not that machine.
fn label_for(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for (i, c) in key.chars().enumerate() {
        match c {
            '_' => out.push(' '),
            c if i == 0 => out.extend(c.to_uppercase()),
            c => out.push(c),
        }
    }
    out
}

impl Menu {
    /// Every setting the bus spec describes, as a menu.
    ///
    /// Built from [`cogware_can::config::ALL_SETTINGS`] in table order, each
    /// item starting at the value the setting currently holds.
    #[must_use]
    pub fn from_config(name: impl Into<String>, title: impl Into<String>) -> Self {
        let mut menu = Menu::new(name, title);
        for setting in ALL_SETTINGS {
            menu.push(item_for(setting));
        }
        menu
    }

    /// Write every item back to the setting with its name.
    ///
    /// Returns how many settings actually changed, so a host can skip saving
    /// a config blob nobody touched. Items naming nothing in the spec -- a
    /// hand-written row mixed in, or a setting this build does not have -- are
    /// skipped rather than refused, the same way a config blob's unknown keys
    /// are.
    pub fn apply_config(&self) -> usize {
        let mut changed = 0;
        for item in self.items() {
            let Some(setting) = cogware_can::config::by_key(&item.name) else {
                continue;
            };
            let want = match &item.kind {
                ItemKind::Number { value, .. } => counts(setting, *value),
                ItemKind::Choice { index, .. } => *index as i32,
                // An action is a press, not a setting. Firing one must not
                // write a 0 or a 1 over whatever the key holds.
                ItemKind::Action { .. } => continue,
            };
            // Read back rather than trusting the return: `Setting::set`
            // answers whether the value had to be clamped, not whether it
            // moved, and `counts` has already clamped it into range.
            let before = setting.get();
            setting.set(want);
            if setting.get() != before {
                changed += 1;
            }
        }
        changed
    }

    /// Set every item from the setting with its name.
    ///
    /// The other direction, for a menu opening on what the display actually
    /// holds rather than on what it held when the menu was built. Returns how
    /// many items moved.
    pub fn load_config(&mut self) -> usize {
        let mut moved = 0;
        // Replaced wholesale rather than poked field by field: `Item::number`
        // is where the clamping and the ordering of a range live, and doing
        // it again here is how the two drift apart.
        let refreshed: Vec<Option<Item>> = self
            .items()
            .iter()
            .map(|i| cogware_can::config::by_key(&i.name).map(item_for))
            .collect();
        for (item, fresh) in self.items_mut().iter_mut().zip(refreshed) {
            let Some(fresh) = fresh else { continue };
            if *item != fresh {
                *item = fresh;
                moved += 1;
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{Button, Value};
    use cogware_can::config;

    /// Serialises these tests and puts the settings back as they were found.
    ///
    /// The settings table is global and live, exactly as the gauge table is,
    /// and the harness runs tests in parallel.
    static SETTINGS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    struct Settings;

    impl Settings {
        fn take() -> Self {
            use core::sync::atomic::Ordering::{Acquire, Relaxed};
            while SETTINGS
                .compare_exchange_weak(false, true, Acquire, Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            config::reset_all();
            Settings
        }
    }

    impl Drop for Settings {
        fn drop(&mut self) {
            config::reset_all();
            SETTINGS.store(false, core::sync::atomic::Ordering::Release);
        }
    }

    #[test]
    fn the_menu_has_a_row_for_every_setting_the_spec_describes() {
        let _s = Settings::take();
        let m = Menu::from_config("settings", "Display");
        assert_eq!(m.items().len(), ALL_SETTINGS.len());
        for (item, setting) in m.items().iter().zip(ALL_SETTINGS) {
            assert_eq!(item.name, setting.key, "rows must follow the table");
        }
    }

    #[test]
    fn a_number_is_shown_in_its_unit_rather_than_in_counts() {
        // Brightness is stored 0..=1000 because percent has a scale of ten.
        // What a person reads is 80, not 800.
        let _s = Settings::take();
        let m = Menu::from_config("settings", "Display");
        assert_eq!(m.value("brightness"), Some(Value::Number(80.0)));
    }

    #[test]
    fn a_choice_offers_the_spec_s_own_labels() {
        let _s = Settings::take();
        let m = Menu::from_config("settings", "Display");
        let item = m.item("units_temp").expect("the spec has one");
        assert_eq!(item.shown(), Some("Celsius"));
        assert_eq!(
            match &item.kind {
                ItemKind::Choice { options, .. } => options.len(),
                _ => 0,
            },
            config::by_key("units_temp").unwrap().choices.len()
        );
    }

    #[test]
    fn a_toggle_reads_as_a_word_rather_than_a_number() {
        let _s = Settings::take();
        let m = Menu::from_config("settings", "Display");
        assert_eq!(m.item("demo_mode").and_then(Item::shown), Some("off"));
    }

    #[test]
    fn a_key_becomes_a_label_a_driver_can_read() {
        assert_eq!(label_for("shift_light_rpm"), "Shift light rpm");
        assert_eq!(label_for("brightness"), "Brightness");
        assert_eq!(label_for(""), "");
    }

    #[test]
    fn editing_an_item_and_applying_it_writes_the_setting() {
        let _s = Settings::take();
        let mut m = Menu::from_config("settings", "Display");
        assert_eq!(m.selected(), Some(0), "brightness is first in the table");
        m.press(Button::Centre);
        m.press(Button::Left);
        assert_eq!(m.apply_config(), 1);
        let b = config::by_key("brightness").unwrap();
        assert!(b.get() < 800, "one press down left it at {}", b.get());
        assert_eq!(
            b.as_f32(),
            m.value("brightness").map_or(0.0, |v| match v {
                Value::Number(n) => n,
                _ => 0.0,
            })
        );
    }

    #[test]
    fn a_full_press_of_a_number_lands_on_a_whole_count() {
        // The round trip through a float must not leave brightness at 799.
        let _s = Settings::take();
        let mut m = Menu::from_config("settings", "Display");
        m.press(Button::Centre);
        for _ in 0..200 {
            m.press(Button::Right);
        }
        m.apply_config();
        let b = config::by_key("brightness").unwrap();
        assert_eq!(b.get(), b.max, "pressed to the top");
    }

    #[test]
    fn applying_an_unchanged_menu_writes_nothing() {
        let _s = Settings::take();
        let m = Menu::from_config("settings", "Display");
        assert_eq!(m.apply_config(), 0);
        assert_eq!(config::changed_count(), 0);
    }

    #[test]
    fn a_choice_applies_as_its_index() {
        let _s = Settings::take();
        let mut m = Menu::from_config("settings", "Display");
        // Walk to units_temp and open it.
        while m.selected().map(|i| m.items()[i].name.as_str()) != Some("units_temp") {
            assert!(m.press(Button::Down), "units_temp must be in the table");
        }
        m.press(Button::Centre);
        m.press(Button::Right);
        m.apply_config();
        assert_eq!(config::by_key("units_temp").unwrap().get(), 1);
        assert_eq!(
            config::by_key("units_temp").unwrap().choice(),
            Some("Fahrenheit")
        );
    }

    #[test]
    fn loading_picks_up_a_setting_changed_behind_the_menu_s_back() {
        let _s = Settings::take();
        let mut m = Menu::from_config("settings", "Display");
        config::by_key("brightness").unwrap().set(300);
        assert_eq!(m.load_config(), 1);
        assert_eq!(m.value("brightness"), Some(Value::Number(30.0)));
        assert_eq!(m.load_config(), 0, "nothing moved the second time");
    }

    #[test]
    fn a_row_the_spec_does_not_know_is_left_alone_rather_than_refused() {
        // A hand-written action mixed in with the settings: applying the menu
        // must not try to write it, and must not give up on the rest.
        let _s = Settings::take();
        let mut m = Menu::from_config("settings", "Display");
        m.push(Item::action("reset trip", "Reset trip"));
        m.push(Item::number("nonesuch", "Nonesuch", 5.0, 0.0, 10.0, 1.0));
        assert_eq!(m.apply_config(), 0);
        assert_eq!(m.load_config(), 0);
        assert_eq!(m.value("reset trip"), Some(Value::Action(false)));
    }

    #[test]
    fn every_setting_survives_a_round_trip_through_the_menu() {
        // The one check that the two representations agree: build a menu from
        // the settings, write it straight back, and nothing should move.
        let _s = Settings::take();
        for setting in ALL_SETTINGS {
            // Somewhere that is not the default, so the trip is not trivially
            // the identity: a third of the way up its own range.
            setting.set(setting.min + (setting.max - setting.min) / 3);
        }
        let m = Menu::from_config("settings", "Display");
        let before: Vec<i32> = ALL_SETTINGS.iter().map(|s| s.get()).collect();
        m.apply_config();
        let after: Vec<i32> = ALL_SETTINGS.iter().map(|s| s.get()).collect();
        for ((s, b), a) in ALL_SETTINGS.iter().zip(&before).zip(&after) {
            // A number is shown to the unit's own precision, so it comes back
            // rounded to that; a choice and a toggle are exact.
            let slack = if s.kind == SettingKind::Number { 1 } else { 0 };
            assert!(
                (a - b).abs() <= slack,
                "{} went out as {b} and came back as {a}",
                s.key
            );
        }
    }

    #[test]
    fn a_step_is_never_zero_however_narrow_the_range() {
        // A step of zero is an item no press can move, which reads on the
        // screen as a broken menu rather than as a fixed setting.
        assert!(step_for(0.0, 0.0) > 0.0);
        assert!(step_for(5.0, 5.0) > 0.0);
        assert!(step_for(0.0, 1.0) > 0.0);
        assert!(step_for(10.0, 0.0) > 0.0, "a reversed range still steps");
    }
}
