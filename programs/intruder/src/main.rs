//! A user program that misbehaves on purpose. Two attacks on the kernel's
//! memory; both must fail:
//!
//! 1. Ask the kernel to print kernel memory for us – the system call must
//!    refuse the pointer (`ERROR_BAD_ADDRESS`).
//! 2. Read kernel memory directly – the page tables don't allow ring 3
//!    there, so the CPU raises a page fault, and the kernel must end this
//!    program, not crash.

#![no_std]
#![no_main]

use userlib::{abi, exit, println, raw};

/// Where the kernel image lives (boot_info::KERNEL_BASE).
const KERNEL_CODE: u64 = 0xFFFF_FFFF_8000_0000;
/// Exit code if the kernel fell for attack 1.
const KERNEL_LEAKED_MEMORY: u64 = 2;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let result = raw::write(KERNEL_CODE, 64);
    if result != abi::ERROR_BAD_ADDRESS {
        exit(KERNEL_LEAKED_MEMORY);
    }
    println!("Asked the kernel to print its own memory: refused, as it should be.");

    println!("Trying to read kernel memory at {KERNEL_CODE:#x} directly...");
    // SAFETY: none – this is the attack. The kernel must stop it.
    let stolen = unsafe { (KERNEL_CODE as *const u64).read_volatile() };
    println!("Got {stolen:#x}. This line must never appear!");
    exit(1)
}
