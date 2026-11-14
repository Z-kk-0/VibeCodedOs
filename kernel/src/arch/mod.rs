//! Everything x86_64-specific. The rest of the kernel should not need
//! to know which CPU it runs on.

mod apic;
pub mod cpu;
mod gdt;
mod idt;
mod interrupts;
mod pic;
mod pit;
pub mod port;
mod start;

pub use apic::TimerSetup;
pub use gdt::set_kernel_stack;
pub use interrupts::{InterruptFrame, yield_now};

/// Installs our own segment and interrupt tables, replacing the firmware's,
/// and silences the legacy interrupt controllers.
pub fn init() {
    // SAFETY: called exactly once, early, with interrupts disabled
    // (start.rs does `cli`), before anything relies on the old tables.
    unsafe {
        gdt::init();
        idt::init();
        pic::disable();
    }
}

/// Starts the periodic timer interrupt and turns interrupts on.
/// Each tick calls `crate::time::on_tick`, then `crate::task::switch`.
pub fn start_timer(interrupts_per_second: u32) -> TimerSetup {
    // SAFETY: `init` ran (IDT present, PIC silenced); every vector has a
    // handler, so enabling interrupts is fine afterwards.
    unsafe {
        let setup = apic::start_timer(interrupts_per_second);
        cpu::enable_interrupts();
        setup
    }
}
