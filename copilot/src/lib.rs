// SPDX-License-Identifier: MIT OR Apache-2.0
#![no_std]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

//! A retained-mode graphics toolkit for embedded and bare-metal targets.
//!
//! The shape of the library is deliberately close to LVGL's: a scene is a tree
//! of widgets that persists between frames, each widget knows when it has
//! changed, and a frame costs only the rectangles that actually moved. What is
//! different is where the boundaries are drawn.
//!
//! # The crate owns no pixels and no operating system
//!
//! Everything here draws through [`Surface`], and a `Surface` is whatever the
//! caller says it is: a VideoCore framebuffer on a Pi, a DRM plane on embedded
//! Linux, a window in the simulator, or a GPU texture behind someone else's
//! driver. That trait is the whole of the "GPU hook-in" — the crate ships one
//! software rasteriser and no drivers, because a driver that lives in a
//! graphics toolkit is a driver nobody can replace.
//!
//! For the same reason there are no files here. A scene arrives as a `&str`
//! the caller has already read; an image arrives as a `&[u8]`. The core crate
//! cannot open a path, and on a target with no filesystem that is not a
//! limitation, it is the only arrangement that works.
//!
//! # Why time is a parameter
//!
//! Animation needs a clock, and there is no portable one. Rather than invent
//! an abstraction over "what time is it", every animated thing is advanced by
//! [`anim::Animator::tick`] with a monotonic microsecond count the caller
//! supplies. A
//! bare-metal caller reads the system timer, the simulator reads `Instant`,
//! and a test passes whatever number it likes — which is what makes animation
//! testable at all.
//!
//! # Async
//!
//! Asynchronous work here is plain [`core::future::Future`]. The crate spawns
//! nothing and owns no executor, so it composes with the BSP's executor on
//! bare metal, with `embassy` on an MCU, and with `tokio` in the simulator.
//!
//! # The one dependency
//!
//! A widget can be bound to a gauge -- `"bind": { "gauge": "RPM", "max": 8000
//! }` -- and the gauge specification lives in [`cogware_can`]: every reading
//! the bus carries, its CAN id, its unit and its fixed-point scale. It is a
//! direct dependency rather than a string the host resolves later because a
//! scene that names a gauge which does not exist should fail to *build*, on the
//! desk, not draw a blank in the car. The crate is re-exported so that an
//! editor or a display sees exactly the table this build was compiled against.

extern crate alloc;

pub use cogware_can;

pub mod anim;
pub mod asset;
pub mod color;
pub mod font;
pub mod geom;
pub mod render;
pub mod rig;
pub mod scene;
pub mod surface;
pub mod surface_mem;
pub mod trig;
pub mod widget;

pub use color::Color;
pub use geom::{Point, Rect, Size};
pub use render::Damage;
pub use surface::{PixelFormat, Surface};
pub use surface_mem::MemorySurface;
pub use widget::{Kind, Node, NodeId, Tree};
