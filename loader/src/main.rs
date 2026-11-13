//! UEFI boot loader. The firmware loads this file from
//! `\EFI\BOOT\BOOTX64.EFI` and calls `efi_main`.

// No standard library (it needs an OS) and no normal `main`
// (the firmware calls `efi_main` directly).
#![no_std]
#![no_main]

mod console;
mod uefi;

use console::Console;
use core::fmt::Write;
use uefi::{Handle, Status, SystemTable};

/// Entry point. The firmware passes our own image handle and the system table.
/// The name `efi_main` is what the Rust UEFI target expects as entry symbol.
#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(_image: Handle, system_table: *mut SystemTable) -> Status {
    // SAFETY: The firmware guarantees `system_table` and its `console_out`
    // are valid while boot services are active, which is all of our runtime.
    let mut console = unsafe { Console::new((*system_table).console_out) };

    console.clear();
    // Ignoring the result: if printing fails there is nowhere to report it.
    let _ = writeln!(console, "Hello from VibeCodedOs!");
    let _ = writeln!(console, "Running as a UEFI application.");

    halt_forever()
}

/// Stops the CPU until the next interrupt, forever.
fn halt_forever() -> ! {
    loop {
        // SAFETY: `hlt` only pauses the CPU until the next interrupt;
        // it touches no memory.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    halt_forever()
}
