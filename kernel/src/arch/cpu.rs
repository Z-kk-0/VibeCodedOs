//! Small wrappers around single CPU instructions.

use core::arch::asm;

/// Stops the CPU for good: interrupts off, then `hlt`. The loop only
/// matters for non-maskable interrupts, which can still wake it.
pub fn halt_forever() -> ! {
    loop {
        // SAFETY: `cli` and `hlt` touch no memory.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

/// Allows hardware interrupts (the `sti` instruction).
///
/// # Safety
/// Every vector that can fire must have a working handler.
pub unsafe fn enable_interrupts() {
    // SAFETY: forwarded to the caller.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// Sleeps until the next interrupt arrives. Interrupts must be enabled,
/// otherwise this never returns.
pub fn wait_for_interrupt() {
    // SAFETY: `hlt` only pauses the CPU; it touches no memory.
    unsafe { asm!("hlt", options(nomem, nostack)) };
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

/// Reads a model-specific register.
///
/// # Safety
/// `msr` must exist on this CPU, otherwise it raises a General Protection Fault.
pub unsafe fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    // SAFETY: forwarded to the caller.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    u64::from(high) << 32 | u64::from(low)
}

/// Writes a model-specific register.
///
/// # Safety
/// `msr` must exist, and `value` must be valid for it – MSRs control the CPU.
pub unsafe fn write_msr(msr: u32, value: u64) {
    // SAFETY: forwarded to the caller.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nostack, preserves_flags),
        );
    }
}
