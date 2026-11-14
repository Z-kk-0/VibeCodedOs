//! The contract between the kernel and user programs (DECISIONS.md, 018,
//! 020 and 021): how to make a system call, and what the numbers mean.
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
//! | Register | `call` / `reply` in            | `call` / `receive` out            |
//! |----------|--------------------------------|-----------------------------------|
//! | `rax`    | call number                    | status: 0 or an `ERROR_*`         |
//! | `rdi`    | endpoint / reply handle        | badge of the sender's handle      |
//! | `rsi`    | attached handle or `NO_HANDLE` | received handle or `NO_HANDLE`    |
//! | `rdx`    | `ATTACH_*` mode                | address of received memory or 0   |
//! | `rcx`    | timeout in ticks, 0 = none     | its page count + `ACCESS_*` << 32 |
//! | `r10`, `r8`, `r9`, `r12` | message words 0–3 | message words 0–3             |
//! | `r13`    | –                              | `receive`: the reply handle       |

#![cfg_attr(not(test), no_std)]

/// The interrupt vector for system calls.
pub const SYSCALL_VECTOR: u8 = 0x80;

/// `exit(code)` – ends the program. Never returns.
pub const EXIT: u64 = 0;
/// `sleep(ticks)` – pauses for at least `ticks` timer ticks (10 ms each).
pub const SLEEP: u64 = 2;
/// `call(object, attachment, words, timeout)` – sends a message and waits
/// for the reply, which comes back like a received message. The timeout
/// only counts while no receiver has taken the call (DECISIONS.md, 021).
/// Kernel objects answer at once: the console prints, a program starts
/// (`SPAWN_*`), a child reports how it ended (`CHILD_*`).
pub const CALL: u64 = 3;
/// `receive(endpoint, timeout)` – waits for the next message on an
/// endpoint this program may receive on. Comes with a reply handle.
pub const RECEIVE: u64 = 4;
/// `reply(reply_handle, attachment, words)` – answers that call and uses
/// the reply handle up. Ends any borrow that came with the call.
pub const REPLY: u64 = 5;
/// `memory_new(pages, align)` – a new, zeroed `Memory` object, owned by and
/// mapped into the caller at a multiple of `align` bytes (`rsi`, a power of
/// two; 0 = a page). Returns the handle in `rsi`, address in `rdx`, size in `rcx`.
pub const MEMORY_NEW: u64 = 6;
/// `drop(handle)` – gives a handle up. Owned memory is freed; a dropped
/// reply handle tells its caller `ERROR_PEER_GONE`; when the last
/// receiving end of an endpoint is gone, the endpoint is closed.
pub const DROP: u64 = 7;
/// `endpoint_new()` – a new endpoint; returns its receiving end in `rsi`.
pub const ENDPOINT_NEW: u64 = 8;
/// `mint(receiving_end, badge)` – a new sending end with that badge (`rsi`).
pub const MINT: u64 = 9;
/// `revoke(receiving_end, badge)` – every sending end with that badge, and
/// every copy of one, stops working (`ERROR_REVOKED`). Mint new ones after.
pub const REVOKE: u64 = 10;
/// `memory_info(handle)` – where memory we hold is mapped (`rdx`) and its
/// size and access (`rcx`, as for received memory). For memory a program
/// got at its start, where no message told it the address.
pub const MEMORY_INFO: u64 = 11;

/// "No handle" in handle registers.
pub const NO_HANDLE: u64 = u64::MAX;

/// How a handle is attached to a message (`rdx`).
pub const ATTACH_NONE: u64 = 0;
/// Endpoints (as a sending end) and programs: the receiver gets a copy.
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

/// The most pages one `Memory` object can have (256 MiB) – only a sanity
/// check; free RAM is the real limit.
pub const MAX_MEMORY_PAGES: u64 = 65_536;
pub const PAGE_SIZE: u64 = 4096;

/// Starting a program: `call(program, lend_shared(list), [count, ..])`.
/// The lent memory holds `count` pairs of `u64`: a handle of ours and how
/// to pass it (`ATTACH_COPY` or `ATTACH_MOVE`; memory can be moved too).
/// The child starts with them as handles 0, 1, 2, …; the reply brings a
/// child handle (`rsi`).
pub const SPAWN_ENTRY_SIZE: u64 = 16;
/// `call(child)` waits until the child has ended. Reply word 0 says how,
/// word 1 is the exit code or the CPU exception that stopped it.
pub const CHILD_EXITED: u64 = 0;
pub const CHILD_KILLED: u64 = 1;

/// Results from `u64::MAX` downwards are errors.
pub const ERROR_UNKNOWN_CALL: u64 = u64::MAX;
/// The handle doesn't exist (or no longer does).
pub const ERROR_BAD_HANDLE: u64 = u64::MAX - 1;
/// The object can't do that: e.g. receive on a sending end, lend an
/// endpoint, move borrowed memory, reply with something that isn't a reply.
pub const ERROR_WRONG_KIND: u64 = u64::MAX - 2;
/// The object is in use: memory that is lent out right now.
pub const ERROR_BUSY: u64 = u64::MAX - 3;
/// The ELF file of a program is broken.
pub const ERROR_BAD_PROGRAM: u64 = u64::MAX - 4;
/// The other side ended (or gave up its reply handle) before answering, or
/// the endpoint has no receiving end anymore. Lent memory is back.
pub const ERROR_PEER_GONE: u64 = u64::MAX - 5;
/// Not enough memory, or more pages than `MAX_MEMORY_PAGES`.
pub const ERROR_OUT_OF_MEMORY: u64 = u64::MAX - 6;
/// A length in a message is larger than the attached memory.
pub const ERROR_TOO_LONG: u64 = u64::MAX - 7;
/// Text that isn't valid UTF-8.
pub const ERROR_NOT_UTF8: u64 = u64::MAX - 8;
/// An unknown `ATTACH_*` mode.
pub const ERROR_BAD_MODE: u64 = u64::MAX - 9;
/// Nobody took the call (or sent us one) before the timeout.
pub const ERROR_TIMED_OUT: u64 = u64::MAX - 10;
/// The receiver revoked this sending end's badge.
pub const ERROR_REVOKED: u64 = u64::MAX - 11;
/// A number that makes no sense here, e.g. an alignment that isn't a power of two.
pub const ERROR_BAD_ARGUMENT: u64 = u64::MAX - 12;

const LOWEST_ERROR: u64 = ERROR_BAD_ARGUMENT;

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
        for error in [ERROR_UNKNOWN_CALL, ERROR_BAD_HANDLE, ERROR_BAD_ARGUMENT] {
            assert!(is_error(error));
        }
        assert!(!is_error(0));
        assert!(!is_error(4096));
    }
}
