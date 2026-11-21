//! The kernel objects a handle can stand for (DECISIONS.md, 020 and 021).

use super::endpoint::Endpoint;
pub use super::memory::{Device, Memory};
use alloc::sync::Arc;
use alloc::vec::Vec;
use capability::Handle;
use paging::Caching;
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
    /// A device's memory, not opened yet; `call` opens it (see `Device`).
    Device(Device),
    /// A device interrupt line (an ISA IRQ); `call` waits until it fires.
    Interrupt(u8),
    /// Some I/O ports; `call` reads or writes one of them.
    Ports(Ports),
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
    /// `abi::ACCESS_SHARED` or `abi::ACCESS_EXCLUSIVE`.
    pub access: u64,
    /// The lender's memory's; anything but write-back is device memory.
    pub caching: Caching,
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

/// Up to 64 I/O ports near each other: `first + n` for every bit `n` set in
/// `mask` (the keyboard controller's are 0x60 and 0x64, not 0x61–0x63).
#[derive(Clone, Copy)]
pub struct Ports {
    pub first: u16,
    pub mask: u64,
}

impl Ports {
    /// The port at `offset`, if this handle covers it.
    pub fn port(self, offset: u64) -> Option<u16> {
        let covered = offset < 64 && self.mask & (1 << offset) != 0;
        if !covered {
            return None;
        }
        self.first.checked_add(offset as u16)
    }
}

#[derive(Clone, Copy)]
pub struct ProgramCapability {
    pub name: &'static str,
    pub image: &'static [u8],
}
