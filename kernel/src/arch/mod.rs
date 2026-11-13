//! Everything x86_64-specific. The rest of the kernel should not need
//! to know which CPU it runs on.

pub mod cpu;
mod gdt;
mod idt;
mod interrupts;
pub mod port;
mod start;

/// Installs our own segment and interrupt tables, replacing the firmware's.
pub fn init() {
    // SAFETY: called exactly once, early, with interrupts disabled
    // (start.rs does `cli`), before anything relies on the old tables.
    unsafe {
        gdt::init();
        idt::init();
    }
}
