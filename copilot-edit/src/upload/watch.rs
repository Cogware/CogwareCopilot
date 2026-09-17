// SPDX-License-Identifier: GPL-3.0-only
//! Watching the car: what it says about itself, live.
//!
//! The server has always sent this -- uptime, what it has put on the bus and
//! how fast, what a host has sent it, checksums it threw away, how many
//! gauges displays have asked for -- once a second, and to whatever link is
//! attached. Nothing read it but a terminal.
//!
//! It is the same frames as everything else, so this works over a cable, over
//! Bluetooth and over the car's own network without knowing which it is on.
//! That matters more than it sounds: the interesting faults are the ones that
//! only happen on one of the three, and a reading that can be taken the same
//! way on all of them is how they get told apart.

use std::time::{Duration, Instant};

use cogware_can::wire::{self, Readings, Sample, Status, KIND_PING, KIND_READINGS, KIND_STATUS};

use super::{Error, Transport};

/// How often to ask, when the server has gone quiet.
///
/// It sends on its own timer as well, so this is a nudge rather than a poll:
/// what it is really for is the moment just after connecting, when waiting a
/// whole second for the first reading would look like a link that does not
/// work.
const ASK_EVERY: Duration = Duration::from_millis(500);

/// How long without a word before the car is called silent.
const QUIET_FOR: Duration = Duration::from_secs(3);

/// What the car says about one gauge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// A raw count, in the gauge's own fixed point.
    Value(i32),
    /// Nothing is filling this channel.
    ///
    /// Either the car sent `NO_DATA` for it outright, or it did not mention
    /// the gauge at all -- which means the same thing, because the car
    /// reports what it is holding a live value for and this reader knows the
    /// whole table. An unplugged sensor, a sender that has stopped and an ECU
    /// that never had the channel all land here, and telling those three
    /// apart is a job for the rest of the panel, not for this.
    NoData,
}

/// A live view of the car.
pub struct Watch {
    latest: Option<Status>,
    at: Option<Instant>,
    /// The set being assembled, and where it has got to. A set is shown only
    /// once its last part has arrived; see `part`.
    pending: Vec<Sample>,
    building: bool,
    next_part: u8,
    /// Every gauge the car last reported, by identifier.
    ///
    /// Kept whole and replaced whole: a gauge that stops being sent has
    /// stopped being read, and leaving its last value on the screen would
    /// hide exactly the fault this is for.
    gauges: Vec<Sample>,
    gauges_at: Option<Instant>,
    asked: Instant,
    incoming: Vec<u8>,
    decoder: wire::Decoder,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    /// A view that has heard nothing yet.
    pub fn new() -> Self {
        Self {
            latest: None,
            at: None,
            gauges: Vec::new(),
            gauges_at: None,
            pending: Vec::new(),
            building: false,
            next_part: 0,
            // Far enough back that the first step asks at once.
            asked: Instant::now() - ASK_EVERY,
            incoming: Vec::new(),
            decoder: wire::Decoder::new(),
        }
    }

    /// The last reading, and how long ago it came.
    pub fn latest(&self) -> Option<(Status, Duration)> {
        let status = self.latest?;
        Some((status, self.at?.elapsed()))
    }

    /// Take one part of a set of readings.
    ///
    /// A set arrives in parts because the smallest link it travels over
    /// cannot carry the whole table in one notification. Part zero starts a
    /// fresh set and the part marked last publishes it, so what a reader sees
    /// is always a whole set or the previous whole set -- never half of one,
    /// which would read as gauges that had stopped.
    ///
    /// A part arriving out of order, or a first part that never came, throws
    /// the set away rather than showing a piece of it. Losing one second of
    /// readings is a smaller lie than a screen of gauges that are missing
    /// because a frame was.
    fn part(&mut self, len: usize) {
        let payload = &self.decoder.payload()[..len];
        let Some(header) = Readings::decode(payload) else {
            return;
        };
        if header.part == 0 {
            self.pending.clear();
            self.building = true;
        } else if !self.building || header.part != self.next_part {
            self.building = false;
            return;
        }
        self.pending.extend(wire::samples(payload));
        self.next_part = header.part.wrapping_add(1);
        if header.last {
            self.building = false;
            core::mem::swap(&mut self.gauges, &mut self.pending);
            self.pending.clear();
            self.gauges_at = Some(Instant::now());
        }
    }

    /// What the car said about one gauge, absence included.
    pub fn reading(&self, id: u16) -> Reading {
        match self.gauges.iter().find(|s| s.id == id) {
            Some(s) if s.raw != cogware_can::NO_DATA => Reading::Value(s.raw),
            _ => Reading::NoData,
        }
    }

    /// How many gauges the car is currently holding a value for.
    pub fn live_gauges(&self) -> usize {
        // Counted the same way `raw` answers, so the number at the top of the
        // panel and the rows under it can never disagree.
        self.gauges.iter().filter(|s| s.raw != cogware_can::NO_DATA).count()
    }

    /// True once the car has sent a set of readings, however empty.
    ///
    /// An empty set is an answer -- the car is quiet -- and is not the same
    /// as never having heard one.
    pub fn has_readings(&self) -> bool {
        self.gauges_at.is_some()
    }

    /// True if the car has said nothing for long enough to call it silent.
    ///
    /// Distinct from having heard nothing at all: a car that answered and
    /// then stopped is a different fault from one that never answered, and
    /// they want different things done about them.
    pub fn silent(&self) -> bool {
        self.at.is_some_and(|at| at.elapsed() > QUIET_FOR)
    }

    /// Take whatever has arrived, and ask again if it is time.
    pub fn step(&mut self, link: &mut dyn Transport) -> Result<(), Error> {
        link.poll(&mut self.incoming)?;
        for byte in std::mem::take(&mut self.incoming) {
            let Some((kind, len)) = self.decoder.push(byte) else {
                continue;
            };
            // Reports go past here too while a push is running on the same
            // link; they belong to the push and are not this view's business.
            match kind {
                KIND_STATUS => {
                    if let Some(status) = Status::decode(&self.decoder.payload()[..len]) {
                        self.latest = Some(status);
                        self.at = Some(Instant::now());
                    }
                }
                KIND_READINGS => self.part(len),
                // Reports go past here too while a push is running on the
                // same link; they belong to the push, not to this view.
                _ => {}
            }
        }
        if self.asked.elapsed() >= ASK_EVERY {
            self.asked = Instant::now();
            let mut frame = [0u8; wire::MAX_FRAME];
            if let Some(n) = wire::encode(KIND_PING, &[], &mut frame) {
                // A refused nudge is not worth reporting: the car sends on
                // its own timer anyway, and the link being busy means a push
                // is using it, which is the case where the nudge matters
                // least.
                let _ = link.send(&frame[..n]);
            }
        }
        Ok(())
    }
}

/// How long a car has been running, in words.
///
/// Hours, minutes and seconds rather than a count of milliseconds: what this
/// is read for is "has it restarted since I last looked", and a number that
/// large cannot be compared at a glance.
pub fn uptime(ms: u32) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload::tests_support::Fake;

    fn framed(status: Status) -> Vec<u8> {
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(KIND_STATUS, &status.encode(), &mut frame).expect("a status frames");
        frame[..n].to_vec()
    }

    #[test]
    fn a_status_off_the_link_becomes_a_reading() {
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        let status = Status {
            uptime_ms: 61_000,
            can_tx: 1234,
            subscribers: 9,
            flags: wire::FLAG_CAN_UP,
            ..Status::default()
        };
        link.replies = framed(status);
        watch.step(&mut link).expect("a step");
        let (seen, age) = watch.latest().expect("a reading");
        assert_eq!(seen, status);
        assert!(age < Duration::from_secs(1));
        assert!(seen.can_up());
    }

    #[test]
    fn a_push_report_on_the_same_link_is_not_read_as_a_status() {
        // Both travel the same wire while a push is running, and a report
        // read as a status would be nonsense numbers on the panel.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(wire::KIND_XFER_REPORT, &[0u8; wire::REPORT_LEN], &mut frame)
            .expect("a report frames");
        link.replies = frame[..n].to_vec();
        watch.step(&mut link).expect("a step");
        assert!(watch.latest().is_none());
    }


    /// One part of a set, framed as the car sends it.
    fn part_of(link: &mut Fake, header: Readings, want: &[Sample]) {
        let mut payload = vec![header.encode()];
        for s in want {
            payload.extend_from_slice(&s.encode());
        }
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(KIND_READINGS, &payload, &mut frame).expect("readings frame");
        link.replies = frame[..n].to_vec();
    }

    /// A whole set of readings in one part, framed as the car sends it.
    fn set_of(link: &mut Fake, want: &[Sample]) {
        let mut payload = vec![Readings { part: 0, last: true }.encode()];
        for s in want {
            payload.extend_from_slice(&s.encode());
        }
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(KIND_READINGS, &payload, &mut frame).expect("readings frame");
        link.replies = frame[..n].to_vec();
    }

    #[test]
    fn readings_arrive_as_raw_counts_against_their_gauges() {
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        set_of(&mut link, &[
            Sample { id: 0x024, raw: 1013 },
            Sample { id: 0x025, raw: -400 },
        ]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x024), Reading::Value(1013));
        // Negative survives: a gauge below its zero is a real reading, and
        // the sign is the difference between cold and broken.
        assert_eq!(watch.reading(0x025), Reading::Value(-400));
        assert_eq!(watch.reading(0x030), Reading::NoData, "a gauge not sent reads as no data");
    }

    #[test]
    fn a_reported_sentinel_reads_as_absent_too() {
        // A server can say "I have this channel and nothing is filling it".
        // On the panel that is the same row as one it never mentioned, and a
        // reader that showed the raw number would print -889275714.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        set_of(&mut link, &[
            Sample { id: 0x024, raw: 1013 },
            Sample { id: 0x026, raw: cogware_can::NO_DATA },
        ]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x024), Reading::Value(1013));
        assert_eq!(watch.reading(0x026), Reading::NoData);
        assert_eq!(watch.live_gauges(), 1, "the sentinel is not a live gauge");
        // The three answers stay apart, because they send somebody to look
        // at three different things.
        assert_eq!(watch.reading(0x024), Reading::Value(1013));
        assert_eq!(watch.reading(0x026), Reading::NoData, "the sentinel, sent outright");
        assert_eq!(watch.reading(0x030), Reading::NoData, "never mentioned, same answer");
    }

    #[test]
    fn a_gauge_that_stops_being_sent_stops_being_shown() {
        // The fault this view exists for. Keeping the last value would hide
        // a sender that has died behind a number that looks fine.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);

        set_of(&mut link, &[Sample { id: 0x024, raw: 1013 }]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x024), Reading::Value(1013));

        // The next set does not mention it.
        set_of(&mut link, &[Sample { id: 0x025, raw: 7 }]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x024), Reading::NoData, "a dead sender kept its last value");
        assert_eq!(watch.reading(0x025), Reading::Value(7));
    }

    #[test]
    fn an_empty_set_of_readings_is_an_answer_not_a_silence() {
        // A car holding nothing still sends a set: one part, marked last,
        // carrying no gauges. That says the car is quiet, which is a
        // different thing from the link being quiet.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        set_of(&mut link, &[]);
        assert!(!watch.has_readings());
        watch.step(&mut link).expect("a step");
        assert!(watch.has_readings(), "an empty set still says the car answered");
        assert_eq!(watch.live_gauges(), 0);
    }

    #[test]
    fn a_set_is_shown_only_once_its_last_part_has_arrived() {
        // Half a set on the screen reads as gauges that have stopped, which
        // is the one fault this view exists to show. It must never show one.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);

        part_of(&mut link, Readings { part: 0, last: false }, &[Sample { id: 0x024, raw: 1013 }]);
        watch.step(&mut link).expect("a step");
        assert!(!watch.has_readings(), "a set that is still arriving is not a set");

        part_of(&mut link, Readings { part: 1, last: true }, &[Sample { id: 0x025, raw: -400 }]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x024), Reading::Value(1013));
        assert_eq!(watch.reading(0x025), Reading::Value(-400));
        assert_eq!(watch.live_gauges(), 2, "both parts, one set");
    }

    #[test]
    fn a_set_missing_a_part_is_thrown_away_rather_than_half_shown() {
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        // A whole set first, so there is something to protect.
        set_of(&mut link, &[Sample { id: 0x030, raw: 77 }]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x030), Reading::Value(77));

        // Now a set whose middle part never came. The previous whole set
        // stands: one second of stale readings is a smaller lie than a screen
        // of gauges missing because a frame was.
        part_of(&mut link, Readings { part: 0, last: false }, &[Sample { id: 0x024, raw: 1 }]);
        watch.step(&mut link).expect("a step");
        part_of(&mut link, Readings { part: 2, last: true }, &[Sample { id: 0x025, raw: 2 }]);
        watch.step(&mut link).expect("a step");
        assert_eq!(watch.reading(0x030), Reading::Value(77), "the last whole set stands");
        assert_eq!(watch.reading(0x024), Reading::NoData, "no half set was shown");
    }

    #[test]
    fn a_payload_with_no_header_at_all_is_ignored() {
        // Not a part of anything. Reading it as gauges would take the first
        // two bytes of a reading as an identifier.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(KIND_READINGS, &[], &mut frame).expect("frame");
        link.replies = frame[..n].to_vec();
        watch.step(&mut link).expect("a step");
        assert!(!watch.has_readings());
    }

    #[test]
    fn the_car_is_asked_as_soon_as_the_view_opens() {
        // Waiting a whole second for the first reading looks like a link that
        // does not work.
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        watch.step(&mut link).expect("a step");
        assert_eq!(link.sent.len(), 1);
        assert_eq!(link.sent[0].get(2), Some(&KIND_PING));
    }

    #[test]
    fn it_is_not_asked_again_on_every_frame() {
        let mut watch = Watch::new();
        let mut link = Fake::with_room(4096);
        for _ in 0..50 {
            watch.step(&mut link).expect("a step");
        }
        assert_eq!(link.sent.len(), 1, "the car was nagged");
    }

    #[test]
    fn having_heard_nothing_is_not_the_same_as_having_gone_quiet() {
        // One is a link that never worked; the other is a car that stopped.
        let watch = Watch::new();
        assert!(watch.latest().is_none());
        assert!(!watch.silent(), "a car never heard from is not a silent one");
    }

    #[test]
    fn uptime_reads_as_time_rather_than_a_count() {
        assert_eq!(uptime(0), "0s");
        assert_eq!(uptime(45_000), "45s");
        assert_eq!(uptime(61_000), "1m 01s");
        assert_eq!(uptime(3_723_000), "1h 02m 03s");
    }
}
