// SPDX-License-Identifier: GPL-3.0-only
//! Pushing a scene, a rig or a firmware image to the car from the editor.
//!
//! The car's server takes an image the same way whatever brought it: the
//! frames are `cogware_can::wire`'s, and the server has one decoder for all
//! of them. So the only thing that differs between a cable, Bluetooth and a
//! network socket is how bytes get from here to there, and that is the whole
//! of [`Transport`].
//!
//! # Why the pacing matters
//!
//! The server streams an image through a buffer a small fraction of its size
//! -- it has kilobytes and an image has hundreds of them -- and passes it out
//! to the bus as it goes. A host that simply writes as fast as the link
//! allows is refused for most of a transfer and spends the link discovering
//! the limit by hitting it. Every report says how much room there is, and
//! [`Push`] sends to that figure and no further.
//!
//! # What is not here
//!
//! No retry of a whole image. A push that fails says why -- the node refused
//! it, the checksum did not match, nobody answered -- and those are things a
//! person should see rather than have papered over by a second attempt that
//! fails the same way.

use cogware_can::wire::{
    self, Report, Target, KIND_XFER_ABORT, KIND_XFER_BEGIN, KIND_XFER_DATA, KIND_XFER_REPORT,
    MAX_PAYLOAD, PHASE_DONE,
};

pub mod bluetooth;
pub mod cli;
pub mod ui;
pub mod watch;
pub mod wifi;

/// Whether to print what a push is deciding, and why.
///
/// Off unless `COGWARE_PUSH_DEBUG` is set. Kept, rather than deleted with the
/// rest of the bring-up scaffolding, because every bug found in this file was
/// found by watching these three numbers move: what the editor has sent, what
/// the car says it has taken, and how much room the car reports. A failure
/// here is always a disagreement between those.
fn tracing() -> bool {
    std::env::var_os("COGWARE_PUSH_DEBUG").is_some()
}

/// Bytes of image in one data frame.
///
/// The payload less the offset that rides in front of it, so a frame is as
/// full as the format allows. Fewer, larger frames is what keeps a slow link
/// from spending its time on headers.
pub const CHUNK: usize = MAX_PAYLOAD - wire::DATA_HEADER_LEN;

/// How a host's bytes reach the car.
///
/// Deliberately small. A transport moves whole frames and says whether it
/// managed; everything about what the frames mean is above it, written once.
pub trait Transport: Send {
    /// Offer one encoded frame to the link.
    ///
    /// `false` means the link is full and has not taken it: try again later,
    /// unchanged. It is never an error, and a caller that treated it as one
    /// would be giving up on a link that is merely busy.
    ///
    /// Not blocking, deliberately. A caller has to keep reading the car's
    /// reports to know when to send more, so a send that waited would stop
    /// the very thing that unblocks it -- and the car, starved of image,
    /// would give the push up as abandoned. Measured doing exactly that.
    fn send(&mut self, frame: &[u8]) -> Result<bool, Error>;

    /// Take whatever has arrived, appending to `out`, without waiting.
    ///
    /// Returning nothing is the ordinary case and not a fault: the server
    /// speaks when it has something to say.
    fn poll(&mut self, out: &mut Vec<u8>) -> Result<(), Error>;

    /// A name for the link, for putting in front of a person.
    fn describe(&self) -> String;
}

/// What went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The link itself failed.
    Link(String),
    /// Nothing was found to connect to.
    NotFound(String),
    /// The server said no, and this is its reason code.
    Refused(u8),
    /// The server stopped saying anything.
    Silent,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Link(why) => write!(f, "the link failed: {why}"),
            Error::NotFound(what) => write!(f, "nothing to connect to: {what}"),
            Error::Refused(code) => write!(f, "the car refused it ({})", reason(*code)),
            Error::Silent => write!(f, "the car stopped answering"),
        }
    }
}

impl std::error::Error for Error {}

/// What a server's refusal code means, in words.
///
/// The low numbers are the node's own answer relayed by the server; the high
/// ones are the server speaking for itself.
pub fn reason(code: u8) -> &'static str {
    match code {
        // The node's own last word, relayed. A push that ends on one of these
        // ended because the display said it, not because the server decided.
        0x01 => "the display was still waiting to start",
        0x02 => "the display was still taking it",
        0x03 => "the display never got part of it",
        0x04 => "the display had already finished",
        0x05 => "the image failed its checksum at the display",
        0x06 => "the display has nowhere to put it",
        0x07 => "the display gave up",
        0x80 => "the car had nowhere to put it",
        0x81 => "it was sent out of order",
        0x82 => "no display answered",
        0x83 => "it was cancelled",
        0x84 => "that address is the server itself",
        0x85 => "one display at a time",
        0x86 => "the editor stopped feeding it",
        _ => "no reason given",
    }
}

/// Where a push has got to, for the editor to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting for the server to admit the push has begun.
    Starting,
    /// Feeding it.
    Sending,
    /// It landed.
    Done,
}

/// One image on its way to the car.
///
/// Drives itself a step at a time so an editor can keep drawing: nothing here
/// blocks, and a pass that can do nothing costs the look that found that out.
pub struct Push {
    target: Target,
    image: Vec<u8>,
    sent: usize,
    /// What the server last said it had taken and had room for.
    staged: u32,
    free: u32,
    state: State,
    /// Whether the last batch of reports showed the car taking more.
    moved: bool,
    /// Attempts at the opening frame.
    ///
    /// Sent until acknowledged, because it is the one frame that must not be
    /// assumed to have landed: everything after it is data, which a server
    /// with no push under way discards, and the whole transfer then waits on
    /// something that was never begun.
    opens: u32,
    /// When the next attempt is due.
    ///
    /// Paced, and that pacing is the whole point: an attempt costs nothing
    /// and is answered in milliseconds, but the answer has to cross a radio
    /// and be read by a thread. Retrying as fast as the caller steps spends
    /// every attempt before the first reply can arrive, and reports a car
    /// that is answering perfectly well as silent. Measured doing exactly
    /// that -- eight attempts in sixteen milliseconds.
    next_open_ms: u64,
    percent: u8,
    /// When the car last said it had taken more than before.
    ///
    /// A link that drops a packet without saying so leaves this still: the
    /// car refuses everything after the gap, because an image must arrive in
    /// order, and its count stops. That stillness is the only evidence there
    /// is, so it is what a rewind is timed from.
    moved_ms: u64,
    incoming: Vec<u8>,
    decoder: wire::Decoder,
    /// Bytes the car has said back, for telling a silent link from a
    /// misunderstood one.
    heard: usize,
    /// Whole frames made out of them.
    frames: usize,
    /// Times this has gone back to what the car said it had.
    rewinds: u32,
}

/// Times the opening frame is offered before the car is taken to be absent.
const OPENS: u32 = 8;

/// How long the car may take nothing new before what was written is assumed
/// lost.
///
/// Generous against the bus: the car takes bytes only as fast as it passes
/// them on, and a pause of a second or so is ordinary. A rewind costs only
/// the bytes since the gap, so erring long is cheap and erring short would
/// resend perfectly good image.
const REWIND_AFTER_MS: u64 = 3_000;

/// Rewinds *in a row*, with the car taking nothing between them, before the
/// link is called hopeless.
///
/// A link that loses the same stretch over and over is not going to carry an
/// image, and saying so beats resending for ever. One that loses the odd
/// packet and then gets on with it is simply a radio.
const REWINDS: u32 = 8;

/// How long to leave between those attempts.
///
/// Long enough for one to be written, carried, answered and read back. Eight
/// of these is the patience a car gets before it is called absent.
const OPEN_EVERY_MS: u64 = 400;

impl Push {
    /// Prepare to send `image` to `target`.
    ///
    /// The checksum is worked out here, over the whole image, because this is
    /// the last place that has all of it: the server never does.
    pub fn new(node: u8, kind: u8, slot: u8, image: Vec<u8>) -> Self {
        let crc = cogware_can::crc::crc32(&image);
        Self {
            target: Target {
                node,
                kind,
                slot,
                size: image.len() as u32,
                crc,
            },
            image,
            sent: 0,
            staged: 0,
            free: CHUNK as u32,
            state: State::Starting,
            moved: false,
            opens: 0,
            next_open_ms: 0,
            percent: 0,
            moved_ms: 0,
            incoming: Vec::new(),
            decoder: wire::Decoder::new(),
            heard: 0,
            frames: 0,
            rewinds: 0,
        }
    }

    /// Bytes the car has said back.
    ///
    /// Zero at the end of a failed push means the link never carried anything
    /// in that direction, which is a different fault from a car that answered
    /// and refused -- and points at a different half of the system.
    pub fn heard(&self) -> usize {
        self.heard
    }

    /// Whole frames made out of what was heard.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// How far the bus has confirmed, not how far this has written.
    ///
    /// The two differ by most of a transfer -- the editor's bytes reach the
    /// car long before the car has passed them to the display -- and showing
    /// the first would be a bar that fills and then waits.
    pub fn percent(&self) -> u8 {
        self.percent
    }

    /// Do whatever can be done now. Call it often; it never blocks.
    pub fn step(&mut self, link: &mut dyn Transport, now_ms: u64) -> Result<State, Error> {
        link.poll(&mut self.incoming)?;
        self.heard += self.incoming.len();
        for byte in std::mem::take(&mut self.incoming) {
            let Some((kind, len)) = self.decoder.push(byte) else {
                continue;
            };
            self.frames += 1;
            if kind != KIND_XFER_REPORT {
                continue;
            }
            let Some(report) = Report::decode(&self.decoder.payload()[..len]) else {
                continue;
            };
            if tracing() {
                eprintln!(
                    "    report phase {} filled {} size {} free {} pct {}",
                    report.phase, report.filled, report.target.size, report.free, report.percent
                );
            }
            if let Some(state) = self.take(report)? {
                return Ok(state);
            }
        }
        if core::mem::take(&mut self.moved) {
            self.moved_ms = now_ms;
            // Counted consecutively, not cumulatively. A long image over a
            // lossy radio rewinds many times and finishes perfectly well;
            // what says a link is hopeless is rewinding again and again with
            // the car taking nothing in between. Measured getting this wrong:
            // a push abandoned at eighty percent for having recovered forty
            // times, each of them successfully.
            self.rewinds = 0;
        } else if self.sent as u32 > self.staged && now_ms >= self.moved_ms + REWIND_AFTER_MS {
            // The car has taken nothing new for a while and is behind what
            // this has written, so the difference was lost on the way. Its
            // count is the truth; ours was only ever a hint.
            self.sent = self.staged as usize;
            self.moved_ms = now_ms;
            self.rewinds += 1;
            if self.rewinds > REWINDS {
                return Err(Error::Silent);
            }
        }
        if self.state == State::Starting {
            self.open(link, now_ms)?;
        }
        self.feed(link)?;
        Ok(self.state)
    }

    /// Act on one report from the server.
    fn take(&mut self, report: Report) -> Result<Option<State>, Error> {
        if report.phase == PHASE_DONE {
            self.percent = 100;
            self.state = State::Done;
            return Ok(Some(State::Done));
        }
        if report.finished() {
            return Err(Error::Refused(report.reason));
        }
        // A report about somebody else's push, or about one from before this
        // editor connected, says nothing about this image.
        if report.target.size != self.target.size {
            return Ok(None);
        }
        if report.filled != self.staged {
            self.staged = report.filled;
            self.moved = true;
        }
        self.free = report.free;
        self.percent = report.percent;
        self.state = State::Sending;
        Ok(None)
    }

    /// Offer the frame that starts the push, until it is acknowledged.
    fn open(&mut self, link: &mut dyn Transport, now_ms: u64) -> Result<(), Error> {
        if now_ms < self.next_open_ms {
            return Ok(());
        }
        if self.opens >= OPENS {
            return Err(Error::Silent);
        }
        self.opens += 1;
        self.next_open_ms = now_ms + OPEN_EVERY_MS;
        let mut frame = [0u8; wire::MAX_FRAME];
        let payload = self.target.encode();
        let n = wire::encode(KIND_XFER_BEGIN, &payload, &mut frame)
            .ok_or_else(|| Error::Link("the opening frame would not encode".into()))?;
        // A link too busy to take it is asked again on the next attempt, and
        // that attempt must not be spent: the car has not been asked yet.
        if !link.send(&frame[..n])? {
            self.opens -= 1;
            self.next_open_ms = now_ms;
        }
        Ok(())
    }

    /// Send as much image as the server has said it has room for.
    fn feed(&mut self, link: &mut dyn Transport) -> Result<(), Error> {
        let mut frame = vec![0u8; wire::MAX_FRAME];
        while self.sent < self.image.len() {
            // What is already on its way and not yet taken, against what the
            // server said it could hold.
            let outstanding = self.sent as u32 - self.staged;
            if outstanding + CHUNK as u32 > self.free {
                if tracing() {
                    eprintln!(
                        "    held: sent {} staged {} outstanding {outstanding} free {}",
                        self.sent, self.staged, self.free
                    );
                }
                return Ok(());
            }
            let end = (self.sent + CHUNK).min(self.image.len());
            let mut payload = Vec::with_capacity(wire::DATA_HEADER_LEN + end - self.sent);
            payload.extend_from_slice(&(self.sent as u32).to_le_bytes());
            payload.extend_from_slice(&self.image[self.sent..end]);
            let n = wire::encode(KIND_XFER_DATA, &payload, &mut frame)
                .ok_or_else(|| Error::Link("a data frame would not encode".into()))?;
            // Only counted once the link has actually taken it. Counting a
            // frame the link refused would have this push believe it is
            // further ahead than it is, and stop feeding a car that is still
            // waiting.
            if !link.send(&frame[..n])? {
                return Ok(());
            }
            self.sent = end;
        }
        Ok(())
    }

    /// Tell the car to forget it.
    ///
    /// Worth sending even when the editor is giving up for its own reasons: a
    /// server left mid-push refuses every later one until it times out.
    pub fn cancel(&mut self, link: &mut dyn Transport) -> Result<(), Error> {
        let mut frame = [0u8; wire::MAX_FRAME];
        let n = wire::encode(KIND_XFER_ABORT, &[], &mut frame)
            .ok_or_else(|| Error::Link("the cancel frame would not encode".into()))?;
        link.send(&frame[..n]).map(|_| ())
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// A car that takes everything and says what a real one would.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub sent: Vec<Vec<u8>>,
        pub replies: Vec<u8>,
        pub free: u32,
        pub taken: u32,
    }

    impl Fake {
        pub fn with_room(free: u32) -> Self {
            Self { free, ..Self::default() }
        }
        /// How many opening frames it has been sent.
        pub fn opens(&self) -> usize {
            self.sent.iter().filter(|f| f.get(2) == Some(&KIND_XFER_BEGIN)).count()
        }

        /// How many data frames carried image starting at `offset`.
        pub fn at_offset(&self, offset: u32) -> usize {
            self.sent
                .iter()
                .filter(|f| {
                    f.get(2) == Some(&KIND_XFER_DATA)
                        && f.get(5..9)
                            .is_some_and(|b| b == offset.to_le_bytes())
                })
                .count()
        }
        pub fn report(&mut self, phase: u8, reason: u8, size: u32, percent: u8) {
            let mut payload = [0u8; wire::REPORT_LEN];
            payload[0] = phase;
            payload[1] = reason;
            payload[5..9].copy_from_slice(&self.taken.to_le_bytes());
            payload[9..13].copy_from_slice(&size.to_le_bytes());
            payload[13] = percent;
            payload[14..18].copy_from_slice(&self.free.to_le_bytes());
            let mut frame = [0u8; wire::MAX_FRAME];
            let n = wire::encode(KIND_XFER_REPORT, &payload, &mut frame).expect("a report frames");
            self.replies.extend_from_slice(&frame[..n]);
        }
    }

    impl Transport for Fake {
        fn send(&mut self, frame: &[u8]) -> Result<bool, Error> {
            self.sent.push(frame.to_vec());
            Ok(true)
        }
        fn poll(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
            out.append(&mut self.replies);
            Ok(())
        }
        fn describe(&self) -> String {
            "a fake".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::Fake;
    use super::*;

    fn kinds(link: &Fake) -> Vec<u8> {
        link.sent
            .iter()
            .filter_map(|f| f.get(2).copied())
            .collect()
    }

    #[test]
    fn the_opening_frame_is_offered_before_any_image() {
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        push.step(&mut link, 0).expect("a step must work");
        assert_eq!(kinds(&link).first(), Some(&KIND_XFER_BEGIN));
    }

    #[test]
    fn it_is_offered_again_until_the_car_admits_it() {
        // The one frame that must not be assumed to have landed.
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        for i in 0..3 {
            push.step(&mut link, i * OPEN_EVERY_MS).expect("a step must work");
        }
        let opens = kinds(&link).iter().filter(|k| **k == KIND_XFER_BEGIN).count();
        assert_eq!(opens, 3, "a silent car must be asked again");
    }

    #[test]
    fn a_car_that_never_admits_it_is_given_up_on_rather_than_asked_for_ever() {
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        let mut last = Ok(State::Starting);
        for i in 0..u64::from(OPENS) + 1 {
            last = push.step(&mut link, i * OPEN_EVERY_MS);
        }
        assert_eq!(last, Err(Error::Silent));
    }

    #[test]
    fn image_follows_once_the_car_says_it_has_begun() {
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        push.step(&mut link, 0).expect("a step");
        link.report(1, 0, 100, 0);
        push.step(&mut link, 1).expect("a step");
        assert!(kinds(&link).contains(&KIND_XFER_DATA));
    }

    #[test]
    fn nothing_is_sent_past_the_room_the_car_said_it_had() {
        // A host that simply writes is refused for most of a transfer and
        // spends the link finding the limit by hitting it.
        let image = vec![7u8; CHUNK * 8];
        let mut push = Push::new(2, 1, 0, image);
        let mut link = Fake::with_room(CHUNK as u32);
        push.step(&mut link, 0).expect("a step");
        link.report(1, 0, (CHUNK * 8) as u32, 0);
        push.step(&mut link, 1).expect("a step");
        let data = kinds(&link).iter().filter(|k| **k == KIND_XFER_DATA).count();
        assert_eq!(data, 1, "only one frame fits the room claimed");
    }

    #[test]
    fn more_room_lets_more_through() {
        let image = vec![7u8; CHUNK * 8];
        let mut push = Push::new(2, 1, 0, image);
        let mut link = Fake::with_room(CHUNK as u32 * 4);
        push.step(&mut link, 0).expect("a step");
        link.report(1, 0, (CHUNK * 8) as u32, 0);
        push.step(&mut link, 1).expect("a step");
        let data = kinds(&link).iter().filter(|k| **k == KIND_XFER_DATA).count();
        assert_eq!(data, 4);
    }

    #[test]
    fn the_whole_image_goes_as_the_car_takes_it() {
        let image = vec![7u8; CHUNK * 3 + 11];
        let mut push = Push::new(2, 1, 0, image.clone());
        let mut link = Fake::with_room(CHUNK as u32);
        push.step(&mut link, 0).expect("a step");
        for i in 0..8u64 {
            link.taken = push.sent as u32;
            link.report(2, 0, image.len() as u32, 0);
            push.step(&mut link, i + 1).expect("a step");
        }
        assert_eq!(push.sent, image.len(), "the image did not finish");
    }

    #[test]
    fn a_refusal_is_reported_with_the_cars_own_reason() {
        let mut push = Push::new(2, 1, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        push.step(&mut link, 0).expect("a step");
        link.report(wire::PHASE_FAILED, 0x05, 100, 0);
        assert_eq!(push.step(&mut link, 1), Err(Error::Refused(0x05)));
    }

    #[test]
    fn every_reason_the_server_can_give_has_words() {
        // Taken from where the codes are defined rather than copied into a
        // list here. The copied list was the bug: it matched the table it was
        // checking, so it passed while four codes the server really sends had
        // no words, and a push that failed because the display missed part of
        // an image said "no reason given" and sent somebody looking at the
        // wrong end of the car.
        use cogware_can::xfer::Status;
        let from_the_node = [
            Status::Ready, Status::Ok, Status::Missing, Status::Done,
            Status::CrcFail, Status::Refused, Status::Aborted,
        ];
        for status in from_the_node {
            let code = status as u8;
            assert_ne!(reason(code), "no reason given", "{status:?} ({code:#04x})");
        }
        for code in [0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86] {
            assert_ne!(reason(code), "no reason given", "code {code:#04x}");
        }
    }

    #[test]
    fn a_finished_push_says_so_and_reads_as_whole() {
        let mut push = Push::new(2, 1, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        push.step(&mut link, 0).expect("a step");
        link.report(PHASE_DONE, 0, 100, 100);
        assert_eq!(push.step(&mut link, 1), Ok(State::Done));
        assert_eq!(push.percent(), 100);
    }

    #[test]
    fn a_report_about_another_image_is_not_taken_as_this_one() {
        // Two editors, or one that reconnected: a stale report would move
        // this push's bar to somebody else's progress.
        let mut push = Push::new(2, 1, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        push.step(&mut link, 0).expect("a step");
        link.report(2, 0, 999_999, 42);
        push.step(&mut link, 1).expect("a step");
        assert_eq!(push.percent(), 0, "another push's progress was taken");
    }

    #[test]
    fn the_checksum_is_of_the_whole_image() {
        let image = vec![1u8, 2, 3, 4, 5];
        let push = Push::new(2, 1, 0, image.clone());
        assert_eq!(push.target.crc, cogware_can::crc::crc32(&image));
        assert_eq!(push.target.size, 5);
    }
}

#[cfg(test)]
mod pacing {
    use super::tests_support::Fake;
    use super::*;

    /// A link that refuses everything, as a busy radio does.
    struct Full;

    impl Transport for Full {
        fn send(&mut self, _frame: &[u8]) -> Result<bool, Error> {
            Ok(false)
        }
        fn poll(&mut self, _out: &mut Vec<u8>) -> Result<(), Error> {
            Ok(())
        }
        fn describe(&self) -> String {
            "a full link".into()
        }
    }

    #[test]
    fn a_frame_the_link_would_not_take_is_not_counted_as_sent() {
        // Counting it would have the push believe it is further ahead than
        // it is, stop feeding, and leave the car waiting until it gave the
        // push up as abandoned. Measured doing that on a radio.
        let mut push = Push::new(2, 1, 0, vec![7; CHUNK * 4]);
        let mut full = Full;
        push.step(&mut full, 0).expect("a step");
        assert_eq!(push.sent, 0, "a refused frame was counted");
    }

    #[test]
    fn what_the_car_never_received_is_sent_again() {
        // A radio drops a write without telling either end. The car then
        // refuses everything after the gap -- an image must arrive in order
        // -- so its count stands still while this one runs away. Measured
        // deadlocking exactly there, seven thousand bytes apart.
        let image = vec![7u8; CHUNK * 8];
        let mut push = Push::new(2, 1, 0, image.clone());
        let mut link = Fake::with_room(CHUNK as u32 * 8);
        push.step(&mut link, 0).expect("a step");
        link.report(2, 0, image.len() as u32, 0);
        push.step(&mut link, 1).expect("a step");
        let ran_to = push.sent;
        assert!(ran_to > 0, "nothing was sent to lose");

        // The car goes on saying it has taken nothing.
        for ms in 2..REWIND_AFTER_MS + 2 {
            link.report(2, 0, image.len() as u32, 0);
            push.step(&mut link, ms).expect("a step");
        }
        // The lost stretch went again: the first block of the image appears
        // on the link more than once. Checking `sent` would prove nothing --
        // a rewind is followed immediately by feeding the same bytes again,
        // which is the point of it.
        assert!(
            link.at_offset(0) > 1,
            "the start of the image was never sent again"
        );
        assert_eq!(ran_to, push.sent, "and it caught back up to where it was");
    }

    #[test]
    fn a_car_that_keeps_taking_more_is_never_rewound() {
        // The cost of getting this wrong is resending perfectly good image.
        let image = vec![7u8; CHUNK * 40];
        let mut push = Push::new(2, 1, 0, image.clone());
        let mut link = Fake::with_room(CHUNK as u32 * 4);
        push.step(&mut link, 0).expect("a step");
        let mut high = 0;
        for ms in 1..REWIND_AFTER_MS * 3 {
            link.taken = push.sent as u32;
            link.report(2, 0, image.len() as u32, 0);
            push.step(&mut link, ms).expect("a step");
            assert!(push.sent >= high, "a steady link was rewound");
            high = push.sent;
        }
    }

    #[test]
    fn a_link_too_busy_for_the_opening_frame_does_not_spend_the_attempt() {
        // The car has not been asked, so the patience must not shrink.
        let mut push = Push::new(2, 1, 0, vec![7; 100]);
        let mut full = Full;
        for ms in 0..OPEN_EVERY_MS * u64::from(OPENS) + 2 {
            push.step(&mut full, ms).expect("a busy link is not a failure");
        }
    }

    #[test]
    fn the_opening_frame_is_not_re_sent_faster_than_it_can_be_answered() {
        // Measured failing: eight attempts inside sixteen milliseconds, and a
        // car answering perfectly well reported as silent.
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        for ms in 0..OPEN_EVERY_MS {
            push.step(&mut link, ms).expect("a step must work");
        }
        assert_eq!(link.opens(), 1, "the car was asked again before it could reply");
    }

    #[test]
    fn the_patience_is_the_whole_window_not_the_step_rate() {
        let mut push = Push::new(2, 2, 0, vec![7; 100]);
        let mut link = Fake::with_room(4096);
        let mut last = Ok(State::Starting);
        // Stepped every millisecond, as a busy editor would.
        for ms in 0..OPEN_EVERY_MS * u64::from(OPENS) + 2 {
            last = push.step(&mut link, ms);
            if last.is_err() {
                assert!(
                    ms >= OPEN_EVERY_MS * u64::from(OPENS - 1),
                    "gave up after only {ms}ms"
                );
                break;
            }
        }
        assert_eq!(last, Err(Error::Silent));
    }
}

/// The kind that carries gauge readings to the master, matching its `host.rs`.
pub const KIND_GAUGE: u8 = 0x01;

/// Bytes one reading takes: a two-byte id then a four-byte value.
const GAUGE_ENTRY: usize = 6;

/// Send one car's worth of readings to the master.
///
/// The master writes them into its gauge table and broadcasts them like any
/// other source, so every display on the bus shows the drive -- which is the
/// point: a layout is read on the glass it will live on, not only in the
/// editor's preview.
pub fn feed_gauges(
    link: &mut dyn Transport,
    car: &cogware_can::drive::Car,
) -> Result<(), Error> {
    let mut payload = Vec::with_capacity(cogware_can::drive::DRIVEN.len() * GAUGE_ENTRY);
    for (id, _) in cogware_can::drive::DRIVEN {
        let Some(value) = car.reading(*id) else { continue };
        payload.extend_from_slice(&id.to_le_bytes());
        payload.extend_from_slice(&value.to_le_bytes());
    }
    if payload.is_empty() {
        return Ok(());
    }
    let mut frame = [0u8; wire::MAX_FRAME];
    let n = wire::encode(KIND_GAUGE, &payload, &mut frame)
        .ok_or_else(|| Error::Link("the readings would not encode".into()))?;
    link.send(&frame[..n])?;
    Ok(())
}

/// The kind that carries a network to the master, matching its `host.rs`.
pub const KIND_NETWORK: u8 = 0x13;

/// Longest name and passphrase the master will store.
pub const SSID_MAX: usize = 32;
/// As above, for the passphrase.
pub const PASS_MAX: usize = 63;

/// Tell the master which network to join, and have it keep it.
///
/// Sent over whichever link is open, which in practice means Bluetooth: these
/// are the credentials for the network, so they cannot arrive over it. The
/// master writes them to flash and joins on its next start.
pub fn set_network(link: &mut dyn Transport, ssid: &str, password: &str) -> Result<(), Error> {
    // Checked here as well as on the board, so somebody typing into a box is
    // told which field is wrong rather than watching a frame be ignored.
    if ssid.is_empty() || ssid.len() > SSID_MAX {
        return Err(Error::Link(format!("a name is 1 to {SSID_MAX} bytes")));
    }
    if password.len() > PASS_MAX {
        return Err(Error::Link(format!("a passphrase is at most {PASS_MAX} bytes")));
    }
    // The zero separates the two, so neither may contain one.
    if ssid.contains('\0') || password.contains('\0') {
        return Err(Error::Link("neither may contain a zero".into()));
    }

    let mut payload = Vec::with_capacity(ssid.len() + 1 + password.len());
    payload.extend_from_slice(ssid.as_bytes());
    payload.push(0);
    payload.extend_from_slice(password.as_bytes());

    let mut frame = [0u8; wire::MAX_FRAME];
    let n = wire::encode(KIND_NETWORK, &payload, &mut frame)
        .ok_or_else(|| Error::Link("the network frame would not encode".into()))?;
    link.send(&frame[..n])?;
    Ok(())
}

#[cfg(test)]
mod network_tests {
    use super::tests_support::Fake;
    use super::*;

    /// The payload of the one frame that went out.
    fn payload_of(link: &Fake) -> Vec<u8> {
        let frame = link.sent.first().expect("one frame");
        assert_eq!(frame[2], KIND_NETWORK, "the kind the master stores");
        // Sync, kind, a two-byte length, then the payload; the tail is a CRC.
        let len = u16::from_le_bytes([frame[3], frame[4]]) as usize;
        frame[5..5 + len].to_vec()
    }

    #[test]
    fn a_drive_goes_out_as_readings_the_master_can_parse() {
        // The master reads a two-byte id then a four-byte value, and rejects a
        // payload that is not a whole number of those. A mismatch here would
        // not error anywhere -- the gauges would simply never move.
        let mut link = Fake::default();
        let mut car = cogware_can::drive::Car::new();
        car.advance(3_000_000);
        feed_gauges(&mut link, &car).expect("sends");

        let frame = link.sent.first().expect("one frame");
        assert_eq!(frame[2], KIND_GAUGE);
        let len = u16::from_le_bytes([frame[3], frame[4]]) as usize;
        assert_eq!(len % GAUGE_ENTRY, 0, "a whole number of readings");

        let payload = &frame[5..5 + len];
        let rpm = cogware_can::RPM.id;
        let found = payload.chunks_exact(GAUGE_ENTRY).find(|e| {
            u16::from_le_bytes([e[0], e[1]]) == rpm
        });
        let entry = found.expect("the drive carries rpm");
        let value = i32::from_le_bytes([entry[2], entry[3], entry[4], entry[5]]);
        assert!((0..=8000).contains(&value), "rpm reads {value}");
    }

    #[test]
    fn a_name_and_secret_go_out_separated_by_a_zero() {
        let mut link = Fake::default();
        set_network(&mut link, "DingusBingus", "hunter2").expect("sends");
        assert_eq!(payload_of(&link), b"DingusBingus\0hunter2");
    }

    #[test]
    fn an_open_network_is_a_name_and_nothing_after_the_zero() {
        let mut link = Fake::default();
        set_network(&mut link, "cafe", "").expect("sends");
        assert_eq!(payload_of(&link), b"cafe\0");
    }

    #[test]
    fn what_the_board_would_refuse_is_refused_here_first() {
        // Said in the window rather than swallowed by a board that ignores
        // the frame, which looks identical to a link that is not there.
        let mut link = Fake::default();
        assert!(set_network(&mut link, "", "x").is_err(), "an empty name");
        assert!(
            set_network(&mut link, &"a".repeat(SSID_MAX + 1), "x").is_err(),
            "an over-long name"
        );
        assert!(
            set_network(&mut link, "net", &"a".repeat(PASS_MAX + 1)).is_err(),
            "an over-long secret"
        );
        assert!(link.sent.is_empty(), "nothing goes out when it is refused");
    }
}
