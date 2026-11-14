//! The first Hematite user program. Runs in ring 3, in its own address
//! space, and can only reach the kernel through system calls.
//!
//! Handles: 0 = log, 1 = a page init wrote a greeting into (memory handed
//! over at the start, DECISIONS.md, 022). Also shows that its stack grows.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use userlib::{Handle, Memory, exit, println, sleep};

const GREETING: Handle = Handle(1);

/// The kernel starts every program here (the ELF entry point).
#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let on_my_stack = 0u8;
    println!("Hello from user mode! Ring 3, my own address space.");
    println!(
        "My code is at {:p}, my stack at {:p}.",
        _start as extern "C" fn() -> ! as *const (), &on_my_stack
    );
    let Ok(greeting) = Memory::from_start_handle(GREETING) else {
        exit(2)
    };
    let text = greeting.bytes();
    let length = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    let Ok(text) = core::str::from_utf8(&text[..length]) else {
        exit(3)
    };
    println!("Got {text:?} at {:p}.", greeting.address());
    // The heap: a large block (8 KiB, its own memory object) and small ones.
    let squares: Vec<u64> = (1..=1000).map(|n| n * n).collect();
    let mut words = String::from("a heap");
    words.push_str(" in user space");
    println!(
        "Vec of {} squares (last {}) and {words:?}: {}.",
        squares.len(),
        squares[999],
        "alloc works here too"
    );
    drop(squares); // its memory object goes back to the kernel

    // About 100 KiB deep – far more than the 16 KiB mapped at the start.
    let sum = recurse(100);
    println!("Recursed 100 levels deep (sum {sum}): my stack grew as it was touched.");
    for round in 1..=3 {
        sleep(5);
        println!("still here after {} ticks of sleep", round * 5);
    }
    exit(0)
}

/// Uses about 1 KiB of stack per level.
fn recurse(depth: u64) -> u64 {
    let frame = core::hint::black_box([depth; 128]);
    if depth == 0 {
        frame[0]
    } else {
        recurse(depth - 1) + frame[1]
    }
}
