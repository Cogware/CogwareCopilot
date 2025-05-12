#![allow(clippy::upper_case_acronyms)]
#![feature(asm_const)]
#![feature(const_option)]
#![feature(format_args_nl)]
#![feature(trait_alias)]
#![feature(alloc_error_handler)]
#![no_main]
#![no_std]

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
use framebuffer::FrameBuffer;
use tinybmp::Bmp;
use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;

use crate::mailbox::{max_clock_speed, set_clock_speed};
use alloc::{format, vec::Vec};
use bcm2837_hal::*;
use bsp::memory::initialize_heap;
use core::{num::Wrapping, time::Duration};
use delay::Timer;
use embedded_hal::spi::*;
use embedded_sdmmc::{sdcard::EMMCController, time::DummyTimesource, Mode, VolumeManager};
use fb_trait::{Color, Coordinates, FrameBufferInterface, WHITE_COLOR};
use fugit::RateExtU32;
use gpio::{pin, GpioExt};
use hvs::{Hvs, Plane};
use hyperpixel::HyperPixel;
use pac::{bsc0::a::W, Peripherals};
use spi::spi::{SPI0Device, SPIZero};
use mcp2515::{error::Error, frame::CanFrame, regs::OpMode, CanSpeed, McpSpeed, MCP2515};
use embedded_hal_0_2::{can::{Frame, Id, StandardId}, digital::v2::OutputPin, prelude::{_embedded_hal_blocking_delay_DelayMs, _embedded_hal_blocking_spi_Transfer}};
use cogware_can::{cli_wri, Gauge, *};
use embedded_graphics::{framebuffer::*, image::Image, pixelcolor::{raw::{LittleEndian, RawU24}, Rgb666}, prelude::Point};

const BOOT_IMAGE_BMP: &'static [u8] = include_bytes!("CogWare4802.bmp");
// use fb_trait::FrameBufferInterface;
// use framebuffer::FrameBuffer;
//static CONFIGGAUGES: [u8; 9] = [0x20, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2D, 0x35, 0x70];
//const BOOT_IMAGE_QOI: &[u8] = include_bytes!("CogWare.qoi");

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
    {
        // let mut u: MaybeUninit<FrameBuffer> = MaybeUninit::uninit();
        driver::driver_manager().init_drivers();
        initialize_heap();
        info!("kernel_init");
        let max_clock_speed = max_clock_speed();
        set_clock_speed(max_clock_speed.unwrap());

        //info!("initializing hvs");
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

        // let u = u.assume_init();

    }
    let mut screen = mailbox::lfb_init(0).expect("Failed to init framebuffer");
    screen.display_boot_image();


    // Transition from unsafe to safe.
    kernel_main(screen)
}

/// The main function running after the early init.
fn kernel_main(mut screen: FrameBuffer) -> ! {

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

    // Test a failing timer case.
    //time::time_manager().spin_for(Duration::from_nanos(1));
    // HyperPixel::new(peripherals.GPIO, &mut timer).set_gpio_mode();
    info!("loading BMP");
    let bmp = Bmp::from_slice(BOOT_IMAGE_BMP).unwrap();
    info!("building image");
    
    screen.clear_screen();
    
    let mut bingus:u8 = 0;

    loop {
        let starttime = time::time_manager().uptime();

        let image = Image::new(&bmp, Point::new(bingus as i32,0));
        //info!("Frame");
        //time::time_manager().spin_for(Duration::from_secs(1));
        screen.clear_screen();
        //screen.draw_rect_fill(&Coordinates::new(0, 0), 480, 480, WHITE_COLOR);

        let drawtime1 = time::time_manager().uptime();
        image.draw(&mut screen).unwrap();
        info!("drawtime: {:?}", time::time_manager().uptime() - drawtime1);

        screen.update();
        bingus = bingus.wrapping_add(1);

        info!("frametime: {:?}", time::time_manager().uptime() - starttime);

    }
}