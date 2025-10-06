#![allow(clippy::upper_case_acronyms)]
#![feature(format_args_nl)]
#![feature(trait_alias)]
#![feature(alloc_error_handler)]
#![no_main]
#![no_std]
#![allow(unused)]

extern crate alloc;

mod bsp;
mod console;
mod cpu;
mod driver;
mod fb_trait;
mod framebuffer;
mod hvs;
mod hyperpixel;
mod mailbox;
mod panic_wait;
mod print;
mod synchronization;
mod time;
use alloc::{string::String, vec};

use crate::mailbox::{max_clock_speed, set_clock_speed};
use alloc::{format, vec::Vec};
use bcm2837_hal::*;
use bsp::memory::initialize_heap;
use cogware_can::{cli_wri, Gauge, *};
use core::time::Duration;
use delay::Timer;
use embedded_hal::spi::*;
use embedded_hal_0_2::{
    can::{Frame, Id, StandardId},
    digital::v2::OutputPin,
    prelude::{_embedded_hal_blocking_delay_DelayMs, _embedded_hal_blocking_spi_Transfer},
};
use embedded_sdmmc::{sdcard::EMMCController, time::DummyTimesource, Mode, VolumeManager};
use fb_trait::FrameBufferInterface;
use fugit::RateExtU32;
use gpio::{pin, GpioExt};
use hvs::{Hvs, Plane};
use hyperpixel::HyperPixel;
use mcp2515::{error::Error, frame::CanFrame, regs::OpMode, CanSpeed, McpSpeed, MCP2515};
use pac::{bsc0::a::W, Peripherals};
use spi::spi::{SPI0Device, SPIZero};
// use fb_trait::FrameBufferInterface;
// use framebuffer::FrameBuffer;
static CONFIGGAUGES: [u8; 9] = [0x20, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2D, 0x35, 0x70];
const BOOT_IMAGE_QOI: &[u8] = include_bytes!("CogWare.qoi");

use log::{error, info};

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order.

unsafe fn kernel_init() -> ! {
    print::SimpleLogger::init(log::LevelFilter::Trace).expect("failed to initialize logger!");
    // Initialize the BSP driver subsystem.
    if let Err(x) = bsp::driver::init() {
        panic!("Error initializing BSP driver subsystem: {}", x);
    }
    {
        // let mut u: MaybeUninit<FrameBuffer> = MaybeUninit::uninit();
        driver::driver_manager().init_drivers();
        initialize_heap();
        info!("kernel_init");
        let max_clock_speed = max_clock_speed();
        set_clock_speed(max_clock_speed.unwrap());
    }

    // Transition from unsafe to safe.
    kernel_main()
}

/// The main function running after the early init.
fn kernel_main() -> ! {
    info!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    info!("Booting on: {}", bsp::board_name());

    info!(
        "Architectural timer resolution: {} ns",
        time::time_manager().resolution().as_nanos()
    );

    let mut timer = Timer::new();

    loop {
        info!("Spinning for 1 second");
        time::time_manager().spin_for(Duration::from_secs(1));
    }
}
