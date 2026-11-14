//! The first Hematite user program. Runs in ring 3, in its own address
//! space, and can only reach the kernel through system calls.

#![no_std]
#![no_main]

use userlib::{exit, println, sleep};

/// The kernel starts every program here (the ELF entry point).
#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let on_my_stack = 0u8;
    println!("Hello from user mode! Ring 3, my own address space.");
    println!(
        "My code is at {:p}, my stack at {:p}.",
        _start as extern "C" fn() -> ! as *const (), &on_my_stack
    );
    for round in 1..=3 {
        sleep(5);
        println!("still here after {} ticks of sleep", round * 5);
    }
    exit(0)
}
