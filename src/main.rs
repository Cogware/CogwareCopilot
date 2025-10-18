// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

// Rust embedded logo for `make doc`.
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/rust-embedded/wg/master/assets/logo/ewg-logo-blue-white-on-transparent.png"
)]

//! The `kernel` binary.

#![feature(format_args_nl)]
#![no_main]
#![no_std]

use libkernel::{
    bsp, cpu, driver, exception, info,
    mailbox::{max_clock_speed, set_clock_speed, Mailboxaddr},
    memory, state, time,
};

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

    let phys_kernel_tables_base_addr = match memory::mmu::kernel_map_binary() {
        Err(string) => panic!("Error mapping kernel binary: {}", string),
        Ok(addr) => addr,
    };

    if let Err(e) = memory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) {
        panic!("Enabling MMU failed: {}", e);
    }

    memory::mmu::post_enable_init();

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
fn kernel_main(vcmail: usize) -> ! {
    info!("{}", libkernel::version());
    info!("Booting on: {}", bsp::board_name());

    info!("MMU online:");
    memory::mmu::kernel_print_mappings();
    info!("VC Mailbox Addr: {:x}", vcmail);

    let newmailbox = Mailboxaddr::new(vcmail);
    let _max_clock_speed = max_clock_speed(newmailbox);

    let (_, privilege_level) = exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    info!("Exception handling state:");
    exception::asynchronous::print_state();

    info!(
        "Architectural timer resolution: {} ns",
        time::time_manager().resolution().as_nanos()
    );

    info!("Drivers loaded:");
    driver::driver_manager().enumerate();

    info!("Registered IRQ handlers:");
    exception::asynchronous::irq_manager().print_handler();

    info!("Echoing input now");
    cpu::wait_forever();
}
