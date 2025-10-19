// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

//! Printing.

use crate::console;
use core::fmt;

use log::LevelFilter;

pub struct SimpleLogger;
static GLOBAL_LOGGER: SimpleLogger = SimpleLogger;

impl SimpleLogger {
    pub fn init(max_level: LevelFilter) -> Result<(), log::SetLoggerError> {
        unsafe { log::set_logger_racy(&GLOBAL_LOGGER).map(|()| log::set_max_level_racy(max_level)) }
    }
}

impl log::Log for SimpleLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let timestamp = crate::time::time_manager().uptime();
        _print(format_args!(
            "\x1b[2;37m[  {:>3}.{:06}]\x1b[0m {}{:<5}\x1b[0m {}\n",
            timestamp.as_secs(),
            timestamp.subsec_micros(),
            level_color(record.level()),
            record.level(),
            record.args()
        ));
    }

    fn flush(&self) {
        console::console().flush();
    }
}

fn level_color(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "\x1b[31m",
        log::Level::Warn => "\x1b[33m",
        log::Level::Info => "\x1b[32m",
        log::Level::Debug => "\x1b[36m",
        log::Level::Trace => "\x1b[2;37m",
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    console::console().write_fmt(args).unwrap();
}

/// Prints without a newline.
///
/// Carbon copy from <https://doc.rust-lang.org/src/std/macros.rs.html>
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)));
}

/// Prints with a newline.
///
/// Carbon copy from <https://doc.rust-lang.org/src/std/macros.rs.html>
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ({
        $crate::print::_print(format_args_nl!($($arg)*));
    })
}

/// Prints an info, with a newline.
#[macro_export]
macro_rules! info {
    ($string:expr) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::print::_print(format_args_nl!(
            concat!("[  {:>3}.{:06}] ", $string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
        ));
    });
    ($format_string:expr, $($arg:tt)*) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::print::_print(format_args_nl!(
            concat!("[  {:>3}.{:06}] ", $format_string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
            $($arg)*
        ));
    })
}

/// Prints a warning, with a newline.
#[macro_export]
macro_rules! warn {
    ($string:expr) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::print::_print(format_args_nl!(
            concat!("[W {:>3}.{:06}] ", $string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
        ));
    });
    ($format_string:expr, $($arg:tt)*) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::print::_print(format_args_nl!(
            concat!("[W {:>3}.{:06}] ", $format_string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
            $($arg)*
        ));
    })
}
