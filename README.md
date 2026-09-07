# CogwareCopilot

A retained-mode graphics toolkit for embedded and bare-metal targets, in the
spirit of LVGL but pure Rust. Scenes are described in a human-editable text
format, drawn with dirty-rectangle rendering, and rendered identically on a
desktop simulator and on real hardware.

```
cargo run -p copilot-sim  -- examples/cluster.scene    # watch it, hot-reloads on save
cargo run -p copilot-edit -- examples/cluster.scene    # edit it
cargo run -p copilot-edit -- examples/z31.rig          # edit a car's three screens at once
```

## The crates

| Crate | | |
| --- | --- | --- |
| `copilot` | `#![no_std]` + `alloc` | The whole toolkit. **One dependency** -- [`cogware-can`](https://codeberg.org/Cogware/CogwareCan), the gauge spec a scene binds to -- and it builds for `aarch64-unknown-none`. |
| `copilot-sim` | std | A window that renders a scene and reloads it when the file changes. |
| `copilot-edit` | std | Visual editor: live preview, outliner, drag to move, dummy values. |
| `fontconv` | std, host only | Converts a TTF into the bitmap atlas `copilot::font` reads. |

Nothing flows the other way. A bare-metal consumer links `copilot` and pays for
none of the rest.

## What is in the box

- **Dirty-rectangle rendering.** A widget marks itself dirty; the compositor
  repaints only the rectangles that changed. On a Pi, whose framebuffer is
  non-cacheable, that is the difference between tens of milliseconds a frame
  and hundreds of microseconds.
- **A `Surface` trait as the only route to pixels.** A VideoCore framebuffer, a
  DRM plane, an SPI panel, a simulator window and someone else's GPU driver are
  all just implementations. The crate ships one software rasteriser and no
  drivers, because a driver inside a graphics toolkit is a driver nobody can
  replace.
- **A scene format that is JSON plus comments and trailing commas**, because a
  format nobody can annotate is a format nobody maintains.
- **Widgets bound to live readings.** A widget says which gauge it shows and
  over what range; the gauge names come from [`cogware-can`](https://codeberg.org/Cogware/CogwareCan)
  and are resolved when the scene is *built*, so a typo fails on the desk
  rather than drawing a blank in the car. A display derives its CAN
  subscription from the bindings and writes each reading onto its widget.
- **Rigs: several screens and modes you name yourself.** A `.rig` file names
  the displays on one bus by CAN node address and the scene each shows in each
  drive mode. The editor opens the lot side by side, with a tab per mode, so a
  change can be checked against the screens it sits beside — and modes are
  added, renamed and removed from the editor, so "rockcrawl" is as much a mode
  as "sport".
- **Images and animation.** QOI and GIF decoders, both written here; property
  animation with easing curves; GIF playback with pause, seek and speed.
- **Bitmap fonts.** A 5x7 face is compiled in so labels work immediately;
  `fontconv` builds a better one from any TTF you have the right to use.
- **Runtime-agnostic async.** Plain `core::future`; the crate spawns nothing and
  owns no executor.

## A scene

```jsonc
{
  "width": 320, "height": 160,
  "node": "0x01",                  // which display on the bus this is for
  "images": ["logo.qoi"],          // paths resolve next to this file
  "root": {
    "type": "panel", "rect": [0, 0, 320, 160], "background": "#0d1117",
    "children": [
      { "type": "label", "rect": [16, 16, 200, 20], "text": "88", "color": "#f0f6fc" },
      { "type": "bar", "rect": [16, 60, 288, 24], "name": "rpm",
        "value": 0.4, "fill": "#58a6ff", "track": "#161b22",
        "bind": { "gauge": "RPM", "min": 0, "max": 8000 },
        "animate": { "property": "value", "from": 0.1, "to": 0.9,
                     "duration_ms": 2600, "easing": "in_out_cubic",
                     "repeat": "ping_pong" } },
    ],
  },
}
```

Every widget type is listed in [`docs/widgets.md`](docs/widgets.md), which also
covers what a `bind` may say and how it sits alongside an `animate`.

## A rig

```jsonc
{
  "modes": ["normal", "sport", "track"],   // the value on the bus is the index
  "displays": [
    { "name": "cluster", "node": "0x01",
      "scenes": { "normal": "z31-normal.scene", "sport": "z31-sport.scene",
                  "track": "z31-track.scene" } },
    // ... one entry per screen; 0x00 is the gateway and is never a display
  ],
}
```

The mode names are yours. Nothing in the format or the code knows "sport"
from "rockcrawl" — a mode is whatever the car needs one for, and the editor's
Modes dialog (the `+` beside the tabs) adds, renames and removes them,
writing a scene file per display as it goes. Every display names a scene for
every mode: a mode with no scene is an error rather than a fallback, because
a car in track mode with a blank gauge is the failure the file exists to rule
out. Switching modes at speed is CogwareCan's job — this crate only makes the
files carry it. `examples/z31.rig` is a cluster and two round gauges in three
modes.

## Using it

The core crate cannot open a file — it has no idea what a filesystem is. A host
reads the bytes and hands them over:

```rust
let doc       = copilot::scene::parse(&text)?;
let mut scene = copilot::scene::build_scene(&doc)?;  // tree, assets, bindings
// ... host loads scene.requests into an ImageTable ...
copilot::render::compose_all(&mut surface, &scene.tree, resources);
```

A display on the bus subscribes to what its scene binds and feeds the readings
back in, once a frame:

```rust
let mut sub = Subscription::new(&scene.wanted());   // the gauge ids it shows
// ... per frame: feed_frame(&f) for everything off the bus ...
scene.anims.tick(&mut scene.tree, now_us);
scene.apply_gauges();                                // live data over the top
copilot::render::compose(&mut surface, &scene.tree, resources);
```

`examples/cluster.scene` is a working instrument cluster; `copilot/tests/`
renders it headless and asserts on pixels, and checks that every scene in
`examples/z31.rig` binds against the real gauge table.

## Building

The desktop crates build on Linux and Windows, x86-64, with a plain `cargo
build`; nothing in them names an OS API directly. On the platform you are on:

```
cargo build --release --workspace
```

To build Windows binaries from Linux there are two routes. Either install a
MinGW cross toolchain (`mingw-w64-gcc` on Arch, `gcc-mingw-w64-x86-64` on
Debian) and build for the target:

```
cargo build --release --workspace --target x86_64-pc-windows-gnu
```

or, without root and without a second toolchain, let zig do the linking:

```
pip install --user ziglang
cargo install cargo-zigbuild
cargo zigbuild --release --workspace --target x86_64-pc-windows-gnu
```

Either way the executables land in `target/x86_64-pc-windows-gnu/release/`.
Without any cross linker, `cargo check --target x86_64-pc-windows-gnu` still
type-checks everything for Windows, which is what the toolchain file lists the
target for.

## Contributing

The rules the code is written to, in short: comments explain *why*, a warning
is a broken build, the core crate depends on `core`, `alloc` and the gauge spec
and nothing else, and anything testable on the host is tested on the host.
