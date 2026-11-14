//! The library every Hematite user program uses: system calls wrapped in
//! safe functions, `println!`, and a panic handler. Calling convention:
//! the `abi` crate.

#![no_std]

// Public only because `println!` expands to code that uses it.
#[doc(hidden)]
pub mod print;

use core::arch::asm;

/// The system call numbers and error codes, for programs that need them.
pub use abi;

/// Raw system call. The kernel preserves every register except `rax`.
fn system_call(number: u64, first: u64, second: u64) -> u64 {
    let result;
    // SAFETY: `int 0x80` enters the kernel, which validates every argument
    // and only writes `rax`. It may read memory we point it to, hence no
    // `nomem`.
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") number => result,
            in("rdi") first,
            in("rsi") second,
            options(nostack, preserves_flags),
        );
    }
    result
}

/// System calls with unchecked arguments – for programs that test whether
/// the kernel's own checks hold.
pub mod raw {
    /// `write` with any address and length, valid or not.
    pub fn write(address: u64, length: u64) -> u64 {
        super::system_call(abi::WRITE, address, length)
    }
}

/// Prints text to the kernel log. Fails only for text over 4 KiB.
pub fn write(text: &str) -> Result<usize, u64> {
    let result = system_call(abi::WRITE, text.as_ptr() as u64, text.len() as u64);
    if abi::is_error(result) {
        Err(result)
    } else {
        Ok(result as usize)
    }
}

/// Pauses for at least `ticks` timer ticks (10 ms each).
pub fn sleep(ticks: u64) {
    system_call(abi::SLEEP, ticks, 0);
}

/// Ends the program with an exit code.
pub fn exit(code: u64) -> ! {
    system_call(abi::EXIT, code, 0);
    unreachable!("the kernel never returns from exit")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info.message());
    exit(101)
}
