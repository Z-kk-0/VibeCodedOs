//! A user program as the kernel sees it: an address space, the handles it
//! holds, and the call it still has to answer (DECISIONS.md, 018 and 020).

use super::object::Capability;
use crate::memory::AddressSpace;
use capability::{Handle, HandleTable, MappingArea};
use paging::{Flags, PAGE_SIZE};
use scheduling::TaskId;

/// Where memory objects appear: far from the program image (low) and the
/// stack (top of the lower half).
const MAPPING_START: u64 = 0x0000_1000_0000_0000;
const MAPPING_END: u64 = 0x0000_7000_0000_0000;

pub struct Process {
    pub space: AddressSpace,
    pub handles: HandleTable<Capability>,
    area: MappingArea,
    /// The call this process received and must answer with `reply`.
    pub replying_to: Option<PendingCall>,
}

pub struct PendingCall {
    pub caller: TaskId,
    pub lend: Option<Lend>,
}

/// A borrow that ends with the reply.
#[derive(Clone, Copy)]
pub struct Lend {
    /// The owner's handle (in the caller).
    pub lender: Handle,
    /// The borrowed handle (in the receiver).
    pub borrower: Handle,
}

impl Process {
    /// A program in `space` that starts with `handles` as 0, 1, 2, …
    pub fn new(space: AddressSpace, handles: impl IntoIterator<Item = Capability>) -> Self {
        let mut process = Self {
            space,
            handles: HandleTable::new(),
            area: MappingArea::new(MAPPING_START, MAPPING_END, PAGE_SIZE),
            replying_to: None,
        };
        for capability in handles {
            process.handles.insert(capability);
        }
        process
    }

    /// Maps `frames` at a fresh address of the mapping area and returns it.
    /// On failure nothing stays mapped.
    pub fn map_frames(&mut self, frames: &[u64], writable: bool) -> Option<u64> {
        let start = self.area.reserve(frames.len() as u64)?;
        for (page, &frame) in (start..).step_by(PAGE_SIZE as usize).zip(frames) {
            if self.space.map(page, frame, user_flags(writable)).is_err() {
                self.unmap_pages(start, frames.len());
                return None;
            }
        }
        Some(start)
    }

    /// Maps `frames` at `address` again, with new permissions.
    pub fn remap_frames(&mut self, address: u64, frames: &[u64], writable: bool) {
        self.unmap_pages(address, frames.len());
        for (page, &frame) in (address..).step_by(PAGE_SIZE as usize).zip(frames) {
            // The page tables for these addresses exist already (the pages
            // were mapped before), so nothing needs allocating.
            self.space
                .map(page, frame, user_flags(writable))
                .expect("remapping a page whose tables exist");
        }
    }

    /// Removes the mappings; the frames stay allocated (their memory object
    /// owns them).
    pub fn unmap_pages(&mut self, address: u64, pages: usize) {
        for page in (address..).step_by(PAGE_SIZE as usize).take(pages) {
            self.space.unmap(page);
        }
    }
}

impl Drop for Process {
    /// Memory objects are mapped in `space`, but their frames belong to
    /// them: unmap first, so the address space doesn't free the frames as
    /// its own – then the objects free them. Runs before `space` drops.
    fn drop(&mut self) {
        let mut handles = core::mem::take(&mut self.handles);
        handles.drain(|capability| match capability {
            Capability::Memory(owned) => {
                self.unmap_pages(owned.address, owned.memory.pages() as usize);
            }
            Capability::Borrowed(borrowed) => {
                self.unmap_pages(borrowed.address, borrowed.frames.len());
            }
            Capability::Endpoint(_) | Capability::Console => {}
        });
    }
}

fn user_flags(writable: bool) -> Flags {
    Flags {
        writable,
        executable: false,
        user: true,
    }
}
