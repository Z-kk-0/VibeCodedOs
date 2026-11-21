//! UEFI boot loader. The firmware loads this file from
//! `\EFI\BOOT\BOOTX64.EFI` and calls `efi_main`.
//!
//! Job: load the embedded kernel, build its page tables, leave the firmware,
//! jump into the kernel.

// No standard library (it needs an OS) and no normal `main`
// (the firmware calls `efi_main` directly).
#![no_std]
#![no_main]

mod address_space;
mod console;
mod handoff;
mod kernel_image;
mod screen;
mod uefi;
mod window;

use console::Console;
use core::fmt::Write;
use handoff::Platform;
use uefi::{ACPI_10_TABLE, ACPI_20_TABLE, BootServices, Handle, MemoryType, Status, SystemTable};

/// The kernel, baked into this binary at compile time (DECISIONS.md, 004).
static KERNEL_ELF: &[u8] = include_bytes!(env!("HEMATITE_KERNEL_ELF"));

/// Entry point. The firmware passes our own image handle and the system table.
/// The name `efi_main` is what the Rust UEFI target expects as entry symbol.
#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(image: Handle, system_table: *mut SystemTable) -> Status {
    // SAFETY: The firmware guarantees the system table and everything it
    // points to stay valid until we call ExitBootServices.
    let (system_table, mut console, boot_services) = unsafe {
        let system_table = &*system_table;
        (
            system_table,
            Console::new(system_table.console_out),
            &*system_table.boot_services,
        )
    };

    console.clear();
    // Ignoring print results: if printing fails there is nowhere to report it.
    let _ = writeln!(console, "Hematite loader");

    let kernel = kernel_image::load(boot_services, KERNEL_ELF)
        .unwrap_or_else(|message| fail(&mut console, message));
    let _ = writeln!(
        console,
        "Kernel: {} KiB at physical {:#x}, will run at {:#x}.",
        kernel.size / 1024,
        kernel.physical_start,
        kernel.entry
    );

    let platform = find_platform(system_table, boot_services);
    let _ = writeln!(
        console,
        "ACPI root at {:#x}, core startup pages at {:#x}.",
        platform.acpi_root, platform.core_startup
    );
    let screen = platform.framebuffer;
    if screen.exists() {
        let _ = writeln!(
            console,
            "Screen: {}x{} pixels ({:?}), framebuffer at {:#x}.",
            screen.width, screen.height, screen.format, screen.address
        );
    } else {
        let _ = writeln!(console, "No screen we can draw on.");
    }

    let handoff =
        handoff::prepare(boot_services).unwrap_or_else(|message| fail(&mut console, message));
    let address_space =
        address_space::build(boot_services, &kernel, &handoff, &platform.framebuffer)
            .unwrap_or_else(|message| fail(&mut console, message));
    let _ = writeln!(
        console,
        "Page tables: {} tables, physical memory window {} GiB ({} MiB device memory uncached, {} KiB framebuffer left out). Leaving the firmware...",
        address_space.tables_used,
        address_space.window_size >> 30,
        address_space.window.uncached_size >> 20,
        address_space.window.left_out_size >> 10
    );

    match handoff.finish(boot_services, image, console, platform) {
        // SAFETY: boot services are gone, and the address space maps the
        // kernel plus everything the loader still runs on.
        Ok(boot_info) => unsafe { kernel.start(&address_space, boot_info) },
        // The console is gone at this point; nothing left to tell anyone.
        Err(_) => halt_forever(),
    }
}

/// What the kernel needs to know about the machine besides its memory. None
/// of it is fatal if missing: without ACPI or startup pages the kernel runs
/// on one core (0 = "none"), without a screen it draws nowhere.
fn find_platform(system_table: &SystemTable, boot_services: &BootServices) -> Platform {
    let acpi_root = system_table
        .find_configuration_table(ACPI_20_TABLE)
        .or_else(|| system_table.find_configuration_table(ACPI_10_TABLE))
        .unwrap_or(0);
    let core_startup = boot_services
        .allocate_pages_below(
            MemoryType::HEMATITE_CORE_STARTUP,
            boot_info::CORE_STARTUP_PAGES as usize,
            boot_info::CORE_STARTUP_LIMIT,
        )
        .unwrap_or(0);
    Platform {
        acpi_root,
        core_startup,
        framebuffer: screen::find(boot_services),
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
