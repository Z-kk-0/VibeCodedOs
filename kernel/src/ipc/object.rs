//! The kernel objects a handle can stand for (DECISIONS.md, 020 and 021).

use super::endpoint::Endpoint;
use crate::memory::{allocate_frame, free_frame, physical_to_pointer};
use alloc::sync::Arc;
use alloc::vec::Vec;
use capability::Handle;
use frame_table::FrameNumber;
use paging::PAGE_SIZE;
use scheduling::TaskId;

/// What one slot of a process's handle table holds.
pub enum Capability {
    Endpoint(EndpointCapability),
    /// Memory this process owns – mapped in its address space.
    Memory(OwnedMemory),
    /// Memory another process lent us for a call we're answering.
    Borrowed(BorrowedMemory),
    /// The kernel's text output. Only the `log` service gets one.
    Console,
    /// The right – and duty – to answer one call. Used up by `reply`.
    Reply(ReplyCapability),
    /// A program image that `call` starts as a new process.
    Program(ProgramCapability),
    /// A process we started; `call` waits until it has ended.
    Child(TaskId),
}

impl Capability {
    /// A new receiving end of `endpoint`.
    pub fn receiving_end(endpoint: &Arc<Endpoint>) -> Self {
        endpoint.add_receiving_end();
        Self::Endpoint(EndpointCapability {
            endpoint: endpoint.clone(),
            badge: 0,
            generation: 0,
            can_receive: true,
        })
    }

    /// Whether this holds a receiving end – whose loss may close its endpoint.
    pub fn is_receiving_end(&self) -> bool {
        matches!(self, Capability::Endpoint(endpoint) if endpoint.can_receive)
    }
}

#[derive(Clone)]
pub struct EndpointCapability {
    pub endpoint: Arc<Endpoint>,
    /// Delivered with every message sent through this handle.
    pub badge: u64,
    /// The badge's generation when this end was minted (see `revoke`).
    pub generation: u64,
    pub can_receive: bool,
}

impl EndpointCapability {
    /// A sending end of the same endpoint with `badge`.
    pub fn mint(&self, badge: u64) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            badge,
            generation: self.endpoint.generation(badge),
            can_receive: false,
        }
    }

    /// What a copy of this handle is: always a sending end. A copy of a
    /// sending end keeps its badge and generation – revoking catches it too.
    pub fn copy(&self) -> Self {
        if self.can_receive {
            self.mint(self.badge)
        } else {
            Self {
                can_receive: false,
                ..self.clone()
            }
        }
    }

    pub fn is_revoked(&self) -> bool {
        !self.can_receive && self.endpoint.generation(self.badge) != self.generation
    }
}

pub struct OwnedMemory {
    pub memory: Memory,
    /// Where it's mapped in the owner's address space.
    pub address: u64,
    /// Lent out right now (the owner waits in `call` for the reply).
    pub lent: bool,
}

pub struct BorrowedMemory {
    /// The lender's frames – not ours to free.
    pub frames: Vec<u64>,
    pub address: u64,
}

pub struct ReplyCapability {
    pub caller: TaskId,
    pub lend: Option<Lend>,
}

/// A borrow that ends with the reply – or when the reply handle is gone.
#[derive(Clone, Copy)]
pub struct Lend {
    /// The owner's handle (in the caller).
    pub lender: Handle,
    /// Who holds the borrowed memory, and under which handle.
    pub borrower_task: TaskId,
    pub borrower: Handle,
}

#[derive(Clone, Copy)]
pub struct ProgramCapability {
    pub name: &'static str,
    pub image: &'static [u8],
}

/// Physical pages, zeroed when made, freed when dropped.
pub struct Memory {
    frames: Vec<FrameNumber>,
}

impl Memory {
    pub fn new(pages: u64) -> Option<Self> {
        let mut memory = Self { frames: Vec::new() };
        for _ in 0..pages {
            // On failure, dropping `memory` returns what we got so far.
            let frame = allocate_frame()?;
            // SAFETY: a fresh frame we own, reached through the window.
            unsafe {
                physical_to_pointer(frame.start_address()).write_bytes(0, PAGE_SIZE as usize)
            };
            memory.frames.push(frame);
        }
        Some(memory)
    }

    pub fn frames(&self) -> Vec<u64> {
        self.frames
            .iter()
            .map(|frame| frame.start_address())
            .collect()
    }

    pub fn pages(&self) -> u64 {
        self.frames.len() as u64
    }
}

impl Drop for Memory {
    /// Must not be mapped anywhere anymore (the owner unmaps it first).
    fn drop(&mut self) {
        for frame in self.frames.drain(..) {
            free_frame(frame);
        }
    }
}
