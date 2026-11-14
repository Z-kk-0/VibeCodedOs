//! A user program as the kernel sees it: an address space, the handles it
//! holds, and what it waits for (DECISIONS.md, 018, 020 and 021).

use super::endpoint::Endpoint;
use super::object::Capability;
use crate::memory::AddressSpace;
use alloc::sync::Arc;
use capability::{HandleTable, MappingArea};
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
    /// Set while the task is blocked somewhere it may be woken from by
    /// something other than a reply: a queue (and maybe a deadline) or a
    /// child it waits for. `None` while it waits for a reply: that only
    /// ends with the reply or the reply handle's loss.
    pub waiting: Option<Waiting>,
}

pub enum Waiting {
    InQueue {
        endpoint: Arc<Endpoint>,
        as_caller: bool,
        /// The tick to give up at (`ERROR_TIMED_OUT`), if any.
        deadline: Option<u64>,
    },
    ForChild(TaskId),
}

impl Process {
    /// A program in `space` that starts with `handles` as 0, 1, 2, …
    pub fn new(space: AddressSpace, handles: impl IntoIterator<Item = Capability>) -> Self {
        let mut process = Self {
            space,
            handles: HandleTable::new(),
            area: MappingArea::new(MAPPING_START, MAPPING_END, PAGE_SIZE),
            waiting: None,
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
            // Reply handles and receiving ends were dealt with when the
            // program ended (`exit.rs`); the rest needs no cleanup.
            _ => {}
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
