//! The VibeCodedOs kernel. Entered from the loader via `_start` (start.rs).

#![no_std]
#![no_main]

mod port;
mod serial;
mod start;

use boot_info::{BootInfo, MemoryKind};
use core::fmt::Write;
use serial::Serial;

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    let mut serial = Serial::com1();
    assert_eq!(
        boot_info.magic,
        boot_info::MAGIC,
        "BootInfo pointer is garbage"
    );

    // Serial output can't fail, so the `fmt::Result`s are ignored throughout.
    let _ = writeln!(
        serial,
        "\nVibeCodedOs kernel: the firmware is gone, the machine is ours."
    );
    print_memory_summary(&mut serial, boot_info);
    let _ = writeln!(serial, "VibeCodedOs kernel ready.");

    halt_forever()
}

fn print_memory_summary(out: &mut Serial, boot_info: &BootInfo) {
    let regions = boot_info.memory_regions();
    let _ = writeln!(out, "Physical memory ({} regions):", regions.len());
    for kind in MemoryKind::ALL {
        let bytes: u64 = regions
            .iter()
            .filter(|region| region.kind == kind)
            .map(|region| region.size())
            .sum();
        if bytes > 0 {
            let _ = writeln!(out, "  {:<22}{:>9} KiB", kind.name(), bytes / 1024);
        }
    }
}

/// Stops the CPU for good. Interrupts are off, so `hlt` never wakes up –
/// the loop is only there in case of a non-maskable interrupt.
fn halt_forever() -> ! {
    loop {
        // SAFETY: `hlt` only pauses the CPU; it touches no memory.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Serial::com1(), "\nKERNEL PANIC: {info}");
    halt_forever()
}
