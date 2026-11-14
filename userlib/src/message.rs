//! What arrives: messages, replies, and the duty to answer (DECISIONS.md,
//! 020 and 021).

use crate::ipc::{Error, Handle, check, drop_handle};
use crate::memory::{Borrowed, Memory, ReceivedMemory};
use crate::system_call::Registers;

/// The right – and duty – to answer one call. Answer with `reply`;
/// dropping it instead tells the caller `ERROR_PEER_GONE`.
pub struct Reply(pub(crate) Handle);

impl Reply {
    /// For handing the duty to someone else (`Attach::MoveHandle`).
    pub fn into_handle(self) -> Handle {
        let handle = self.0;
        core::mem::forget(self);
        handle
    }

    /// A reply handle received from someone who handed theirs on.
    pub fn from_handle(handle: Handle) -> Self {
        Self(handle)
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        drop_handle(self.0);
    }
}

/// A message (or a reply).
pub struct Received {
    /// Who sent it: the badge of their handle.
    pub badge: u64,
    pub words: [u64; 4],
    /// A handle (not memory) that came with it.
    pub handle: Option<Handle>,
    pub memory: Option<ReceivedMemory>,
    /// For received messages: how to answer this one.
    pub reply: Option<Reply>,
}

/// The message in the registers. Only `receive` brings a reply handle.
pub(crate) fn decode(result: Registers, with_reply: bool) -> Result<Received, Error> {
    check(result.rax)?;
    let handle = (result.rsi != abi::NO_HANDLE).then_some(Handle(result.rsi));
    let reply = (with_reply && result.r13 != abi::NO_HANDLE).then(|| Reply(Handle(result.r13)));
    let mut received = Received {
        badge: result.rdi,
        words: [result.r10, result.r8, result.r9, result.r12],
        handle,
        memory: None,
        reply,
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
