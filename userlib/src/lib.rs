//! The library every Hematite user program uses: system calls wrapped in
//! safe functions, `println!`, and a panic handler. Calling convention:
//! the `abi` crate.
//!
//! IPC (DECISIONS.md, 020) mirrors Rust's own rules in the types: lending
//! memory for a call takes `&Memory` or `&mut Memory` for exactly as long
//! as the call runs, moving it takes the `Memory` by value. So the compiler
//! already refuses what the kernel's page tables would stop anyway.

#![no_std]

// Box, Vec, String & co. for every program, backed by memory objects
// (allocator.rs).
extern crate alloc;

mod allocator;
pub mod device;
pub mod directory;
pub mod display;
mod endpoint;
pub mod font;
pub mod interrupt;
mod ipc;
pub mod keyboard;
mod memory;
mod message;
pub mod ports;
// Public only because `println!` expands to code that uses it.
#[doc(hidden)]
pub mod print;
mod program;
mod system_call;

pub use endpoint::{endpoint_new, mint, revoke};
pub use ipc::{
    Attach, Error, Handle, call, call_timeout, drop_handle, receive, receive_timeout, reply,
};
pub use memory::{Borrowed, Memory, ReceivedMemory};
pub use message::{Received, Reply};
pub use print::set_log;
pub use program::{Ended, Pass, spawn, wait};

/// The system call numbers and error codes, for programs that need them.
pub use abi;

use system_call::{Registers, system_call};

/// Pauses for at least `ticks` timer ticks (10 ms each).
pub fn sleep(ticks: u64) {
    system_call(Registers {
        rax: abi::SLEEP,
        rdi: ticks,
        ..Registers::default()
    });
}

/// Ends the program with an exit code.
pub fn exit(code: u64) -> ! {
    system_call(Registers {
        rax: abi::EXIT,
        rdi: code,
        ..Registers::default()
    });
    unreachable!("the kernel never returns from exit")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info.message());
    exit(101)
}
