# The widget set

What exists, what is planned, and why the list stops where it does.

`copilot` has no input handling, so LVGL's interactive widgets — button, slider,
switch, checkbox, dropdown, roller, textarea, keyboard — are out of scope until
it does. What is left is the display half of LVGL's catalogue, which is what a
dashboard needs anyway.

## Shipped

The three angular widgets share one sweep convention -- degrees clockwise from
twelve o'clock -- so an `arc`, a `scale` and a `needle` stacked in one node
agree about where a value sits without the scene file restating it.

| Widget | |
| --- | --- |
| `panel` | Solid rectangle. Also the grouping element: a transparent one draws nothing but still positions its children. |
| `frame` | One-pixel outline, drawn just inside the node. |
| `label` | Text in the current font, with integer magnification and alignment on both axes. |
| `image` | A still from the scene's image table, scaled nearest-neighbour. |
| `anim` | A GIF from the animation table, with play, pause, seek and speed. |
| `bar` | Horizontal or vertical fill against a track. |
| `segbar` | The same reading as discrete cells, the way a vacuum-fluorescent panel shows it: colour bands fixed to the scale, an optional profile cutting each cell to a curve, a second axis setting how tall the lit cells stand, and optional divisions breaking each cell into dashes on a grid shared right along the bank. |
| `arc` | A ring filled between two angles. Every round gauge starts here. |
| `needle` | A pointer swinging over a sweep, with a hub over the pivot. |
| `scale` | Tick marks around a dial's rim, with major divisions longer and thicker. |
| `ruler` | The same marks along a straight edge, for graduating a bar. |
| `roundrect` | A panel with rounded corners. |
| `led` | A telltale lamp: on, off, or dim, with an unlit glow so the symbol still reads. |
| `line` | A polyline through listed points, given as fractions of the box so a curve survives a change of screen. |
| `chart` | A series across the box, as a trace or a filled area. |
| `sevenseg` | Digits drawn from the seven bars a real display has, unlit segments included, with a decimal point and an optional fixed number of cells. |
| `gradient` | A fill ramping between two colours, down or across. |
| `grid` | Lines ruled across the box, for the etched grid a real panel's glass carries. |
| `polygon` | A filled shape through listed points, even-odd so a notch stays a notch. |

## Planned

Nothing outstanding. The display half of LVGL's catalogue is covered; what
remains in the list below is out of scope rather than merely unwritten.

There is no icon widget, and there will not be one. A turn arrow, a warning
triangle and a chevron are three points and seven points; a scene that can say
so needs no built-in symbol set to be argued about and kept up to date.

`segbar`'s bands run from the top of the scale down, which is what a
tachometer, a temperature gauge and a boost gauge all want. A tank is the
exception -- low is the bad end -- and reads better as a telltale beside the
gauge than as an inverted band inside it.

## Antialiasing

Off by default, so a scene written before the setting existed renders exactly
as it did. Switched on for the whole scene with a top-level key, and for any
one widget -- either way -- with the same key on the widget, which its
children inherit:

```json
{ "width": 800, "height": 480, "antialias": true,
  "root": { "type": "panel", "rect": [0, 0, 800, 480], "children": [
    { "type": "needle", "rect": [40, 40, 200, 200], "value": 0.6 },
    { "type": "sevenseg", "rect": [300, 40, 120, 60], "text": "88", "antialias": false }
  ] } }
```

What it changes: the edges of a `needle`, a `scale`'s ticks, an `arc`, a
`line`, a `chart`'s stroke and the top of its fill, a `polygon`, a
`roundrect`'s corners, the leading edge of a `bar`, and the cell boundaries of
a `segbar`. Those last two are the reason a slow sweep moves smoothly and a
row of twenty cells in a box that does not divide by twenty looks even: the
cell edges land where the arithmetic put them, to a fraction of a pixel, and
the pixel each edge falls in is blended by how much of it the cell covers.
A `segbar` with no gap between its cells keeps whole-pixel edges even when
antialiased, because two blended edges meeting in one pixel leave a faint
seam, which is worse than the pixel of unevenness it would replace.

Everything with a straight, pixel-aligned edge -- `panel`, `frame`, `label`,
`image`, `anim`, `led`, `gradient`, `grid`, `ruler`, `sevenseg` -- is drawn as
before. Text in particular is a bitmap font at an integer magnification and
stays crisp on purpose.

Blending needs the pixel underneath. A surface that can be read back
(`row_mut`, which every memory-mapped framebuffer has) blends; one that cannot
gets the pixels that are at least half covered written solid and the rest
left alone -- the aliased picture rather than a wrong one. The cost is a few
samples per edge pixel, paid only along the edges of the shapes listed above.

## Binding to a gauge

A widget can say what it *shows*, not only what it looks like at one authored
value. The gauge is one from the bus spec's table -- `RPM`, `CLNT`, `MAP`,
`BAT_VOL`, `FUEL_LEVEL`, and so on, spelled the way the table spells them --
and the range says how a reading maps onto the widget:

```json
{ "type": "segbar", "rect": [212, 350, 1400, 500], "value": 0.0,
  "bind": { "gauge": "RPM", "min": 0, "max": 8000 } }
{ "type": "sevenseg", "rect": [600, 80, 400, 200], "text": "0",
  "bind": { "gauge": "CLNT", "unit": "°F", "decimals": 0 } }
{ "type": "needle", "rect": [40, 40, 400, 400], "value": 0.0,
  "bind": { "gauge": "MAP", "unit": "psi", "min": -15, "max": 30 } }
```

`bar`, `segbar`, `arc`, `needle` and `led` take the reading as their 0..1
fraction: `min` maps to empty and `max` to full, and `max` is required, because
a tachometer with no range is pinned full from one rev per minute. `label` and
`sevenseg` take it as text, formatted to `decimals` places and space-padded on
the left to `pad` characters, so a three-cell speedo reads `  7` rather than
`7  `; there `min` and `max` are optional and only describe the span the
editor's dummy values sweep.

`unit` is optional. Without it the reading is in the gauge's own unit (kPa,
°C, km/h, V ...); with it the reading is converted first, and it has to be a
unit of the same dimension -- `"unit": "psi"` on `RPM` fails to build. Plain
keyboard spellings work: `F`, `C`, `lambda`, `kph`, `us`.

`divide` and `offset` are the arithmetic between the unit and the widget:
the reading becomes `reading / divide + offset`. They are what stops a
display needing code for the two things every dashboard does anyway:

```json
{ "type": "sevenseg", "text": " 0",
  "bind": { "gauge": "RPM", "divide": 100, "pad": 2 } }
{ "type": "needle",
  "bind": { "gauge": "MAP", "unit": "psi", "offset": -14.7, "min": -15, "max": 30 } }
```

The first is a tachometer whose face is printed `x100 r/min`: the bus carries
3500 and the cells show 35. The second is a boost gauge, which reads zero at
atmospheric rather than 14.7. The division happens before the offset, so an
offset is in the units the reader actually sees, and both happen before
`min`/`max` -- a range is written in what the widget shows. `divide` defaults
to 1, `offset` to 0, and a `divide` of 0 stops the scene from building.

A gauge the table does not have, or a bind on a widget that shows nothing
(`panel`, `frame`, `grid`...), stops the scene from building. That is the
point of resolving at build time: a typo fails on the desk, not in the car.

`bind` and `animate` can sit on the same widget. A display applies bindings
after ticking the animations, so live data wins wherever the gauge has a
reading and the animation carries on wherever it does not -- which is what
lets one file demonstrate itself in the simulator and read the engine in the
car. The authored `value` or `text` is what shows before the first frame
arrives.

Two scene-level keys go with this. `"node"` is the CAN address of the display
the scene is for -- `1` or `"0x01"`, never `0x00` (the gateway) or `0xFF`
(everyone) -- and `"shape": "round"` marks a panel whose corners are behind a
bezel, so a preview can mask them.

## A bank has two axes

`value` says how far along its scale a `segbar` has got, and so how many cells
light. `height` says how far up their envelope the lit ones stand, and defaults
to 1.0 -- every lit cell at full height, which is a plain segmented bar and
what every scene written before the field existed draws.

Two axes because the instrument this replicates has two. Its columns light
left to right with the revs, and their height is boost: the top row is full
boost, which is the detail that makes it odd and worth copying properly. Both
can be bound at once, and a `bind` may be an array to say so:

```json
{ "type": "segbar", "rect": [212, 350, 1400, 500], "segments": 40,
  "profile": [0.3, 0.45, 0.6, 0.75, 0.9, 1.0, 0.95, 0.8],
  "bind": [
    { "gauge": "RPM", "min": 0, "max": 7000 },
    { "gauge": "MAP", "unit": "psi", "min": 0, "max": 20, "property": "height" }
  ] }
```

`property` names the axis a block drives -- `value`, `height` or `text` -- and
is only needed when it is not the widget's own: a bar has one axis and never
needs it. Asking for an axis a widget does not have (`height` on a `bar`)
stops the scene from building rather than doing nothing quietly.

The envelope and the axis multiply: a cell reaches `profile[i] × height` across
its box. A `profile` alone is the shape printed on the lens, and `height` is
how far up it the reading stands.

## Shaping a curve

`profile`, a `line` or `polygon`'s `points`, and a `chart`'s `values` are all
rows of numbers, and a power curve typed as twenty-eight decimals is a curve
nobody adjusts twice. The editor shapes them on the preview instead: select
the widget and press **Shape the envelope…** (or **Move the points…**).

The widget is zoomed to fill the canvas and everything else goes behind a
veil, because a handle sits on the very pixels a selection drag would use and
one gesture cannot mean both. Drag a handle to move it, click the curve to add
a point, press Delete to remove the one last touched, and press **Done** to
give the editor back. Each change is spliced into the text as it happens, so
the widget reshapes under the handles, and the whole drag is one Ctrl+Z.

A handle's place *along* an envelope is not draggable: its handles are cells,
and cell three is cell three. A polyline's points move freely, because a
polyline is a shape.

## A readout's digits and its point

`sevenseg` shows a decimal point, and it does not cost a cell: like the eighth
segment on real hardware it hangs off the bottom-right of the digit before it,
so `14.7` is three cells and the digits either side of the point do not shift.
A readout showing a point anywhere shows its unlit points too, on every cell,
because a display with points has them whether or not they are on; one that
never shows a fraction draws none, so a speedometer does not grow dark dots it
will never use.

`digits` fixes how many cells the readout has, whatever the text is:

```json
{ "type": "sevenseg", "rect": [96, 18, 620, 176], "text": "0", "digits": 3,
  "bind": { "gauge": "VSS", "unit": "mph" } }
{ "type": "sevenseg", "rect": [0, 0, 200, 80], "text": "14.7", "digits": 3,
  "bind": { "gauge": "AFR_PRI", "decimals": 1 } }
```

Without it the cells are sized from the text, so `9` and `188` draw at two
different widths in the same box and a speedometer visibly rearranges itself
every time it crosses a hundred. With it the text is right-aligned into the
field and the spare cells on the left are blank — a four-digit panel showing
`42` still looks like a four-digit panel. A value too big for its field keeps
its last digits, which are the ones still moving. `digits: 0`, the default,
sizes the field to the text.

For a bound readout, `decimals` on the binding is what makes it a decimal one:
`decimals: 1` on an AFR gauge gives `14.7`, and the widget draws the point.

## A trap worth knowing

Colours are written, not blended: the surfaces this targets have no alpha to
blend against. A "dim" version of a colour has to be an opaque dark colour, not
the bright one at low alpha -- that lands as the bright one. It is why
`sevenseg`'s unlit segments and `led`'s unlit glow are both specified the long
way round. Antialiasing is the one exception, and it blends by reading the
surface back rather than by asking it to.

## Deliberately absent

`table`, `list`, `tabview`, `msgbox`, `calendar`, `spinbox` — all of them are
either interactive or assume a scrolling viewport, and a fixed instrument panel
has neither. A dashboard that needs a table has outgrown this toolkit.

`canvas` — an escape hatch for drawing arbitrary pixels from a scene file. The
scene format cannot express a program, and adding one would make a scene
something you have to audit rather than read.
