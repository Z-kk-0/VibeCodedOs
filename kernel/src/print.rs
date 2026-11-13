//! `print!` / `println!` for the kernel, writing to the serial port.
//!
//! One `print!` call is never torn apart by another task: output goes
//! through a lock. Code that may run while that lock is held – the panic
//! handler, exception reports – uses the `emergency_` variants instead,
//! which skip the lock (and may interleave, but never deadlock).

use crate::serial::Serial;
use crate::sync::SpinLock;
use core::fmt::{self, Write};

static OUTPUT: SpinLock<()> = SpinLock::new(());

pub fn print(arguments: fmt::Arguments) {
    let _guard = OUTPUT.lock();
    // Serial output cannot fail, so the `fmt::Result` is ignored.
    let _ = Serial::com1().write_fmt(arguments);
}

pub fn print_emergency(arguments: fmt::Arguments) {
    let _ = Serial::com1().write_fmt(arguments);
}

#[allow(unused_macros, reason = "counterpart of println!, not needed yet")]
macro_rules! print {
    ($($argument:tt)*) => {
        $crate::print::print(format_args!($($argument)*))
    };
}

macro_rules! println {
    () => { print!("\n") };
    ($($argument:tt)*) => {
        $crate::print::print(format_args!("{}\n", format_args!($($argument)*)))
    };
}

macro_rules! emergency_print {
    ($($argument:tt)*) => {
        $crate::print::print_emergency(format_args!($($argument)*))
    };
}

macro_rules! emergency_println {
    () => { emergency_print!("\n") };
    ($($argument:tt)*) => {
        $crate::print::print_emergency(format_args!("{}\n", format_args!($($argument)*)))
    };
}
