//! The kernel objects a handle can stand for (DECISIONS.md, 020).

use super::message::Message;
use crate::memory::{allocate_frame, free_frame, physical_to_pointer};
use crate::sync::SpinLock;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use frame_table::FrameNumber;
use paging::PAGE_SIZE;
use scheduling::TaskId;

/// What one slot of a process's handle table holds.
pub enum Capability {
    Endpoint(EndpointCapability),
    /// Memory this process owns – mapped in its address space.
    Memory(OwnedMemory),
    /// Memory another process lent us for the current call.
    Borrowed(BorrowedMemory),
    /// The kernel's text output. Only the `log` service gets one.
    Console,
}

impl Capability {
    /// The receiving end of `endpoint`: may receive and give copies away.
    pub fn receiver(endpoint: &Arc<Endpoint>) -> Self {
        Self::Endpoint(EndpointCapability {
            endpoint: endpoint.clone(),
            badge: 0,
            can_receive: true,
        })
    }

    /// A sending end of `endpoint`, badged with `name` (≤ 8 bytes), so the
    /// receiver knows who's talking.
    pub fn sender(endpoint: &Arc<Endpoint>, name: &str) -> Self {
        Self::Endpoint(EndpointCapability {
            endpoint: endpoint.clone(),
            badge: abi::badge_from_name(name),
            can_receive: false,
        })
    }
}

#[derive(Clone)]
pub struct EndpointCapability {
    pub endpoint: Arc<Endpoint>,
    /// Delivered with every message sent through this handle.
    pub badge: u64,
    pub can_receive: bool,
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

/// A rendezvous point: servers wait here for messages, callers for servers.
/// Only touched with the scheduler lock held, so the inner lock is never
/// contended – it's there to make sharing between processes sound.
pub struct Endpoint {
    queues: SpinLock<Queues>,
}

#[derive(Default)]
struct Queues {
    receivers: VecDeque<TaskId>,
    callers: VecDeque<WaitingCall>,
}

/// A caller waiting for a receiver. The attachment is still the caller's:
/// it changes hands only when the message is delivered.
pub struct WaitingCall {
    pub caller: TaskId,
    pub badge: u64,
    pub message: Message,
}

impl Endpoint {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queues: SpinLock::new(Queues::default()),
        })
    }

    pub fn push_receiver(&self, task: TaskId) {
        self.queues.lock().receivers.push_back(task);
    }

    pub fn pop_receiver(&self) -> Option<TaskId> {
        self.queues.lock().receivers.pop_front()
    }

    /// For a receiver that must keep its place (delivery to it failed).
    pub fn return_receiver(&self, task: TaskId) {
        self.queues.lock().receivers.push_front(task);
    }

    pub fn push_caller(&self, call: WaitingCall) {
        self.queues.lock().callers.push_back(call);
    }

    pub fn pop_caller(&self) -> Option<WaitingCall> {
        self.queues.lock().callers.pop_front()
    }
}
