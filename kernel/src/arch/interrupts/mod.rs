//! The single Rust entry point for every interrupt and exception.

mod exceptions;
mod frame;
mod stubs;

pub use frame::InterruptFrame;

use super::apic;
use x86::exceptions::FIRST_NON_EXCEPTION;

const STUB_SIZE: u64 = 16;

/// Software interrupt a task raises to give up the CPU (`int 0x30`).
pub const YIELD_VECTOR: u64 = 0x30;

unsafe extern "C" {
    /// Start of the stub block in `stubs.rs`. Only its address matters.
    static interrupt_stubs: u8;
}

pub fn stub_address(vector: u64) -> u64 {
    (&raw const interrupt_stubs) as u64 + vector * STUB_SIZE
}

/// Asks the scheduler to run another task now, through the same path a
/// timer interrupt takes – so there is only one way to switch tasks.
pub fn yield_now() {
    // SAFETY: our IDT handles this vector and returns here (eventually).
    unsafe { core::arch::asm!("int 0x30", options(nomem, nostack)) };
}

/// Returns the frame to resume: `frame` itself, or another task's.
extern "sysv64" fn interrupt_dispatch(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    match frame.vector {
        vector if vector < FIRST_NON_EXCEPTION => exceptions::handle(frame),
        apic::TIMER_VECTOR => {
            crate::time::on_tick();
            apic::end_of_interrupt();
            return crate::task::switch(frame);
        }
        YIELD_VECTOR => return crate::task::switch(frame),
        // Spurious interrupts must *not* be acknowledged (SDM Vol. 3A, §11.9).
        apic::SPURIOUS_VECTOR => {}
        vector => println!("Unexpected interrupt {vector} - no device is set up for it yet."),
    }
    frame
}
