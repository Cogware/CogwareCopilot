#![cfg_attr(not(test), no_std)]
#![allow(missing_docs)]
#![allow(unused)]
pub use bcm2837_lpa as pac;
#[cfg(feature = "critical-section-impl")]
mod critical_section;

pub mod delay;
pub mod gpio;
pub mod interrupt;
pub(crate) mod macros;
pub mod spi;

pub use embedded_hal::delay::DelayNs;
