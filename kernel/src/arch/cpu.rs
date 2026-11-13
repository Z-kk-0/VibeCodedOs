//! Small wrappers around single CPU instructions.

use core::arch::asm;

/// Stops the CPU for good. Interrupts are off, so `hlt` only wakes up
/// for a non-maskable interrupt – hence the loop.
pub fn halt_forever() -> ! {
    loop {
        // SAFETY: `hlt` only pauses the CPU; it touches no memory.
        unsafe { asm!("hlt", options(nomem, nostack)) };
    }
}

/// Triggers the breakpoint exception (#3) – a harmless round trip
/// through the whole interrupt path.
pub fn breakpoint() {
    // SAFETY: our IDT handles vector 3 and returns right after `int3`.
    unsafe { asm!("int3", options(nomem, nostack)) };
}

/// CR2 holds the address that caused the most recent page fault.
pub fn page_fault_address() -> u64 {
    let address;
    // SAFETY: reading CR2 has no side effects.
    unsafe { asm!("mov {}, cr2", out(reg) address, options(nomem, nostack, preserves_flags)) };
    address
}
