//! Everything x86_64-specific. The rest of the kernel should not need
//! to know which CPU it runs on.

mod apic;
pub mod cpu;
mod gdt;
mod idt;
mod interrupts;
mod per_core;
mod pic;
mod pit;
pub mod port;
mod shootdown;
mod smp;
pub mod start;

pub use apic::TimerSetup;
pub use gdt::set_kernel_stack;
pub use interrupts::{InterruptFrame, yield_now};
pub use per_core::{CoreSet, release_after_switch, this_core_index as core_index};
pub use shootdown::{
    any_pending as shootdown_pending, sent_count as shootdown_count,
    serve_pending as serve_shootdown,
};
pub use smp::{report_ready, start_other_cores, stop_other_cores, stop_this_core};

use core::sync::atomic::Ordering;
use per_core::BOOT_CORE;

const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
#[allow(
    dead_code,
    reason = "only the CPU uses this memory, Rust never reads it"
)]
struct Stack([u8; DOUBLE_FAULT_STACK_SIZE]);

/// The boot core's double-fault stack (the others get theirs from the heap).
static mut BOOT_DOUBLE_FAULT_STACK: Stack = Stack([0; DOUBLE_FAULT_STACK_SIZE]);

/// Installs our own segment and interrupt tables on the boot core,
/// replacing the firmware's, and silences the legacy interrupt controllers.
pub fn init() {
    // SAFETY: called exactly once, early, with interrupts disabled
    // (start.rs does `cli`), before anything relies on the old tables.
    // The stack is a static that only the CPU touches.
    unsafe {
        let stack = (&raw const BOOT_DOUBLE_FAULT_STACK) as u64;
        gdt::init(&BOOT_CORE, stack + DOUBLE_FAULT_STACK_SIZE as u64);
        idt::init();
        pic::disable();
    }
    per_core::register(&BOOT_CORE);
    per_core::set_online(&BOOT_CORE);
}

/// Starts the boot core's periodic timer interrupt and turns interrupts on.
/// Each tick calls `crate::task::switch` (and on core 0 `crate::time::on_tick`).
pub fn start_timer(interrupts_per_second: u32) -> TimerSetup {
    // SAFETY: `init` ran (IDT present, PIC silenced); every vector has a
    // handler, so enabling interrupts is fine afterwards.
    unsafe {
        let setup = apic::start_timer(interrupts_per_second);
        BOOT_CORE.apic_id.store(apic::id(), Ordering::Relaxed);
        cpu::enable_interrupts();
        setup
    }
}

/// The same for a core started later, with the boot core's measurement.
pub fn start_timer_on_this_core() {
    // SAFETY: this core's GDT and IDT are loaded (`smp::ap_entry`), the
    // boot core measured the timer before starting any other core.
    unsafe {
        apic::start_timer_on_this_core();
        cpu::enable_interrupts();
    }
}

/// After unmapping pages of the kernel's half: every core forgets them.
pub fn flush_kernel_pages_everywhere(start: u64, pages: u64) {
    shootdown::flush_kernel_pages(start, pages);
}

/// After changing pages of an address space active on `cores`.
pub fn flush_pages_on(cores: CoreSet, start: u64, pages: u64) {
    shootdown::flush_pages(cores, start, pages);
}

/// How the local APICs are reached: "x2APIC" or "xAPIC".
pub fn apic_mode() -> &'static str {
    apic::mode()
}
