//! UEFI boot loader. The firmware loads this file from
//! `\EFI\BOOT\BOOTX64.EFI` and calls `efi_main`.
//!
//! Job: load the embedded kernel, leave the firmware, jump into the kernel.

// No standard library (it needs an OS) and no normal `main`
// (the firmware calls `efi_main` directly).
#![no_std]
#![no_main]

mod console;
mod handoff;
mod kernel_image;
mod uefi;

use console::Console;
use core::fmt::Write;
use uefi::{Handle, Status, SystemTable};

/// The kernel, baked into this binary at compile time (DECISIONS.md, 004).
static KERNEL_ELF: &[u8] = include_bytes!(env!("HEMATITE_KERNEL_ELF"));

/// Entry point. The firmware passes our own image handle and the system table.
/// The name `efi_main` is what the Rust UEFI target expects as entry symbol.
#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(image: Handle, system_table: *mut SystemTable) -> Status {
    // SAFETY: The firmware guarantees the system table and everything it
    // points to stay valid until we call ExitBootServices.
    let (mut console, boot_services) = unsafe {
        let system_table = &*system_table;
        (
            Console::new(system_table.console_out),
            &*system_table.boot_services,
        )
    };

    console.clear();
    // Ignoring print results: if printing fails there is nowhere to report it.
    let _ = writeln!(console, "Hematite loader");

    let kernel = match kernel_image::load(boot_services, KERNEL_ELF) {
        Ok(kernel) => kernel,
        Err(message) => fail(&mut console, message),
    };
    let _ = writeln!(
        console,
        "Kernel loaded at {:#x} ({} KiB), entry {:#x}. Leaving the firmware...",
        kernel.physical_start,
        kernel.size / 1024,
        kernel.entry
    );

    match handoff::exit_boot_services(boot_services, image, console) {
        Ok(boot_info) => kernel.start(boot_info),
        // The console is gone at this point; nothing left to tell anyone.
        Err(_) => halt_forever(),
    }
}

fn fail(console: &mut Console, message: &str) -> ! {
    let _ = writeln!(console, "Boot failed: {message}");
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
