//! Waiting for several things at once (DECISIONS.md, 027).
//!
//! A `Mailbox` is an endpoint of our own. `send` is a `call` that doesn't
//! wait: its answer – the reply, or why there is none – arrives later in
//! the mailbox, tagged with a number we chose. Children (their end) and
//! interrupt lines (their next interrupt) answer the same way. So one
//! `receive` waits for all of it, and the tag says what came.
//!
//! The ownership rules hold across the wait: memory lent with a `send`
//! goes into the `Pending` it returns, and comes back out of it only after
//! its answer arrived (`Pending::finish`). Until then the kernel has it
//! mapped read-only (`Lend`) or not at all (`LendMut`) for us – and our
//! code can't reach it either, since `Pending` holds it.

use crate::endpoint::endpoint_new;
use crate::ipc::{Error, Handle, check, drop_handle, with_words};
use crate::memory::Memory;
use crate::message::{Received, decode};
use crate::system_call::{Registers, system_call};

/// An endpoint of ours that answers arrive in (and calls, if we hand out
/// sending ends of it).
pub struct Mailbox {
    endpoint: Handle,
}

/// What goes with a `send`. Memory is passed by value: moved for good, or
/// lent until the answer (then `Pending::finish` gives it back).
pub enum Send {
    Nothing,
    /// A copy: endpoints (as a sending end) and programs.
    Copy(Handle),
    /// Any handle but memory: the receiver gets it, we lose it.
    MoveHandle(Handle),
    Move(Memory),
    Lend(Memory),
    LendMut(Memory),
}

/// A `send` on its way. Keep it until its answer comes, then `finish` it.
/// Dropping it earlier keeps lent memory away for good (the kernel won't
/// free memory that is lent).
#[must_use = "lent memory comes back only through `finish`"]
pub struct Pending {
    tag: u64,
    memory: Option<Memory>,
}

/// The kernel refused a `send` at once; what was to go with it is back.
pub struct Refused {
    pub error: Error,
    pub memory: Option<Memory>,
}

/// What `Mailbox::receive` brings.
pub enum Incoming {
    /// A call to us – it comes with a reply handle.
    Call(Received),
    Answer(Answer),
}

/// The answer to one of our `send`s, found by its tag.
pub struct Answer {
    pub tag: u64,
    /// The reply, or why there is none (`ERROR_PEER_GONE`: the other side
    /// ended or dropped the call).
    pub result: Result<Received, Error>,
}

impl Mailbox {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            endpoint: endpoint_new()?,
        })
    }

    /// The receiving end – to mint sending ends of it.
    pub fn handle(&self) -> Handle {
        self.endpoint
    }

    /// Sends `words` (and `attach`) to `object` and returns at once; the
    /// answer arrives here with `tag`.
    pub fn send(
        &self,
        object: Handle,
        attach: Send,
        words: [u64; 4],
        tag: u64,
    ) -> Result<Pending, Refused> {
        let (handle, mode, memory) = match attach {
            Send::Nothing => (abi::NO_HANDLE, abi::ATTACH_NONE, None),
            Send::Copy(handle) => (handle.0, abi::ATTACH_COPY, None),
            Send::MoveHandle(handle) => (handle.0, abi::ATTACH_MOVE, None),
            Send::Move(memory) => (memory.handle().0, abi::ATTACH_MOVE, Some(memory)),
            Send::Lend(memory) => (memory.handle().0, abi::ATTACH_LEND_SHARED, Some(memory)),
            Send::LendMut(memory) => (memory.handle().0, abi::ATTACH_LEND_EXCLUSIVE, Some(memory)),
        };
        let result = system_call(Registers {
            rax: abi::SEND,
            rdi: object.0,
            rsi: handle,
            rdx: mode,
            rcx: self.endpoint.0,
            r13: tag,
            ..with_words(words)
        });
        if let Err(error) = check(result.rax) {
            return Err(Refused { error, memory });
        }
        let memory = match (mode, memory) {
            (abi::ATTACH_MOVE, Some(memory)) => {
                let _ = memory.into_handle(); // the kernel has it now
                None
            }
            (_, memory) => memory,
        };
        Ok(Pending { tag, memory })
    }

    /// Waits for the next answer or call.
    pub fn receive(&self) -> Result<Incoming, Error> {
        let result = system_call(Registers {
            rax: abi::RECEIVE,
            rdi: self.endpoint.0,
            ..Registers::default()
        });
        let is_answer = result.rax == 0 && result.r13 == abi::NO_HANDLE;
        if !is_answer {
            return decode(result, true).map(Incoming::Call);
        }
        let tag = result.rdi;
        let result = match check(result.r14) {
            Ok(()) => decode(result, false),
            Err(error) => Err(error),
        };
        Ok(Incoming::Answer(Answer { tag, result }))
    }
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        drop_handle(self.endpoint);
    }
}

impl Pending {
    pub fn tag(&self) -> u64 {
        self.tag
    }

    /// After its answer arrived: what we lent is ours again.
    pub fn finish(self) -> Option<Memory> {
        self.memory
    }
}
