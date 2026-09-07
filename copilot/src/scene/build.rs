// SPDX-License-Identifier: MIT OR Apache-2.0
//! Turning a parsed document into a widget tree.
//!
//! This is the schema layer. [`crate::scene::parse()`] guarantees the text was
//! well-formed; everything here is about whether it described a scene that
//! makes sense, and every rejection names the field that caused it so an
//! error can point a scene author at the line they need to fix.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::anim::{Animation, Animator, Easing, Property, Repeat};
use crate::asset::Scene;
use crate::color::parse_hex;
use crate::widget::{Align, Kind, Node, NodeId, ROOT, Tree, VAlign};
use crate::{Color, Rect};

use super::bind::{self, Binding, Shape};
use super::value::Value;

/// Why a scene document could not be turned into a tree.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
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
    /// "type" named a widget kind that does not exist.
    UnknownType,
    /// A colour string was not valid hex.
    BadColor {
        /// Name of the field that held the bad colour.
        field: &'static str,
    },
    /// The tree nested deeper than the builder allows.
    TooDeep,
    /// `bind.gauge` named a gauge the bus spec does not have.
    UnknownGauge(String),
    /// `bind.unit` named a unit the bus spec does not have.
    UnknownUnit(String),
    /// `bind.unit` is a real unit, but not of the gauge's dimension.
    WrongUnit {
        /// The gauge, by the spec's name for it.
        gauge: &'static str,
        /// The unit as the scene wrote it.
        unit: String,
    },
    /// `node` named an address the bus reserves: the gateway or broadcast.
    ReservedNode(u8),
}

/// Maximum node nesting depth.
pub const MAX_DEPTH: u32 = 64;

/// Build a widget tree, discarding any image requests.
///
/// For scenes that reference no assets, and for tests. A scene that names
/// images will still build; its `Image` widgets simply draw the missing-asset
/// placeholder because nothing populated a table for them.
pub fn build(doc: &Value) -> Result<Tree, BuildError> {
    build_scene(doc).map(|s| s.tree)
}

/// Build a widget tree and collect the image paths it asked for.
///
/// The host is expected to read those paths, decode them, and push one entry
/// per request into an [`ImageTable`] -- including a placeholder for any it
/// could not load, or every later index shifts.
///
/// [`ImageTable`]: crate::asset::ImageTable
pub fn build_scene(doc: &Value) -> Result<Scene, BuildError> {
    let width = doc
        .get("width")
        .and_then(Value::as_i64)
        .ok_or(BuildError::Missing { field: "width" })?;
    let height = doc
        .get("height")
        .and_then(Value::as_i64)
        .ok_or(BuildError::Missing { field: "height" })?;

    if width <= 0 {
        return Err(BuildError::BadField { field: "width" });
    }
    if height <= 0 {
        return Err(BuildError::BadField { field: "height" });
    }

    let root_val = doc
        .get("root")
        .ok_or(BuildError::Missing { field: "root" })?;

    let requests = string_list(doc, "images")?;

    let anim_requests = string_list(doc, "anims")?;

    // `as u32` on an i64 that only passed a `> 0` check wraps silently:
    // 5_000_000_000 becomes 705_032_704, and the scene renders at a size
    // nobody asked for. A dimension the format cannot express is refused.
    let width = u32::try_from(width).map_err(|_| BuildError::BadField { field: "width" })?;
    let height = u32::try_from(height).map_err(|_| BuildError::BadField { field: "height" })?;
    let bounds = Rect::new(0, 0, width, height);
    let mut tree = Tree::new(bounds);
    let mut anims = Animator::new();
    // The scene's own antialiasing setting is what the document root draws
    // with unless it says otherwise, and every widget below inherits it from
    // there. Off by default: a scene written before the setting existed must
    // render exactly as it did.
    let antialias = flag(doc, "antialias", false)?;
    // Which display on the bus this is for, and what its panel looks like.
    // Both optional: a bench scene has no bus and a monitor has corners.
    let node = match doc.get("node") {
        None | Some(Value::Null) => None,
        Some(v) => Some(bind::node_address(v)?),
    };
    let shape = match doc.get("shape") {
        None | Some(Value::Null) => Shape::Rect,
        Some(v) => v
            .as_str()
            .and_then(Shape::parse)
            .ok_or(BuildError::BadField { field: "shape" })?,
    };
    let mut bindings = Vec::new();
    insert(
        &mut tree,
        &mut anims,
        &mut bindings,
        ROOT,
        root_val,
        1,
        Some(antialias),
    )?;
    Ok(Scene {
        tree,
        requests,
        anim_requests,
        anims,
        bindings,
        node,
        shape,
    })
}

/// Build one node, push it under `parent`, then recurse into its children.
///
/// The recursion carries `&mut Tree` rather than returning a `Node` for the
/// caller to attach, because a child's `NodeId` only exists once it has been
/// pushed -- a parent cannot record ids for children it has not inserted yet.
fn insert(
    tree: &mut Tree,
    anims: &mut Animator,
    bindings: &mut Vec<Binding>,
    parent: NodeId,
    val: &Value,
    depth: u32,
    default_antialias: Option<bool>,
) -> Result<NodeId, BuildError> {
    if depth > MAX_DEPTH {
        return Err(BuildError::TooDeep);
    }

    let node = build_node(val, default_antialias)?;
    // `push` only fails on a parent that does not exist, and `parent` was
    // returned by a previous `push` or is ROOT.
    let id = tree.push(parent, node).ok_or(BuildError::TooDeep)?;

    if let Some(spec) = val.get("animate") {
        anims.push(animation(id, spec)?);
    }

    if let Some(spec) = val.get("bind") {
        // The node was pushed a moment ago, so the lookup cannot fail; the
        // `ok_or` keeps the builder honest about it rather than unwrapping.
        let kind = &tree.get(id).ok_or(BuildError::TooDeep)?.kind;
        bind::bindings(id, kind, spec, bindings)?;
    }

    match val.get("children") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let kids = v
                .as_array()
                .ok_or(BuildError::BadField { field: "children" })?;
            for child in kids {
                insert(tree, anims, bindings, id, child, depth + 1, None)?;
            }
        }
    }
    Ok(id)
}

/// Read an optional array of strings, such as an asset list.
fn string_list(doc: &Value, field: &'static str) -> Result<Vec<String>, BuildError> {
    match doc.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(v) => {
            let arr = v.as_array().ok_or(BuildError::BadField { field })?;
            let mut out = Vec::with_capacity(arr.len());
            for entry in arr {
                out.push(
                    entry
                        .as_str()
                        .ok_or(BuildError::BadField { field })?
                        .to_string(),
                );
            }
            Ok(out)
        }
    }
}

/// Read an `animate` block.
///
/// Every field except `property`, `from` and `to` has a default, so the common
/// case -- a gauge sweeping between two readings -- is three lines in a scene
/// file rather than seven.
fn animation(node: NodeId, spec: &Value) -> Result<Animation, BuildError> {
    let property = spec
        .get("property")
        .and_then(Value::as_str)
        .ok_or(BuildError::Missing { field: "property" })
        .and_then(|n| Property::parse(n).ok_or(BuildError::BadField { field: "property" }))?;

    let from = spec
        .get("from")
        .and_then(Value::as_f64)
        .ok_or(BuildError::Missing { field: "from" })? as f32;
    let to = spec
        .get("to")
        .and_then(Value::as_f64)
        .ok_or(BuildError::Missing { field: "to" })? as f32;

    let duration_ms = match spec.get("duration_ms") {
        None => 1_000,
        Some(v) => {
            let ms = v.as_i64().ok_or(BuildError::BadField {
                field: "duration_ms",
            })?;
            u32::try_from(ms).map_err(|_| BuildError::BadField {
                field: "duration_ms",
            })?
        }
    };

    let easing = match spec.get("easing") {
        None => Easing::default(),
        Some(v) => v
            .as_str()
            .and_then(Easing::parse)
            .ok_or(BuildError::BadField { field: "easing" })?,
    };

    let repeat = match spec.get("repeat") {
        None => Repeat::default(),
        Some(v) => v
            .as_str()
            .and_then(Repeat::parse)
            .ok_or(BuildError::BadField { field: "repeat" })?,
    };

    Ok(Animation::new(
        node,
        property,
        from,
        to,
        duration_ms,
        easing,
        repeat,
    ))
}

/// Read an optional colour field, or parse `default` when it is absent.
fn colour(val: &Value, field: &'static str, default: &str) -> Result<Color, BuildError> {
    match val.get(field) {
        Some(v) => {
            let s = v.as_str().ok_or(BuildError::BadField { field })?;
            parse_hex(s).ok_or(BuildError::BadColor { field })
        }
        // The defaults are literals in this file and are all valid, so the
        // only way this fails is a typo in the source, which the tests catch.
        None => parse_hex(default).ok_or(BuildError::BadColor { field }),
    }
}

/// Read an optional 0.0..=1.0 field, clamping rather than rejecting.
///
/// Someone typing 1.5 for a brightness means "full", not "reject my file".
fn fraction(val: &Value, field: &'static str, default: f32) -> Result<f32, BuildError> {
    match val.get(field) {
        Some(v) => {
            let f = v.as_f64().ok_or(BuildError::BadField { field })? as f32;
            Ok(if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) })
        }
        None => Ok(default),
    }
}

/// Read an optional signed integer field.
///
/// The `try_from` is the point: a scene file is JSON and its numbers are
/// `i64`, so a value a user typed with one digit too many would otherwise
/// wrap silently into a plausible-looking coordinate.
fn int(val: &Value, field: &'static str, default: i32) -> Result<i32, BuildError> {
    match val.get(field) {
        Some(v) => {
            let n = v.as_i64().ok_or(BuildError::BadField { field })?;
            i32::try_from(n).map_err(|_| BuildError::BadField { field })
        }
        None => Ok(default),
    }
}

/// Read an optional unsigned integer field, treating a negative as zero.
///
/// Zero is meaningful for all of these -- no hub, no ticks, no major marks --
/// so a negative is a slip rather than a request, and clamping keeps the rest
/// of the scene on screen.
fn count(val: &Value, field: &'static str, default: u32) -> Result<u32, BuildError> {
    match val.get(field) {
        Some(v) => {
            let n = v.as_i64().ok_or(BuildError::BadField { field })?;
            u32::try_from(n.max(0)).map_err(|_| BuildError::BadField { field })
        }
        None => Ok(default),
    }
}

/// Read a `points` array of `[x, y]` pairs.
///
/// A point is a two-element array rather than an object: a curve is dozens of
/// them, and `[0.1, 0.8]` stays readable in a scene file where
/// `{"x": 0.1, "y": 0.8}` would not. Absent is an empty shape rather than an
/// error, so an author sketching a layout gets an empty node instead of a
/// rejected file.
fn read_points(val: &Value) -> Result<alloc::vec::Vec<(f32, f32)>, BuildError> {
    let mut points = alloc::vec::Vec::new();
    let Some(v) = val.get("points") else {
        return Ok(points);
    };
    let arr = v
        .as_array()
        .ok_or(BuildError::BadField { field: "points" })?;
    for pair in arr {
        let xy = pair
            .as_array()
            .ok_or(BuildError::BadField { field: "points" })?;
        let [px, py] = xy else {
            return Err(BuildError::BadField { field: "points" });
        };
        let px = px
            .as_f64()
            .ok_or(BuildError::BadField { field: "points" })? as f32;
        let py = py
            .as_f64()
            .ok_or(BuildError::BadField { field: "points" })? as f32;
        points.push((px, py));
    }
    Ok(points)
}

/// Read an optional number that is a fraction but may sit outside 0..=1.
///
/// A band threshold is one of these: putting it above the top of the scale is
/// how a scene says "no band", and clamping it to 1.0 would silently turn that
/// into "the last cell only".
fn unbounded(val: &Value, field: &'static str, default: f32) -> Result<f32, BuildError> {
    match val.get(field) {
        Some(v) => {
            let f = v.as_f64().ok_or(BuildError::BadField { field })? as f32;
            Ok(if f.is_nan() { default } else { f })
        }
        None => Ok(default),
    }
}

/// Read an optional array of 0.0..=1.0 fractions.
fn read_fractions(val: &Value, field: &'static str) -> Result<alloc::vec::Vec<f32>, BuildError> {
    let mut out = alloc::vec::Vec::new();
    let Some(v) = val.get(field) else {
        return Ok(out);
    };
    let arr = v.as_array().ok_or(BuildError::BadField { field })?;
    for n in arr {
        let f = n.as_f64().ok_or(BuildError::BadField { field })? as f32;
        out.push(if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) });
    }
    Ok(out)
}

/// Read a flag, defaulting rather than rejecting a missing one.
fn flag(val: &Value, field: &'static str, default: bool) -> Result<bool, BuildError> {
    match val.get(field) {
        Some(v) => v.as_bool().ok_or(BuildError::BadField { field }),
        None => Ok(default),
    }
}

/// Read the fields common to every node, without touching its children.
///
/// `default_antialias` is what the node draws with when it does not say:
/// the scene's setting for the document root, and nothing for everything
/// else, which inherits at draw time.
fn build_node(val: &Value, default_antialias: Option<bool>) -> Result<Node, BuildError> {
    let type_str = val
        .get("type")
        .and_then(Value::as_str)
        .ok_or(BuildError::Missing { field: "type" })?;

    let rect = build_rect(val)?;

    let name = match val.get("name") {
        Some(v) => Some(
            v.as_str()
                .ok_or(BuildError::BadField { field: "name" })?
                .to_string(),
        ),
        None => None,
    };

    let visible = match val.get("visible") {
        Some(v) => v
            .as_bool()
            .ok_or(BuildError::BadField { field: "visible" })?,
        None => true,
    };

    let antialias = match val.get("antialias") {
        Some(v) => Some(
            v.as_bool()
                .ok_or(BuildError::BadField { field: "antialias" })?,
        ),
        None => default_antialias,
    };

    Ok(Node {
        rect,
        kind: build_kind(type_str, val)?,
        visible,
        antialias,
        name,
        children: Vec::new(),
        parent: None,
    })
}

fn build_rect(val: &Value) -> Result<Rect, BuildError> {
    let arr = val
        .get("rect")
        .and_then(Value::as_array)
        .ok_or(BuildError::Missing { field: "rect" })?;
    if arr.len() != 4 {
        return Err(BuildError::BadField { field: "rect" });
    }
    // Every one of these goes through try_from rather than `as`. A coordinate
    // outside i32, or a size outside u32, wraps silently under a cast and the
    // widget lands somewhere it was never asked to be -- which looks like a
    // layout bug rather than the bad input it is.
    let field = "rect";
    let num = |v: &Value| v.as_i64().ok_or(BuildError::BadField { field });
    let x = i32::try_from(num(&arr[0])?).map_err(|_| BuildError::BadField { field })?;
    let y = i32::try_from(num(&arr[1])?).map_err(|_| BuildError::BadField { field })?;
    let w = u32::try_from(num(&arr[2])?).map_err(|_| BuildError::BadField { field })?;
    let h = u32::try_from(num(&arr[3])?).map_err(|_| BuildError::BadField { field })?;
    Ok(Rect::new(x, y, w, h))
}

fn build_kind(type_str: &str, val: &Value) -> Result<Kind, BuildError> {
    match type_str {
        "panel" => {
            let bg = match val.get("background") {
                Some(v) => {
                    let s = v.as_str().ok_or(BuildError::BadField {
                        field: "background",
                    })?;
                    parse_hex(s).ok_or(BuildError::BadColor {
                        field: "background",
                    })?
                }
                None => parse_hex("#00000000").unwrap(),
            };
            Ok(Kind::Panel { background: bg })
        }
        "frame" => {
            let color = match val.get("color") {
                Some(v) => {
                    let s = v.as_str().ok_or(BuildError::BadField { field: "color" })?;
                    parse_hex(s).ok_or(BuildError::BadColor { field: "color" })?
                }
                None => parse_hex("#ffffffff").unwrap(),
            };
            Ok(Kind::Frame { color })
        }
        "label" => {
            let text = val
                .get("text")
                .and_then(Value::as_str)
                .ok_or(BuildError::Missing { field: "text" })?
                .to_string();
            let color = match val.get("color") {
                Some(v) => {
                    let s = v.as_str().ok_or(BuildError::BadField { field: "color" })?;
                    parse_hex(s).ok_or(BuildError::BadColor { field: "color" })?
                }
                None => parse_hex("#ffffffff").unwrap(),
            };
            let scale = match val.get("scale") {
                Some(v) => {
                    let n = v.as_i64().ok_or(BuildError::BadField { field: "scale" })?;
                    u8::try_from(n.max(1)).map_err(|_| BuildError::BadField { field: "scale" })?
                }
                None => 1,
            };
            let align = match val.get("align") {
                Some(v) => match v.as_str() {
                    Some("left") => Align::Left,
                    Some("center") => Align::Center,
                    Some("right") => Align::Right,
                    _ => return Err(BuildError::BadField { field: "align" }),
                },
                None => Align::Left,
            };
            let valign = match val.get("valign") {
                Some(v) => match v.as_str() {
                    Some("top") => VAlign::Top,
                    Some("middle") => VAlign::Middle,
                    Some("bottom") => VAlign::Bottom,
                    _ => return Err(BuildError::BadField { field: "valign" }),
                },
                None => VAlign::Top,
            };
            Ok(Kind::Label {
                text,
                color,
                scale,
                align,
                valign,
            })
        }
        "image" => {
            let image = val
                .get("image")
                .and_then(Value::as_i64)
                .ok_or(BuildError::Missing { field: "image" })?;
            if image < 0 {
                return Err(BuildError::BadField { field: "image" });
            }
            Ok(Kind::Image {
                image: u32::try_from(image).map_err(|_| BuildError::BadField { field: "image" })?,
            })
        }
        "anim" => {
            let anim = val
                .get("anim")
                .and_then(Value::as_i64)
                .ok_or(BuildError::Missing { field: "anim" })?;
            if anim < 0 {
                return Err(BuildError::BadField { field: "anim" });
            }
            let playing = match val.get("playing") {
                Some(v) => v
                    .as_bool()
                    .ok_or(BuildError::BadField { field: "playing" })?,
                None => true,
            };
            let speed = match val.get("speed") {
                Some(v) => v.as_f64().ok_or(BuildError::BadField { field: "speed" })? as f32,
                None => 1.0,
            };
            let frame = match val.get("frame") {
                Some(v) => {
                    let f = v.as_i64().ok_or(BuildError::BadField { field: "frame" })?;
                    u32::try_from(f).map_err(|_| BuildError::BadField { field: "frame" })?
                }
                None => 0,
            };
            Ok(Kind::Anim {
                anim: u32::try_from(anim).map_err(|_| BuildError::BadField { field: "anim" })?,
                frame,
                playing,
                speed,
                elapsed_us: 0,
            })
        }
        "arc" => {
            let angle = |field: &'static str, default: i64| -> Result<i32, BuildError> {
                match val.get(field) {
                    Some(v) => {
                        let n = v.as_i64().ok_or(BuildError::BadField { field })?;
                        i32::try_from(n).map_err(|_| BuildError::BadField { field })
                    }
                    None => Ok(default as i32),
                }
            };
            // A dial that sweeps from seven o'clock to five o'clock is what
            // most gauges look like, so that is the default rather than a
            // full circle nobody asked for.
            let start = angle("start", 225)?;
            let end = angle("end", 495)?;
            let thickness = match val.get("thickness") {
                Some(v) => {
                    let n = v
                        .as_i64()
                        .ok_or(BuildError::BadField { field: "thickness" })?;
                    u32::try_from(n.max(1))
                        .map_err(|_| BuildError::BadField { field: "thickness" })?
                }
                None => 12,
            };
            Ok(Kind::Arc {
                start,
                end,
                value: fraction(val, "value", 0.0)?,
                thickness,
                fill: colour(val, "fill", "#ffffffff")?,
                track: colour(val, "track", "#00000000")?,
            })
        }
        "needle" => Ok(Kind::Needle {
            start: int(val, "start", 225)?,
            end: int(val, "end", 495)?,
            value: fraction(val, "value", 0.0)?,
            width: count(val, "width", 3)?,
            color: colour(val, "color", "#ffffffff")?,
            hub: count(val, "hub", 5)?,
        }),
        "scale" => Ok(Kind::Scale {
            start: int(val, "start", 225)?,
            end: int(val, "end", 495)?,
            ticks: count(val, "ticks", 11)?,
            major_every: count(val, "major_every", 0)?,
            length: count(val, "length", 8)?,
            width: count(val, "width", 2)?,
            color: colour(val, "color", "#ffffffff")?,
            // Majors default to the minor colour so that a scale with
            // `major_every` set but no palette still reads: the length
            // difference alone is enough.
            major_color: colour(val, "major_color", "#ffffffff")?,
        }),
        "polygon" => Ok(Kind::Polygon {
            points: read_points(val)?,
            color: colour(val, "color", "#ffffffff")?,
        }),
        "line" => Ok(Kind::Line {
            points: read_points(val)?,
            width: count(val, "width", 2)?,
            color: colour(val, "color", "#ffffffff")?,
            closed: flag(val, "closed", false)?,
        }),
        "chart" => {
            let mut values = alloc::vec::Vec::new();
            if let Some(v) = val.get("values") {
                let arr = v
                    .as_array()
                    .ok_or(BuildError::BadField { field: "values" })?;
                for n in arr {
                    let f = n.as_f64().ok_or(BuildError::BadField { field: "values" })? as f32;
                    values.push(if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) });
                }
            }
            Ok(Kind::Chart {
                values,
                width: count(val, "width", 2)?,
                stroke: colour(val, "stroke", "#ffffffff")?,
                // No fill by default: a bare trace is the honest reading of
                // "chart", and an area nobody asked for hides what is behind it.
                fill: colour(val, "fill", "#00000000")?,
            })
        }
        "sevenseg" => Ok(Kind::SevenSeg {
            text: val
                .get("text")
                .and_then(Value::as_str)
                .ok_or(BuildError::Missing { field: "text" })?
                .to_string(),
            color: colour(val, "color", "#ffffffff")?,
            // A dark grey, not a faint white. Colours here are written, not
            // blended -- the surfaces this targets have no alpha channel to
            // blend against -- so a low-alpha version of the lit colour lands
            // as the lit colour and every digit comes out an eight. Opaque and
            // dim is the only way to say dim.
            ghost: colour(val, "ghost", "#22262cff")?,
            thickness: count(val, "thickness", 4)?,
            // Zero by default, so a scene written before the field existed
            // still sizes its cells to whatever it is showing.
            digits: count(val, "digits", 0)?,
        }),
        "gradient" => Ok(Kind::Gradient {
            from: colour(val, "from", "#00000000")?,
            to: colour(val, "to", "#00000000")?,
            vertical: flag(val, "vertical", true)?,
        }),
        "ruler" => Ok(Kind::Ruler {
            ticks: count(val, "ticks", 11)?,
            major_every: count(val, "major_every", 0)?,
            length: count(val, "length", 8)?,
            width: count(val, "width", 2)?,
            color: colour(val, "color", "#ffffffff")?,
            major_color: colour(val, "major_color", "#ffffffff")?,
            vertical: flag(val, "vertical", false)?,
        }),
        "segbar" => Ok(Kind::SegBar {
            value: fraction(val, "value", 0.0)?,
            segments: count(val, "segments", 10)?,
            gap: count(val, "gap", 2)?,
            fill: colour(val, "fill", "#3fb950ff")?,
            track: colour(val, "track", "#12161cff")?,
            // Above the top of the scale by default, so a gauge that never
            // asked for a redline does not get one on its last cell.
            warn: unbounded(val, "warn", 2.0)?,
            warn_fill: colour(val, "warn_fill", "#d29922ff")?,
            danger: unbounded(val, "danger", 2.0)?,
            danger_fill: colour(val, "danger_fill", "#f85149ff")?,
            vertical: flag(val, "vertical", false)?,
            profile: read_fractions(val, "profile")?,
            // One by default, so a bank with no second reading stands at its
            // full envelope and draws as it always did.
            height: fraction(val, "height", 1.0)?,
            divisions: count(val, "divisions", 0)?,
            div_gap: count(val, "div_gap", 1)?,
        }),
        "grid" => Ok(Kind::Grid {
            pitch_x: count(val, "pitch_x", 0)?,
            pitch_y: count(val, "pitch_y", 0)?,
            width: count(val, "width", 1)?,
            color: colour(val, "color", "#ffffff14")?,
        }),
        "led" => {
            let color = colour(val, "color", "#ffffffff")?;
            let level = fraction(val, "level", 0.0)?;
            // A default glow rather than zero: an unlit telltale that draws
            // nothing is indistinguishable from one the author forgot.
            let glow = fraction(val, "glow", 0.12)?;
            Ok(Kind::Led { color, level, glow })
        }
        "roundrect" => {
            let background = colour(val, "background", "#00000000")?;
            let radius = match val.get("radius") {
                Some(v) => {
                    let n = v.as_i64().ok_or(BuildError::BadField { field: "radius" })?;
                    u32::try_from(n).map_err(|_| BuildError::BadField { field: "radius" })?
                }
                None => 8,
            };
            Ok(Kind::RoundRect { background, radius })
        }
        "bar" => {
            let value = match val.get("value") {
                Some(v) => {
                    let f = v.as_f64().ok_or(BuildError::BadField { field: "value" })? as f32;
                    f.clamp(0.0, 1.0)
                }
                None => 0.0,
            };
            let fill = match val.get("fill") {
                Some(v) => {
                    let s = v.as_str().ok_or(BuildError::BadField { field: "fill" })?;
                    parse_hex(s).ok_or(BuildError::BadColor { field: "fill" })?
                }
                None => parse_hex("#ffffffff").unwrap(),
            };
            let track = match val.get("track") {
                Some(v) => {
                    let s = v.as_str().ok_or(BuildError::BadField { field: "track" })?;
                    parse_hex(s).ok_or(BuildError::BadColor { field: "track" })?
                }
                None => parse_hex("#00000000").unwrap(),
            };
            let vertical = match val.get("vertical") {
                Some(v) => v
                    .as_bool()
                    .ok_or(BuildError::BadField { field: "vertical" })?,
                None => false,
            };
            Ok(Kind::Bar {
                value,
                fill,
                track,
                vertical,
            })
        }
        _ => Err(BuildError::UnknownType),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;
    use crate::scene::parse;
    use crate::widget::{Align, Kind, ROOT, VAlign};

    /// Build from source text, so the tests read as the scene files they are.
    fn go(src: &str) -> Result<Tree, BuildError> {
        build(&parse(src).expect("test document must parse"))
    }

    fn doc(root: &str) -> alloc::string::String {
        alloc::format!(r#"{{"width":100,"height":50,"root":{root}}}"#)
    }

    /// The single child of ROOT, which is where the document's root lands.
    fn top(t: &Tree) -> &crate::widget::Node {
        let id = t.get(ROOT).unwrap().children[0];
        t.get(id).unwrap()
    }

    // --- the happy path ---

    #[test]
    fn a_minimal_document_builds() {
        let t = go(&doc(r#"{"type":"panel","rect":[0,0,10,10]}"#)).unwrap();
        assert_eq!(t.len(), 2, "root plus the document's node");
        assert_eq!(top(&t).rect, Rect::new(0, 0, 10, 10));
    }

    #[test]
    fn the_document_root_becomes_a_child_of_tree_root() {
        let t = go(&doc(r#"{"type":"panel","rect":[1,2,3,4]}"#)).unwrap();
        assert_eq!(t.get(ROOT).unwrap().children.len(), 1);
        assert_eq!(top(&t).parent, Some(ROOT));
    }

    #[test]
    fn children_are_built_in_order() {
        let t = go(&doc(r#"{"type":"panel","rect":[0,0,10,10],"children":[
                 {"type":"panel","rect":[0,0,1,1],"name":"first"},
                 {"type":"panel","rect":[0,0,2,2],"name":"second"}]}"#))
        .unwrap();
        let kids = &top(&t).children;
        assert_eq!(kids.len(), 2);
        assert_eq!(t.get(kids[0]).unwrap().name.as_deref(), Some("first"));
        assert_eq!(t.get(kids[1]).unwrap().name.as_deref(), Some("second"));
    }

    #[test]
    fn negative_coordinates_are_allowed() {
        let t = go(&doc(r#"{"type":"panel","rect":[-5,-9,10,10]}"#)).unwrap();
        assert_eq!(top(&t).rect, Rect::new(-5, -9, 10, 10));
    }

    #[test]
    fn visible_defaults_true_and_can_be_set_false() {
        let t = go(&doc(r#"{"type":"panel","rect":[0,0,1,1]}"#)).unwrap();
        assert!(top(&t).visible);
        let t = go(&doc(r#"{"type":"panel","rect":[0,0,1,1],"visible":false}"#)).unwrap();
        assert!(!top(&t).visible);
    }

    // --- each widget kind ---

    #[test]
    fn panel_background_defaults_transparent() {
        let t = go(&doc(r#"{"type":"panel","rect":[0,0,1,1]}"#)).unwrap();
        assert_eq!(
            top(&t).kind,
            Kind::Panel {
                background: Color::TRANSPARENT
            }
        );
    }

    #[test]
    fn panel_background_is_read_from_hex() {
        let t = go(&doc(
            r##"{"type":"panel","rect":[0,0,1,1],"background":"#ff0000"}"##,
        ))
        .unwrap();
        assert_eq!(
            top(&t).kind,
            Kind::Panel {
                background: Color::rgb(255, 0, 0)
            }
        );
    }

    #[test]
    fn frame_colour_defaults_white() {
        let t = go(&doc(r#"{"type":"frame","rect":[0,0,1,1]}"#)).unwrap();
        assert_eq!(
            top(&t).kind,
            Kind::Frame {
                color: Color::WHITE
            }
        );
    }

    #[test]
    fn label_carries_its_text() {
        let t = go(&doc(r#"{"type":"label","rect":[0,0,1,1],"text":"88 mph"}"#)).unwrap();
        match &top(&t).kind {
            Kind::Label { text, color, .. } => {
                assert_eq!(text, "88 mph");
                assert_eq!(*color, Color::WHITE);
            }
            other => panic!("expected a label, got {other:?}"),
        }
    }

    #[test]
    fn image_carries_its_index() {
        let t = go(&doc(r#"{"type":"image","rect":[0,0,1,1],"image":7}"#)).unwrap();
        assert_eq!(top(&t).kind, Kind::Image { image: 7 });
    }

    #[test]
    fn bar_defaults_are_applied() {
        let t = go(&doc(r#"{"type":"bar","rect":[0,0,1,1]}"#)).unwrap();
        match top(&t).kind {
            Kind::Bar {
                value, vertical, ..
            } => {
                assert_eq!(value, 0.0);
                assert!(!vertical);
            }
            ref other => panic!("expected a bar, got {other:?}"),
        }
    }

    #[test]
    fn a_bar_value_outside_the_range_clamps_rather_than_failing() {
        // A scene author typing 1.5 means "full", not "reject my file".
        for (input, want) in [("1.5", 1.0f32), ("-2", 0.0), ("0.25", 0.25)] {
            let src = alloc::format!(r#"{{"type":"bar","rect":[0,0,1,1],"value":{input}}}"#);
            let t = go(&doc(&src)).unwrap();
            match top(&t).kind {
                Kind::Bar { value, .. } => assert_eq!(value, want, "for {input}"),
                ref other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn an_integer_is_accepted_where_a_float_is_expected() {
        let t = go(&doc(r#"{"type":"bar","rect":[0,0,1,1],"value":1}"#)).unwrap();
        match top(&t).kind {
            Kind::Bar { value, .. } => assert_eq!(value, 1.0),
            ref other => panic!("{other:?}"),
        }
    }

    // --- rejection ---

    #[test]
    fn missing_top_level_fields_are_reported_by_name() {
        assert_eq!(
            build(&parse(r#"{"height":1,"root":{"type":"panel","rect":[0,0,1,1]}}"#).unwrap())
                .unwrap_err(),
            BuildError::Missing { field: "width" }
        );
        assert_eq!(
            build(&parse(r#"{"width":1,"root":{"type":"panel","rect":[0,0,1,1]}}"#).unwrap())
                .unwrap_err(),
            BuildError::Missing { field: "height" }
        );
        assert_eq!(
            build(&parse(r#"{"width":1,"height":1}"#).unwrap()).unwrap_err(),
            BuildError::Missing { field: "root" }
        );
    }

    #[test]
    fn a_non_positive_dimension_is_rejected() {
        for src in [
            r#"{"width":0,"height":1,"root":{"type":"panel","rect":[0,0,1,1]}}"#,
            r#"{"width":1,"height":0,"root":{"type":"panel","rect":[0,0,1,1]}}"#,
            r#"{"width":-4,"height":1,"root":{"type":"panel","rect":[0,0,1,1]}}"#,
        ] {
            assert!(
                matches!(
                    build(&parse(src).unwrap()),
                    Err(BuildError::BadField { .. })
                ),
                "{src} should be rejected"
            );
        }
    }

    #[test]
    fn a_missing_type_is_reported() {
        assert_eq!(
            go(&doc(r#"{"rect":[0,0,1,1]}"#)).unwrap_err(),
            BuildError::Missing { field: "type" }
        );
    }

    #[test]
    fn an_unknown_type_is_reported() {
        assert_eq!(
            go(&doc(r#"{"type":"hologram","rect":[0,0,1,1]}"#)).unwrap_err(),
            BuildError::UnknownType
        );
    }

    #[test]
    fn a_malformed_rect_is_rejected() {
        for r in ["[0,0,1]", "[0,0,1,1,1]", "[]", r#""0,0,1,1""#, "[0,0,-1,1]"] {
            let src = alloc::format!(r#"{{"type":"panel","rect":{r}}}"#);
            assert!(
                matches!(
                    go(&doc(&src)),
                    Err(BuildError::BadField { field: "rect" } | BuildError::Missing { .. })
                ),
                "rect {r} should be rejected"
            );
        }
    }

    #[test]
    fn a_missing_rect_is_reported() {
        assert_eq!(
            go(&doc(r#"{"type":"panel"}"#)).unwrap_err(),
            BuildError::Missing { field: "rect" }
        );
    }

    #[test]
    fn a_label_without_text_is_rejected() {
        assert_eq!(
            go(&doc(r#"{"type":"label","rect":[0,0,1,1]}"#)).unwrap_err(),
            BuildError::Missing { field: "text" }
        );
    }

    #[test]
    fn an_image_without_an_index_is_rejected() {
        assert_eq!(
            go(&doc(r#"{"type":"image","rect":[0,0,1,1]}"#)).unwrap_err(),
            BuildError::Missing { field: "image" }
        );
    }

    #[test]
    fn a_negative_image_index_is_rejected() {
        assert_eq!(
            go(&doc(r#"{"type":"image","rect":[0,0,1,1],"image":-1}"#)).unwrap_err(),
            BuildError::BadField { field: "image" }
        );
    }

    #[test]
    fn a_bad_colour_names_the_field_that_held_it() {
        assert_eq!(
            go(&doc(
                r#"{"type":"panel","rect":[0,0,1,1],"background":"nonsense"}"#
            ))
            .unwrap_err(),
            BuildError::BadColor {
                field: "background"
            }
        );
        assert_eq!(
            go(&doc(
                r##"{"type":"frame","rect":[0,0,1,1],"color":"#12345"}"##
            ))
            .unwrap_err(),
            BuildError::BadColor { field: "color" }
        );
    }

    #[test]
    fn deep_nesting_is_rejected_rather_than_overflowing_the_stack() {
        // Same reasoning as the parser: on bare metal a stack overflow is a
        // silent walk past the guard page, not a panic.
        // The parser caps nesting at the same depth, so a deep *document*
        // is refused before the builder sees it. `build` is public and takes a
        // Value, so the cap here is reached by handing it one directly -- which
        // is exactly how a caller with its own document source would hit it.
        fn node(children: Value) -> Value {
            Value::Object(alloc::vec![
                ("type".into(), Value::Str("panel".into())),
                (
                    "rect".into(),
                    Value::Array(alloc::vec![
                        Value::Int(0),
                        Value::Int(0),
                        Value::Int(1),
                        Value::Int(1)
                    ])
                ),
                ("children".into(), children),
            ])
        }
        let mut root = node(Value::Array(alloc::vec![]));
        for _ in 0..(MAX_DEPTH + 5) {
            root = node(Value::Array(alloc::vec![root]));
        }
        let doc = Value::Object(alloc::vec![
            ("width".into(), Value::Int(100)),
            ("height".into(), Value::Int(50)),
            ("root".into(), root),
        ]);
        assert_eq!(build(&doc).unwrap_err(), BuildError::TooDeep);
    }

    #[test]
    fn a_document_that_is_not_an_object_is_rejected() {
        for src in ["[]", "42", r#""hello""#, "null", "true"] {
            assert!(build(&parse(src).unwrap()).is_err(), "{src}");
        }
    }

    #[test]
    fn a_child_that_is_not_an_object_is_rejected() {
        assert!(go(&doc(r#"{"type":"panel","rect":[0,0,1,1],"children":[42]}"#)).is_err());
    }

    // --- label alignment ---

    #[test]
    fn a_label_takes_its_alignment() {
        for (word, want) in [
            ("left", Align::Left),
            ("center", Align::Center),
            ("right", Align::Right),
        ] {
            let src = alloc::format!(
                r#"{{"type":"label","rect":[0,0,50,20],"text":"x","align":"{word}"}}"#
            );
            let t = go(&doc(&src)).unwrap();
            let Kind::Label { align, .. } = &top(&t).kind else {
                panic!("expected a label");
            };
            assert_eq!(*align, want);
        }
    }

    #[test]
    fn a_label_defaults_to_the_left() {
        let t = go(&doc(r#"{"type":"label","rect":[0,0,50,20],"text":"x"}"#)).unwrap();
        let Kind::Label { align, .. } = &top(&t).kind else {
            panic!("expected a label");
        };
        assert_eq!(*align, Align::Left);
    }

    #[test]
    fn an_unknown_alignment_is_rejected() {
        // Not silently defaulted: "centre" is the spelling half the world
        // reaches for, and a label that quietly stayed left would be blamed
        // on the renderer.
        for bad in [r#""align":"centre""#, r#""align":"middle""#, r#""align":3"#] {
            let src = alloc::format!(r#"{{"type":"label","rect":[0,0,50,20],"text":"x",{bad}}}"#);
            assert!(
                matches!(go(&doc(&src)), Err(BuildError::BadField { field: "align" })),
                "{bad} should have been rejected"
            );
        }
    }

    #[test]
    fn a_label_takes_its_vertical_alignment() {
        for (word, want) in [
            ("top", VAlign::Top),
            ("middle", VAlign::Middle),
            ("bottom", VAlign::Bottom),
        ] {
            let src = alloc::format!(
                r#"{{"type":"label","rect":[0,0,50,20],"text":"x","valign":"{word}"}}"#
            );
            let t = go(&doc(&src)).unwrap();
            let Kind::Label { valign, .. } = &top(&t).kind else {
                panic!("expected a label");
            };
            assert_eq!(*valign, want);
        }
    }

    #[test]
    fn an_unknown_vertical_alignment_is_rejected() {
        // "centre" is rejected on the other axis for the same reason: a label
        // that quietly stayed put would be blamed on the renderer.
        for bad in [
            r#""valign":"center""#,
            r#""valign":"centre""#,
            r#""valign":2"#,
        ] {
            let src = alloc::format!(r#"{{"type":"label","rect":[0,0,50,20],"text":"x",{bad}}}"#);
            assert!(
                matches!(
                    go(&doc(&src)),
                    Err(BuildError::BadField { field: "valign" })
                ),
                "{bad} should have been rejected"
            );
        }
    }

    // --- the plotted kinds ---

    #[test]
    fn a_line_takes_its_points_in_order() {
        let t = go(&doc(r#"{"type":"line","rect":[0,0,50,50],
                "points":[[0.0,1.0],[0.5,0.0],[1.0,1.0]],"width":3,"closed":true}"#))
        .unwrap();
        let Kind::Line {
            points,
            width,
            closed,
            ..
        } = &top(&t).kind
        else {
            panic!("expected a line");
        };
        assert_eq!(points.len(), 3);
        assert!((points[1].0 - 0.5).abs() < 1e-6 && points[1].1.abs() < 1e-6);
        assert_eq!(*width, 3);
        assert!(*closed);
    }

    #[test]
    fn a_line_without_points_is_empty_rather_than_an_error() {
        // An author sketching a layout should get an empty node, not a
        // rejected file, until they fill the curve in.
        let t = go(&doc(r#"{"type":"line","rect":[0,0,50,50]}"#)).unwrap();
        let Kind::Line { points, closed, .. } = &top(&t).kind else {
            panic!("expected a line");
        };
        assert!(points.is_empty());
        assert!(!*closed);
    }

    #[test]
    fn a_malformed_point_is_rejected() {
        for bad in [
            r#""points":[[0.0]]"#,
            r#""points":[[0.0,1.0,2.0]]"#,
            r#""points":[0.5]"#,
            r#""points":"nope""#,
        ] {
            let src = alloc::format!(r#"{{"type":"line","rect":[0,0,50,50],{bad}}}"#);
            assert!(
                matches!(
                    go(&doc(&src)),
                    Err(BuildError::BadField { field: "points" })
                ),
                "{bad} should have been rejected"
            );
        }
    }

    #[test]
    fn a_chart_clamps_its_values() {
        let t = go(&doc(
            r#"{"type":"chart","rect":[0,0,50,50],"values":[-2.0,0.5,7.0]}"#,
        ))
        .unwrap();
        let Kind::Chart { values, .. } = &top(&t).kind else {
            panic!("expected a chart");
        };
        assert_eq!(values.len(), 3);
        assert!(values[0].abs() < 1e-6);
        assert!((values[1] - 0.5).abs() < 1e-6);
        assert!((values[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_chart_defaults_to_no_fill() {
        let t = go(&doc(
            r#"{"type":"chart","rect":[0,0,50,50],"values":[0.5]}"#,
        ))
        .unwrap();
        let Kind::Chart { fill, stroke, .. } = &top(&t).kind else {
            panic!("expected a chart");
        };
        assert!(fill.is_transparent(), "a bare trace unless asked otherwise");
        assert!(!stroke.is_transparent());
    }

    // --- the dial kinds ---

    #[test]
    fn a_needle_takes_its_fields() {
        let t = go(&doc(
            r##"{"type":"needle","rect":[0,0,50,50],"start":-30,"end":210,
                "value":0.75,"width":4,"color":"#ff0000ff","hub":9}"##,
        ))
        .unwrap();
        let Kind::Needle {
            start,
            end,
            value,
            width,
            color,
            hub,
        } = &top(&t).kind
        else {
            panic!("expected a needle");
        };
        assert_eq!((*start, *end), (-30, 210));
        assert!((*value - 0.75).abs() < 1e-6);
        assert_eq!((*width, *hub), (4, 9));
        assert_eq!(*color, Color::rgb(0xff, 0x00, 0x00));
    }

    #[test]
    fn a_needle_defaults_to_a_seven_to_five_sweep() {
        let t = go(&doc(r#"{"type":"needle","rect":[0,0,50,50]}"#)).unwrap();
        let Kind::Needle { start, end, .. } = &top(&t).kind else {
            panic!("expected a needle");
        };
        assert_eq!((*start, *end), (225, 495));
    }

    #[test]
    fn a_scale_takes_its_fields() {
        let t = go(&doc(
            r#"{"type":"scale","rect":[0,0,50,50],"ticks":21,"major_every":5,
                "length":10,"width":3}"#,
        ))
        .unwrap();
        let Kind::Scale {
            ticks,
            major_every,
            length,
            width,
            ..
        } = &top(&t).kind
        else {
            panic!("expected a scale");
        };
        assert_eq!((*ticks, *major_every, *length, *width), (21, 5, 10, 3));
    }

    #[test]
    fn a_negative_tick_count_becomes_none() {
        // Clamped rather than rejected: it is a slip, and the rest of the
        // cluster is worth more than the missing scale.
        let t = go(&doc(r#"{"type":"scale","rect":[0,0,50,50],"ticks":-4}"#)).unwrap();
        let Kind::Scale { ticks, .. } = &top(&t).kind else {
            panic!("expected a scale");
        };
        assert_eq!(*ticks, 0);
    }

    #[test]
    fn a_needle_value_out_of_range_is_clamped() {
        let t = go(&doc(r#"{"type":"needle","rect":[0,0,50,50],"value":4.0}"#)).unwrap();
        let Kind::Needle { value, .. } = &top(&t).kind else {
            panic!("expected a needle");
        };
        assert!((*value - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod anim_tests {
    use super::*;
    use crate::anim::{Easing, Repeat};
    use crate::scene::parse;

    fn scene(root: &str) -> Scene {
        let src = alloc::format!(r#"{{"width":100,"height":50,"root":{root}}}"#);
        build_scene(&parse(&src).expect("must parse")).expect("must build")
    }

    #[test]
    fn a_scene_without_animation_has_none() {
        let s = scene(r#"{"type":"bar","rect":[0,0,10,10]}"#);
        assert!(s.anims.is_empty());
    }

    #[test]
    fn an_animate_block_is_collected() {
        let s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],
                "animate":{"property":"value","from":0,"to":1}}"#,
        );
        assert_eq!(s.anims.len(), 1);
    }

    #[test]
    fn animations_on_children_are_collected_too() {
        let s = scene(
            r#"{"type":"panel","rect":[0,0,10,10],"children":[
                 {"type":"bar","rect":[0,0,5,5],
                  "animate":{"property":"value","from":0,"to":1}},
                 {"type":"bar","rect":[5,0,5,5],
                  "animate":{"property":"x","from":0,"to":9}}]}"#,
        );
        assert_eq!(s.anims.len(), 2);
    }

    #[test]
    fn optional_fields_default() {
        // The common case is a gauge sweeping between two readings; making
        // easing, repeat and duration mandatory would put five lines of
        // boilerplate on every one of them.
        let s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],
                "animate":{"property":"value","from":0,"to":1}}"#,
        );
        assert_eq!(s.anims.len(), 1);
        let _ = Easing::default();
        let _ = Repeat::default();
    }

    #[test]
    fn every_easing_and_repeat_name_is_accepted() {
        for e in [
            "linear",
            "in_quad",
            "out_quad",
            "in_out_quad",
            "in_cubic",
            "out_cubic",
            "in_out_cubic",
            "step",
        ] {
            let src = alloc::format!(
                r#"{{"type":"bar","rect":[0,0,1,1],
                     "animate":{{"property":"value","from":0,"to":1,"easing":"{e}"}}}}"#
            );
            let _ = scene(&src);
        }
        for r in ["once", "loop", "ping_pong"] {
            let src = alloc::format!(
                r#"{{"type":"bar","rect":[0,0,1,1],
                     "animate":{{"property":"value","from":0,"to":1,"repeat":"{r}"}}}}"#
            );
            let _ = scene(&src);
        }
    }

    fn bad(root: &str) -> BuildError {
        let src = alloc::format!(r#"{{"width":100,"height":50,"root":{root}}}"#);
        build_scene(&parse(&src).expect("must parse")).unwrap_err()
    }

    #[test]
    fn a_missing_animation_field_is_reported_by_name() {
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],"animate":{"from":0,"to":1}}"#),
            BuildError::Missing { field: "property" }
        );
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],"animate":{"property":"value","to":1}}"#),
            BuildError::Missing { field: "from" }
        );
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],"animate":{"property":"value","from":0}}"#),
            BuildError::Missing { field: "to" }
        );
    }

    #[test]
    fn an_unknown_property_easing_or_repeat_is_rejected() {
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],
                    "animate":{"property":"colour","from":0,"to":1}}"#),
            BuildError::BadField { field: "property" }
        );
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],
                    "animate":{"property":"value","from":0,"to":1,"easing":"bounce"}}"#),
            BuildError::BadField { field: "easing" }
        );
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],
                    "animate":{"property":"value","from":0,"to":1,"repeat":"forever"}}"#),
            BuildError::BadField { field: "repeat" }
        );
    }

    #[test]
    fn a_negative_duration_is_rejected_rather_than_wrapping() {
        assert_eq!(
            bad(r#"{"type":"bar","rect":[0,0,1,1],
                    "animate":{"property":"value","from":0,"to":1,"duration_ms":-5}}"#),
            BuildError::BadField {
                field: "duration_ms"
            }
        );
    }
}

#[cfg(test)]
mod overflow_tests {
    use super::*;
    use crate::scene::parse;

    #[test]
    fn an_oversized_dimension_is_rejected_not_truncated() {
        // 5_000_000_000 passes `> 0` and then wraps to 705032704 under `as
        // u32`. A scene that asked for an impossible size must be refused, not
        // silently given a different one.
        let src = r#"{"width":5000000000,"height":10,
                      "root":{"type":"panel","rect":[0,0,1,1]}}"#;
        assert_eq!(
            build(&parse(src).unwrap()).unwrap_err(),
            BuildError::BadField { field: "width" }
        );
    }

    #[test]
    fn an_oversized_rect_is_rejected_not_truncated() {
        let src = r#"{"width":10,"height":10,
                      "root":{"type":"panel","rect":[0,0,5000000000,1]}}"#;
        assert_eq!(
            build(&parse(src).unwrap()).unwrap_err(),
            BuildError::BadField { field: "rect" }
        );
    }

    #[test]
    fn an_out_of_range_coordinate_is_rejected_not_truncated() {
        // x and y go through `as i32`, which wraps just as silently.
        let src = r#"{"width":10,"height":10,
                      "root":{"type":"panel","rect":[5000000000,0,1,1]}}"#;
        assert_eq!(
            build(&parse(src).unwrap()).unwrap_err(),
            BuildError::BadField { field: "rect" }
        );
    }

    #[test]
    fn an_out_of_range_angle_is_rejected_rather_than_wrapped() {
        let src = r#"{"width":10,"height":10,
                      "root":{"type":"needle","rect":[0,0,10,10],"start":5000000000}}"#;
        assert_eq!(
            build(&parse(src).unwrap()).unwrap_err(),
            BuildError::BadField { field: "start" }
        );
    }

    #[test]
    fn an_out_of_range_tick_count_is_rejected_rather_than_wrapped() {
        let src = r#"{"width":10,"height":10,
                      "root":{"type":"scale","rect":[0,0,10,10],"ticks":5000000000}}"#;
        assert_eq!(
            build(&parse(src).unwrap()).unwrap_err(),
            BuildError::BadField { field: "ticks" }
        );
    }
}
