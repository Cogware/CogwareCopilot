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

use log::info;

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order.

unsafe fn kernel_init() -> ! {
    // Initialize the BSP driver subsystem.
    if let Err(x) = bsp::driver::init() {
        panic!("Error initializing BSP driver subsystem: {}", x);
    }
    print::SimpleLogger::init(log::LevelFilter::Trace).expect("failed to initialize logger!");
    {
        // let mut u: MaybeUninit<FrameBuffer> = MaybeUninit::uninit();
        driver::driver_manager().init_drivers();
        initialize_heap();
        info!("kernel_init");
        let max_clock_speed = max_clock_speed();
        set_clock_speed(max_clock_speed.unwrap());

        // info!("initializing hvs");
        /*let (header, image) =
            qoi::decode_to_vec(BOOT_IMAGE_QOI).expect("Failed to decode boot image (wtf?)");

        let mut hvs = Hvs::new();
        // this doesn't work
        // why
        // hvs.add_plane(Plane::from_qoi(header, image));
        hvs.add_plane(Plane::white());
        hvs.draw();

        timer.delay_ms(1000);

        hvs.add_plane(Plane::green_half_alpha());
        hvs.draw();

        timer.delay_ms(1000);
        hvs.add_plane(Plane::from_qoi(header, image));
        hvs.draw();

        timer.delay_ms(1000);
        info!("SUGMA");

        // loop {
        //     timer.delay_ns(500_000_000);
        //     hvs.reset();
        //     hvs.add_plane(Plane::from_qoi(header, image.clone()));
        //     hvs.draw();
        //     timer.delay_ns(500_000_000);
        // }*/
        // let mut fb = mailbox::lfb_init(0).expect("Failed to init framebuffer");
        // fb.display_boot_image();
        // let u = u.assume_init();
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

    info!("Initializing EMMC Controller...");
    let mut card = EMMCController::new();
    card.emmc_init_card();

    info!("EMMC Controller initialized!");

    info!("Initializing Volume Manager...");
    let time_source = DummyTimesource::default();
    let mut volume_mgr = VolumeManager::new(card, time_source);
    info!("Volume Manager initialized!");

    info!("Opening Volume 0...");
    let mut volume0 = volume_mgr
        .open_volume(embedded_sdmmc::VolumeIdx(0))
        .expect("Failed to open volume 0");

    info!("Done!");
    info!("Volume 0: {:?}", volume0);

    info!("Opening Volume 0...");
    let mut root_dir = volume0
        .open_root_dir()
        .expect("Failed to open root directory");

    info!("Done!");
    info!("Root directory: {:#?}", root_dir);

    let mut cfg_file = root_dir
        .open_file_in_dir("CONFIG.TXT", Mode::ReadOnly)
        .expect("Failed to open CONFIG.TXT");

    let out = match cfg_file.read_to_string() {
        Ok(f) => f,
        Err(e) => format!("{:?}", e),
    };

    info!("CONFIG.TXT:\n{}", out);

    info!("Drivers loaded:");
    driver::driver_manager().enumerate();

    let mut timer = Timer::new();
    // Test a failing timer case.
    //time::time_manager().spin_for(Duration::from_nanos(1));

    // let peripherals = Peripherals::take().expect("failed to get peripherals");
    // let mut gpio = peripherals.GPIO.split();
    // gpio.pins[9..=11].iter().for_each(|p| {
    //     p.set_mode(gpio::PinMode::AF0);
    // });
    // let mut cs = &mut gpio.pins[27];
    // cs.set_mode(gpio::PinMode::Output);

    // HyperPixel::new(peripherals.GPIO, &mut timer).set_gpio_mode();

    // let mut spi = SPIZero::new(&peripherals.SPI0);
    // spi.init(embedded_hal::spi::MODE_0, 10.MHz());
    // info!("in theory SPI inited");
    //
    // let mut can = MCP2515::new(spi, cs);
    // info!("initing CAN");
    // can.init(
    //     &mut timer,
    //     mcp2515::Settings {
    //         mode: OpMode::Normal,
    //         can_speed: CanSpeed::Kbps1000,
    //         mcp_speed: McpSpeed::MHz16,
    //         clkout_en: false,
    //     },
    // )
    // .unwrap();
    //
    // let masterack = Id::Standard(StandardId::ZERO);
    // let clirequest = Id::Standard(StandardId::new(0x015).expect("bad address"));
    // let mut gaugelisten = Vec::new();
    // for i in CONFIGGAUGES {
    //     gaugelisten.push(i);
    // }
    //
    // for val in &gaugelisten {
    //     'read: loop {
    //         match can.read_message() {
    //             Ok(frame) => {
    //                 if frame.id() == masterack && frame.data()[0] == *val {
    //                     break 'read;
    //                 }
    //             }
    //             Err(Error::NoMessage) => {}
    //             Err(_) => {}
    //         }
    //         let frame = CanFrame::new(clirequest, &[*val]).unwrap();
    //         can.send_message(frame).ok();
    //     }
    // }
    // let mut dispgauge0: String;
    // let mut dispgauge1: String;
    // let mut dispgauge2: String;
    // let mut dispgauge3: String;
    // let mut dispgauge4: String;
    // let mut dispgauge5: String;
    // let mut dispgauge6: String;
    // let mut dispgauge7: String;
    // let mut dispgauge8: String;
    // let mut dispgauge9: String;
    // let mut bingus: u8 = 0;
    loop {
        // let timeout = timer.now() + Duration::from_millis(15);
        // while timer.now() <= timeout {
        //     match can.read_message() {
        //         Ok(frame) => {
        //             // bingles = format!("{:?} {:?}", frame.id(), frame.data());
        //             if let Id::Standard(standard_id) = frame.id() {
        //                 let primitive_id: u16 = standard_id.as_raw();
        //                 if gaugelisten.contains(&primitive_id.try_into().unwrap()) {
        //                     cli_wri(frame, primitive_id);
        //                 }
        //             }
        //         }
        //         Err(Error::NoMessage) => {}
        //         Err(_) => panic!("Oh no!"),
        //     }
        // }
        // let boost = (MAP.get() as f64 * 0.145038) - 14.5038;
        // dispgauge0 = format!("STA: {:?}", STA_TIME.get());
        // dispgauge1 = format!("BOOST: {:.1}", boost);
        // dispgauge2 = format!("IAT: {:?}", ((IAT.get() * 2) - 91));
        // dispgauge3 = format!("CLNT: {:?}", ((CLNT.get() * 2) - 91));
        // dispgauge4 = format!("BATVOL: {:?}", BAT_VOL.get());
        // dispgauge5 = format!("AFR: {:?}", (AFR_PRI.get() as f64 / 10.00));
        // dispgauge6 = format!("RPM: {:?}", RPM.get());
        // dispgauge7 = format!("TPS: {:?}", TPS.get());
        // dispgauge8 = format!("CliAlive: {:?}", bingus);
        // dispgauge9 = format!("ServAli: {:?}", MASTERALIVE.get());
        // bingus = bingus.wrapping_add(1);
        // info!("{:?}", dispgauge0);
        // info!("{:?}", dispgauge1);
        // info!("{:?}", dispgauge2);
        // info!("{:?}", dispgauge3);
        // info!("{:?}", dispgauge4);
        // info!("{:?}", dispgauge5);
        // info!("{:?}", dispgauge6);
        // info!("{:?}", dispgauge7);
        // info!("{:?}", dispgauge8);
        // info!("{:?}", dispgauge9);
        info!("Spinning for 1 second");
        time::time_manager().spin_for(Duration::from_secs(1));
    }
}
