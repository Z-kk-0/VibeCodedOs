//! Messages as they sit in a task's registers (layout: `abi` crate docs).

use crate::arch::InterruptFrame;
use capability::Handle;

pub struct Message {
    pub words: [u64; 4],
    pub attachment: Option<Attachment>,
}

#[derive(Clone, Copy)]
pub struct Attachment {
    pub handle: Handle,
    pub mode: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Copy,
    Move,
    LendShared,
    LendExclusive,
}

impl Mode {
    pub fn is_lend(self) -> bool {
        matches!(self, Mode::LendShared | Mode::LendExclusive)
    }
}

impl Message {
    /// The message a `call` or `reply` passes in `frame`.
    pub fn from_frame(frame: &InterruptFrame) -> Result<Self, u64> {
        let mode = match frame.rdx {
            abi::ATTACH_NONE => None,
            abi::ATTACH_COPY => Some(Mode::Copy),
            abi::ATTACH_MOVE => Some(Mode::Move),
            abi::ATTACH_LEND_SHARED => Some(Mode::LendShared),
            abi::ATTACH_LEND_EXCLUSIVE => Some(Mode::LendExclusive),
            _ => return Err(abi::ERROR_BAD_MODE),
        };
        Ok(Self {
            words: [frame.r10, frame.r8, frame.r9, frame.r12],
            attachment: mode.map(|mode| Attachment {
                handle: Handle(frame.rsi),
                mode,
            }),
        })
    }
}

/// What a receiver (or a caller getting its reply) finds in its registers.
pub struct Delivery {
    pub badge: u64,
    pub words: [u64; 4],
    pub received: Received,
    /// For `receive`: the handle to answer with.
    pub reply: Option<Handle>,
}

/// The handle that came with a message, if any.
#[derive(Clone, Copy, Default)]
pub struct Received {
    pub handle: Option<Handle>,
    /// For memory: where it's mapped now, its size, `abi::ACCESS_*`.
    pub memory: Option<(u64, u64, u64)>,
}

impl Delivery {
    pub fn write_to(&self, frame: &mut InterruptFrame) {
        frame.rax = 0;
        frame.rdi = self.badge;
        frame.rsi = self
            .received
            .handle
            .map_or(abi::NO_HANDLE, |handle| handle.0);
        let (address, pages, access) = self.received.memory.unwrap_or_default();
        frame.rdx = address;
        frame.rcx = pages | access << 32;
        [frame.r10, frame.r8, frame.r9, frame.r12] = self.words;
        frame.r13 = self.reply.map_or(abi::NO_HANDLE, |handle| handle.0);
    }
}

/// Success with a new handle in `rsi` (`endpoint_new`, `mint`, spawning).
pub fn write_handle(frame: &mut InterruptFrame, handle: Handle) {
    write_error(frame, 0);
    frame.rsi = handle.0;
}

/// The result of a call that failed, as the caller sees it.
pub fn write_error(frame: &mut InterruptFrame, error: u64) {
    frame.rax = error;
    frame.rsi = abi::NO_HANDLE;
    frame.rdx = 0;
    frame.rcx = 0;
}
