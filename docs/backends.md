# Writing a backend

A backend is one `impl copilot::Surface`. There is no registration and no
feature flag: a display crate implements the trait and a host passes it to
`copilot::render::frame()`.

`copilot/src/surface/mod.rs` is the reference; this is the part that does not
fit in doc comments.

---

## The smallest thing that works

Five methods, and every widget in the crate draws correctly in software.

```rust
use copilot::{Color, PixelFormat, Rect, Size, Surface};

struct Framebuffer { size: Size }

impl Surface for Framebuffer {
    fn size(&self) -> Size { self.size }
    fn format(&self) -> PixelFormat { PixelFormat::Bgrx8888 }

    fn fill_span(&mut self, x: i32, y: i32, count: u32, color: Color) {
        let (packed, bytes) = color.pack(PixelFormat::Bgrx8888);
        // ... write `count` pixels from (x, y) ...
    }

    fn blit_span(&mut self, x: i32, y: i32, src: &[Color]) {
        // ... convert and write src.len() pixels from (x, y) ...
    }

    fn present(&mut self, _damage: Option<Rect>) { /* flip, or nothing */ }
}
```

Two guarantees, so those bodies need nothing defensive:

- **Every coordinate is already clipped** to `size()`. A span you are handed
  lands.
- **Colours arrive as `Color`**, not as your format. `Color::pack` converts.

Everything past these five is optional and defaults to something correct.

---

## Capabilities

| Capability | Read by | Set it when |
|---|---|---|
| `RETAINS_CONTENT` | **The renderer** — `frame()` repaints the damage rather than the screen | The buffer you are given still holds what you last drew |
| `READ_BACK` | The host | `row_mut` returns rows, or `blend_span` composites |
| `ACCELERATED` | The host | Drawing is hardware |
| `SCRATCH` | **The renderer** — a cross-fade blends through the second buffer instead of cutting | You have the memory for a second full-size buffer |

Only the first changes what the toolkit does; the other two are declarations
for diagnostics. `Caps::NONE` is the conservative default.

`RETAINS_CONTENT` is a claim about the buffer you are about to draw into, not
the one on the glass. A page flip hands back a buffer two frames old, and a GPU
may hand back a fresh render target. Claiming it falsely leaves an old frame's
pixels wherever nothing moved.

---

## Call order

When a scene loads, once:

```text
render::upload_assets  ->  Surface::upload(id, pixels, w, h)
```

Each frame:

```text
render::frame  ->  Surface::begin_frame(&damage)
                   the draw_* hooks and, where one declined, spans
                   Surface::present(bounds)
```

When the scene is replaced, `render::release_assets` hands the ids back to
`Surface::release`.

Nothing between `begin_frame` and `present` allocates, and nothing there calls
`upload`. That is enforced: `copilot/tests/no_alloc.rs` installs a counting
allocator and fails the build on a single allocation, so you can rely on it
when sizing a fixed heap.

**Wait in `begin_frame`, not `present`** — for a flip, a fence, a DMA — so the
renderer composes during the gap instead of blocking after it. `begin_frame` is
also where a command list opens and where scissor rectangles are set, being the
only point at which the whole damage set is visible.

---

## The hooks

Each is handed geometry that is already clipped and returns whether it drew it.
`false`, the default, sends the work to the software rasteriser.

| Hook | Reaches it |
|---|---|
| `draw_rect(rect, color)` | Panels, frames, bar segments, glyph ink. Take this first |
| `draw_image(dst, area, src, src_w, src_h)` | Image and animation widgets whose pixels you do not hold |
| `draw_gradient(dst, area, from, to, vertical)` | Gradient panels |
| `draw_texture(id, dst, area)` | Image and animation widgets whose pixels you do hold |
| `draw_primitive(prim, area, color, aa)` | Every dial, needle, chart line and hub |

**They decline rather than override** because hardware accelerates a case, not
an operation. A blitter that fills rectangles up to 2048 wide has nothing to do
with a wider one:

```rust
fn draw_rect(&mut self, rect: Rect, color: Color) -> bool {
    if rect.size.w > 2048 || color.a != 255 {
        return false;          // the rasteriser emits spans instead
    }
    self.queue_fill(rect, color);
    true
}
```

"Already clipped" means in bounds, not within *your* limits — nothing knows
your blitter's maximum width. Declining is the mechanism, not a defensive check.

**If you return `true`, you drew it.** Returning `true` without drawing leaves
a hole; drawing and returning `false` paints it twice, which is invisible on a
memory buffer and a torn frame on hardware.

**`dst` fixes the mapping, `area` bounds the painting.** `dst` is where the
whole thing would go — which source pixel lands where, where a ramp starts and
ends. `area` is the part you may touch. Computing the mapping from `area`
restarts a gradient at the edge of whatever was dirty and slides an image under
its own clip. `draw_rect` has no source to map, so it gets one rectangle.

---

## Holding pixels

A GPU cannot re-upload a gauge face every frame, so the upload happens once and
the draw names the result. `TextureId` is derived from the scene —
`TextureId::Image { index }` is the number the scene file's image list uses —
because a driver-allocated handle would have to live in the scene tables, and
that would make `copilot::asset` depend on whichever backend is linked.

It derives `Ord`: `core` has no hash map, so a bare-metal driver binary-searches
a sorted array.

```rust
fn upload(&mut self, id: TextureId, pixels: &[Color], _w: u32, _h: u32) -> bool {
    if self.vram.len() + pixels.len() > VRAM_BYTES {
        return false;               // no room is not a failure
    }
    let offset = self.vram.len();
    self.vram.extend_from_slice(pixels);
    self.resident.push(Resident { id, offset });
    self.resident.sort_unstable_by_key(|r| r.id);
    true
}

fn draw_texture(&mut self, id: TextureId, dst: Rect, area: Rect) -> bool {
    let Ok(i) = self.resident.binary_search_by_key(&id, |r| r.id) else {
        return false;               // never took it; the rasteriser will blit
    };
    self.queue_textured_quad(self.resident[i].offset, dst, area);
    true
}
```

That impl is a compile-checked doctest on `TextureId`, so it cannot drift.
`TextureId` is `#[non_exhaustive]`; match with `_ => false`.

---

## Curves and diagonals

`draw_primitive` decides whether a cluster is worth accelerating. Arcs, discs
and lines are drawn in software by sampling coverage inside every pixel they
touch; to hardware each is one primitive.

```rust
fn draw_primitive(&mut self, prim: Primitive, area: Rect, color: Color, aa: bool) -> bool {
    match prim {
        Primitive::Arc { centre, inner, outer, start, sweep } => {
            self.queue_ring(centre, inner, outer, start, sweep, color, aa);
            true
        }
        _ => false,          // needles and hubs stay on the sampler for now
    }
}
```

- **Coordinates are floats and unclipped.** A needle passes through every angle
  as it sweeps; `area` is the clipped rectangle, the shape is not.
- **Angles are brads**, a full turn being `trig::TURN` = 4096, and `sweep` is
  signed. Losing the sign draws the complement of the ring.
- **`inner` is already clamped** to at most `outer`.
- **Decline when `aa` is set and you cannot blend**, so the software path draws
  what the scene asked for.

### What it is worth

`examples/gauge-left-normal.scene` is a 480x480 gauge with an arc sweep, a rim
scale, a needle, a hub and a seven-segment readout. Counted by
`copilot/tests/render_scene.rs`:

| Backend takes | Primitives | Rectangles | Spans |
|---|---|---|---|
| nothing | — | — | 2363 |
| `draw_primitive` | 14 | — | 1091 |
| `draw_primitive` + `draw_rect` | 14 | 148 | **0** |

Call counts, not times. Shapes alone leave half the spans behind because the
readout and panels are rectangles: the two hooks are complementary, and a
driver wants both.

---

## Cross-fading needs a second buffer

Eight of the nine transitions — the cut, the four slides and the four wipes —
are a clip and an offset over two ordinary tree walks, and cost a backend
nothing. A cross-fade is the exception: blending two scenes per pixel means
both have to exist at once, which is a second full-size buffer.

**If your board cannot spare one, implement nothing.** Leave `SCRATCH` clear
and a scene asking for `"transition": "crossfade"` gets a cut instead. That is
the entire penalty — no error, no blank frame, nothing to handle.

If it can, implement `with_scratch`, which draws and blends in one call so the
pair cannot be half-implemented:

```rust
fn caps(&self) -> Caps { Caps::SCRATCH | Caps::RETAINS_CONTENT }

fn with_scratch(&mut self, alpha: u8, draw: &mut dyn FnMut(&mut dyn Surface)) -> bool {
    let Some(mut second) = self.spare.take() else {
        return false;             // declining per frame is allowed too
    };
    draw(&mut second);            // the renderer paints the outgoing scene
    self.blend_over_front(&second, alpha);
    self.spare = Some(second);
    true
}
```

The renderer draws the *incoming* scene to the front buffer itself and hands
you the *outgoing* one to blend back at `alpha`, so the two sum to one picture
at every point of the fade.

## What is still spans

Seven-segment digits, segment bars, grids, polygon fills, and anything whose
hook you declined arrive as `fill_span`, `blit_span` and `blend_span`. A hook
per widget kind would grow the trait every time the scene format learns one, so
the answer is to **batch**: accumulate spans into a vertex buffer and flush in
`present`.

`blend_span` is worth overriding directly. Its default reads the row back
through `row_mut`; a surface that cannot be read gets pixels at least half
covered written opaque and the rest left alone, which is the aliased picture
rather than a wrong one.

---

## Checklist

- [ ] The five required methods, nothing defensive in them
- [ ] `caps()` — and `RETAINS_CONTENT` only if it is true
- [ ] Every hook returning `true` drew it, exactly once
- [ ] `dst` fixes the mapping; `area` bounds the painting
- [ ] `draw_primitive` matches with `_ => false`, and declines blending it cannot do
- [ ] Waiting happens in `begin_frame`
- [ ] `upload` declines gracefully when there is no room
