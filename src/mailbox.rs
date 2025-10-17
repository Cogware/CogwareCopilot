use crate::{info, mailbox::ReqResp::ResponseSuccessful};
use core::sync::atomic::Ordering::Relaxed;
use core::{arch::aarch64::float32x2_t, mem, ops::BitAnd, sync::atomic::AtomicU32};

const VIDEOCORE_MBOX_BASE: u32 = 0x000f_0000;

use tock_registers::{
    interfaces::{Readable, Writeable},
    registers::{ReadOnly, WriteOnly},
};

// TODO: wrap into registers map lib
#[repr(C)]
struct RawMailbox {
    read: ReadOnly<u32>,
    _unused: u32,
    _unused2: u32,
    _unused3: u32,
    poll: u32,
    sender: u32,
    status: ReadOnly<u32>,
    config: u32,
    write: WriteOnly<u32>,
}

pub struct Mailboxaddr {
    pub address: u32,
}
impl Mailboxaddr {
    pub fn new(address: AtomicU32) -> Self {
        let newaddr = address.load(Relaxed);
        Mailboxaddr { address: newaddr }
    }
}

impl RawMailbox {
    pub(crate) fn is_empty(&self) -> bool {
        let status = self.get_status();
        status & STATUS_EMPTY == STATUS_EMPTY
    }

    fn is_full(&self) -> bool {
        let status = self.get_status();
        status & STATUS_FULL == STATUS_FULL
    }

    pub(crate) fn get_read(&self) -> u32 {
        self.read.get()
    }

    pub(crate) fn write_address(&mut self, address: usize) {
        self.write.set(address as u32)
    }

    fn get_status(&self) -> u32 {
        self.status.get()
    }
}

const STATUS_FULL: u32 = 0x80000000;
const STATUS_EMPTY: u32 = 0x40000000;

impl RawMailbox {}

#[derive(Debug, Copy, Clone)]
enum ReqResp {
    ResponseSuccessful,
    ResponseError,
    Request,
}

impl PartialEq<Self> for ReqResp {
    fn eq(&self, other: &Self) -> bool {
        let other = *other as u32;
        (*self as u32).eq(&other)
    }
}

impl Eq for ReqResp {}

impl Into<u32> for ReqResp {
    fn into(self) -> u32 {
        use ReqResp::*;
        match self {
            Request => 0x00000000,
            ResponseSuccessful => 0x80000000,
            ResponseError => 0x80000001,
        }
    }
}
impl From<u32> for ReqResp {
    fn from(val: u32) -> Self {
        use ReqResp::*;
        match val {
            0x00000000 => Request,
            0x80000000 => ResponseSuccessful,
            _ => ResponseError,
        }
    }
}
const MBOX_REQUEST: u32 = 0;
const BOARD_SERIAL_REQ: u32 = 0x00010004;
const GET_MAX_CLOCK_RATE: u32 = 0x00030004;
const SET_CLOCK_RATE: u32 = 0x00038002;
const GET_CURRENT_CLOCK_RATE: u32 = 0x00030002;
const SET_VIRTUAL_BUFFER_OFFSET_TAG: u32 = 0x00048009;
const TEST_SET_VIRTUAL_BUFFER_OFFSET_TAG: u32 = 0x00044009;
const LAST_TAG: u32 = 0;

#[repr(align(16))]
#[derive(Debug, Copy, Clone)]
struct Message<const T: usize>([u32; T]);

impl<const T: usize> Message<T> {
    pub fn response_status(&self) -> ReqResp {
        ReqResp::from(self.0[1])
    }
    pub fn is_response_successfull(&self) -> bool {
        self.response_status() == ResponseSuccessful
    }
}

pub fn query_board_serial(mailboxaddr: Mailboxaddr) -> Option<u64> {
    info!("Preparing board message..");
    let message = board_serial_message();
    info!("Sending message to channel PROP: {:?}", message);

    return if send_message_sync(mailboxaddr.address, Channel::PROP, &message) {
        info!(
            "Serial number is: {:#04x}/{:#04x}",
            message.0[5], message.0[4]
        );
        let b = message.0[4].to_ne_bytes();
        let c = message.0[5].to_ne_bytes();
        let single = [b[0], b[1], b[2], b[3], c[0], c[1], c[2], c[3]];
        info!("Single: {:?}", single);
        Some(u64::from_ne_bytes(single))
    } else {
        info!("Failed to sending message to query the board serial.");
        None
    };
}

pub fn set_clock_speed(mailboxaddr: Mailboxaddr, new_clock: u32) {
    let message = get_set_clock_rate_message(new_clock);
    // info!(
    //    "Sending message to channel PROP to set clock speed: {:?}",
    //    message
    //);

    if send_message_sync(mailboxaddr.address, Channel::PROP, &message) {
        //  info!("message: {:?}", message);
        let rate = message.0[6];
        let ratecalc: f64 = rate.into();
        info!(
            "New rate for ARM CORE is: {:?}Ghz",
            ratecalc / 1_000_000_000.0
        );
    } else {
        info!("Failed to sending message to set clock speed.");
    }
    let message2 = get_current_clock_rate_message();
    // info!(
    //   "Sending message to channel PROP to read clock speed: {:?}",
    //   message2
    //);
    if send_message_sync(mailboxaddr.address, Channel::PROP, &message2) {
        info!("message: {:?}", message2);
        let rate = message2.0[6];
        let ratecalc: f64 = rate.into();

        info!(
            "Rate Readback to check ARM CORE is: {:?}Ghz",
            ratecalc / 1_000_000_000.0
        );
    } else {
        info!("Failed to sending message to set clock speed.");
    }
}

pub fn max_clock_speed(mailboxaddr: Mailboxaddr) -> Option<u32> {
    // command 0x00030004 ARM clock ID = 0x3
    // BCM2835_MAILBOX_TAG_GET_MAX_CLOCK_RATE 0x00030004
    let message2 = get_current_clock_rate_message();
    // info!(
    //   "Sending message to channel PROP to read clock speed: {:?}",
    //   message2
    //);
    if send_message_sync(mailboxaddr.address, Channel::PROP, &message2) {
        info!("message: {:?}", message2);
        let rate = message2.0[6];
        let ratecalc: f64 = rate.into();

        info!(
            "Current ARM CORE rate is: {:?}Ghz",
            ratecalc / 1_000_000_000.0
        );
    } else {
        info!("Failed to sending message to set clock speed.");
    }
    let message = max_clock_rate_message();
    // info!(
    //    "Sending message to channel PROP for max clock speed: {:?}",
    //    message
    //);

    if send_message_sync(mailboxaddr.address, Channel::PROP, &message) {
        info!("message: {:?}", message);
        let max_speed_hz = message.0[6];
        let ratecalc: f64 = max_speed_hz.into();
        info!(
            "Max clock speed for ARM CORE is : {:?}Ghz",
            ratecalc / 1_000_000_000.0
        );
        Some(max_speed_hz)
    } else {
        info!("Failed to sending message to query max clock speed.");
        None
    }
}

const GET_CURRENT_CLOCK_RATE_MESSAGE_SIZE: usize = 9;
fn get_current_clock_rate_message() -> Message<GET_CURRENT_CLOCK_RATE_MESSAGE_SIZE> {
    let mut ret = [0u32; GET_CURRENT_CLOCK_RATE_MESSAGE_SIZE];
    ret[0] = (GET_CURRENT_CLOCK_RATE_MESSAGE_SIZE * mem::size_of::<u32>()) as u32;
    ret[1] = MBOX_REQUEST;

    ret[2] = GET_CURRENT_CLOCK_RATE; // set clock rate
    ret[3] = 8; // value buffer size in bytes
    ret[4] = 8; // clock id
    ret[5] = 0x3; // rate in hz
    ret[6] = 0; // skip setting turbo
    ret[7] = LAST_TAG;
    Message(ret)
}

const GET_CLOCK_RATE_MESSAGE_SIZE: usize = 10;
fn get_set_clock_rate_message(new_clock_hz: u32) -> Message<GET_CLOCK_RATE_MESSAGE_SIZE> {
    let mut ret = [0u32; GET_CLOCK_RATE_MESSAGE_SIZE];
    ret[0] = (GET_CLOCK_RATE_MESSAGE_SIZE * mem::size_of::<u32>()) as u32;
    ret[1] = MBOX_REQUEST;

    ret[2] = SET_CLOCK_RATE; // set clock rate
    ret[3] = 8; // value buffer size in bytes
    ret[4] = 8; // clock id
    ret[5] = 0x3; // rate in hz
    ret[6] = new_clock_hz; // skip setting turbo
    ret[7] = LAST_TAG;
    Message(ret)
}

/// rate in hz.
const MAX_CLOCK_RATE_MESSAGE_SIZE: usize = 9;
fn max_clock_rate_message() -> Message<MAX_CLOCK_RATE_MESSAGE_SIZE> {
    let mut ret = [0u32; MAX_CLOCK_RATE_MESSAGE_SIZE];
    ret[0] = (MAX_CLOCK_RATE_MESSAGE_SIZE * mem::size_of::<u32>()) as u32;
    ret[1] = MBOX_REQUEST;

    // tag:
    ret[2] = GET_MAX_CLOCK_RATE; // get serial number command
    ret[3] = 8; // value buffer size in bytes
    ret[4] = 8; // :b 31 clear: request, | b31 set: response b30-b0: value length in bytes

    ret[5] = 0x3; // clock id
    ret[6] = 0; // used by the response.
    ret[7] = LAST_TAG;
    Message(ret)
}

const SERIAL_MESSAGE_SIZE: usize = 9;
fn board_serial_message() -> Message<SERIAL_MESSAGE_SIZE> {
    const SERIAL_MESSAGE_TAG: u32 = 0x00010004;
    let mut ret = [0u32; SERIAL_MESSAGE_SIZE];
    ret[0] = (SERIAL_MESSAGE_SIZE * mem::size_of::<u32>()) as u32;
    ret[1] = MBOX_REQUEST;

    ret[2] = SERIAL_MESSAGE_TAG; // tag identifier
    ret[3] = 8; // value buffer size in bytes
    ret[4] = 8; // Request codes:b 31 clear: request
    ret[5] = 8; // clear output buffer
    ret[6] = 0;

    ret[7] = LAST_TAG;
    Message(ret)
}

fn send_message_sync<const T: usize>(mailbox: u32, channel: Channel, message: &Message<T>) -> bool {
    let raw_ptr = message.0.as_ptr();
    // This is needed because slices are fat pointers and I need to convert it to a thin pointer
    // first.
    let raw_ptr_addr = raw_ptr.cast::<usize>();
    let raw_ptr_addr = raw_ptr_addr as usize;
    // !0x0F is 1...10000
    let addr_clear_last_4_bits = raw_ptr_addr.bitand(!0x0F);
    let ch_clear_everything_but_last_4_vits = channel as usize & 0xF;
    let final_addr = addr_clear_last_4_bits | ch_clear_everything_but_last_4_vits;

    let raw_mailbox_ptr = mailbox as *mut RawMailbox;
    let raw_mailbox = unsafe { &mut *raw_mailbox_ptr };

    // wait until we can write to the mailbox
    while raw_mailbox.is_full() {
        core::hint::spin_loop();
    }

    raw_mailbox.write_address(final_addr);

    // now wait for the response
    loop {
        // is there a response?
        while raw_mailbox.is_empty() {
            core::hint::spin_loop();
        }

        if raw_mailbox.get_read() == final_addr as u32 {
            return match message.response_status() {
                ReqResp::Request => {
                    info!("message stll contains a request ?!");
                    false
                }
                ReqResp::ResponseError => {
                    info!("Something failed, the response is an error");
                    false
                }
                ReqResp::ResponseSuccessful => true,
            };
        }
    }
}
#[derive(Copy, Clone)]
pub enum Channel {
    POWER = 0,
    FB = 1,
    VUART = 2,
    VCHIQ = 3,
    LEDS = 4,
    BTNS = 5,
    TOUCH = 6,
    COUNT = 7,
    PROP = 8,
}
