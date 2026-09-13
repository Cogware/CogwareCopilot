# The widget set

What exists, what is planned, and why the list stops where it does.

Most of LVGL's interactive widgets — slider, switch, checkbox, dropdown,
roller, textarea, keyboard — are out of scope: they assume a pointer and a
keyboard, and a dashboard has a finger. What a finger needs is any widget
being pressable and a way to swipe between screens, which is `touch` and
`pager` below rather than a catalogue of controls.

## Shipped

The three angular widgets share one sweep convention -- degrees clockwise from
twelve o'clock -- so an `arc`, a `scale` and a `needle` stacked in one node
agree about where a value sits without the scene file restating it.

| Widget | |
| --- | --- |
| `panel` | Solid rectangle. Also the grouping element: a transparent one draws nothing but still positions its children. |
| `pager` | A stack of pages, one showing. Each child is a page; swiping or a `touch` action turns it. |
| `frame` | One-pixel outline, drawn just inside the node. |
| `label` | Text in the current font, with integer magnification and alignment on both axes. |
| `image` | A still from the scene's image table, scaled nearest-neighbour. |
| `anim` | A GIF from the animation table, with play, pause, seek and speed. |
| `bar` | Horizontal or vertical fill against a track, in as many colour bands as the scale has. |
| `segbar` | The same reading as discrete cells, the way a vacuum-fluorescent panel shows it: colour bands fixed to the scale, colour optionally ramping across the cells, a brighter cell marking the reading, a say in which cells the reading lights at all, an optional profile cutting each cell to a curve, a say in how much of its cell an unlit one paints, a second axis setting how tall the lit cells stand, and optional divisions breaking each cell into dashes on a grid shared right along the bank. |
| `arc` | A ring filled between two angles, banded by the scale and optionally cut into countable segments with one of them marked. Every round gauge starts here. |
| `needle` | A pointer swinging over a sweep, with a hub over the pivot. |
| `scale` | Tick marks around a dial's rim, with major divisions longer and thicker. |
| `ruler` | The same marks along a straight edge, for graduating a bar. |
| `roundrect` | A panel with rounded corners. |
| `led` | A telltale lamp: on, off, or dim, with an unlit colour so the symbol still reads. |
| `line` | A polyline through listed points, given as fractions of the box so a curve survives a change of screen, optionally ramping colour along its length. |
| `chart` | A series across the box, as a trace or a filled area, optionally tracking a reading as it arrives and colouring by value and by age. |
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

## Touch

Any widget can be something a finger presses. A `touch` block says what
pressing it asks for, and sits beside the geometry:

```json
{ "type": "roundrect", "rect": [248, 366, 64, 48], "radius": 8,
  "touch": { "action": "menu", "menu": "display", "button": "centre" } }
```

| `action` | | |
| --- | --- | --- |
| `page` | `pager`, `to` | Turn a pager. `pager` names one; left out, it is the nearest one around this widget. `to` is `"next"`, `"prev"`, a page's name, or an index. |
| `menu` | `menu`, `button` | Press `up`/`down`/`left`/`right`/`centre` on one of the rig's menus. |
| `mode` | `mode` | Ask the bus to put every display into a drive mode, by the name the rig lists it under. |
| `emit` | `name` | Hand a name to the host, which decides what it means. |

Only `page` happens inside the toolkit. The rest name something it does not
own — the menus live on the rig, the mode lives on the bus — so `Scene::touch`
returns them as a `scene::Event` and the host acts. A hit walks up from the
topmost widget under the finger, so a label inside a button still presses the
button.

The host turns its panel into gestures with `input::Recognizer`, which takes
`Touch` samples and gives back a `Tap`, a `Swipe`, a `Press` or a `Release`.
The simulator feeds it the mouse: click to tap, drag to swipe.

## Pages

A `pager`'s children are pages and only one is visible, so a page turn costs
the damage a change of visibility costs and the compositor, the hit test and
the transitions all learn nothing:

```json
{ "type": "pager", "rect": [0, 0, 480, 480], "name": "pages",
  "axis": "horizontal", "page": 0, "wrap": false, "swipe": true,
  "children": [
    { "type": "panel", "rect": [0, 0, 480, 480], "name": "drive", "children": [] },
    { "type": "panel", "rect": [0, 0, 480, 480], "name": "trip",  "children": [] }
  ] }
```

`axis` says which swipes turn it; dragging the page away brings the next one
on, so left and up advance. `wrap` is off by default, because a driver who has
reached the end of four screens is better told so by nothing happening than by
landing back at the start without knowing it. `swipe` off leaves the pager to
its buttons. A `page` past the last one opens on the first, since a blank
screen is the one thing a cluster must never do by accident.

This is not a mode change. The bus is not told, no scene file is swapped, and
the other displays carry on with what they were showing — it is one screen
choosing which of its own faces to put up. A mode change is the rig's job and
is still `"transition"` in the `.rig` file. Pagers nest, and the innermost one
under the finger answers first. `examples/touch.scene` is four pages -- the
engine, the trip, the GPS and the display's own settings -- with a menu driven
by buttons and a row of tell-tales over all of them. Open it with
`examples/touch.rig`, which owns the menus and the pins; both are in the
editor under File > Examples.

## Tell-tales

The bus spec packs sixty-four dashboard lamps into two bitfield gauges. A
widget binds to one by name and never has to know which bank or which bit:

```json
{ "type": "led", "rect": [186, 44, 24, 24], "color": "#f85149",
  "level": 0.0, "glow": 0.12,
  "bind": { "indicator": "OIL_PRESSURE" } }
{ "type": "image", "rect": [186, 44, 24, 24], "image": 0, "visible": false,
  "bind": { "indicator": "LOW_FUEL", "property": "visible" } }
```

`level` lights the lamp from `off` towards `color`. `off` defaults to `color`
scaled by `glow`, which is what a scene naming only a `glow` has always drawn;
naming it outright is how a lamp gets an unlit colour that is not simply a
darker version of its lit one -- a grey lens over an amber bulb is *paler* when
it is out, which no multiplier can reach.

Lit is 1 and out is 0, so no range is needed. The binding subscribes to the
bank as an ordinary gauge — `Scene::wanted` needs to know nothing about lamps
— and an unset bank leaves the widget as authored, because "the server has not
said" is not "every lamp is out". A name the spec does not have, or naming
both an `indicator` and a `gauge`, stops the scene from building. The names
are `cogware_can::indicators::ALL_INDICATORS`.

`"property": "visible"` is the second form and the one a real cluster uses: a
printed symbol appears rather than a rectangle brightening. It works on any
widget, bound to a lamp or to a plain gauge, and anything at or past halfway
shows. On a panel it hides everything inside, which is how a warning banner
with its own icon and text is authored. Do not put it on a pager's page — the
pager owns that flag and wins the moment the page turns.

## What an unlit cell paints

A lit cell is cut twice: `profile` gives it the envelope printed on the lens,
and `height` says how far up that envelope the reading stands. That leaves two
parts of the bank undrawn -- the cells past the reading, and the part of a lit
cell *above* where its reading stands -- and `track_shape` fills both.

| `track_shape` | A cell is painted unlit up to |
|---|---|
| `lit` | Exactly what it would be if it were lit: envelope and height both. The default, and what every scene drew before this existed. |
| `envelope` | The whole envelope `profile` cuts, whatever `height` says -- the shape the bank fills *into*. |
| `box` | The whole cell, ignoring both. The row of empty boxes that fills up. |

Both edges land on a dash boundary when `divisions` cuts the cells into one.
A dash is lit or it is not: half of one is a segment nothing on the instrument
can show, and a row of them reads as a mask laid over a smooth bar rather than
as a bank of segments lighting up. A dash more than half covered lights, so the
bank reads where the reading is rather than a dash short of it.

The second part is the one that catches people. A bank with no `height` has no
remainder -- a cell is lit or it is not, which is why a plain bargraph never
needed this -- but give it a second axis and every lit column stops part way
up. Under `lit` nothing is drawn above it and the column reads as a short
column rather than a full one part lit, so the bank looks like it is drawing
cells as the reading reaches them. `envelope` draws the curve the reading is
filling into; `box` draws the whole cell.

## Colouring a scale in bands

`bar`, `arc` and `segbar` take a `zones` list. Each band says where it starts,
and it runs until the next one begins:

```jsonc
"zones": [
  { "at": 0.00, "color": "#8a2a26", "cursor": "#ffb4ae" },  // danger, low
  { "at": 0.20, "color": "#7a5a12", "cursor": "#ffd97a" },  // caution, low
  { "at": 0.35, "color": "#1f6f34", "cursor": "#a8f5c0" },  // normal
  { "at": 0.82, "color": "#7a5a12", "cursor": "#ffd97a" },  // caution, high
  { "at": 0.93, "color": "#8a2a26", "cursor": "#ffb4ae" },  // danger, high
]
```

Two thresholds could not say that. An oil pressure gauge is dangerous when it
is *low* and dangerous again when it is high, with a caution band either side
of normal, and that is five bands and not two.

A band colours the **reading**, not the track: where a gauge has got to is what
the colour is about, and an unlit segment sitting in the redline is still
unlit. Bands are held in ascending order whatever order they are written in, so
a list that reads out of order still draws what it says.

A band's `cursor` is what the marker looks like inside it. Without one the
marker takes the band's own colour, which is a marker nobody can see -- and the
redline is exactly where knowing the reading matters most. With one, both
survive: the band still says the reading is in the redline and the marker still
says where in it. Usually the same hue a shade or two brighter, which is what
the editor's **mark** box fills in when it is ticked.

`warn`, `warn_fill`, `danger` and `danger_fill` are the two-band shorthand and
still mean exactly what they did, so no scene written before this needs an
edit. A threshold above 1.0 switches its band off, which is the default -- a
gauge that never asked for a redline does not get one.

Zones are a list, so like a line's `points` they are edited in the text pane
rather than the properties pane.

## A line that moves across the bank

`segbar` takes a `cursor`: the colour of the cell the reading is standing on.
Without one the bar is lit evenly, which is what every scene drew before it
existed.

```json
{ "type": "segbar", "rect": [60, 100, 1240, 320], "segments": 40,
  "fill": "#1f6f34", "cursor": "#a8f5c0", "lights": "all",
  "track": "#1d2b22", "track_shape": "envelope",
  "bind": { "gauge": "RPM" } }
```

`cursor_size` widens it, counting cells back from the reading so the mark's
leading edge stays put however wide it is made. 0 and 1 both mean the single
cell the reading stands on, and a mark wider than the reading stops at the
first cell rather than running off the end.

It takes the cell from `fill` and from the ramp, but not from `warn` and
`danger`: a redline a marker could cover would be a redline that disappears at
exactly the revs it is there for.

## A chart that follows a reading

`history` turns a chart from a picture into a window. Past zero a reading
pushes a sample onto the end and the oldest falls off, so a bound gauge draws
its own recent past:

```json
{ "type": "chart", "rect": [20, 20, 640, 110], "history": 120,
  "width": 3, "stroke": "#3fb950", "fill": "#12301c",
  "warn": 0.6, "warn_stroke": "#d29922", "warn_fill": "#3a2f10",
  "danger": 0.85, "danger_stroke": "#f85149", "danger_fill": "#3d1512",
  "fade_to": "#05070a", "newest": "right",
  "bind": { "gauge": "BOOST" } }
```

`history: 0` is the listed series every scene drew before this existed, and a
chart with one takes no binding at all -- a picture somebody typed is not a
window on a gauge, and a binding must not quietly start rewriting it.

The same reading twice is two samples, not nothing happening: a gauge sitting
still is exactly when a trace has to keep drawing a flat line. The window is
reserved when the scene builds, so pushing never allocates once it has settled.

### Two gradients

`warn` and `danger` colour by what a reading **means**, and the line and the
area band separately: `warn_fill` and `danger_fill` default to `fill`, so a
band changes the trace and leaves the area alone. Giving both the same colour
turns a dim area bright at the threshold, which reads as the chart changing
rather than the reading crossing.

`fade_to` colours by **how old** a sample is, ramping the oldest end towards
whatever is named -- the panel behind the chart, usually. A colour rather than
an alpha, for the reason in *A trap worth knowing*: these surfaces write
colours instead of blending them, so a trace at low alpha lands at full alpha
and a fade has to say what it is fading into.

`newest` says which end new samples arrive at, `right` or `left`, and the
history runs away from it.

### A dial cut into segments

`arc` takes the same three ideas. `segments` cuts the ring into that many
pieces with `gap` degrees of dark between them -- degrees, not pixels, because
the axis is an angle and a gap measured along the rim would be wider at the
outside of a thick ring than the inside. `cursor` and `lights` then mean
exactly what they do on a bank.

```json
{ "type": "arc", "rect": [8, 8, 200, 200], "value": 0.62, "thickness": 26,
  "segments": 28, "gap": 3, "lights": "all",
  "fill": "#1f6f34", "cursor": "#a8f5c0", "track": "#1d2b22" }
```

`segments: 0` is the smooth ring every scene drew before this existed, and is
the default. A sweep whose gaps would eat all of it draws nothing rather than
wrapping round.

A cursor works on a smooth ring as well as a cut one: with nothing to light it
becomes a band sitting at the reading, at the leading edge of the fill, which
is where a bank's marked cell is too. `cursor_size` is in degrees here, for the
reason `gap` is, and 0 sizes the mark for the dial -- one segment on a cut
ring, and on a smooth one a band as wide as the ring is thick, which is the
mark that looks square at whatever radius the scene gives it. A mark that would
reach back past the start of the sweep is clipped to it rather than wrapping
round to the far end.

### Which cells the reading lights

A bargraph fills to its reading, and that is `lights: "bar"`, the default. A
bank showing two readings at once does not. Think of it as a block of cells
with revs across and boost up: boost lights every column, and the revs only say
which one of them you are at.

| `lights` | The reading lights |
|---|---|
| `bar` | The cells up to it, none past it. The default, and the fuel gauge every scene already has. |
| `all` | Every cell, with the reading only saying which one is marked. |
| `one` | Only the cell at the reading: the bare moving line, nothing lit behind it. |

With `all`, `height` is the height of the whole bank and `value` is a position
along it, which is what makes the two readings independent. Set the body dim
and the cursor bright and the block reads the way the instrument does: a glance
finds the line, a longer look reads the shape behind it.

## Ramping a colour

`line` takes a `color_to` and `segbar` a `fill_to`. Each defaults to the colour
beside it, which is a flat stroke or a flat bar and what every scene written
before these existed draws, so neither costs an existing file an edit.

```json
{ "type": "line", "rect": [60, 100, 1240, 320], "width": 4,
  "color": "#3fb950", "color_to": "#f85149", "points": [] }
{ "type": "segbar", "rect": [60, 100, 1240, 320], "segments": 40,
  "fill": "#3fb950", "fill_to": "#d29922", "danger": 0.9 }
```

A stroke's ramp is measured along the path's length, so an uneven curve changes
colour at an even rate rather than racing through its short segments. It steps
once per segment rather than per pixel: a curve with enough points to look like
a curve has enough to look like a smooth ramp, and a four-point line shows three
bands.

A bank's ramp is measured across the whole bank rather than across the lit part
of it, so a cell keeps its colour as the reading passes it. `warn` and `danger`
still win where they apply, which is how a gauge ramps green to amber and keeps
a hard redline.

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
`sevenseg`'s unlit segments and `led`'s `off` colour are both specified the
long way round. Antialiasing is the one exception, and it blends by reading the
surface back rather than by asking it to.

## Deliberately absent

`table`, `list`, `tabview`, `msgbox`, `calendar`, `spinbox` — all of them are
either interactive or assume a scrolling viewport, and a fixed instrument panel
has neither. A dashboard that needs a table has outgrown this toolkit.

`canvas` — an escape hatch for drawing arbitrary pixels from a scene file. The
scene format cannot express a program, and adding one would make a scene
something you have to audit rather than read.
