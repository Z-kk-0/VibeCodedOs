//! x86 I/O ports: a separate 16-bit address space reached with the
//! `in`/`out` instructions. Old PC devices like the serial port live there.

use core::arch::asm;

/// # Safety
/// Writing to a port does whatever the device behind it does with the value.
/// The caller must know which device sits at `port`.
pub unsafe fn write_u8(port: u16, value: u8) {
    // SAFETY: forwarded to the caller.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

/// Gives slow old devices time to process the previous port write, by
/// writing to port 0x80 (POST diagnostics; nothing listens there).
///
/// # Safety
/// Only meaningful on PC-compatible machines, where port 0x80 is unused.
pub unsafe fn wait_a_moment() {
    // SAFETY: forwarded to the caller.
    unsafe { write_u8(0x80, 0) }
}

/// # Safety
/// Reading a port can have side effects on the device (e.g. pop a FIFO).
pub unsafe fn read_u8(port: u16) -> u8 {
    let value;
    // SAFETY: forwarded to the caller.
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}
