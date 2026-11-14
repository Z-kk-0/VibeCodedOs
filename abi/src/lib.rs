//! The contract between the kernel and user programs (DECISIONS.md, 018):
//! how to make a system call, and what the numbers mean.
//!
//! A program puts the call number in `rax`, arguments in `rdi`, `rsi`,
//! `rdx`, and executes `int 0x80`. The result comes back in `rax`; every
//! other register is preserved.
//!
//! This first set is deliberately tiny. With capabilities (roadmap) most of
//! it will become messages to services instead.

#![no_std]

/// The interrupt vector for system calls.
pub const SYSCALL_VECTOR: u8 = 0x80;

/// `exit(code)` – ends the program. Never returns.
pub const EXIT: u64 = 0;
/// `write(address, length)` – prints UTF-8 text to the kernel log.
/// Returns the number of bytes written.
pub const WRITE: u64 = 1;
/// `sleep(ticks)` – pauses for at least `ticks` timer ticks (10 ms each).
pub const SLEEP: u64 = 2;

/// Results from `u64::MAX` downwards are errors.
pub const ERROR_UNKNOWN_CALL: u64 = u64::MAX;
/// A pointer argument points to memory the program may not read.
pub const ERROR_BAD_ADDRESS: u64 = u64::MAX - 1;
/// Text that isn't valid UTF-8.
pub const ERROR_NOT_UTF8: u64 = u64::MAX - 2;
/// More data than one call accepts (see `MAX_WRITE`).
pub const ERROR_TOO_LONG: u64 = u64::MAX - 3;

/// The most bytes one `write` takes.
pub const MAX_WRITE: usize = 4096;

pub fn is_error(result: u64) -> bool {
    result >= ERROR_TOO_LONG
}
