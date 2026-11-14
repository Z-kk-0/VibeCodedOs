//! Handles, memory objects and messages (DECISIONS.md, 020).

use crate::memory::{Borrowed, Memory, ReceivedMemory};
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
            abi::ERROR_NO_CALLER => "no caller to reply to",
            abi::ERROR_PEER_GONE => "the other side is gone",
            abi::ERROR_OUT_OF_MEMORY => "out of memory",
            abi::ERROR_TOO_LONG => "too long",
            abi::ERROR_NOT_UTF8 => "not UTF-8",
            abi::ERROR_BAD_MODE => "bad attach mode",
            _ => "unknown error",
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A message (or a reply).
pub struct Received {
    /// Who sent it: the badge of their handle (the kernel uses program names).
    pub badge: u64,
    pub words: [u64; 4],
    /// An endpoint handle that came with it.
    pub handle: Option<Handle>,
    pub memory: Option<ReceivedMemory>,
}

/// What goes with a message. Lending borrows the memory for the call –
/// exactly the time the kernel takes it away from us.
pub enum Attach<'a> {
    Nothing,
    /// Gives the receiver a sending handle to the same endpoint.
    CopyEndpoint(Handle),
    MoveEndpoint(Handle),
    /// The memory becomes the receiver's (if the call fails, it is freed).
    Move(Memory),
    Lend(&'a Memory),
    LendMut(&'a mut Memory),
}

impl Attach<'_> {
    fn encode(self) -> (u64, u64) {
        match self {
            Attach::Nothing => (abi::NO_HANDLE, abi::ATTACH_NONE),
            Attach::CopyEndpoint(handle) => (handle.0, abi::ATTACH_COPY),
            Attach::MoveEndpoint(handle) => (handle.0, abi::ATTACH_MOVE),
            Attach::Move(memory) => {
                let handle = memory.handle;
                core::mem::forget(memory); // the kernel hands it over, not we
                (handle.0, abi::ATTACH_MOVE)
            }
            Attach::Lend(memory) => (memory.handle.0, abi::ATTACH_LEND_SHARED),
            Attach::LendMut(memory) => (memory.handle.0, abi::ATTACH_LEND_EXCLUSIVE),
        }
    }
}

/// Sends a message to `endpoint` and waits for the reply.
pub fn call(endpoint: Handle, attach: Attach, words: [u64; 4]) -> Result<Received, Error> {
    let (handle, mode) = attach.encode();
    let result = system_call(Registers {
        rax: abi::CALL,
        rdi: endpoint.0,
        rsi: handle,
        rdx: mode,
        ..with_words(words)
    });
    decode(result)
}

/// Waits for the next message on `endpoint` (which we must be able to
/// receive on). Answer it with `reply` before receiving the next one.
pub fn receive(endpoint: Handle) -> Result<Received, Error> {
    let result = system_call(Registers {
        rax: abi::RECEIVE,
        rdi: endpoint.0,
        ..Registers::default()
    });
    decode(result)
}

/// Answers the last received message. Borrowed memory goes back now.
pub fn reply(attach: Attach, words: [u64; 4]) -> Result<(), Error> {
    let (handle, mode) = attach.encode();
    let result = system_call(Registers {
        rax: abi::REPLY,
        rsi: handle,
        rdx: mode,
        ..with_words(words)
    });
    check(result.rax)
}

pub(crate) fn drop_handle(handle: Handle) {
    system_call(Registers {
        rax: abi::DROP,
        rdi: handle.0,
        ..Registers::default()
    });
}

fn with_words([r10, r8, r9, r12]: [u64; 4]) -> Registers {
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

fn decode(result: Registers) -> Result<Received, Error> {
    check(result.rax)?;
    let handle = (result.rsi != abi::NO_HANDLE).then_some(Handle(result.rsi));
    let mut received = Received {
        badge: result.rdi,
        words: [result.r10, result.r8, result.r9, result.r12],
        handle,
        memory: None,
    };
    if result.rdx != 0 {
        let (address, pages) = (result.rdx as *mut u8, result.rcx & 0xFFFF_FFFF);
        let borrowed = |exclusive| Borrowed {
            address,
            pages,
            exclusive,
        };
        received.memory = match result.rcx >> 32 {
            abi::ACCESS_OWNED => Some(ReceivedMemory::Owned(Memory {
                handle: received.handle.take().expect("memory comes with a handle"),
                address,
                pages,
            })),
            abi::ACCESS_SHARED => Some(ReceivedMemory::Shared(borrowed(false))),
            _ => Some(ReceivedMemory::Exclusive(borrowed(true))),
        };
        // Borrowed memory's handle is the kernel's business: it goes with the reply.
        received.handle = None;
    }
    Ok(received)
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
    decode(result)
}
