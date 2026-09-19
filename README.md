# CogwareCopilot

A retained-mode graphics toolkit for embedded and bare-metal targets, in the
spirit of LVGL but pure Rust. Scenes are described in a human-editable text
format, drawn with dirty-rectangle rendering, and rendered identically on a
desktop simulator and on real hardware.

```
cargo run -p copilot-sim  -- examples/cluster.scene    # watch it, hot-reloads on save
cargo run -p copilot-sim  -- --rig examples/touch.rig examples/touch.scene  # poke it: the mouse is the finger
cargo run -p copilot-edit -- examples/cluster.scene    # edit it
cargo run -p copilot-edit -- examples/z31.rig          # edit a car's three screens at once
cargo run -p copilot-edit -- examples/touch.rig        # pages, tell-tales and the pins behind them
```

## Editing a scene

The top rows separate document commands, canvas settings and preview playback.
Use **Open**, **Save**, **Undo** and **Redo** directly, or the menus for the rest.
**Add widget** groups readable widget names by purpose; the scene format keeps
its existing identifiers. Select a widget on the canvas or in the tree, then
use **Widget properties** to edit it. Controls are grouped with units, searchable
data sources and visible validation. Touch actions use **Apply touch action**
and **Revert changes** so incomplete typing does not change the scene.
**Scene source** edits the same document.

- **File > New scene**, **Open scene or rig** and **Examples** ask before
  discarding unsaved scenes, including other displays in a rig. **Save all scenes**
  saves those documents together and reports any files that could not be saved.
  In a rig, **Export scene copy** saves another copy without changing the file
  referenced by the rig; it does not copy external assets.
- **Fit scene** restores the whole canvas view. **100%** uses one screen point
  per scene pixel. **Snap to edges** and **Grid** control placement; hold Ctrl
  while dragging to bypass snapping.
- **Preview** controls simulated or recorded readings without modifying the
  document. **Playback options** contains log selection, timing and sweep speed.
  **Send simulated readings** is a separate, explicit choice that writes values
  to a connected controller; uploading a scene is a different operation.
- **View > Interface size** enlarges text and controls without changing the
  scene resolution. **Help > Editor guide and shortcuts**, or F1, explains the
  workflows and keyboard shortcuts.

Invalid source is labelled clearly and disables widget edits against an old
preview. Repair it in **Scene source** before saving or uploading.
**File > Format source** asks before removing comments, and Ctrl+Z restores
the original text.

## Engine simulation

Enable **Use preview values**, then **Play drive**. **Playback options > Generic
engine** is the default source, with road driving, idle, acceleration and overrun
scenarios. **Cold engine on restart** starts at ambient temperature; otherwise
the engine begins warm. **Restart drive** resets the simulation, including
temperatures and cumulative fuel/distance. Disabling preview values or stopping
playback pauses its clock.

The model fills every supported engine/vehicle gauge, including gauges used
only by GPIO outputs. RPM, road speed, gearing, throttle, manifold pressure,
fuelling, ignition, temperatures, VVT and exhaust readings share a common engine
state. Measured AFR has sensor lag, mixture variation and transient error rather
than copying target AFR. This is a generic petrol-engine demonstration for
dashboard design, not an ECU calibration or diagnostic prediction.

**Fill preview values** takes a static snapshot and stops playback/sweeping.
It changes neither scene text nor authored values. **All generated readings**
in Playback options shows the entire catalogue, even values not bound to the
current scene, with a name filter and clear **Simulated**, **Recorded** or
**Nominal demo value** labels. Unsupported equipment and diagnostic counters
use explicit nominal defaults instead of arbitrary animation. Indicator lights
remain manual switches; the simulator never generates a protocol heartbeat.

Choose **Sample drive (US06)** or **Open driving log...** to replay recorded
data. Recorded values remain authoritative, and recorded primary inputs such
as speed/RPM/load condition the missing simulated channels. Seeking or looping
restarts generated transients; generated values are not recovered measurements.
The command-line `--watch --drive` path uses the same model to fill its selected
log (US06 unless `--log` is supplied). Transmission remains an explicit
**Send simulated readings** choice in the editor.

## Editor connections

Open **File > Connections** (or **Connections** in the status bar).
For Bluetooth, choose **Find devices**, select the controller by name and
address, then **Connect**. Discovery and connection run in the background;
progress is shown and either can be cancelled without stopping the editor.
**Controller Wi-Fi** uses the controller's access point; **Local network**
uses an address on a network the computer and controller already share.

The connection window shows live readings. Closing it leaves the connection
open; **Disconnect** explicitly ends it. **Configure controller Wi-Fi** sends
network settings through the existing Bluetooth connection, without opening a
second one. The protocol does not acknowledge saving those settings.

**File > Upload to display** sends the open scene or rig definition. Disconnect
the live connection first, choose the controller, target display and mode, then
upload. The active display and mode initialise the destination; rigs offer names
rather than only numeric addresses. Upload defaults to **Current scene**; choose
**Rig definition** explicitly to send the rig without its scene files or assets.
Progress follows the
display's acknowledgement; cancelling waits for the controller to release the
transfer and reports if that acknowledgement does not arrive.

## The crates

| Crate | | |
| --- | --- | --- |
| `copilot` | `#![no_std]` + `alloc` | The whole toolkit. **One dependency** -- [`cogware-can`](https://codeberg.org/Cogware/CogwareCan), the gauge spec a scene binds to -- and it builds for `aarch64-unknown-none`. |
| `copilot-sim` | std | A window that renders a scene and reloads it when the file changes. |
| `copilot-edit` | std | Visual editor: live preview, outliner, drag to move, preview values. |
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
- **A GPU seam that does not demand a whole GPU.** Five methods make a working
  backend; past them a driver takes whole rectangles, pictures, gradients,
  dials and resident textures, declining whatever its hardware lacks so the
  software rasteriser draws it instead. See
  [`docs/backends.md`](docs/backends.md).
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
- **Touch, beside the buttons rather than instead of them.** A panel's samples
  become taps and swipes; any widget can carry an action, and a `pager` holds
  several pages one display swipes between without the bus, the scene file or
  the other screens changing. A mode change is still the rig's job — this is a
  screen choosing which of its own faces to show.
- **Tell-tales and GPS.** The sixty-four dashboard lamps the bus spec packs
  into two bitfield gauges bind by name — `"bind": {"indicator":
  "OIL_PRESSURE"}` — and can light a lamp or reveal a printed symbol. GPS
  speed, fix and satellite count, and the odometer in km or miles, are
  ordinary gauges and bind like any other.
- **Settings the display already has.** A rig menu that says `"from": "config"`
  is built from `cogware-can`'s own settings table, so the day the bus spec
  grows a setting the menu grows a row and no file here is edited. A menu
  longer than the box it is drawn in scrolls to keep the cursor on the screen.
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

A display can also say what its spare pins are wired to:

```jsonc
{ "name": "cluster", "node": "0x01", "gpio": 8,   // eight pins, numbered 1..8
  "pins": [
    { "pin": 1, "label": "left stalk", "indicator": "TURN_LEFT" },
    { "pin": 3, "indicator": "HIGH_BEAM", "active": "low", "pull": "up" },
    { "pin": 5, "action": "menu", "menu": "display", "button": "centre" },
    { "pin": 6, "dir": "out", "indicator": "SHIFT_LIGHT" },
    { "pin": 7, "dir": "out", "gauge": "RPM", "above": 5000 },
  ],
  "scenes": { /* ... */ } }
```

Pins are 1..n and nothing else — which physical pin logical 3 lands on is the
board's, in the board's own code, where the schematic is. The firmware reads
its port into a word and hands it to `Pins::feed`; `pull` and `dir` are the
endpoints it configures the port from. An input does **not** light its own
lamp: it reports the change with `bus::BusView::report_lamp`, the master folds
the bit into the real `INDICATORS` frame, and the lamp comes back on the bus —
on this screen and every other one at once. `Pins::lamp_states` hands the whole
loom over in one frame after a boot or a bus drop. The editor's **GPIO pins** button edits
the lot, with a picker for all sixty-four tell-tales. Its logical pin numbers
are not physical connector numbers. Drafts require review before saving, and
the dialog protects them when changing pins or displays or closing. Saving
updates the rig file, not connected hardware; these changes are separate from
scene undo.

For a value-driven output, choose **Use as > Value threshold** (which selects
Output), set the gauge, comparison and threshold, then review and save. For example,
**RPM > 5000** commands HIGH above 5000 and LOW at or below it. Thresholds use the
gauge's own unit shown beside the value, not a widget's converted or scaled
display value. The rig accepts `above`, `at_least`, `below` or `at_most`, with
exactly one comparison per output and no simultaneous indicator or button action.
A missing gauge reading commands LOW; the latest received reading remains in use
until the bus layer updates or clears it.

The GPIO preview is one compact row of numbered lights. Hover for the wire label,
mapping and state; click a light to configure its pin. Scroll horizontally for
more pins rather than wrapping the labels. **Preview values** includes gauges
and indicators used only by GPIO, even if no widget displays them. Enable
**Use preview values** to try a threshold; sweeping and drive playback use the
same comparison as the runtime. Green means an output command is on, amber an
input indicator is on, and a hollow light means no preview state is available.
The strip never reads or drives physical pins.

Physical outputs require firmware built with this GPIO runtime, subscribing to
`Pins::wanted()` and applying `Pins::outputs()`. Uploading a rig does not update
firmware. HIGH/LOW here are output commands; physical pin assignment, electrical
inversion and driver circuitry remain the board's responsibility.

The mode names are yours. Nothing in the format or the code knows "sport"
from "rockcrawl" — a mode is whatever the car needs one for, and the editor's
**Manage modes** dialog (beside the mode tabs) adds, renames and removes them,
with an explicit review before a file-changing operation. Adding a mode
creates a scene file per display; removing one retains its scene files. Every display names a scene for
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
copilot::render::frame(&mut surface, &mut scene.tree, resources);
```

`frame` asks the backend whether the last frame's pixels are still there and
repaints either the damage or the screen accordingly.

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

Sending a patch accepts the contributor agreement in `CONTRIBUTING.md`. The
project is dual licensed, and that agreement is what keeps the commercial half
possible.

## License

GPL-3.0-only, with `tools/fontconv` under MIT OR Apache-2.0. A commercial
license is available for firmware you cannot ship under the GPL; `LICENSING.md`
explains both and how to ask.
