//! Memory objects as a program sees them (DECISIONS.md, 020): owned
//! (`Memory`) or borrowed for the call being answered (`Borrowed`).

use crate::ipc::{Error, Handle, check, drop_handle};
use crate::system_call::{Registers, system_call};

/// A memory object this program owns, mapped into its address space.
/// Dropping it gives it back to the kernel.
pub struct Memory {
    pub(crate) handle: Handle,
    pub(crate) address: *mut u8,
    pub(crate) pages: u64,
}

impl Memory {
    /// `pages` fresh, zeroed pages (4 KiB each).
    pub fn new(pages: u64) -> Result<Self, Error> {
        let result = system_call(Registers {
            rax: abi::MEMORY_NEW,
            rdi: pages,
            ..Registers::default()
        });
        check(result.rax)?;
        Ok(Self {
            handle: Handle(result.rsi),
            address: result.rdx as *mut u8,
            pages,
        })
    }

    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// The handle without the duty to give it back – for moving it away.
    pub(crate) fn into_handle(self) -> Handle {
        let handle = self.handle;
        core::mem::forget(self); // the kernel hands it over, not we
        handle
    }

    /// Where it's mapped – for programs that test what happens after it
    /// isn't anymore.
    pub fn address(&self) -> *const u8 {
        self.address
    }

    pub fn len(&self) -> usize {
        (self.pages * abi::PAGE_SIZE) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.pages == 0
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: while we own it, the kernel keeps it mapped readable and
        // writable here – lending it needs a borrow of `self`.
        unsafe { core::slice::from_raw_parts(self.address, self.len()) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`, and `&mut self` makes this access exclusive.
        unsafe { core::slice::from_raw_parts_mut(self.address, self.len()) }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        drop_handle(self.handle);
    }
}

/// Memory another program lent us for the call we're answering. Valid
/// until our `reply` – the kernel unmaps it then.
pub struct Borrowed {
    pub(crate) address: *mut u8,
    pub(crate) pages: u64,
    pub(crate) exclusive: bool,
}

impl Borrowed {
    pub fn len(&self) -> usize {
        (self.pages * abi::PAGE_SIZE) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.pages == 0
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: mapped (at least readable) until we reply.
        unsafe { core::slice::from_raw_parts(self.address, self.len()) }
    }

    /// Only for `&mut` borrows; `None` for shared ones.
    pub fn bytes_mut(&mut self) -> Option<&mut [u8]> {
        // SAFETY: an exclusive borrow is mapped writable until we reply.
        self.exclusive
            .then(|| unsafe { core::slice::from_raw_parts_mut(self.address, self.len()) })
    }

    /// The raw address – for programs that test the kernel's protection.
    pub fn address(&self) -> *mut u8 {
        self.address
    }
}

pub enum ReceivedMemory {
    /// Moved to us: ours now.
    Owned(Memory),
    /// `&`: read-only, until we reply.
    Shared(Borrowed),
    /// `&mut`: ours to change, until we reply.
    Exclusive(Borrowed),
}
