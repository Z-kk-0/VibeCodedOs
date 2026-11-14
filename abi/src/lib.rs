//! The contract between the kernel and user programs (DECISIONS.md, 018 and
//! 020): how to make a system call, and what the numbers mean.
//!
//! A program puts the call number in `rax` and executes `int 0x80`. Every
//! register the kernel doesn't name below as a result is preserved.
//!
//! Almost everything is a message over a capability (DECISIONS.md, 020): a
//! program holds *handles* to kernel objects and can only use what it has
//! a handle for. A message is four words in registers, plus at most one
//! attached handle. Data never travels inside a message – it travels as a
//! `Memory` object that is moved or lent.
//!
//! | Register | `call` / `reply` in          | `call` / `receive` out          |
//! |----------|------------------------------|---------------------------------|
//! | `rax`    | call number                  | status: 0 or an `ERROR_*`       |
//! | `rdi`    | endpoint (not for `reply`)   | badge of the sender's handle    |
//! | `rsi`    | attached handle or `NO_HANDLE` | received handle or `NO_HANDLE` |
//! | `rdx`    | `ATTACH_*` mode              | address of received memory or 0 |
//! | `rcx`    | –                            | its page count + `ACCESS_*` << 32 |
//! | `r10`, `r8`, `r9`, `r12` | message words 0–3 | message words 0–3         |

#![cfg_attr(not(test), no_std)]

/// The interrupt vector for system calls.
pub const SYSCALL_VECTOR: u8 = 0x80;

/// `exit(code)` – ends the program. Never returns.
pub const EXIT: u64 = 0;
/// `sleep(ticks)` – pauses for at least `ticks` timer ticks (10 ms each).
pub const SLEEP: u64 = 2;
/// `call(endpoint, attachment, words)` – sends a message and waits for the
/// reply, which comes back like a received message.
pub const CALL: u64 = 3;
/// `receive(endpoint)` – waits for the next message on an endpoint this
/// program may receive on. Answer it with `reply` before receiving again.
pub const RECEIVE: u64 = 4;
/// `reply(attachment, words)` – answers the last received message. Ends
/// any borrow that came with it.
pub const REPLY: u64 = 5;
/// `memory_new(pages)` – a new, zeroed `Memory` object, owned by and mapped
/// into the caller. Returns the handle in `rsi`, address in `rdx`, size in `rcx`.
pub const MEMORY_NEW: u64 = 6;
/// `drop(handle)` – gives a handle up. Owned memory is freed.
pub const DROP: u64 = 7;

/// "No handle" in handle registers.
pub const NO_HANDLE: u64 = u64::MAX;

/// How a handle is attached to a message (`rdx`).
pub const ATTACH_NONE: u64 = 0;
/// Endpoints only: the receiver gets its own copy (send rights only).
pub const ATTACH_COPY: u64 = 1;
/// The receiver gets the object; the sender loses it.
pub const ATTACH_MOVE: u64 = 2;
/// Memory only, `call` only: `&T`. Read-only for both until the reply.
pub const ATTACH_LEND_SHARED: u64 = 3;
/// Memory only, `call` only: `&mut T`. Gone from the sender until the reply.
pub const ATTACH_LEND_EXCLUSIVE: u64 = 4;

/// How the receiver may use received memory (`rcx >> 32`).
pub const ACCESS_OWNED: u64 = 1;
pub const ACCESS_SHARED: u64 = 2;
pub const ACCESS_EXCLUSIVE: u64 = 3;

/// The most pages one `Memory` object can have (1 MiB).
pub const MAX_MEMORY_PAGES: u64 = 256;
pub const PAGE_SIZE: u64 = 4096;

/// Results from `u64::MAX` downwards are errors.
pub const ERROR_UNKNOWN_CALL: u64 = u64::MAX;
/// The handle doesn't exist (or no longer does).
pub const ERROR_BAD_HANDLE: u64 = u64::MAX - 1;
/// The object can't do that: e.g. receive on a send-only endpoint, lend
/// an endpoint, move borrowed memory.
pub const ERROR_WRONG_KIND: u64 = u64::MAX - 2;
/// The object is in use: memory already lent, a reply still owed.
pub const ERROR_BUSY: u64 = u64::MAX - 3;
/// `reply` without a received message.
pub const ERROR_NO_CALLER: u64 = u64::MAX - 4;
/// The other side ended before answering. Lent memory is back.
pub const ERROR_PEER_GONE: u64 = u64::MAX - 5;
/// Not enough memory, or more pages than `MAX_MEMORY_PAGES`.
pub const ERROR_OUT_OF_MEMORY: u64 = u64::MAX - 6;
/// A length in a message is larger than the attached memory.
pub const ERROR_TOO_LONG: u64 = u64::MAX - 7;
/// Text that isn't valid UTF-8.
pub const ERROR_NOT_UTF8: u64 = u64::MAX - 8;
/// An unknown `ATTACH_*` mode.
pub const ERROR_BAD_MODE: u64 = u64::MAX - 9;

const LOWEST_ERROR: u64 = ERROR_BAD_MODE;

pub fn is_error(result: u64) -> bool {
    result >= LOWEST_ERROR
}

/// A short name packed into a badge: up to 8 ASCII bytes, little-endian.
/// The kernel badges each program's handles with the program's name.
pub fn badge_from_name(name: &str) -> u64 {
    let mut bytes = [0u8; 8];
    for (slot, byte) in bytes.iter_mut().zip(name.bytes()) {
        *slot = byte;
    }
    u64::from_le_bytes(bytes)
}

/// The name in a badge made by `badge_from_name`.
pub fn name_from_badge(badge: u64) -> BadgeName {
    let bytes = badge.to_le_bytes();
    let length = bytes.iter().position(|&byte| byte == 0).unwrap_or(8);
    BadgeName { bytes, length }
}

pub struct BadgeName {
    bytes: [u8; 8],
    length: usize,
}

impl BadgeName {
    /// The name without the trailing zeros.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_survive_a_badge() {
        assert_eq!(
            name_from_badge(badge_from_name("hello")).as_bytes(),
            b"hello"
        );
        assert_eq!(
            name_from_badge(badge_from_name("intruder")).as_bytes(),
            b"intruder"
        );
        assert_eq!(
            name_from_badge(badge_from_name("too long name")).as_bytes(),
            b"too long"
        );
        assert_eq!(name_from_badge(0).as_bytes(), b"");
    }

    #[test]
    fn every_error_counts_as_an_error() {
        for error in [ERROR_UNKNOWN_CALL, ERROR_BAD_HANDLE, ERROR_BAD_MODE] {
            assert!(is_error(error));
        }
        assert!(!is_error(0));
        assert!(!is_error(4096));
    }
}
