//! A user program that misbehaves on purpose. Two attacks on the kernel;
//! both must fail:
//!
//! 1. Forge handles: one that was never given out, and one with the right
//!    slot but the wrong generation. The kernel must refuse both
//!    (`ERROR_BAD_HANDLE`) – a handle is only what the kernel says it is.
//! 2. Read kernel memory directly – the page tables don't allow ring 3
//!    there, so the CPU raises a page fault, and the kernel must end this
//!    program, not crash.
//!
//! Handles: 0 = log.

#![no_std]
#![no_main]

use userlib::{Attach, Error, Handle, abi, call, exit, println};

/// Where the kernel image lives (boot_info::KERNEL_BASE).
const KERNEL_CODE: u64 = 0xFFFF_FFFF_8000_0000;
/// Exit code if the kernel fell for attack 1.
const KERNEL_ACCEPTED_FORGERY: u64 = 2;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let never_given = Handle(42);
    let wrong_generation = Handle(1 << 32); // slot 0 (our log handle), generation 1
    for forged in [never_given, wrong_generation] {
        if call(forged, Attach::Nothing, [0; 4]).err() != Some(Error(abi::ERROR_BAD_HANDLE)) {
            exit(KERNEL_ACCEPTED_FORGERY);
        }
    }
    println!("Forged two handles: both refused, as they should be.");

    println!("Trying to read kernel memory at {KERNEL_CODE:#x} directly...");
    // SAFETY: none – this is the attack. The kernel must stop it.
    let stolen = unsafe { (KERNEL_CODE as *const u64).read_volatile() };
    println!("Got {stolen:#x}. This line must never appear!");
    exit(1)
}
