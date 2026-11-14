//! System calls: the only way user programs reach the kernel (`int 0x80`,
//! calling convention and numbers in the `abi` crate; DECISIONS.md, 018).
//!
//! Arguments come straight from the program's registers, so every one of
//! them is checked before use – especially pointers.

use crate::arch::InterruptFrame;
use crate::memory::copy_from_user;
use crate::task::{self, Exit};

/// Handles the call described by the frame's registers. Returns the frame
/// to resume – a different task's if the call ended or paused this one.
pub fn handle(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let (number, first, second) = (frame.rax, frame.rdi, frame.rsi);
    match number {
        abi::EXIT => return task::end_current(frame, Exit::Code(first)),
        abi::SLEEP => return task::sleep_current(frame, first),
        abi::WRITE => frame.rax = write(first, second),
        _ => frame.rax = abi::ERROR_UNKNOWN_CALL,
    }
    frame
}

/// `write(address, length)`: prints the program's text, prefixed with its
/// name.
fn write(address: u64, length: u64) -> u64 {
    if length > abi::MAX_WRITE as u64 {
        return abi::ERROR_TOO_LONG;
    }
    let bytes = task::with_current_address_space(|space| {
        space.and_then(|space| copy_from_user(space, address, length as usize))
    });
    let Some(bytes) = bytes else {
        return abi::ERROR_BAD_ADDRESS;
    };
    let Ok(text) = core::str::from_utf8(&bytes) else {
        return abi::ERROR_NOT_UTF8;
    };
    print!("[{}] {text}", task::current_name());
    length
}
