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

/// Turns interrupts off and reports whether they were on before, so the
/// caller can restore the previous state with `enable_interrupts`.
pub fn disable_interrupts() -> bool {
    const INTERRUPT_FLAG: u64 = 1 << 9; // in RFLAGS, Intel SDM Vol. 1, §3.4.3
    let flags: u64;
    // SAFETY: reads RFLAGS via the stack and clears IF; no other effects.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem)) };
    flags & INTERRUPT_FLAG != 0
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

/// Physical address of the active top-level page table (from CR3).
pub fn page_table_root() -> u64 {
    const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    let cr3: u64;
    // SAFETY: reading CR3 has no side effects.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    cr3 & ADDRESS_MASK
}

/// Makes the CPU forget cached translations (the TLB) by reloading CR3.
/// Needed after removing or changing mappings.
pub fn flush_translation_cache() {
    // SAFETY: writes back the value CR3 already has.
    unsafe {
        asm!("mov {0}, cr3", "mov cr3, {0}", out(reg) _, options(nostack, preserves_flags));
    }
}

/// Makes the CPU forget the cached translation of one page (`invlpg`).
pub fn flush_translation(virtual_address: u64) {
    // SAFETY: only drops a cached translation; the next access re-reads the tables.
    unsafe { asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags)) };
}

/// Whether the no-execute bit in page tables is active (EFER.NXE).
pub fn no_execute_enabled() -> bool {
    const EFER: u32 = 0xC000_0080;
    const NO_EXECUTE_ENABLE: u64 = 1 << 11;
    // SAFETY: EFER exists on every x86_64 CPU.
    unsafe { read_msr(EFER) & NO_EXECUTE_ENABLE != 0 }
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
