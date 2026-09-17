//! The cockpit's own rig must parse and describe that board's loom.
//!
//! It reads a file from a sibling checkout, so it skips when there is none:
//! this crate has to build and test on its own, and a rig it does not own is
//! not a reason to fail.
use copilot::gpio::{Dir, Level, Pull, What};

#[test]
fn the_cockpit_rig_parses_and_maps_the_cluster_loom() {
    let path = std::env::var("COCKPIT_RIG").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../cogware-cockpit/rig/z31.rig").to_string()
    });
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("no cockpit rig at {path}; skipping");
        return;
    };
    let rig = copilot::rig::parse(&text).expect("z31.rig parses");
    let cluster = rig
        .displays
        .iter()
        .find(|d| d.node == 0x01)
        .expect("a display at node 0x01");

    assert_eq!(cluster.pins.count(), 19, "the board has nineteen pins");

    // The lamps this loom drives, and the feeds it reads.
    for (pin, name, dir) in [
        (1u8, "TURN_LEFT", Dir::Out),
        (2, "HIGH_BEAM", Dir::Out),
        (3, "BATTERY", Dir::Out),
        (4, "CHECK_ENGINE", Dir::Out),
        (5, "OIL_PRESSURE", Dir::Out),
        (6, "PARK_BRAKE", Dir::Out),
        (10, "TURN_RIGHT", Dir::Out),
        (7, "LOW_BEAM", Dir::In),
        // Crossed on purpose: the stalk's wires land the other way round from
        // the pin numbering, and the lamps are the ones that are correct.
        (8, "TURN_RIGHT", Dir::In),
        (9, "TURN_LEFT", Dir::In),
    ] {
        let p = cluster.pins.get(pin).unwrap_or_else(|| panic!("pin {pin} mapped"));
        assert_eq!(p.dir, dir, "pin {pin} direction");
        match &p.what {
            What::Lamp(ind) => assert_eq!(ind.name(), name, "pin {pin} lamp"),
            other => panic!("pin {pin} is {other:?}, not a lamp"),
        }
    }

    // Every feed is switched to ground, so it reads low when on and wants the
    // board to hold it high when nothing is pulling it.
    for pin in [7u8, 8, 9] {
        let p = cluster.pins.get(pin).unwrap();
        assert_eq!(p.active, Level::Low, "pin {pin} active level");
        assert_eq!(cluster.pins.pull(pin), Pull::Up, "pin {pin} pull");
    }

    // The spares are inputs driving nothing, which is what an unwired pin on
    // a header has to be.
    for pin in 11..=19u8 {
        assert!(cluster.pins.get(pin).is_none(), "pin {pin} is spare");
        assert_eq!(cluster.pins.dir(pin), Dir::In, "spare {pin} drives nothing");
    }
}

/// A pin driving a lamp has to be subscribed to, or the bulb never lights.
#[test]
fn a_loom_that_drives_lamps_asks_for_the_indicator_banks() {
    use copilot::gpio::Pins;

    let path = std::env::var("COCKPIT_RIG").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../cogware-cockpit/rig/z31.rig").to_string()
    });
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("no cockpit rig at {path}; skipping");
        return;
    };
    let rig = copilot::rig::parse(&text).expect("z31.rig parses");
    let cluster = rig.displays.iter().find(|d| d.node == 0x01).unwrap();

    let wanted = cluster.pins.wanted();
    assert!(
        wanted.contains(&(cogware_can::INDICATORS1.id as u8)),
        "the cluster drives lamps, so it must ask for bank 1"
    );
    assert!(wanted.contains(&(cogware_can::INDICATORS2.id as u8)));

    // A display with no pins asks for nothing extra: subscribing to banks it
    // never reads would be bus traffic nobody wants.
    assert!(Pins::none().wanted().is_empty());
}

/// The bulb check belongs to the board, so the rig has to carry it.
#[test]
fn the_cluster_sweeps_its_own_bulbs_left_to_right_at_boot() {
    let path = std::env::var("COCKPIT_RIG").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../cogware-cockpit/rig/z31.rig").to_string()
    });
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("no cockpit rig at {path}; skipping");
        return;
    };
    let rig = copilot::rig::parse(&text).expect("z31.rig parses");
    let pins = &rig.displays.iter().find(|d| d.node == 0x01).unwrap().pins;

    // Physical order across the binnacle, which is what somebody watching
    // sees: the two turn lamps are the ends, whatever pin numbers they have.
    assert_eq!(pins.sweep_pins(), [1, 2, 3, 4, 5, 6, 10]);

    // One lamp at a time, each for its step, and nothing lit once it is over.
    let step = pins.sweep_us() / pins.sweep_pins().len() as u64;
    let start = 7_000; // a boot happens whenever it happens, not at zero
    for (i, pin) in pins.sweep_pins().iter().enumerate() {
        let word = pins
            .sweep(start + step * i as u64, start)
            .unwrap_or_else(|| panic!("step {i} still sweeping"));
        assert_eq!(word, 1 << (pin - 1), "step {i} lights pin {pin} alone");
    }
    assert!(
        pins.sweep(start + pins.sweep_us(), start).is_none(),
        "the sweep is over once every lamp has had its turn"
    );

    // A display that asks for no sweep never starts one.
    assert!(copilot::gpio::Pins::none().sweep(1_000, 0).is_none());
}
