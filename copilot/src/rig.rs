// SPDX-License-Identifier: MIT OR Apache-2.0
//! A rig: every display on one bus, and the scene each shows in each mode.
//!
//! A car has more than one screen -- a 2400x900 cluster and a pair of round
//! 480x480 gauges, say -- and more than one way of driving. The rig file is
//! where those two lists meet:
//!
//! ```jsonc
//! {
//!   "modes": ["normal", "sport", "track"],       // index = the value on the bus
//!   "displays": [
//!     { "name": "cluster", "node": "0x01",
//!       "scenes": { "normal": "z31-normal.scene", "sport": "z31-sport.scene",
//!                   "track": "z31-track.scene" } },
//!     { "name": "left gauge", "node": "0x02", "scenes": { /* ... */ } },
//!   ]
//! }
//! ```
//!
//! Every display names a scene for every mode. A mode with no scene is an
//! error rather than a fallback, because a car in track mode with a blank
//! gauge is precisely the failure this file exists to rule out.
//!
//! # What is deliberately not here
//!
//! Size and shape: each scene already says how big it is and whether its
//! corners are behind a bezel, and stating it twice is a way to be wrong once.
//! Files: the paths are opaque strings relative to the rig, and the host reads
//! them, exactly as it does a scene's `images`. Which gauge carries the mode
//! value on the bus: that is the bus spec's business; this crate only maps the
//! value it is handed to a mode index with [`Rig::mode_index`].

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::asset::Scene;
use crate::scene::bind::node_address;
use crate::scene::{BuildError, ParseError, Value};

/// Why a rig file could not be used.
#[derive(Clone, Debug, PartialEq)]
pub enum RigError {
    /// The text was not a well-formed document.
    Parse(ParseError),
    /// A required field was absent.
    Missing {
        /// Name of the absent field.
        field: &'static str,
    },
    /// A field had the wrong type or an unusable value.
    BadField {
        /// Name of the field that was unusable.
        field: &'static str,
    },
    /// `modes` was empty. A rig with nothing to switch between is a scene.
    NoModes,
    /// The same mode was listed twice.
    DuplicateMode(String),
    /// Two displays claimed one node address.
    DuplicateNode(u8),
    /// A display claimed an address the bus reserves.
    ReservedNode(u8),
    /// A display named no scene for one of the modes.
    NoSceneForMode {
        /// The display, by its `name`.
        display: String,
        /// The mode it has nothing for.
        mode: String,
    },
    /// A display named a scene for a mode `modes` does not list.
    UnknownMode {
        /// The display, by its `name`.
        display: String,
        /// The mode that is not in the list.
        mode: String,
    },
    /// A scene says it is for a different node than the rig placed it on.
    NodeMismatch {
        /// The scene, by the path the rig gave for it.
        path: String,
        /// What the rig says.
        rig: u8,
        /// What the scene says.
        scene: u8,
    },
}

/// One screen on the bus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    /// What a person calls it: "cluster", "left gauge".
    pub name: String,
    /// Its CAN node address. Never `0x00` or `0xFF`.
    pub node: u8,
    /// The scene it shows in each mode, indexed like [`Rig::modes`].
    pub scenes: Vec<String>,
}

/// Every display on the bus and the scene each shows in each mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rig {
    /// The mode names, in bus order: the value broadcast for a mode is its
    /// index here.
    pub modes: Vec<String>,
    /// The displays, in file order.
    pub displays: Vec<Display>,
}

impl Rig {
    /// The display at `node`, if the rig has one there.
    #[must_use]
    pub fn display(&self, node: u8) -> Option<&Display> {
        self.displays.iter().find(|d| d.node == node)
    }

    /// The scene the display at `node` shows in mode `mode`.
    #[must_use]
    pub fn scene_path(&self, node: u8, mode: usize) -> Option<&str> {
        self.display(node)?.scenes.get(mode).map(String::as_str)
    }

    /// The mode a value off the bus names, or `None` for one past the list.
    ///
    /// `None` rather than clamping: a display that cannot understand the
    /// mode it was told should keep showing the one it has, not jump to the
    /// last one in the file.
    #[must_use]
    pub fn mode_index(&self, bus_value: u8) -> Option<usize> {
        let i = usize::from(bus_value);
        (i < self.modes.len()).then_some(i)
    }

    /// The index of the mode called `name`.
    #[must_use]
    pub fn mode_named(&self, name: &str) -> Option<usize> {
        self.modes.iter().position(|m| m == name)
    }

    /// Whether `scene`, loaded from the path the rig gave for `node`, agrees
    /// about which node it is for.
    ///
    /// A scene that does not say is taken at the rig's word; one that says
    /// something else has been copied to the wrong slot, and drawing a gauge
    /// panel on the cluster is not a thing to do quietly.
    pub fn check(&self, node: u8, mode: usize, scene: &Scene) -> Result<(), RigError> {
        match scene.node {
            Some(n) if n != node => Err(RigError::NodeMismatch {
                path: self.scene_path(node, mode).unwrap_or("").to_string(),
                rig: node,
                scene: n,
            }),
            _ => Ok(()),
        }
    }
}

/// Read a rig from its text.
pub fn parse(text: &str) -> Result<Rig, RigError> {
    let doc = crate::scene::parse(text).map_err(RigError::Parse)?;

    let modes = string_list(&doc, "modes")?;
    if modes.is_empty() {
        return Err(RigError::NoModes);
    }
    for (i, m) in modes.iter().enumerate() {
        if modes[..i].contains(m) {
            return Err(RigError::DuplicateMode(m.clone()));
        }
    }

    let entries = doc
        .get("displays")
        .ok_or(RigError::Missing { field: "displays" })?
        .as_array()
        .ok_or(RigError::BadField { field: "displays" })?;

    let mut displays: Vec<Display> = Vec::with_capacity(entries.len());
    for entry in entries {
        let d = display(entry, &modes)?;
        if displays.iter().any(|x| x.node == d.node) {
            return Err(RigError::DuplicateNode(d.node));
        }
        displays.push(d);
    }

    Ok(Rig { modes, displays })
}

/// Read one `displays` entry against the mode list.
fn display(val: &Value, modes: &[String]) -> Result<Display, RigError> {
    let name = val
        .get("name")
        .ok_or(RigError::Missing { field: "name" })?
        .as_str()
        .ok_or(RigError::BadField { field: "name" })?
        .to_string();

    let node = val
        .get("node")
        .ok_or(RigError::Missing { field: "node" })
        .and_then(|v| {
            node_address(v).map_err(|e| match e {
                BuildError::ReservedNode(n) => RigError::ReservedNode(n),
                _ => RigError::BadField { field: "node" },
            })
        })?;

    let Value::Object(fields) = val
        .get("scenes")
        .ok_or(RigError::Missing { field: "scenes" })?
    else {
        return Err(RigError::BadField { field: "scenes" });
    };

    // One slot per mode, filled by name, so the file can list them in any
    // order and a missing one is found by the hole it leaves.
    let mut scenes: Vec<Option<String>> = alloc::vec![None; modes.len()];
    for (mode, path) in fields {
        let i = modes
            .iter()
            .position(|m| m == mode)
            .ok_or_else(|| RigError::UnknownMode {
                display: name.clone(),
                mode: mode.clone(),
            })?;
        let path = path
            .as_str()
            .ok_or(RigError::BadField { field: "scenes" })?;
        scenes[i] = Some(path.to_string());
    }
    let scenes = scenes
        .into_iter()
        .zip(modes)
        .map(|(s, mode)| {
            s.ok_or_else(|| RigError::NoSceneForMode {
                display: name.clone(),
                mode: mode.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Display { name, node, scenes })
}

/// Read a required array of strings.
fn string_list(doc: &Value, field: &'static str) -> Result<Vec<String>, RigError> {
    let arr = doc
        .get(field)
        .ok_or(RigError::Missing { field })?
        .as_array()
        .ok_or(RigError::BadField { field })?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(ToString::to_string)
                .ok_or(RigError::BadField { field })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREE: &str = r#"{
        // A cluster and two round gauges, three ways of driving.
        "modes": ["normal", "sport", "track"],
        "displays": [
            { "name": "cluster", "node": "0x01",
              "scenes": { "normal": "c-n.scene", "sport": "c-s.scene", "track": "c-t.scene" } },
            { "name": "left", "node": 2,
              "scenes": { "track": "l-t.scene", "normal": "l-n.scene", "sport": "l-s.scene" } },
            { "name": "right", "node": "0x03",
              "scenes": { "normal": "r-n.scene", "sport": "r-s.scene", "track": "r-t.scene" } },
        ],
    }"#;

    #[test]
    fn a_rig_reads_its_modes_and_displays() {
        let rig = parse(THREE).unwrap();
        assert_eq!(rig.modes, ["normal", "sport", "track"]);
        assert_eq!(rig.displays.len(), 3);
        assert_eq!(rig.display(2).unwrap().name, "left");
        // Scenes are slotted by mode name, whatever order the file used.
        assert_eq!(rig.scene_path(2, 0), Some("l-n.scene"));
        assert_eq!(rig.scene_path(2, 2), Some("l-t.scene"));
        assert_eq!(rig.scene_path(1, 1), Some("c-s.scene"));
        assert_eq!(rig.scene_path(9, 0), None);
        assert_eq!(rig.scene_path(1, 3), None);
    }

    #[test]
    fn a_bus_value_names_a_mode_or_nothing() {
        let rig = parse(THREE).unwrap();
        assert_eq!(rig.mode_index(0), Some(0));
        assert_eq!(rig.mode_index(2), Some(2));
        assert_eq!(rig.mode_index(3), None, "not clamped to the last mode");
        assert_eq!(rig.mode_named("sport"), Some(1));
        assert_eq!(rig.mode_named("valet"), None);
    }

    fn rig(modes: &str, displays: &str) -> Result<Rig, RigError> {
        parse(&alloc::format!(
            r#"{{"modes":{modes},"displays":[{displays}]}}"#
        ))
    }

    #[test]
    fn a_display_missing_a_mode_is_refused_by_name() {
        assert_eq!(
            rig(
                r#"["normal","sport"]"#,
                r#"{"name":"cluster","node":1,"scenes":{"normal":"a.scene"}}"#
            )
            .unwrap_err(),
            RigError::NoSceneForMode {
                display: "cluster".into(),
                mode: "sport".into()
            }
        );
    }

    #[test]
    fn a_scene_for_a_mode_the_rig_does_not_have_is_refused() {
        assert_eq!(
            rig(
                r#"["normal"]"#,
                r#"{"name":"cluster","node":1,"scenes":{"normal":"a.scene","valet":"v.scene"}}"#
            )
            .unwrap_err(),
            RigError::UnknownMode {
                display: "cluster".into(),
                mode: "valet".into()
            }
        );
    }

    #[test]
    fn node_addresses_are_checked_the_way_a_scenes_are() {
        let two = |a: &str, b: &str| {
            rig(
                r#"["normal"]"#,
                &alloc::format!(
                    r#"{{"name":"a","node":{a},"scenes":{{"normal":"a.scene"}}}},
                        {{"name":"b","node":{b},"scenes":{{"normal":"b.scene"}}}}"#
                ),
            )
        };
        assert_eq!(two("1", "1").unwrap_err(), RigError::DuplicateNode(1));
        assert_eq!(
            two(r#""0x01""#, "1").unwrap_err(),
            RigError::DuplicateNode(1),
            "the two spellings are one address"
        );
        assert_eq!(two("0", "1").unwrap_err(), RigError::ReservedNode(0));
        assert_eq!(
            two(r#""0xff""#, "1").unwrap_err(),
            RigError::ReservedNode(0xff)
        );
        assert_eq!(
            two(r#""one""#, "1").unwrap_err(),
            RigError::BadField { field: "node" }
        );
    }

    #[test]
    fn an_empty_or_repeated_mode_list_is_refused() {
        assert_eq!(rig("[]", "").unwrap_err(), RigError::NoModes);
        assert_eq!(
            rig(r#"["a","b","a"]"#, "").unwrap_err(),
            RigError::DuplicateMode("a".into())
        );
    }

    #[test]
    fn missing_fields_are_reported_by_name() {
        assert_eq!(
            parse(r#"{"displays":[]}"#).unwrap_err(),
            RigError::Missing { field: "modes" }
        );
        assert_eq!(
            parse(r#"{"modes":["a"]}"#).unwrap_err(),
            RigError::Missing { field: "displays" }
        );
        assert_eq!(
            rig(r#"["a"]"#, r#"{"node":1,"scenes":{"a":"x"}}"#).unwrap_err(),
            RigError::Missing { field: "name" }
        );
        assert_eq!(
            rig(r#"["a"]"#, r#"{"name":"n","scenes":{"a":"x"}}"#).unwrap_err(),
            RigError::Missing { field: "node" }
        );
        assert_eq!(
            rig(r#"["a"]"#, r#"{"name":"n","node":1}"#).unwrap_err(),
            RigError::Missing { field: "scenes" }
        );
        assert!(matches!(parse("{"), Err(RigError::Parse(_))));
    }

    #[test]
    fn a_scene_on_the_wrong_node_is_caught() {
        let rig = parse(THREE).unwrap();
        let scene = |node: &str| {
            let src = alloc::format!(
                r#"{{"width":10,"height":10,{node}"root":{{"type":"panel","rect":[0,0,1,1]}}}}"#
            );
            crate::scene::build_scene(&crate::scene::parse(&src).unwrap()).unwrap()
        };
        assert_eq!(rig.check(1, 0, &scene(r#""node":1,"#)), Ok(()));
        assert_eq!(
            rig.check(1, 0, &scene("")),
            Ok(()),
            "a silent scene is trusted"
        );
        assert_eq!(
            rig.check(1, 2, &scene(r#""node":"0x02","#)),
            Err(RigError::NodeMismatch {
                path: "c-t.scene".into(),
                rig: 1,
                scene: 2
            })
        );
    }
}
