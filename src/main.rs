// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

// Rust embedded logo for `make doc`.
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/rust-embedded/wg/master/assets/logo/ewg-logo-blue-white-on-transparent.png"
)]

//! The `kernel` binary.

#![no_main]
#![no_std]

use core::num::NonZeroU32;

use gl::{i12f4, I12F4};
use libkernel::{bsp, cpu, driver, exception, memory, state};

use gl::mem::mapper::IdentityMapper;
use gl::{
    control_list::{fixed::RenderControlList, *},
    mem::{mapper::MemoryMapper, ArmAddress},
    nv::shader::NvShaderFlags,
    register::*,
    Align16,
};
use gl::{mem::BusAddress, nv::primitive::*};

const SCREEN_WIDTH: u16 = 1920;
const SCREEN_HEIGHT: u16 = 1080;
const TILE_WIDTH: usize = (SCREEN_WIDTH as usize).div_ceil(64);
const TILE_HEIGHT: usize = (SCREEN_HEIGHT as usize).div_ceil(64);

const BINNING_BUFFER_SIZE: usize = 0x100_000;

/// Binning buffer in .bss — must NOT be on the stack because the V3D DMA
/// writes to it via bus addresses and can corrupt adjacent stack frames.
static mut BINNING_BUFFER: Align16<[u8; BINNING_BUFFER_SIZE]> =
    Align16::new([0u8; BINNING_BUFFER_SIZE]);

const TILE_STATE_SIZE: usize = 48 * (TILE_WIDTH * TILE_HEIGHT);

static mut TILE_STATE_BUFFER: Align16<[u8; TILE_STATE_SIZE]> = Align16::new([0u8; TILE_STATE_SIZE]);

extern crate quiche_gl as gl;

use log::{debug, info};

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order:
///     - MMU + Data caching must be activated at the earliest. Without it, any atomic operations,
///       e.g. the yet-to-be-introduced spinlocks in the device drivers (which currently employ
///       IRQSafeNullLocks instead of spinlocks), will fail to work (properly) on the RPi SoCs.
#[no_mangle]
unsafe fn kernel_init() -> ! {
    exception::handling_init();

    _ = libkernel::print::SimpleLogger::init(log::LevelFilter::Trace);

    let phys_kernel_tables_base_addr = match memory::mmu::kernel_map_binary() {
        Err(string) => panic!("Error mapping kernel binary: {}", string),
        Ok(addr) => addr,
    };

    if let Err(e) = memory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) {
        panic!("Enabling MMU failed: {}", e);
    }

    // Initialize the BSP driver subsystem.
    let vcmail = bsp::driver::init().unwrap();

    // Initialize all device drivers.
    driver::driver_manager().init_drivers_and_irqs();

    //set_clock_speed(newmailbox, max_clock_speed.unwrap());

    // Unmask interrupts on the boot CPU core.
    exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    state::state_manager().transition_to_single_core_main();

    // Transition from unsafe to safe.
    kernel_main(vcmail)
}

/// The main function running after the early init.
fn kernel_main(_vcmail: usize) -> ! {
    info!("{}", libkernel::version());
    info!("Booting on: {}", bsp::board_name());

    info!("MMU online!");
    //memory::mmu::kernel_print_mappings();

    //let (_, privilege_level) = exception::current_privilege_level();
    //info!("Current privilege level: {}", privilege_level);

    //info!("Exception handling state:");
    //exception::asynchronous::print_state();

    //info!(
    //    "Architectural timer resolution: {} ns",
    //    time::time_manager().resolution().as_nanos()
    //);

    //info!("Registered IRQ handlers:");
    //exception::asynchronous::irq_manager().print_handler();

    // QuicheGL initialization
    info!("Initializing QuicheGL context");
    let mut gl_ctx = gl::Context::new_identity_mapped(SCREEN_WIDTH as _, SCREEN_HEIGHT as _, 32)
        .initialize(NonZeroU32::new(2).unwrap())
        .expect("failed to initialize QuicheGL context!");
    info!("QuicheGL context initialized!");

    // V3D hardware init: verify access, reset threads, clear caches
    unsafe {
        let ident0 = V3D_IDENT0.read_volatile();
        info!("V3D_IDENT0: {:#010X}", ident0);

        // Reset CL executor threads to clear any stale state/semaphores
        V3D_CT0CS.write_volatile(1 << 15);
        V3D_CT1CS.write_volatile(1 << 15);

        // Clear V3D L2 cache
        V3D_L2CACTL.write_volatile(4);
        // Clear all slice caches
        V3D_SLCACTL.write_volatile(0x0F0F_0F0F);

        debug!(
            "CT0CS: {:#010X}, CT1CS: {:#010X}",
            V3D_CT0CS.read_volatile(),
            V3D_CT1CS.read_volatile()
        );
    }
    //info!("Context: {:#?}", gl_ctx);

    info!(
        "Screen: {}x{}, tiles: {}x{} ({})",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        TILE_WIDTH,
        TILE_HEIGHT,
        TILE_WIDTH * TILE_HEIGHT
    );

    let bin_buf_ptr = &raw mut BINNING_BUFFER;

    debug!("Building binning mode config");
    let bin_buf_bus = unsafe { BusAddress::from_ptr(bin_buf_ptr) };
    // Tile state data array: 48 bytes per tile, at offset 0x1000 from allocation base
    // Tile allocation memory (sub-lists): starts at bin_buf_bus
    // These MUST NOT overlap or the binner corrupts memory
    let tile_state_bus = unsafe { BusAddress::from_ptr(&raw mut TILE_STATE_BUFFER) };
    debug!(
        "Binning buffer @ {:p}, bus: {:#010X}, tile_state: {:#010X}",
        bin_buf_ptr,
        bin_buf_bus.addr(),
        tile_state_bus.addr()
    );
    let bin_mode_config = gl::control_list::TileBinningModeConfig {
        address: bin_buf_bus,
        size: BINNING_BUFFER_SIZE as u32,
        base_address: tile_state_bus,
        tile_width: TILE_WIDTH as u8,
        tile_height: TILE_HEIGHT as u8,
        flags: TileBinningModeFlags::AutoInitialiseTileStateDataArray,
    };

    debug!("Building clip config");
    let clip_config = gl::control_list::ClipWindowConfig {
        left: 0,
        bottom: 0,
        width: SCREEN_WIDTH,
        height: SCREEN_HEIGHT,
    };

    debug!("Building tile binning config");
    let bin_config = TileBinningConfig {
        data8: TileBinningFlags8::EnableForwardFacingPrimitive
            | TileBinningFlags8::EnableReverseFacingPrimitive,
        data16: TileBinningFlags16::DepthTestFunctionAlways
            | TileBinningFlags16::EarlyZUpdatesEnable,
    };

    debug!("Rendering rainbow triangle");
    let verts = RAINBOW_TRI;
    let vert_list = RAINBOW_TRI_INDICES;

    // debug!("Rendering rainbow quad");
    // let verts = RAINBOW_QUAD;
    // let vert_list = RAINBOW_QUAD_INDICES;

    debug!("Building NV shader state");
    let shader_ptr = gl::shader::VERTEX_COLOR_FRAG_SHADER.as_ptr();
    debug!(
        "Shader code ptr: {:p} (aligned: {})",
        shader_ptr,
        (shader_ptr as usize).is_multiple_of(8)
    );
    let nv_shader_state = unsafe {
        gl::nv::shader::NvShaderState::new_aligned(
            NvShaderFlags::empty(),
            6 * 4,
            0,
            3,
            BusAddress::from_ptr(shader_ptr),
            BusAddress::from_ptr(core::ptr::null::<()>()),
            BusAddress::from_ptr(&verts),
        )
    };

    debug!("Building primitive command");
    let vert_list_bus = unsafe { BusAddress::from_ptr(&vert_list) };
    let primitives = IndexedPrimitiveList::new(
        IndexedPrimitiveListFlags::TriangleFan | IndexedPrimitiveListFlags::IndexType8,
        vert_list.len() as _,
        vert_list_bus,
        2,
    );

    debug!("NV shader state @ {:p}", &nv_shader_state);
    debug!(
        "Shader code   @ {:p}",
        gl::shader::VERTEX_COLOR_FRAG_SHADER.as_ptr()
    );
    debug!("Vertex data   @ {:p}", &verts);
    debug!("Vertex stride = {}", 6 * 4);

    const OFFSET_X: I12F4 = i12f4!(SCREEN_HEIGHT / 2);
    const OFFSET_Y: I12F4 = i12f4!(SCREEN_HEIGHT / 2);

    let mapper = IdentityMapper;
    debug!("Building binning list");
    let bin_list = TileBinningControlList::new(
        bin_mode_config,
        clip_config,
        bin_config,
        ViewportOffset {
            x: OFFSET_X,
            y: OFFSET_Y,
        },
        mapper
            .virt_to_phys_addr(ArmAddress::from(&raw const nv_shader_state))
            .unwrap()
            .into(),
        primitives,
    );

    debug!("Building render mode config");
    let fb_ptr = gl_ctx.curr_screen_buffer_ptr();
    debug!("Framebuffer ptr: {:p}", fb_ptr);
    let framebuf_bus_addr = unsafe { BusAddress::from_ptr(fb_ptr) };
    debug!("Framebuffer bus addr: {:#010X}", framebuf_bus_addr.addr());
    let rend_mode_config = TileRenderingModeConfig {
        address: framebuf_bus_addr,
        width: SCREEN_WIDTH,
        height: SCREEN_HEIGHT,
        flags: TileRenderingModeFlags::FrameBufferColorFormatRGBA8888,
    };
    debug!("Building render list");

    let render_list = RenderControlList::<TILE_WIDTH, TILE_HEIGHT>::new(
        ClearColors {
            color: Color::rgba8(0x64, 0x95, 0xED, 0xFF),
            clear_zs_vg_mask: 0,
            clear_stencil: 0,
        },
        rend_mode_config,
        bin_buf_bus,
    );

    info!("Running control list...");
    gl_ctx
        .run_bin_control_list(core::pin::pin!(bin_list))
        .expect("failed to run binning control list");

    info!("Running rendering list...");
    gl_ctx
        .run_render_control_list(core::pin::pin!(render_list))
        .expect("failed to run binning control list");

    info!("Well, we got here. Is there anything on the screen?");

    info!("Echoing input now");
    cpu::wait_forever();
}

const SIZE: i16 = (SCREEN_HEIGHT / 4) as _;

const TOP: i16 = -SIZE;
const LEFT: i16 = -SIZE;
const RIGHT: i16 = SIZE;
const BOTTOM: i16 = SIZE;

#[allow(unused)]
const RAINBOW_TRI_INDICES: [u8; 3] = [0, 1, 2];
#[allow(unused)]
const RAINBOW_TRI: [ShadedVertex<VertexColor>; 3] = [
    ShadedVertex::new(
        FixedVec2::new(0, TOP),
        1.0,
        1.0,
        VertexColor {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
    ),
    ShadedVertex::new(
        FixedVec2::new(RIGHT, BOTTOM),
        1.0,
        1.0,
        VertexColor {
            r: 0.0,
            g: 1.0,
            b: 0.0,
        },
    ),
    ShadedVertex::new(
        FixedVec2::new(LEFT, BOTTOM),
        1.0,
        1.0,
        VertexColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        },
    ),
];

#[allow(unused)]
const RAINBOW_QUAD_INDICES: [u8; 4] = [0, 1, 2, 3];
#[allow(unused)]
const RAINBOW_QUAD: [ShadedVertex<VertexColor>; 4] = [
    ShadedVertex::new(
        FixedVec2::new(LEFT, TOP),
        1.0,
        1.0,
        VertexColor {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
    ),
    ShadedVertex::new(
        FixedVec2::new(LEFT, BOTTOM),
        1.0,
        1.0,
        VertexColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        },
    ),
    ShadedVertex::new(
        FixedVec2::new(RIGHT, BOTTOM),
        1.0,
        1.0,
        VertexColor {
            r: 0.0,
            g: 1.0,
            b: 1.0,
        },
    ),
    ShadedVertex::new(
        FixedVec2::new(RIGHT, TOP),
        1.0,
        1.0,
        VertexColor {
            r: 0.0,
            g: 1.0,
            b: 0.0,
        },
    ),
];
