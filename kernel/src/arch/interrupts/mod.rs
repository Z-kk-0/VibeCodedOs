//! The single Rust entry point for every interrupt and exception.

mod exceptions;
mod frame;
mod stubs;

pub use frame::InterruptFrame;

use super::per_core::{take_release_flag, this_core};
use super::{apic, shootdown, smp};
use core::sync::atomic::AtomicBool;
use x86::exceptions::FIRST_NON_EXCEPTION;

const STUB_SIZE: u64 = 16;

/// Software interrupt a task raises to give up the CPU (`int 0x30`).
pub const YIELD_VECTOR: u64 = 0x30;
/// User programs call the kernel with `int 0x80` (see the `abi` crate).
const SYSCALL_VECTOR: u64 = abi::SYSCALL_VECTOR as u64;
const SHOOTDOWN_VECTOR: u64 = shootdown::SHOOTDOWN_VECTOR as u64;
const STOP_VECTOR: u64 = smp::STOP_VECTOR as u64;

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

/// What the stub does after the dispatcher: continue with `frame`, then –
/// once it's off the old stack – clear `release` (DECISIONS.md, 019).
/// Two words, so it comes back in `rax` and `rdx` (System V ABI).
#[repr(C)]
struct Resume {
    frame: *mut InterruptFrame,
    release: *const AtomicBool,
}

extern "sysv64" fn interrupt_dispatch(frame: &mut InterruptFrame) -> Resume {
    let frame = dispatch(frame);
    Resume {
        frame,
        release: take_release_flag(),
    }
}

/// Returns the frame to resume: `frame` itself, or another task's.
fn dispatch(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    match frame.vector {
        vector if vector < FIRST_NON_EXCEPTION => return exceptions::handle(frame),
        SYSCALL_VECTOR => return crate::syscall::handle(frame),
        apic::TIMER_VECTOR => {
            // Every core ticks; one clock is enough.
            if this_core().index == 0 {
                crate::time::on_tick();
            }
            apic::end_of_interrupt();
            return crate::task::switch(frame);
        }
        YIELD_VECTOR => return crate::task::switch(frame),
        SHOOTDOWN_VECTOR => {
            shootdown::serve_pending();
            apic::end_of_interrupt();
        }
        STOP_VECTOR => smp::stop_this_core(),
        // Spurious interrupts must *not* be acknowledged (SDM Vol. 3A, §11.9).
        apic::SPURIOUS_VECTOR => {}
        vector => println!("Unexpected interrupt {vector} - no device is set up for it yet."),
    }
    frame
}
