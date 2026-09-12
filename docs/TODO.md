# To do

Wanted features, in the order they depend on each other. Each entry says what
it is for and the decision that has to be made before it can be built.

---

## 1. Menus — done

The rig owns menus and their values; a `menu` widget in a scene is a view onto
one. A host reads a value with `rig.menu("display").value("display brightness")`
and applies it wherever it belongs.

The split follows the one the rest of the crate already uses: `copilot` owns
the menu, its items, the selection and the current values; it owns no hardware
and applies nothing. A consuming crate asks for a value by name — the shape the
user asked for is `copilot::<crate>::display brightness` — and applies it
wherever it belongs, exactly as a display already reads `Scene::wanted` and
feeds gauge readings back in.

Still open:

- **Whether values persist**, and if so who writes them. The core crate cannot
  open a file, so persistence is the host's, and a menu has to be serialisable
  back out for it. Nothing writes a menu back to a rig file yet.
- **Whether a host wants a change list** rather than polling `value()` by name,
  the way damage works. Polling is enough for a handful of settings.

## 2. Menu rendering — done

Three colours: an ordinary row, the selected row (brighter, with a bar behind
it), and the row being edited. `examples/menu.scene` shows one.

## 3. Example menus in the rig — done

`display` and `trip` in `examples/z31.rig`, shown by `examples/menu.scene`.

## 4. Simulator button pad — done

`copilot-sim --rig <file> <scene>` opens a second window of five buttons,
driven by mouse or arrow keys. The core crate learns nothing about input
devices: the host pushes `menu::Button` in, as it already pushes time.

## 5. Scene and mode transition animations — done

`render::transition` draws the frame `t` of the way from one tree to another.
A rig names one with `"transition": "slide-left"` and `"transition_ms"`.

Slides and wipes turned out to need no second buffer: each is a clip and an
offset over two ordinary tree walks. Both trees are repainted in full, because
during a transition everything is moving and damage has nothing to save.

A cross-fade is the exception and does need a second full-size buffer, since
blending two scenes per pixel means having both at once. A board provides one
through `Surface::with_scratch` and claims `Caps::SCRATCH`; one that cannot
spare the memory implements nothing and a cross-fade cuts instead.

The simulator drives one: `--rig` gives it the rig, and TAB cycles the
display's modes, loading the next scene and running the transition over it.

## 6. Editor support

All of the above authored in `copilot-edit`: menu items added and reordered,
ranges and defaults set, transitions chosen per mode, and the menu previewed
with the same button pad as the simulator.

Comes last by definition — it can only edit what the format can express.
