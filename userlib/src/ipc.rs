//! Handles and messages (DECISIONS.md, 020 and 021).

use crate::memory::Memory;
use crate::message::{Received, Reply, decode};
use crate::system_call::{Registers, system_call};
use core::fmt;

/// A handle: what this program holds in the kernel's table for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle(pub u64);

/// An `abi::ERROR_*` code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error(pub u64);

impl Error {
    pub fn name(self) -> &'static str {
        match self.0 {
            abi::ERROR_UNKNOWN_CALL => "unknown call",
            abi::ERROR_BAD_HANDLE => "bad handle",
            abi::ERROR_WRONG_KIND => "wrong kind of object",
            abi::ERROR_BUSY => "busy",
            abi::ERROR_BAD_PROGRAM => "broken program",
            abi::ERROR_PEER_GONE => "the other side is gone",
            abi::ERROR_OUT_OF_MEMORY => "out of memory",
            abi::ERROR_TOO_LONG => "too long",
            abi::ERROR_NOT_UTF8 => "not UTF-8",
            abi::ERROR_BAD_MODE => "bad attach mode",
            abi::ERROR_TIMED_OUT => "timed out",
            abi::ERROR_REVOKED => "revoked",
            _ => "unknown error",
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// What goes with a message. Lending borrows the memory for the call –
/// exactly the time the kernel takes it away from us.
pub enum Attach<'a> {
    Nothing,
    /// A copy: endpoints (as a sending end) and programs.
    Copy(Handle),
    /// Any handle but memory: the receiver gets it, we lose it.
    MoveHandle(Handle),
    /// The memory becomes the receiver's (if the call fails, it is freed).
    Move(Memory),
    Lend(&'a Memory),
    LendMut(&'a mut Memory),
}

impl Attach<'_> {
    pub(crate) fn encode(self) -> (u64, u64) {
        match self {
            Attach::Nothing => (abi::NO_HANDLE, abi::ATTACH_NONE),
            Attach::Copy(handle) => (handle.0, abi::ATTACH_COPY),
            Attach::MoveHandle(handle) => (handle.0, abi::ATTACH_MOVE),
            Attach::Move(memory) => (memory.into_handle().0, abi::ATTACH_MOVE),
            Attach::Lend(memory) => (memory.handle().0, abi::ATTACH_LEND_SHARED),
            Attach::LendMut(memory) => (memory.handle().0, abi::ATTACH_LEND_EXCLUSIVE),
        }
    }
}

/// Sends a message and waits for the reply. Also how kernel objects are
/// used: the console, programs (`spawn`), children (`wait`).
pub fn call(object: Handle, attach: Attach, words: [u64; 4]) -> Result<Received, Error> {
    call_timeout(object, attach, words, 0)
}

/// `call`, but gives up with `ERROR_TIMED_OUT` if no receiver takes it
/// within `ticks` (once taken, it waits for the reply).
pub fn call_timeout(
    object: Handle,
    attach: Attach,
    words: [u64; 4],
    ticks: u64,
) -> Result<Received, Error> {
    let (handle, mode) = attach.encode();
    let result = system_call(Registers {
        rax: abi::CALL,
        rdi: object.0,
        rsi: handle,
        rdx: mode,
        rcx: ticks,
        ..with_words(words)
    });
    decode(result, false)
}

/// Waits for the next message on `endpoint` (a receiving end).
pub fn receive(endpoint: Handle) -> Result<Received, Error> {
    receive_timeout(endpoint, 0)
}

/// `receive`, but gives up with `ERROR_TIMED_OUT` after `ticks`.
pub fn receive_timeout(endpoint: Handle, ticks: u64) -> Result<Received, Error> {
    let result = system_call(Registers {
        rax: abi::RECEIVE,
        rdi: endpoint.0,
        rcx: ticks,
        ..Registers::default()
    });
    decode(result, true)
}

/// Answers a call. Borrowed memory goes back now. If the reply fails (a
/// bad attachment), the duty is given up: the caller hears the other side
/// is gone.
pub fn reply(reply: Reply, attach: Attach, words: [u64; 4]) -> Result<(), Error> {
    let (handle, mode) = attach.encode();
    let result = system_call(Registers {
        rax: abi::REPLY,
        rdi: reply.0.0,
        rsi: handle,
        rdx: mode,
        ..with_words(words)
    });
    let outcome = check(result.rax);
    if outcome.is_ok() {
        core::mem::forget(reply); // used up by the kernel
    }
    outcome
}

/// Gives a handle up (memory: freed; reply: the caller is told).
pub fn drop_handle(handle: Handle) {
    system_call(Registers {
        rax: abi::DROP,
        rdi: handle.0,
        ..Registers::default()
    });
}

pub(crate) fn with_words([r10, r8, r9, r12]: [u64; 4]) -> Registers {
    Registers {
        r10,
        r8,
        r9,
        r12,
        ..Registers::default()
    }
}

pub(crate) fn check(status: u64) -> Result<(), Error> {
    if abi::is_error(status) {
        Err(Error(status))
    } else {
        Ok(())
    }
}

/// `call` with a raw handle and mode – for `println!`, whose page lives in
/// a static and isn't a `Memory` value.
pub(crate) fn call_raw(
    endpoint: Handle,
    handle: Handle,
    mode: u64,
    words: [u64; 4],
) -> Result<Received, Error> {
    let result = system_call(Registers {
        rax: abi::CALL,
        rdi: endpoint.0,
        rsi: handle.0,
        rdx: mode,
        ..with_words(words)
    });
    decode(result, false)
}
