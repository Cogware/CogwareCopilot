// SPDX-License-Identifier: GPL-3.0-only
//! What a display's spare pins are wired to.
//!
//! A screen in a dashboard usually has pins going spare, and the things a car
//! wants to put on them are the same few every time: the turn stalk, the high
//! beam feed, a button on the wheel, a relay for a warning bulb. This is where
//! a rig says which is which.
//!
//! ```jsonc
//! { "name": "cluster", "node": "0x01",
//!   "gpio": 8,                                         // eight pins, 1..8
//!   "pins": [
//!     { "pin": 1, "indicator": "TURN_LEFT" },
//!     { "pin": 3, "indicator": "HIGH_BEAM", "active": "low", "pull": "up" },
//!     { "pin": 5, "action": "menu", "menu": "display", "button": "centre" },
//!     { "pin": 6, "dir": "out", "indicator": "SHIFT_LIGHT" },
//!     { "pin": 7, "dir": "out", "gauge": "RPM", "above": 5000 },
//!   ],
//!   "scenes": { /* ... */ } }
//! ```
//!
//! # The pins are 1..n and nothing else
//!
//! A pin here is a number a person picked off a list, not a port and a bit.
//! Which physical pin logical 3 lands on is the board's, in the board's own
//! code, where the schematic is: a rig that named `GPIO27` would be a rig that
//! only fits one board, and the whole point of the file is that it fits the
//! car rather than the hardware under it.
//!
//! So the rig says how many there are and what each is for, and the firmware
//! reads its port into a word -- bit 0 is pin 1 -- and hands it to
//! [`Pins::feed`]. [`Pins::pull`] and [`Pins::dir`] are the endpoints a board
//! configures its port from at boot; nothing here drives a pin or knows how.
//!
//! # An input does not light its own lamp
//!
//! A pin wired to the left turn circuit does **not** write `TURN_LEFT` into
//! the gauge table. It reports the change, the firmware tells the master, the
//! master folds the bit into the real `INDICATORS1` frame, and the lamp lights
//! on the way back -- on this display and on every other one at the same time.
//!
//! The round trip is the point. A display that lit its own lamp locally would
//! be the one screen in the car that disagreed with the others, and the master
//! would no longer be the single authority on what the dash is showing. It
//! also costs nothing: the widget is already bound with
//! `"bind": { "indicator": "TURN_LEFT" }` and needs no change at all.
//!
//! ```ignore
//! // Once, after a boot or a bus drop: the whole loom in one frame, because
//! // the master has no idea what this display's pins are doing until told.
//! let loom: Vec<_> = display.pins.lamp_states().collect();
//! can.send_message(view.report_lamps(&loom)?).ok();
//!
//! // Per frame: only what moved.
//! for pin in display.pins.feed(port.read(), now_us).iter() {
//!     match display.pins.get(pin).map(Pin::what) {
//!         // The master owns the banks and will broadcast them back.
//!         Some(What::Lamp(ind)) => {
//!             can.send_message(view.report_lamp(*ind, display.pins.asserted(pin))?).ok();
//!         }
//!         // A hard button, which does exactly what a widget's touch does.
//!         Some(What::Press(action)) => { scene.run(ROOT, action); }
//!         Some(What::Threshold(_)) | None => {}
//!     }
//! }
//! port.write(display.pins.outputs());   // logical lamp and threshold commands
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use cogware_can::indicators::Indicator;

use crate::scene::Value;
use crate::scene::bind::indicator_by_name;
use crate::scene::touch::{self, Action};

mod threshold;
pub use threshold::{Comparison, Threshold};

#[cfg(test)]
mod threshold_tests;

/// The most pins a display can have, fixed by the word [`Pins::feed`] takes.
///
/// Thirty-two is far past what any panel in a dashboard has spare, and a word
/// is what a firmware reads a port into anyway.
pub const MAX_PINS: u8 = 32;

/// How long a pin must hold its new state before it counts as changed.
///
/// Twenty milliseconds: long enough for a relay to stop chattering and a
/// switch contact to settle, short enough that nobody feels it. It matters
/// more here than it would on a keypad, because every accepted change sends a
/// frame, and a bouncing turn-signal relay would otherwise put a burst of
/// them on the bus twice a second.
pub const DEBOUNCE_US: u64 = 20_000;

/// Which way a pin goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dir {
    /// The car drives the pin and the display reads it.
    #[default]
    In,
    /// The display drives the pin: a bulb, a buzzer, a relay.
    Out,
}

impl Dir {
    /// Parse the name a rig file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "in" => Some(Self::In),
            "out" => Some(Self::Out),
            _ => None,
        }
    }

    /// The name a rig file uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

/// Which electrical level means "yes".
///
/// Both are ordinary: a feed off an ignition switch is active high, and
/// anything switched to chassis ground -- which is most of a car -- is active
/// low. Getting it wrong shows every lamp lit with the engine off, so it is a
/// field rather than an assumption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// A high pin is asserted.
    #[default]
    High,
    /// A low pin is asserted.
    Low,
}

impl Level {
    /// Parse the name a rig file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "high" => Some(Self::High),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    /// The name a rig file uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }

    /// Whether a pin reading `high` is asserted at this level.
    #[must_use]
    pub const fn asserted(self, high: bool) -> bool {
        match self {
            Self::High => high,
            Self::Low => !high,
        }
    }
}

/// What the board should tie an input to when nothing is driving it.
///
/// Carried here and applied by the board, which is the only thing that knows
/// whether its pins have pulls at all. A floating input reads as noise, and an
/// active-low switch to ground needs a pull-up or it never reads high.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pull {
    /// Leave it floating. Right where something else already drives the line.
    #[default]
    None,
    /// Tie it high.
    Up,
    /// Tie it low.
    Down,
}

impl Pull {
    /// Parse the name a rig file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }

    /// The name a rig file uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// What a pin is for.
#[derive(Clone, Debug, PartialEq)]
pub enum What {
    /// A tell-tale. On an input, asserting the pin is a lamp the master should
    /// be told about; on an output, the display drives the pin from whatever
    /// the bus says that lamp is doing.
    Lamp(Indicator),
    /// A hard button, firing the same action a widget's `touch` fires.
    ///
    /// Inputs only: an action is something that happens, and there is nothing
    /// for an output pin to read from it.
    Press(Action),
    /// An output commanded by a numeric gauge in its canonical unit.
    Threshold(Threshold),
}

/// One pin, and what it is wired to.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// Which pin, 1..=[`Pins::count`].
    pub pin: u8,
    /// What a person calls the wire: "left stalk", "handbrake switch".
    ///
    /// For the editor's list and for whoever opens the loom in three years.
    /// Nothing reads it.
    pub label: Option<String>,
    /// Which way it goes.
    pub dir: Dir,
    /// Which electrical level means yes. Meaningless on an output, where the
    /// board inverts if its hardware needs it.
    pub active: Level,
    /// What the board should tie an input to. Meaningless on an output.
    pub pull: Pull,
    /// What it is wired to.
    pub what: What,
}

impl Pin {
    /// What it is wired to.
    #[must_use]
    pub const fn what(&self) -> &What {
        &self.what
    }

    /// The tell-tale it carries, if it carries one.
    #[must_use]
    pub const fn indicator(&self) -> Option<Indicator> {
        match self.what {
            What::Lamp(i) => Some(i),
            What::Press(_) | What::Threshold(_) => None,
        }
    }

    /// The action it fires, if it is a button.
    #[must_use]
    pub const fn action(&self) -> Option<&Action> {
        match &self.what {
            What::Press(a) => Some(a),
            What::Lamp(_) | What::Threshold(_) => None,
        }
    }

    /// Its bit in the word [`Pins::feed`] takes, or 0 for a pin out of range.
    #[must_use]
    pub const fn mask(&self) -> u32 {
        if self.pin == 0 || self.pin > MAX_PINS {
            0
        } else {
            1u32 << (self.pin - 1)
        }
    }
}

/// Which pins changed on the last [`Pins::feed`], as a mask over 1..=count.
///
/// A mask rather than a list of events, so that a display reading its port
/// sixty times a second allocates nothing to be told that nothing happened.
/// The caller walks it and asks [`Pins::get`] what each one means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed(u32);

impl Changed {
    /// Whether nothing changed, which is the answer almost every frame.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// How many pins changed.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether `pin` is among them.
    #[must_use]
    pub const fn contains(self, pin: u8) -> bool {
        pin >= 1 && pin <= MAX_PINS && self.0 & (1u32 << (pin - 1)) != 0
    }

    /// The pins that changed, lowest first.
    pub fn iter(self) -> impl Iterator<Item = u8> {
        (1..=MAX_PINS).filter(move |p| self.contains(*p))
    }
}

/// How long each lamp of a boot sweep is held, when a rig does not say.
///
/// Slow enough to watch one bulb at a time, fast enough that the whole run is
/// over before anybody wonders whether the dash is stuck.
pub const SWEEP_STEP_US: u64 = 120_000;

/// A display's pins: how many there are, and what each is wired to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins {
    count: u8,
    pins: Vec<Pin>,
    debounce_us: u64,
    /// The pins a boot sweep walks, in the order it walks them, and how long
    /// each is held. Empty for a display that asks for none.
    sweep: Vec<u8>,
    sweep_step_us: u64,
    /// The raw word last accepted, before any active level is applied.
    settled: u32,
    /// The raw word last seen, which may still be bouncing.
    pending: u32,
    /// When `pending` last differed from what came before it.
    since_us: u64,
    /// Whether a word has ever been fed, so the first one is a change from
    /// nothing rather than a change from an imagined all-low port.
    seen: bool,
}

impl Pins {
    /// A display with no pins, which is what a rig that says nothing gets.
    #[must_use]
    pub fn none() -> Self {
        Self { sweep_step_us: SWEEP_STEP_US, ..Self::default() }
    }

    /// How many pins this display has. They are numbered 1..=this.
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// Whether the display has no pins at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// How long a pin must hold a state before the change is accepted.
    #[must_use]
    pub const fn debounce_us(&self) -> u64 {
        self.debounce_us
    }

    /// The pins that are wired to something, in the order the rig lists them.
    ///
    /// Not every pin: a display with eight and three of them used has three
    /// entries here, and the other five are numbers nobody has assigned yet.
    #[must_use]
    pub fn mapped(&self) -> &[Pin] {
        &self.pins
    }

    /// What pin `n` is wired to, or `None` if nothing is.
    #[must_use]
    pub fn get(&self, n: u8) -> Option<&Pin> {
        self.pins.iter().find(|p| p.pin == n)
    }

    /// Which way pin `n` goes. An unmapped pin is an input, because an
    /// unconfigured output is a pin driving something nobody asked it to.
    #[must_use]
    pub fn dir(&self, n: u8) -> Dir {
        self.get(n).map_or(Dir::In, |p| p.dir)
    }

    /// What the board should tie pin `n` to before it reads it.
    #[must_use]
    pub fn pull(&self, n: u8) -> Pull {
        self.get(n).map_or(Pull::None, |p| p.pull)
    }

    /// Whether pin `n` is asserted, as of the last accepted word.
    ///
    /// Already through the pin's `active` level, so this is "the stalk is
    /// pushed", not "the pin is at 12 volts".
    #[must_use]
    pub fn asserted(&self, n: u8) -> bool {
        let Some(p) = self.get(n) else { return false };
        p.active.asserted(self.settled & p.mask() != 0)
    }

    /// Feed the port, and get back which pins changed.
    ///
    /// `bits` is the whole port as the firmware read it, bit 0 being pin 1 and
    /// every bit raw -- no `active` level applied, because that is this
    /// module's business and not the board's. `now_us` is the same monotonic
    /// microsecond count [`crate::anim::Animator::tick`] takes.
    ///
    /// A word has to hold still for [`Self::debounce_us`] before it is
    /// accepted, so a contact that chatters produces one change rather than a
    /// dozen. Every accepted change is one the firmware will put on the bus,
    /// which is exactly why that matters. A debounce of zero accepts
    /// immediately, for a board whose hardware has already done the work.
    ///
    /// Output pins are ignored: the display drives those, and reading one back
    /// and calling it a change would report the display's own doing as if the
    /// car had done it.
    pub fn feed(&mut self, bits: u32, now_us: u64) -> Changed {
        let bits = bits & self.inputs_mask();
        if !self.seen {
            // The first word is the port as found, not a change from an
            // imagined all-low one: a car parked with its lights on must not
            // look like somebody just turned them on.
            self.seen = true;
            self.settled = bits;
            self.pending = bits;
            self.since_us = now_us;
            return Changed::default();
        }
        if bits != self.pending {
            self.pending = bits;
            self.since_us = now_us;
            return Changed::default();
        }
        if bits == self.settled {
            return Changed::default();
        }
        // `saturating_sub` so a clock that wrapped backwards accepts the
        // change rather than holding the dash stale until it catches up.
        if now_us.saturating_sub(self.since_us) < self.debounce_us {
            return Changed::default();
        }
        let changed = self.settled ^ bits;
        self.settled = bits;
        Changed(changed)
    }

    /// The boot sweep as a word to drive the port with, or `None` once it is
    /// over or if the rig asked for none.
    ///
    /// A wave over this display's own pins, driven by this display, before and
    /// regardless of anything on the bus. It has to be local: the wave is a
    /// check of these bulbs, and a master running it centrally would be
    /// flashing lamps at a node that has not subscribed yet -- or at one that
    /// is not powered, which is exactly when somebody is watching.
    ///
    /// `now_us` is the same monotonic microsecond count [`Self::feed`] takes.
    /// Returns the word for the lamp whose turn it is, so the board writes it
    /// the same way it writes [`Self::outputs`].
    #[must_use]
    pub fn sweep(&self, now_us: u64, started_us: u64) -> Option<u32> {
        if self.sweep.is_empty() || self.sweep_step_us == 0 {
            return None;
        }
        let step = now_us.saturating_sub(started_us) / self.sweep_step_us;
        let at = usize::try_from(step).ok()?;
        // One past the end is the step that puts the last lamp out; anything
        // beyond that is the sweep being over.
        let pin = self.sweep.get(at)?;
        Some(1 << (pin - 1))
    }

    /// How long a full boot sweep takes, so a board knows when it is done
    /// without having to reason about the step count itself.
    #[must_use]
    pub fn sweep_us(&self) -> u64 {
        self.sweep_step_us * self.sweep.len() as u64
    }

    /// The pins the boot sweep walks, in order.
    #[must_use]
    pub fn sweep_pins(&self) -> &[u8] {
        &self.sweep
    }

    /// The gauges a node needs to subscribe to for these pins to work.
    ///
    /// An output reads its lamp out of the indicator banks, and a node only
    /// receives what it asked for -- so without this a relay wired to a real
    /// bulb stays dark however the scene is drawn. The banks are ordinary
    /// gauge rows, so this is the same list a scene's bindings produce and
    /// belongs beside them in whatever the node subscribes with.
    /// Threshold sources are included once each even when no scene binds them.
    /// This list is allocated at subscription setup, not during frame evaluation.
    ///
    /// Inputs are not here. A pin reporting a stalk does not read the banks;
    /// its lamp comes back because the master broadcasts it, and whether this
    /// display draws it is the scene's business.
    #[must_use]
    pub fn wanted(&self) -> Vec<u8> {
        let drives_a_lamp = self
            .pins
            .iter()
            .any(|p| p.dir == Dir::Out && matches!(p.what, What::Lamp(_)));
        let mut wanted = if drives_a_lamp {
            alloc::vec![
                cogware_can::INDICATORS1.id as u8,
                cogware_can::INDICATORS2.id as u8,
            ]
        } else {
            Vec::new()
        };
        for p in &self.pins {
            if p.dir == Dir::Out
                && let What::Threshold(threshold) = &p.what
                && let Ok(id) = u8::try_from(threshold.gauge.id)
                && !wanted.contains(&id)
            {
                wanted.push(id);
            }
        }
        wanted
    }

    /// What the output pins should be driven to, as a word shaped like the one
    /// [`Self::feed`] takes.
    ///
    /// Read from the gauge table, so an output follows whatever the master is
    /// broadcasting -- including a bulb check, which lights a real bulb on a
    /// relay exactly as it lights a drawn one. A pin whose bank the bus has
    /// never written reads as off; a threshold with no finite reading is also
    /// off and otherwise compares in the gauge's canonical unit.
    ///
    /// These are logical commands: `active` is not applied and polarity belongs
    /// to the board, which is where a driver transistor that inverts lives.
    #[must_use]
    pub fn outputs(&self) -> u32 {
        let mut out = 0;
        for p in &self.pins {
            if p.dir != Dir::Out {
                continue;
            }
            let on = match &p.what {
                What::Lamp(ind) => cogware_can::indicators::get(*ind),
                What::Threshold(threshold) => threshold
                    .gauge
                    .as_f32()
                    .is_some_and(|reading| threshold.matches(reading)),
                What::Press(_) => false,
            };
            if on {
                out |= p.mask();
            }
        }
        out
    }

    /// Forget the port, so the next word read is the port as found.
    ///
    /// For a display coming back from a sleep or a bus drop, where the first
    /// thing read is the state of the car and not news about it.
    pub fn reset(&mut self) {
        self.seen = false;
        self.settled = 0;
        self.pending = 0;
    }

    /// The tell-tales this display can report, for a firmware building its
    /// first message to the master.
    pub fn reported(&self) -> impl Iterator<Item = Indicator> + '_ {
        self.pins
            .iter()
            .filter(|p| p.dir == Dir::In)
            .filter_map(Pin::indicator)
    }

    /// Every input tell-tale and the state it is in, as
    /// [`cogware_can::bus::BusView::report_lamps`] takes them.
    ///
    /// The whole loom in one go, which is what a node hands over after a boot
    /// or a bus drop: the master has no idea what this display's pins are
    /// doing until it is told, and a dash that arrives in two halves is a dash
    /// that is briefly wrong. Seven fit one frame, which is more pins than a
    /// screen has spare.
    ///
    /// The ordinary path is [`Self::feed`] and the pins it says changed. This
    /// is for the moment there is no "changed" to speak of.
    pub fn lamp_states(&self) -> impl Iterator<Item = (Indicator, bool)> + '_ {
        self.pins
            .iter()
            .filter(|p| p.dir == Dir::In)
            .filter_map(|p| {
                p.indicator()
                    .map(|i| (i, p.active.asserted(self.settled & p.mask() != 0)))
            })
    }

    /// The bits of the port that are inputs.
    fn inputs_mask(&self) -> u32 {
        let all = if self.count >= MAX_PINS {
            u32::MAX
        } else {
            (1u32 << self.count) - 1
        };
        let outputs: u32 = self
            .pins
            .iter()
            .filter(|p| p.dir == Dir::Out)
            .map(Pin::mask)
            .fold(0, |a, b| a | b);
        all & !outputs
    }
}

/// Why a display's pins could not be used.
///
/// Not `Eq`: it carries a [`crate::scene::BuildError`], which is not.
#[derive(Clone, Debug, PartialEq)]
pub enum PinError {
    /// `gpio` was not a count, or was over [`MAX_PINS`].
    BadCount,
    /// `pins` was not an array, or an entry was not an object.
    BadList,
    /// A pin entry had no `pin` number, or one outside 1..=`gpio`.
    BadPin(i64),
    /// Two entries claimed one pin.
    Duplicate(u8),
    /// A pin field had an invalid name, type or non-finite numeric value.
    BadField {
        /// The field that was unusable.
        field: &'static str,
    },
    /// A pin did not name exactly one of `indicator`, `action` and `gauge`.
    NotWired(u8),
    /// `indicator` named a tell-tale the bus spec does not have.
    UnknownIndicator(String),
    /// `gauge` named a reading the bus spec does not have.
    UnknownGauge(String),
    /// An indicator bank was used as a numeric reading.
    NotNumericGauge(String),
    /// A gauge's CAN ID does not fit the subscription protocol.
    BadGaugeId(u16),
    /// A comparison was supplied without a `gauge`.
    MissingGauge(u8),
    /// A gauge did not have exactly one comparison field.
    BadComparison(u8),
    /// An input named a gauge, which can only command an output.
    InputGauge(u8),
    /// An `action` could not be read; the scene builder says why.
    BadAction(crate::scene::BuildError),
    /// An output named an `action`, which has nothing for a pin to read.
    OutputAction(u8),
    /// A pin's `page` action named no pager.
    ///
    /// A widget's may leave it out and mean the nearest one around itself; a
    /// pin is not anywhere in the tree and has nothing to look up from.
    NoPager(u8),
}

/// Read a display's `gpio` count and `pins` list.
pub(crate) fn pins(val: &Value) -> Result<Pins, PinError> {
    let count = match val.get("gpio") {
        None | Some(Value::Null) => 0,
        Some(v) => {
            let n = v.as_i64().ok_or(PinError::BadCount)?;
            u8::try_from(n)
                .ok()
                .filter(|n| *n <= MAX_PINS)
                .ok_or(PinError::BadCount)?
        }
    };

    let debounce_us = match val.get("debounce_us") {
        None | Some(Value::Null) => DEBOUNCE_US,
        Some(v) => u64::try_from(v.as_i64().ok_or(PinError::BadField {
            field: "debounce_us",
        })?)
        .map_err(|_| PinError::BadField {
            field: "debounce_us",
        })?,
    };

    let mut pins: Vec<Pin> = Vec::new();
    if let Some(list) = val.get("pins") {
        let entries = list.as_array().ok_or(PinError::BadList)?;
        for entry in entries {
            let p = pin(entry, count)?;
            if pins.iter().any(|x| x.pin == p.pin) {
                return Err(PinError::Duplicate(p.pin));
            }
            pins.push(p);
        }
    }

    let sweep = sweep(val, count)?;

    Ok(Pins {
        count,
        pins,
        debounce_us,
        sweep,
        sweep_step_us: SWEEP_STEP_US,
        settled: 0,
        pending: 0,
        since_us: 0,
        seen: false,
    })
}

/// Read the optional `boot_sweep` block: which pins it walks and how fast.
///
/// The order is the file's, not the pin numbering's, because it is a wave
/// somebody watches: what matters is the bulbs lighting in the order they sit
/// in the binnacle, whatever pins they happen to land on.
///
/// Just the pins. How long each is held is [`SWEEP_STEP_US`] and not the
/// rig's to set: nobody asked to tune it, and the key that carried it put a
/// short string in this function that LLVM's Xtensa backend could not select,
/// which stopped the ESP32 nodes building at all.
fn sweep(val: &Value, count: u8) -> Result<Vec<u8>, PinError> {
    let Some(block) = val.get("boot_sweep") else {
        return Ok(Vec::new());
    };
    if matches!(block, Value::Null) {
        return Ok(Vec::new());
    }
    let mut order = Vec::new();
    for entry in block.as_array().ok_or(PinError::BadList)? {
        let n = entry.as_i64().unwrap_or(-1);
        let pin = u8::try_from(n)
            .ok()
            .filter(|p| *p >= 1 && *p <= count)
            .ok_or(PinError::BadPin(n))?;
        order.push(pin);
    }
    Ok(order)
}

/// Read one `pins` entry against the display's pin count.
fn pin(val: &Value, count: u8) -> Result<Pin, PinError> {
    let n = val.get("pin").and_then(Value::as_i64).unwrap_or(-1);
    let pin = u8::try_from(n)
        .ok()
        .filter(|p| *p >= 1 && *p <= count)
        .ok_or(PinError::BadPin(n))?;

    let word = |field: &'static str| -> Result<Option<&str>, PinError> {
        match val.get(field) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(v.as_str().ok_or(PinError::BadField { field })?)),
        }
    };

    let dir = match word("dir")? {
        None => Dir::In,
        Some(s) => Dir::parse(s).ok_or(PinError::BadField { field: "dir" })?,
    };
    let active = match word("active")? {
        None => Level::High,
        Some(s) => Level::parse(s).ok_or(PinError::BadField { field: "active" })?,
    };
    let pull = match word("pull")? {
        None => Pull::None,
        Some(s) => Pull::parse(s).ok_or(PinError::BadField { field: "pull" })?,
    };
    let label = word("label")?.map(ToString::to_string);

    let what = if let Some(threshold) = threshold::parse(val, pin, dir)? {
        What::Threshold(threshold)
    } else {
        match (word("indicator")?, val.get("action")) {
            // Naming both is a file contradicting itself about what the wire does.
            (Some(_), Some(_)) | (None, None) => return Err(PinError::NotWired(pin)),
            (Some(name), None) => What::Lamp(
                indicator_by_name(name)
                    .ok_or_else(|| PinError::UnknownIndicator(name.to_string()))?,
            ),
            (None, Some(_)) => {
                if dir == Dir::Out {
                    return Err(PinError::OutputAction(pin));
                }
                let action = touch::action(val).map_err(PinError::BadAction)?;
                // A widget's `page` action may leave the pager out and mean the
                // nearest one enclosing it. A pin is not in the tree at all, so
                // there is nothing to look up from and the rig has to say.
                if matches!(&action, Action::Page { pager: None, .. }) {
                    return Err(PinError::NoPager(pin));
                }
                What::Press(action)
            }
        }
    };

    Ok(Pin {
        pin,
        label,
        dir,
        active,
        pull,
        what,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parse;

    /// Read a display's pins from the fields of a `displays` entry.
    fn read(src: &str) -> Result<Pins, PinError> {
        pins(&parse(src).expect("must parse"))
    }

    fn ok(src: &str) -> Pins {
        read(src).expect("must build")
    }

    /// Eight pins: the stalks on 1 and 2, a button on 5, a relay on 6.
    fn loom() -> Pins {
        ok(r#"{ "gpio": 8, "pins": [
                 { "pin": 1, "indicator": "TURN_LEFT", "label": "left stalk" },
                 { "pin": 2, "indicator": "TURN_RIGHT" },
                 { "pin": 3, "indicator": "HIGH_BEAM", "active": "low", "pull": "up" },
                 { "pin": 5, "action": "emit", "name": "horn" },
                 { "pin": 6, "dir": "out", "indicator": "SHIFT_LIGHT" } ] }"#)
    }

    // --- the format ---

    #[test]
    fn a_display_that_says_nothing_has_no_pins() {
        let p = ok(r#"{ "name": "x" }"#);
        assert!(p.is_empty());
        assert_eq!(p.count(), 0);
        assert!(p.mapped().is_empty());
    }

    #[test]
    fn the_count_is_all_a_display_needs_to_declare() {
        // "just tell it how many": eight pins, none of them wired to anything
        // yet, and the numbers 1..8 are what the editor offers.
        let p = ok(r#"{ "gpio": 8 }"#);
        assert_eq!(p.count(), 8);
        assert!(p.mapped().is_empty());
        assert_eq!(p.dir(1), Dir::In, "an unassigned pin never drives anything");
        assert_eq!(p.pull(1), Pull::None);
    }

    #[test]
    fn a_pin_carries_everything_a_board_needs_to_set_its_port_up() {
        let p = loom();
        assert_eq!(p.count(), 8);
        assert_eq!(p.mapped().len(), 5);
        let third = p.get(3).expect("pin 3 is wired");
        assert_eq!(third.active, Level::Low);
        assert_eq!(third.pull, Pull::Up);
        assert_eq!(third.dir, Dir::In);
        assert_eq!(
            p.get(1).and_then(|x| x.label.as_deref()),
            Some("left stalk")
        );
    }

    #[test]
    fn a_pin_is_wired_to_a_lamp_or_to_a_button_and_not_to_both() {
        assert_eq!(
            read(r#"{ "gpio": 4, "pins": [{ "pin": 1 }] }"#).unwrap_err(),
            PinError::NotWired(1)
        );
        assert_eq!(
            read(
                r#"{ "gpio": 4, "pins": [
                     { "pin": 1, "indicator": "ABS", "action": "emit", "name": "x" }] }"#
            )
            .unwrap_err(),
            PinError::NotWired(1)
        );
    }

    #[test]
    fn a_pin_past_the_count_fails_on_the_desk() {
        // The commonest mistake there is: a loom drawn for twelve pins put on
        // a board that has eight.
        assert_eq!(
            read(r#"{ "gpio": 8, "pins": [{ "pin": 9, "indicator": "ABS" }] }"#).unwrap_err(),
            PinError::BadPin(9)
        );
        assert_eq!(
            read(r#"{ "gpio": 8, "pins": [{ "pin": 0, "indicator": "ABS" }] }"#).unwrap_err(),
            PinError::BadPin(0),
            "pins are 1..n, so there is no pin zero"
        );
    }

    #[test]
    fn two_entries_cannot_claim_one_pin() {
        assert_eq!(
            read(
                r#"{ "gpio": 8, "pins": [
                     { "pin": 2, "indicator": "ABS" },
                     { "pin": 2, "indicator": "BRAKE" }] }"#
            )
            .unwrap_err(),
            PinError::Duplicate(2)
        );
    }

    #[test]
    fn a_lamp_nobody_has_heard_of_fails_on_the_desk() {
        assert_eq!(
            read(r#"{ "gpio": 4, "pins": [{ "pin": 1, "indicator": "WARP_CORE" }] }"#).unwrap_err(),
            PinError::UnknownIndicator("WARP_CORE".into())
        );
    }

    #[test]
    fn a_level_or_a_pull_that_is_not_one_is_refused() {
        for (field, src) in [
            (
                "active",
                r#"{ "gpio": 4, "pins": [{"pin":1,"indicator":"ABS","active":"middling"}] }"#,
            ),
            (
                "pull",
                r#"{ "gpio": 4, "pins": [{"pin":1,"indicator":"ABS","pull":"sideways"}] }"#,
            ),
            (
                "dir",
                r#"{ "gpio": 4, "pins": [{"pin":1,"indicator":"ABS","dir":"both"}] }"#,
            ),
        ] {
            assert_eq!(
                read(src).unwrap_err(),
                PinError::BadField { field },
                "{field}"
            );
        }
    }

    #[test]
    fn more_pins_than_a_word_holds_is_refused() {
        assert_eq!(read(r#"{ "gpio": 33 }"#).unwrap_err(), PinError::BadCount);
        assert_eq!(ok(r#"{ "gpio": 32 }"#).count(), 32, "a full word is fine");
    }

    // --- buttons ---

    #[test]
    fn a_button_pin_fires_the_same_action_a_widget_does() {
        let p = ok(r#"{ "gpio": 4, "pins": [
                 { "pin": 1, "action": "menu", "menu": "display", "button": "centre" }] }"#);
        assert_eq!(
            p.get(1).and_then(Pin::action),
            Some(&Action::Menu {
                menu: "display".into(),
                button: crate::menu::Button::Centre
            })
        );
    }

    #[test]
    fn a_button_pin_turning_a_page_has_to_say_which_pager() {
        // A widget may leave it out and mean the nearest one around itself.
        // A pin is not in the tree, so there is nothing to look up from.
        assert_eq!(
            read(r#"{ "gpio": 4, "pins": [{ "pin": 1, "action": "page", "to": "next" }] }"#)
                .unwrap_err(),
            PinError::NoPager(1)
        );
        let p = ok(r#"{ "gpio": 4, "pins": [
                 { "pin": 1, "action": "page", "pager": "pages", "to": "next" }] }"#);
        assert!(p.get(1).and_then(Pin::action).is_some());
    }

    #[test]
    fn an_output_cannot_be_wired_to_an_action() {
        // An action is something that happens; there is nothing in one for a
        // pin to be driven from.
        assert_eq!(
            read(r#"{ "gpio": 4, "pins": [{ "pin": 1, "dir": "out", "action": "emit", "name": "x" }] }"#)
                .unwrap_err(),
            PinError::OutputAction(1)
        );
    }

    #[test]
    fn an_action_the_scene_format_refuses_is_refused_here_too() {
        assert!(matches!(
            read(r#"{ "gpio": 4, "pins": [{ "pin": 1, "action": "explode" }] }"#).unwrap_err(),
            PinError::BadAction(_)
        ));
    }

    // --- reading the port ---

    #[test]
    fn the_first_word_is_the_port_as_found_rather_than_news() {
        // A car parked with its lights on must not look like somebody just
        // turned them on the moment the display woke up.
        let mut p = loom();
        assert!(p.feed(0b0000_0001, 0).is_empty());
        assert!(p.asserted(1), "and the state is still known");
    }

    #[test]
    fn a_pin_that_settles_reports_once() {
        let mut p = loom();
        p.feed(0, 0);
        assert!(p.feed(0b01, 1_000).is_empty(), "still bouncing");
        let changed = p.feed(0b01, 1_000 + DEBOUNCE_US);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(1));
        assert!(p.asserted(1));
        assert!(
            p.feed(0b01, 9_000_000).is_empty(),
            "an unchanged port is not news every frame"
        );
    }

    #[test]
    fn a_chattering_contact_reports_once_rather_than_a_dozen_times() {
        // Every accepted change is a frame on the bus, which is why this
        // matters more here than it would on a keypad.
        let mut p = loom();
        p.feed(0, 0);
        let mut reported = 0;
        for i in 0..40u64 {
            // Bouncing every 2 ms, well inside the debounce window.
            let bits = u32::from(i % 2 == 0);
            reported += p.feed(bits, i * 2_000).len();
        }
        assert_eq!(reported, 0, "a bouncing contact was reported");
        // It settles, and now it counts.
        let t = 40 * 2_000;
        p.feed(0b01, t);
        assert_eq!(p.feed(0b01, t + DEBOUNCE_US).len(), 1);
    }

    #[test]
    fn two_pins_changing_together_come_back_together() {
        let mut p = loom();
        p.feed(0, 0);
        p.feed(0b011, 0);
        let changed = p.feed(0b011, DEBOUNCE_US);
        assert_eq!(changed.iter().collect::<Vec<_>>(), alloc::vec![1, 2]);
    }

    #[test]
    fn an_active_low_pin_is_asserted_when_it_reads_low() {
        // Anything switched to chassis ground, which is most of a car. Pin 3
        // is the high beam feed and is wired active low.
        let mut p = loom();
        p.feed(0b1111_1111, 0);
        assert!(!p.asserted(3), "the port is high, so the switch is open");

        p.feed(0b1111_1011, 0);
        assert!(
            p.feed(0b1111_1011, DEBOUNCE_US).contains(3),
            "pin 3 going low is a change"
        );
        assert!(p.asserted(3), "switched to ground, so the beam is on");
    }

    #[test]
    fn an_active_high_pin_reads_the_other_way_round() {
        // Same port, two conventions, which is the whole reason the field
        // exists: pin 1 is a feed off the stalk and pin 3 is a switch to
        // ground, and one word has to mean both.
        let mut p = loom();
        p.feed(0, 0);
        p.feed(0b0000_0001, 0);
        p.feed(0b0000_0001, DEBOUNCE_US);
        assert!(p.asserted(1), "pin 1 is active high and reads high");
        assert!(p.asserted(3), "pin 3 is active low and reads low");
    }

    #[test]
    fn a_debounce_of_zero_accepts_at_once() {
        // For a board whose hardware has already done the work.
        let mut p = ok(r#"{ "gpio": 4, "debounce_us": 0,
                 "pins": [{ "pin": 1, "indicator": "ABS" }] }"#);
        p.feed(0, 0);
        p.feed(0b01, 0);
        assert!(p.feed(0b01, 0).contains(1));
    }

    #[test]
    fn a_clock_that_jumps_backwards_neither_panics_nor_sticks() {
        // `saturating_sub`: a timer that wrapped must not underflow, and must
        // not leave the display unable to report anything ever again. It
        // waits the debounce out afresh from the new stamp, and then accepts.
        let mut p = loom();
        p.feed(0, 9_000_000);
        p.feed(0b01, 5);
        assert!(p.feed(0b01, 6).is_empty(), "a microsecond is not settled");
        assert!(p.feed(0b01, 5 + DEBOUNCE_US).contains(1), "held stale");
    }

    #[test]
    fn an_output_pin_is_never_read_back_as_an_input() {
        // Reading the display's own doing and calling it a change would have
        // it report to the master what the master just told it.
        let mut p = loom();
        p.feed(0, 0);
        p.feed(0b10_0000, 0);
        assert!(
            p.feed(0b10_0000, 9_000_000).is_empty(),
            "pin 6 is an output and answered as an input"
        );
    }

    #[test]
    fn bits_past_the_pin_count_are_ignored() {
        // A firmware handing over a whole port register must not make a
        // display with four pins report on the other twenty-eight.
        let mut p = ok(r#"{ "gpio": 4, "pins": [{ "pin": 1, "indicator": "ABS" }] }"#);
        p.feed(0, 0);
        p.feed(u32::MAX, 0);
        let changed = p.feed(u32::MAX, DEBOUNCE_US);
        assert_eq!(changed.iter().collect::<Vec<_>>(), alloc::vec![1, 2, 3, 4]);
    }

    #[test]
    fn forgetting_the_port_makes_the_next_word_the_port_as_found() {
        let mut p = loom();
        p.feed(0, 0);
        p.feed(0b01, 0);
        p.feed(0b01, DEBOUNCE_US);
        p.reset();
        assert!(p.feed(0, 9_000_000).is_empty(), "coming back reported news");
    }

    // --- driving a pin ---

    #[test]
    fn an_output_follows_what_the_bus_says_the_lamp_is_doing() {
        // Not what this display thinks: the master owns the banks, and an
        // output is a bulb showing the same thing the drawn lamp shows.
        let banks = crate::banks::Banks::take();
        let p = loom();
        banks.set(&[Indicator::ShiftLight]);
        assert_eq!(p.outputs(), 0b10_0000, "pin 6 should be driven");
        banks.set(&[]);
        assert_eq!(p.outputs(), 0);
    }

    #[test]
    fn an_input_wired_to_a_lamp_never_drives_a_pin() {
        // Every lamp this display is wired to, lit at once. Only the output
        // among them may move a pin; the three inputs are the car telling us,
        // and a display that drove those would be answering itself.
        let banks = crate::banks::Banks::take();
        let p = loom();
        banks.set(&[
            Indicator::TurnLeft,
            Indicator::TurnRight,
            Indicator::HighBeam,
            Indicator::ShiftLight,
        ]);
        assert_eq!(p.outputs(), 0b10_0000);
    }

    #[test]
    fn a_whole_loom_can_be_handed_over_at_once_with_its_states() {
        // What a node sends after a boot or a bus drop. Seven fit one frame;
        // this display's four inputs are well inside that.
        let mut p = loom();
        // Left stalk on, high beam on (it is active low, so its pin is low).
        p.feed(0b0000_0001, 0);
        let states: Vec<_> = p.lamp_states().collect();
        assert_eq!(
            states,
            alloc::vec![
                (Indicator::TurnLeft, true),
                (Indicator::TurnRight, false),
                (Indicator::HighBeam, true),
            ],
            "the states are the ones the active level says, not the raw port"
        );
        assert!(
            states.len() <= cogware_can::bus::LAMPS_PER_MSG,
            "a loom this size must fit one frame"
        );
    }

    #[test]
    fn the_lamps_a_display_can_report_are_its_inputs_and_not_its_outputs() {
        // What a firmware builds its first message to the master from.
        let p = loom();
        assert_eq!(
            p.reported().collect::<Vec<_>>(),
            alloc::vec![
                Indicator::TurnLeft,
                Indicator::TurnRight,
                Indicator::HighBeam
            ],
            "an output is something the master tells us, not the other way"
        );
    }
}
