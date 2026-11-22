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
//! | Register | `call` / `send` / `reply` in   | `call` / `receive` out            |
//! |----------|--------------------------------|-----------------------------------|
//! | `rax`    | call number                    | status: 0 or an `ERROR_*`         |
//! | `rdi`    | endpoint / reply handle        | badge of the sender's handle (answer: its tag) |
//! | `rsi`    | attached handle or `NO_HANDLE` | received handle or `NO_HANDLE`    |
//! | `rdx`    | `ATTACH_*` mode                | address of received memory or 0   |
//! | `rcx`    | timeout in ticks, 0 = none; `send`: the mailbox | its page count + `ACCESS_*` << 32 |
//! | `r10`, `r8`, `r9`, `r12` | message words 0–3 | message words 0–3             |
//! | `r13`    | `send`: the tag                | `receive`: the reply handle, `NO_HANDLE` for an answer |
//! | `r14`    | –                              | `receive`: an answer's status (0 for a call) |
//!
//! `receive` brings two kinds of message (DECISIONS.md, 027): a *call*,
//! with a reply handle, and an *answer* – the reply to a `send` whose
//! mailbox this endpoint is, a child's end, an interrupt – without one.

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
/// (`SPAWN_*`), a child reports how it ended (`CHILD_*`), a device opens.
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
/// `send(object, attachment, words, mailbox, tag)` – a `call` that doesn't
/// wait (DECISIONS.md, 027). Returns at once; the answer – the reply, or
/// why there is none – arrives later in `mailbox` (`rcx`, a receiving end
/// of ours) with `tag` (`r13`) in `rdi` and its status in `r14`. What is
/// attached leaves the sender at once: moved things for good, lent memory
/// until the answer (if the call fails, moved things are given up). Takes
/// endpoints, children (`CHILD_WAIT`: the answer comes when the child has
/// ended) and interrupts (the answer comes when the line fires). Objects
/// that answer at once are used with `call`.
pub const SEND: u64 = 12;

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
/// `call(child, [op, 0, 0, 0])`, with word 0 one of:
/// - `CHILD_WAIT`: waits until the child has ended – at most `rcx` ticks
///   (0 = no limit), else `ERROR_TIMED_OUT`.
/// - `CHILD_POLL`: answers at once: how it ended, or `ERROR_TIMED_OUT`.
/// - `CHILD_STOP`: asks the kernel to end it (DECISIONS.md, 026); answers
///   at once. The child ends the next time it would run in user mode;
///   whatever it waits for – a reply too (027) – is cut short. Waiting for
///   it then says `CHILD_STOPPED`.
///
/// When it has ended, reply word 0 says how, word 1 is the exit code or
/// the CPU exception that stopped it.
pub const CHILD_WAIT: u64 = 0;
pub const CHILD_POLL: u64 = 1;
pub const CHILD_STOP: u64 = 2;
pub const CHILD_EXITED: u64 = 0;
pub const CHILD_KILLED: u64 = 1;
/// The kernel failed while working for the child – a kernel bug it could
/// contain by ending only that program (DECISIONS.md, 024). Word 1 is the
/// CPU exception, or `KERNEL_PANIC`.
pub const CHILD_KERNEL_FAILED: u64 = 2;
/// Its parent stopped it (`CHILD_STOP`). Word 1 is 0.
pub const CHILD_STOPPED: u64 = 3;
/// Not a CPU exception (those are 0–31): the kernel panicked.
pub const KERNEL_PANIC: u64 = 256;

/// `call(device)` opens device memory (DECISIONS.md, 023): the handle becomes
/// an owned `Memory` object, mapped with the caching the device needs (the
/// screen: write-combining). The reply brings it like received memory
/// (`rsi`, `rdx`, `rcx`) plus four words the kernel knows about the device.
/// The screen's: width, height, stride (pixels per row in memory) and the
/// pixel format, one of `PIXEL_*` (4 bytes per pixel, in that byte order).
pub const PIXEL_RED_GREEN_BLUE: u64 = 0;
pub const PIXEL_BLUE_GREEN_RED: u64 = 1;

/// `call(interrupt)` waits until the device interrupt line behind the handle
/// fired (DECISIONS.md, 025) – at most `rcx` ticks, like a queue (0 = no
/// limit). Reply word 0: how often it fired since the last call (at least
/// 1). Interrupts that come while nobody waits are counted, not lost.
///
/// `call(ports, words)` runs up to four I/O port operations, one per word
/// (`PortOp`), until the first word that is 0 – all checked before the
/// first runs. The port is the handle's first port + `offset`.
/// - `PORT_READ`: the argument is a condition: the read runs only if the
///   operation before it was a read that ran and found one of these bits
///   set (0: always). "Read the status, and the data only if there is any"
///   is one call.
/// - `PORT_WRITE`: the argument is the byte.
///
/// Reply word `i`: `PORT_DONE` | the byte for a read that ran, `PORT_DONE`
/// for a write, 0 for an operation that was skipped (or not there).
pub const PORT_READ: u8 = 1;
pub const PORT_WRITE: u8 = 2;
pub const PORT_DONE: u64 = 1 << 8;

/// One port operation, as it travels in a message word:
/// `op | offset << 8 | argument << 16`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortOp {
    pub op: u8,
    pub offset: u8,
    pub argument: u8,
}

impl PortOp {
    pub fn read(offset: u8) -> Self {
        Self {
            op: PORT_READ,
            offset,
            argument: 0,
        }
    }

    /// Reads only if the operation before found one of `bits` set.
    pub fn read_if(offset: u8, bits: u8) -> Self {
        Self {
            op: PORT_READ,
            offset,
            argument: bits,
        }
    }

    pub fn write(offset: u8, byte: u8) -> Self {
        Self {
            op: PORT_WRITE,
            offset,
            argument: byte,
        }
    }

    pub fn to_word(self) -> u64 {
        u64::from(self.op) | u64::from(self.offset) << 8 | u64::from(self.argument) << 16
    }
}

/// Runs a `call(ports, words)` (see `PORT_READ`): `covered(offset)` says
/// whether the handle covers a port, `io` does one operation and returns
/// the byte read (anything for a write). The reply words, or
/// `ERROR_BAD_ARGUMENT` – then nothing ran. Shared by the kernel (which
/// does the `in`/`out`) and the host tests below.
pub fn run_port_ops(
    words: [u64; 4],
    covered: impl Fn(u8) -> bool,
    mut io: impl FnMut(PortOp) -> u8,
) -> Result<[u64; 4], u64> {
    let count = words.iter().position(|&word| word == 0).unwrap_or(4);
    let ops = &words[..count];
    let valid = count > 0
        && words[count..].iter().all(|&word| word == 0)
        && ops.iter().enumerate().all(|(index, &word)| {
            let op = decode(word);
            word >> 24 == 0
                && covered(op.offset)
                && match op.op {
                    PORT_READ => index > 0 || op.argument == 0,
                    PORT_WRITE => true,
                    _ => false,
                }
        });
    if !valid {
        return Err(ERROR_BAD_ARGUMENT);
    }
    let mut reply = [0; 4];
    // What the operation before left: the byte, if it was a read that ran.
    let mut previous: Option<u8> = None;
    for (slot, &word) in reply.iter_mut().zip(ops) {
        let op = decode(word);
        let runs = op.op == PORT_WRITE
            || op.argument == 0
            || previous.is_some_and(|byte| byte & op.argument != 0);
        previous = None;
        if !runs {
            continue;
        }
        let byte = io(op);
        *slot = PORT_DONE
            | if op.op == PORT_READ {
                u64::from(byte)
            } else {
                0
            };
        if op.op == PORT_READ {
            previous = Some(byte);
        }
    }
    Ok(reply)
}

fn decode(word: u64) -> PortOp {
    PortOp {
        op: word as u8,
        offset: (word >> 8) as u8,
        argument: (word >> 16) as u8,
    }
}

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
/// The machine doesn't have that device (e.g. no screen).
pub const ERROR_NO_DEVICE: u64 = u64::MAX - 13;

const LOWEST_ERROR: u64 = ERROR_NO_DEVICE;

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
    fn port_ops_run_in_order_and_reads_can_depend_on_a_status() {
        let ports = core::cell::RefCell::new([0x01u8, 0x42]); // status: data ready
        let mut writes = 0;
        let run = |words, writes: &mut u32| {
            run_port_ops(
                words,
                |offset| offset < 2,
                |op| {
                    if op.op == PORT_WRITE {
                        *writes += 1;
                        ports.borrow_mut()[usize::from(op.offset)] = op.argument;
                    }
                    ports.borrow()[usize::from(op.offset)]
                },
            )
        };
        let status_then_data = [
            PortOp::read(0).to_word(),
            PortOp::read_if(1, 0x01).to_word(),
            0,
            0,
        ];
        assert_eq!(
            run(status_then_data, &mut writes),
            Ok([PORT_DONE | 0x01, PORT_DONE | 0x42, 0, 0])
        );
        ports.borrow_mut()[0] = 0; // no data: the second read is skipped
        assert_eq!(run(status_then_data, &mut writes), Ok([PORT_DONE, 0, 0, 0]));
        let write = [
            PortOp::write(1, 7).to_word(),
            PortOp::read(1).to_word(),
            0,
            0,
        ];
        assert_eq!(
            run(write, &mut writes),
            Ok([PORT_DONE, PORT_DONE | 7, 0, 0])
        );
        assert_eq!(writes, 1);
    }

    #[test]
    fn bad_port_ops_run_nothing() {
        let mut ran = false;
        let mut run = |words| {
            run_port_ops(
                words,
                |offset| offset == 0,
                |_| {
                    ran = true;
                    0
                },
            )
        };
        let uncovered = [PortOp::read(0).to_word(), PortOp::read(1).to_word(), 0, 0];
        assert_eq!(run(uncovered), Err(ERROR_BAD_ARGUMENT));
        assert_eq!(run([0; 4]), Err(ERROR_BAD_ARGUMENT)); // nothing to do
        let first_has_a_condition = [PortOp::read_if(0, 1).to_word(), 0, 0, 0];
        assert_eq!(run(first_has_a_condition), Err(ERROR_BAD_ARGUMENT));
        let gap = [PortOp::read(0).to_word(), 0, PortOp::read(0).to_word(), 0];
        assert_eq!(run(gap), Err(ERROR_BAD_ARGUMENT));
        assert_eq!(run([3, 0, 0, 0]), Err(ERROR_BAD_ARGUMENT)); // no such op
        assert_eq!(run([1 << 24 | 1, 0, 0, 0]), Err(ERROR_BAD_ARGUMENT));
        assert!(!ran);
    }

    #[test]
    fn every_error_counts_as_an_error() {
        for error in [ERROR_UNKNOWN_CALL, ERROR_BAD_HANDLE, ERROR_NO_DEVICE] {
            assert!(is_error(error));
        }
        assert!(!is_error(0));
        assert!(!is_error(4096));
    }
}
