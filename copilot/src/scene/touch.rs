// SPDX-License-Identifier: GPL-3.0-only
//! What a widget does when it is touched.
//!
//! A scene says `"touch": { "action": "page", "to": "gps" }` beside a widget's
//! geometry, and the widget becomes something a finger can press. The
//! [`Hotspot`]s live on the [`Scene`], not on the tree, for the same reason
//! [`Binding`](super::Binding)s do: the tree is what gets drawn, and a
//! rectangle that means something is not a drawing property. It also keeps
//! [`Kind`](crate::widget::Kind) to what the rasteriser reads, which is what
//! makes it cheap to clone.
//!
//! # What the core crate will and will not do
//!
//! Turning a page is entirely inside the scene, so [`Scene::touch`] does it
//! and tells the caller it happened. Everything else names something this
//! crate does not own -- a menu lives on the rig, a mode lives on the bus --
//! and comes back as an [`Event`] for the host to act on. That is the same
//! line every other module here draws: the toolkit decides *what was asked
//! for*, the host decides what the car does about it.
//!
//! ```ignore
//! match scene.touch(gesture) {
//!     Some(Event::Paged { page, .. }) => { /* already done; redraw */ }
//!     Some(Event::Menu { menu, button }) => { rig.menu_mut(&menu)?.press(button); }
//!     Some(Event::Mode { mode }) => { can.send(view.request_mode(rig.mode_named(&mode)? as u8)?); }
//!     Some(Event::Emit { name }) => { /* whatever the scene meant by it */ }
//!     None => {}
//! }
//! ```

use alloc::string::{String, ToString};

use crate::Point;
use crate::asset::Scene;
use crate::input::Gesture;
use crate::menu::Button;
use crate::widget::{NodeId, hit_test};

use super::build::BuildError;
use super::value::Value;

/// Which page a [`Action::Page`] means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    /// The page whose widget carries this `name`.
    ///
    /// The form to prefer. Inserting a page in front of another must not
    /// silently re-point every button that jumped past it, which is exactly
    /// what counting to it would do.
    Named(String),
    /// The page at this index.
    Index(u32),
    /// One page further along.
    Next,
    /// One page back.
    Prev,
}

/// What touching a widget asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Turn a pager.
    Page {
        /// Which pager, by the `name` its widget carries. `None` is the
        /// nearest one enclosing the widget that was touched, which is what a
        /// button sitting inside the page it turns almost always wants.
        pager: Option<String>,
        /// Where to turn it to.
        to: Page,
    },
    /// Press a button on one of the rig's menus.
    ///
    /// The menus live on the rig so that a setting survives a mode change, so
    /// this crate cannot press one from inside a scene; it says which menu and
    /// which button and the host does it.
    Menu {
        /// The menu, by its `name` in the rig.
        menu: String,
        /// The button to press.
        button: Button,
    },
    /// Ask the bus for a drive mode, by the name the rig lists it under.
    ///
    /// A whole-bus mode change, unlike a page: every display moves together
    /// and the master decides whether it is allowed. See
    /// [`cogware_can::bus::BusView::request_mode`].
    Mode {
        /// The mode, by its name in the rig's `modes`.
        mode: String,
    },
    /// A name the host attaches its own meaning to.
    ///
    /// The escape hatch, and deliberately opaque: resetting a trip, arming a
    /// launch, sounding a horn. The toolkit has no business knowing what any
    /// of those are, and a scene that needs one should not have to wait for
    /// this enum to grow a variant.
    Emit {
        /// Whatever the host and the scene agreed it means.
        name: String,
    },
}

/// One widget a finger can press, and what it asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotspot {
    /// The widget, whose rectangle is the area that answers.
    pub node: NodeId,
    /// What pressing it asks for.
    pub action: Action,
}

/// What a touch turned out to mean, once the scene has done its half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A pager turned. Already done; reported so the host can redraw and so a
    /// display can tell the master which page it is on if it wants to.
    Paged {
        /// The pager that moved.
        pager: NodeId,
        /// The page it is now showing.
        page: u32,
    },
    /// The host should press `button` on the rig's menu called `menu`.
    Menu {
        /// The menu, by its `name` in the rig.
        menu: String,
        /// The button to press.
        button: Button,
    },
    /// The host should ask the bus for the mode called `mode`.
    Mode {
        /// The mode, by its name in the rig's `modes`.
        mode: String,
    },
    /// The host should do whatever it decided this name means.
    Emit {
        /// The name the scene gave.
        name: String,
    },
}

/// The hotspot a point lands on, if any.
///
/// Walks up from the topmost widget under `point` to the root and answers with
/// the first thing that has an action, so a label inside a button still
/// presses the button.
///
/// Free rather than a method because both hosts here keep their tree apart
/// from the scene it was built from -- the simulator reloads a tree on every
/// save, the editor never builds a [`Scene`] at all -- and a dispatch that
/// insisted on one would send them both round the houses.
/// [`Scene::hotspot_at`] is the same thing for a caller that does have one.
#[must_use]
pub fn hotspot_at<'a>(
    tree: &crate::widget::Tree,
    hotspots: &'a [Hotspot],
    point: Point,
) -> Option<&'a Hotspot> {
    let mut cursor = hit_test(tree, point);
    // Bounded by the node count: the walk is strictly towards the root, but a
    // corrupt tree must not hang the display.
    for _ in 0..=tree.len() {
        let id = cursor?;
        if let Some(h) = hotspots.iter().find(|h| h.node == id) {
            return Some(h);
        }
        cursor = tree.get(id)?.parent;
    }
    None
}

/// Feed a gesture in, and get back what the host has to do about it.
///
/// See [`Scene::touch`], which is this with the tree and hotspots it already
/// holds filled in.
pub fn dispatch(
    tree: &mut crate::widget::Tree,
    hotspots: &[Hotspot],
    gesture: Gesture,
) -> Option<Event> {
    match gesture {
        Gesture::Tap(point) => {
            // Cloned out before the tree is touched: running the action needs
            // the tree mutably and the hotspot is borrowed from the list.
            let h = hotspot_at(tree, hotspots, point)?;
            let (node, action) = (h.node, h.action.clone());
            run(tree, node, &action)
        }
        Gesture::Swipe { from, dir, .. } => {
            let pager = tree.pager_at(from, dir.axis())?;
            let page = tree.step_page(pager, dir.advances())?;
            Some(Event::Paged { pager, page })
        }
        Gesture::Press(_) | Gesture::Release(_) => None,
    }
}

/// Carry out `action` as though the widget at `from` had been touched.
///
/// Split out because the editor drives an action without a finger, and because
/// a host with a hard button wired to the same thing a widget does should not
/// have to fake a tap to use it.
pub fn run(tree: &mut crate::widget::Tree, from: NodeId, action: &Action) -> Option<Event> {
    match action {
        Action::Page { pager, to } => {
            let pager = match pager {
                Some(name) => tree.find(name)?,
                None => tree.enclosing_pager(from)?,
            };
            let page = match to {
                Page::Named(name) => tree
                    .page_named(pager, name)
                    .and_then(|p| tree.set_page(pager, p))?,
                Page::Index(i) => tree.set_page(pager, *i)?,
                Page::Next => tree.step_page(pager, true)?,
                Page::Prev => tree.step_page(pager, false)?,
            };
            Some(Event::Paged { pager, page })
        }
        Action::Menu { menu, button } => Some(Event::Menu {
            menu: menu.clone(),
            button: *button,
        }),
        Action::Mode { mode } => Some(Event::Mode { mode: mode.clone() }),
        Action::Emit { name } => Some(Event::Emit { name: name.clone() }),
    }
}

impl Scene {
    /// The hotspot a point lands on, if any. See [`hotspot_at`].
    ///
    /// A host wanting to light a widget under the finger needs exactly this,
    /// which is why it is public and not only what [`Self::touch`] calls.
    #[must_use]
    pub fn hotspot_at(&self, point: Point) -> Option<&Hotspot> {
        hotspot_at(&self.tree, &self.hotspots, point)
    }

    /// Feed a gesture in, and get back what the host has to do about it.
    ///
    /// A [`Gesture::Tap`] fires the hotspot under it; a [`Gesture::Swipe`]
    /// turns the pager it started over. [`Gesture::Press`] and
    /// [`Gesture::Release`] return `None` -- a press has not committed to
    /// anything yet, and a host that wants to draw a pressed state has
    /// [`Self::hotspot_at`] for it.
    ///
    /// `None` also means "nothing here answered", which a host may use to fall
    /// back to whatever else a swipe across a blank screen should do.
    pub fn touch(&mut self, gesture: Gesture) -> Option<Event> {
        dispatch(&mut self.tree, &self.hotspots, gesture)
    }

    /// Carry out `action` as though the widget at `from` had been touched.
    /// See [`run`].
    pub fn run(&mut self, from: NodeId, action: &Action) -> Option<Event> {
        run(&mut self.tree, from, action)
    }
}

/// The button a `"button"` field names.
fn button(name: &str) -> Option<Button> {
    Some(match name {
        "up" => Button::Up,
        "down" => Button::Down,
        "left" => Button::Left,
        "right" => Button::Right,
        // Both spellings, because the enum is spelt one way and half the
        // keyboards in the world are set up the other.
        "centre" | "center" => Button::Centre,
        _ => return None,
    })
}

/// The name a button is written by, for an editor writing one back out.
#[must_use]
pub fn button_name(b: Button) -> &'static str {
    match b {
        Button::Up => "up",
        Button::Down => "down",
        Button::Left => "left",
        Button::Right => "right",
        Button::Centre => "centre",
    }
}

/// Read a widget's `touch` block.
pub(super) fn hotspot(node: NodeId, spec: &Value) -> Result<Hotspot, BuildError> {
    Ok(Hotspot {
        node,
        action: action(spec)?,
    })
}

/// Read an `action` and whatever fields it needs, from the object holding it.
///
/// Public and taking the whole object rather than the widget, because a
/// [`crate::gpio`] pin fires the same actions from the same field names: a
/// hard button wired to a stalk and a panel a finger presses are the same
/// thing asked for two ways, and two parsers would drift.
pub fn action(spec: &Value) -> Result<Action, BuildError> {
    let name = spec
        .get("action")
        .and_then(Value::as_str)
        .ok_or(BuildError::Missing { field: "action" })?;

    let action = match name {
        "page" => {
            let pager = match spec.get("pager") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_str()
                        .ok_or(BuildError::BadField { field: "pager" })?
                        .to_string(),
                ),
            };
            Action::Page {
                pager,
                to: page(spec)?,
            }
        }
        "menu" => Action::Menu {
            menu: spec
                .get("menu")
                .and_then(Value::as_str)
                .ok_or(BuildError::Missing { field: "menu" })?
                .to_string(),
            button: spec
                .get("button")
                .and_then(Value::as_str)
                .and_then(button)
                .ok_or(BuildError::BadField { field: "button" })?,
        },
        "mode" => Action::Mode {
            mode: spec
                .get("mode")
                .and_then(Value::as_str)
                .ok_or(BuildError::Missing { field: "mode" })?
                .to_string(),
        },
        "emit" => Action::Emit {
            name: spec
                .get("name")
                .and_then(Value::as_str)
                .ok_or(BuildError::Missing { field: "name" })?
                .to_string(),
        },
        other => return Err(BuildError::UnknownAction(other.to_string())),
    };
    Ok(action)
}

/// Read a `page` action's `to`: a name, an index, or a step.
///
/// Defaults to `next`, because a row of arrows down one edge of a screen is
/// the commonest pager there is and `{"action": "page", "to": "next"}` twice
/// over says nothing the shape of the file did not already.
fn page(spec: &Value) -> Result<Page, BuildError> {
    match spec.get("to") {
        None | Some(Value::Null) => Ok(Page::Next),
        Some(Value::Int(i)) => u32::try_from(*i)
            .map(Page::Index)
            .map_err(|_| BuildError::BadField { field: "to" }),
        Some(Value::Str(s)) => Ok(match s.as_str() {
            "next" => Page::Next,
            "prev" | "previous" => Page::Prev,
            name => Page::Named(name.to_string()),
        }),
        Some(_) => Err(BuildError::BadField { field: "to" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Dir;
    use crate::scene::{build_scene, parse};
    use crate::widget::Kind;
    use alloc::format;

    /// A three-page horizontal pager filling a 100x100 scene, with `extra`
    /// spliced in beside it at the root.
    fn paged(extra: &str) -> Scene {
        let src = format!(
            r#"{{"width":100,"height":100,"root":{{
                 "type":"panel","rect":[0,0,100,100],"children":[
                   {{"type":"pager","rect":[0,0,100,100],"name":"pages","children":[
                      {{"type":"panel","rect":[0,0,100,100],"name":"drive"}},
                      {{"type":"panel","rect":[0,0,100,100],"name":"trip"}},
                      {{"type":"panel","rect":[0,0,100,100],"name":"gps"}}]}}
                   {extra}]}}}}"#
        );
        build_scene(&parse(&src).expect("must parse")).expect("must build")
    }

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    fn swipe(dir: Dir) -> Gesture {
        Gesture::Swipe {
            from: at(50, 50),
            to: at(50, 50),
            dir,
        }
    }

    /// Which page a scene's named pager is showing.
    fn showing(s: &Scene) -> u32 {
        let id = s.tree.find("pages").expect("the pager is named");
        s.tree
            .get(id)
            .expect("it exists")
            .kind
            .page()
            .expect("a pager")
    }

    #[test]
    fn a_pager_opens_on_its_first_page_with_the_rest_hidden() {
        let s = paged("");
        assert_eq!(showing(&s), 0);
        assert!(s.tree.get(s.tree.find("drive").unwrap()).unwrap().visible);
        assert!(!s.tree.get(s.tree.find("trip").unwrap()).unwrap().visible);
        assert!(!s.tree.get(s.tree.find("gps").unwrap()).unwrap().visible);
    }

    #[test]
    fn a_pager_can_open_on_a_page_that_is_not_the_first() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"pager","rect":[0,0,10,10],"page":1,"name":"p","children":[
              {"type":"panel","rect":[0,0,10,10],"name":"a"},
              {"type":"panel","rect":[0,0,10,10],"name":"b"}]}}"#;
        let s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(
            s.tree.get(s.tree.find("p").unwrap()).unwrap().kind.page(),
            Some(1)
        );
        assert!(!s.tree.get(s.tree.find("a").unwrap()).unwrap().visible);
        assert!(s.tree.get(s.tree.find("b").unwrap()).unwrap().visible);
    }

    #[test]
    fn a_page_past_the_last_one_opens_on_the_first_instead_of_on_nothing() {
        // A blank screen is the one thing a cluster must never do by
        // accident, so an out-of-range `page` is corrected at build time.
        let src = r#"{"width":10,"height":10,"root":
            {"type":"pager","rect":[0,0,10,10],"page":9,"name":"p","children":[
              {"type":"panel","rect":[0,0,10,10],"name":"a"}]}}"#;
        let s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(showing_named(&s, "p"), 0);
        assert!(s.tree.get(s.tree.find("a").unwrap()).unwrap().visible);
    }

    fn showing_named(s: &Scene, name: &str) -> u32 {
        let id = s.tree.find(name).expect("named");
        s.tree.get(id).unwrap().kind.page().expect("a pager")
    }

    #[test]
    fn swiping_left_turns_a_horizontal_pager_forwards() {
        // Left, as in dragging the page away to the left, brings the next
        // one on -- the convention a page of text uses.
        let mut s = paged("");
        assert_eq!(
            s.touch(swipe(Dir::Left)),
            Some(Event::Paged {
                pager: s.tree.find("pages").unwrap(),
                page: 1
            })
        );
        assert_eq!(showing(&s), 1);
        assert!(s.tree.get(s.tree.find("trip").unwrap()).unwrap().visible);
        assert!(!s.tree.get(s.tree.find("drive").unwrap()).unwrap().visible);
    }

    #[test]
    fn swiping_right_turns_it_back() {
        let mut s = paged("");
        s.touch(swipe(Dir::Left));
        s.touch(swipe(Dir::Right));
        assert_eq!(showing(&s), 0);
    }

    #[test]
    fn a_pager_that_does_not_wrap_stops_at_its_ends() {
        // And says so by answering `None`, which is what lets the swipe fall
        // through to an enclosing pager rather than being swallowed.
        let mut s = paged("");
        assert_eq!(s.touch(swipe(Dir::Right)), None, "already at the first");
        for _ in 0..2 {
            s.touch(swipe(Dir::Left));
        }
        assert_eq!(showing(&s), 2);
        assert_eq!(s.touch(swipe(Dir::Left)), None, "already at the last");
        assert_eq!(showing(&s), 2);
    }

    #[test]
    fn a_wrapping_pager_comes_round() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"pager","rect":[0,0,10,10],"wrap":true,"name":"p","children":[
              {"type":"panel","rect":[0,0,10,10]},
              {"type":"panel","rect":[0,0,10,10]}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        s.touch(Gesture::Swipe {
            from: at(5, 5),
            to: at(5, 5),
            dir: Dir::Right,
        });
        assert_eq!(
            showing_named(&s, "p"),
            1,
            "back past the first wraps to the last"
        );
    }

    #[test]
    fn a_swipe_along_the_wrong_axis_turns_nothing() {
        let mut s = paged("");
        assert_eq!(s.touch(swipe(Dir::Up)), None);
        assert_eq!(s.touch(swipe(Dir::Down)), None);
        assert_eq!(showing(&s), 0);
    }

    #[test]
    fn a_pager_with_swipe_off_is_left_to_its_buttons() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"pager","rect":[0,0,10,10],"swipe":false,"name":"p","children":[
              {"type":"panel","rect":[0,0,10,10]},
              {"type":"panel","rect":[0,0,10,10]}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(s.touch(swipe(Dir::Left)), None);
        assert_eq!(showing_named(&s, "p"), 0);
    }

    #[test]
    fn tapping_a_button_turns_the_pager_it_sits_inside() {
        // No "pager" named: the nearest enclosing one, found by walking up.
        let src = r#"{"width":100,"height":100,"root":
            {"type":"pager","rect":[0,0,100,100],"name":"p","children":[
              {"type":"panel","rect":[0,0,100,100],"children":[
                 {"type":"panel","rect":[0,0,20,20],"name":"btn",
                  "touch":{"action":"page","to":"next"}}]},
              {"type":"panel","rect":[0,0,100,100]}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert!(matches!(
            s.touch(Gesture::Tap(at(10, 10))),
            Some(Event::Paged { page: 1, .. })
        ));
    }

    #[test]
    fn a_tap_on_a_child_fires_the_hotspot_on_its_parent() {
        // The label inside a button is what the finger actually lands on.
        let src = r#"{"width":100,"height":100,"root":
            {"type":"panel","rect":[0,0,100,100],"touch":{"action":"emit","name":"boom"},
             "children":[{"type":"label","rect":[10,10,40,10],"text":"go"}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(
            s.touch(Gesture::Tap(at(15, 15))),
            Some(Event::Emit {
                name: "boom".into()
            })
        );
    }

    #[test]
    fn a_named_page_is_jumped_to_directly() {
        let mut s = paged(
            r#",{"type":"panel","rect":[0,0,10,10],"name":"jump",
                "touch":{"action":"page","pager":"pages","to":"gps"}}"#,
        );
        assert!(matches!(
            s.touch(Gesture::Tap(at(5, 5))),
            Some(Event::Paged { page: 2, .. })
        ));
        assert_eq!(showing(&s), 2);
    }

    #[test]
    fn a_page_named_by_index_is_jumped_to_as_well() {
        let mut s = paged(
            r#",{"type":"panel","rect":[0,0,10,10],
                "touch":{"action":"page","pager":"pages","to":1}}"#,
        );
        assert!(matches!(
            s.touch(Gesture::Tap(at(5, 5))),
            Some(Event::Paged { page: 1, .. })
        ));
    }

    #[test]
    fn jumping_to_a_page_that_is_not_there_does_nothing() {
        let mut s = paged(
            r#",{"type":"panel","rect":[0,0,10,10],
                "touch":{"action":"page","pager":"pages","to":"nowhere"}}"#,
        );
        assert_eq!(s.touch(Gesture::Tap(at(5, 5))), None);
        assert_eq!(showing(&s), 0, "and leaves the page it was on up");
    }

    #[test]
    fn a_menu_action_comes_back_for_the_host_to_press() {
        // The menus live on the rig, so the scene can only say what was asked.
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],
             "touch":{"action":"menu","menu":"display","button":"centre"}}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(
            s.touch(Gesture::Tap(at(5, 5))),
            Some(Event::Menu {
                menu: "display".into(),
                button: Button::Centre
            })
        );
    }

    #[test]
    fn both_spellings_of_centre_are_accepted() {
        for spelling in ["centre", "center"] {
            let src = format!(
                r#"{{"width":10,"height":10,"root":
                  {{"type":"panel","rect":[0,0,10,10],
                    "touch":{{"action":"menu","menu":"m","button":"{spelling}"}}}}}}"#
            );
            let s = build_scene(&parse(&src).unwrap()).unwrap();
            assert_eq!(
                s.hotspots[0].action,
                Action::Menu {
                    menu: "m".into(),
                    button: Button::Centre
                },
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_mode_action_comes_back_for_the_host_to_ask_the_bus_for() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],"touch":{"action":"mode","mode":"track"}}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(
            s.touch(Gesture::Tap(at(5, 5))),
            Some(Event::Mode {
                mode: "track".into()
            })
        );
    }

    #[test]
    fn a_press_and_a_release_ask_for_nothing() {
        // A finger that has landed has not committed to anything yet.
        let mut s = paged("");
        assert_eq!(s.touch(Gesture::Press(at(50, 50))), None);
        assert_eq!(s.touch(Gesture::Release(at(50, 50))), None);
        assert_eq!(showing(&s), 0);
    }

    #[test]
    fn a_tap_on_nothing_asks_for_nothing() {
        let mut s = paged("");
        assert_eq!(s.touch(Gesture::Tap(at(50, 50))), None);
    }

    #[test]
    fn a_host_can_ask_what_is_under_a_finger_before_it_lifts() {
        let s = paged(
            r#",{"type":"panel","rect":[0,0,10,10],"name":"btn",
                "touch":{"action":"emit","name":"trip"}}"#,
        );
        let id = s.tree.find("btn").unwrap();
        assert_eq!(s.hotspot_at(at(5, 5)).map(|h| h.node), Some(id));
        assert!(s.hotspot_at(at(80, 80)).is_none());
    }

    #[test]
    fn an_action_nobody_has_heard_of_fails_to_build() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],"touch":{"action":"explode"}}}"#;
        assert_eq!(
            build_scene(&parse(src).unwrap()).unwrap_err(),
            BuildError::UnknownAction("explode".into())
        );
    }

    #[test]
    fn a_touch_block_with_no_action_fails_to_build() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],"touch":{"to":"next"}}}"#;
        assert_eq!(
            build_scene(&parse(src).unwrap()).unwrap_err(),
            BuildError::Missing { field: "action" }
        );
    }

    #[test]
    fn a_page_action_defaults_to_the_next_page() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],"touch":{"action":"page"}}}"#;
        let s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(
            s.hotspots[0].action,
            Action::Page {
                pager: None,
                to: Page::Next
            }
        );
    }

    #[test]
    fn an_inner_pager_takes_the_swipe_before_the_one_around_it() {
        // Innermost wins, so a strip of pages inside a page answers first.
        let src = r#"{"width":100,"height":100,"root":
            {"type":"pager","rect":[0,0,100,100],"name":"outer","children":[
              {"type":"pager","rect":[0,0,50,50],"name":"inner","children":[
                 {"type":"panel","rect":[0,0,50,50]},
                 {"type":"panel","rect":[0,0,50,50]}]},
              {"type":"panel","rect":[0,0,100,100]}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        let inner = s.tree.find("inner").unwrap();
        assert_eq!(
            s.touch(Gesture::Swipe {
                from: at(10, 10),
                to: at(10, 10),
                dir: Dir::Left
            }),
            Some(Event::Paged {
                pager: inner,
                page: 1
            })
        );
        // Outside the inner pager's rectangle, the outer one answers.
        let outer = s.tree.find("outer").unwrap();
        assert_eq!(
            s.touch(Gesture::Swipe {
                from: at(80, 80),
                to: at(80, 80),
                dir: Dir::Left
            }),
            Some(Event::Paged {
                pager: outer,
                page: 1
            })
        );
    }

    #[test]
    fn a_hidden_pager_answers_nothing() {
        let src = r#"{"width":100,"height":100,"root":
            {"type":"pager","rect":[0,0,100,100],"name":"p","visible":false,"children":[
              {"type":"panel","rect":[0,0,100,100]},
              {"type":"panel","rect":[0,0,100,100]}]}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        assert_eq!(s.touch(swipe(Dir::Left)), None);
    }

    #[test]
    fn an_empty_pager_turns_nothing_rather_than_dividing_by_zero() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"pager","rect":[0,0,10,10],"name":"p","wrap":true}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        let id = s.tree.find("p").unwrap();
        assert_eq!(s.tree.step_page(id, true), None);
        assert_eq!(s.tree.set_page(id, 0), None);
        assert_eq!(s.touch(swipe(Dir::Left)), None);
    }

    #[test]
    fn stepping_something_that_is_not_a_pager_does_nothing() {
        let src = r#"{"width":10,"height":10,"root":
            {"type":"panel","rect":[0,0,10,10],"name":"p"}}"#;
        let mut s = build_scene(&parse(src).unwrap()).unwrap();
        let id = s.tree.find("p").unwrap();
        assert_eq!(s.tree.step_page(id, true), None);
        assert_eq!(s.tree.set_page(id, 0), None);
        assert!(matches!(s.tree.get(id).unwrap().kind, Kind::Panel { .. }));
    }

    #[test]
    fn turning_a_page_marks_both_pages_dirty() {
        // The one that went and the one that came: a page turn that only
        // repaints the arriving page leaves the old one showing through
        // wherever the new one is transparent.
        let mut s = paged("");
        s.tree.clear_damage();
        s.touch(swipe(Dir::Left));
        assert!(!s.tree.damage().is_empty(), "a page turn repainted nothing");
    }
}
