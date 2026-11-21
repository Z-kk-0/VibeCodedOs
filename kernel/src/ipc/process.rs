//! A user program as the kernel sees it: an address space, the handles it
//! holds, and what it waits for (DECISIONS.md, 018, 020 and 021).

use super::endpoint::Endpoint;
use super::object::Capability;
use crate::memory::AddressSpace;
use alloc::sync::Arc;
use capability::{HandleTable, MappingArea};
use paging::{Caching, Flags, PAGE_SIZE};
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
    /// Its parent asked to stop it (DECISIONS.md, 026): it ends the next
    /// time it would return to user mode (`stop.rs`).
    pub stop_requested: bool,
}

pub enum Waiting {
    InQueue {
        endpoint: Arc<Endpoint>,
        as_caller: bool,
        /// The tick to give up at (`ERROR_TIMED_OUT`), if any.
        deadline: Option<u64>,
    },
    ForChild {
        child: TaskId,
        /// The tick to give up at (`ERROR_TIMED_OUT`), if any.
        deadline: Option<u64>,
    },
    /// For a device interrupt (`interrupt.rs`), maybe with a deadline.
    ForInterrupt { irq: u8, deadline: Option<u64> },
}

impl Process {
    /// A program in `space` that starts with `handles` as 0, 1, 2, …
    pub fn new(space: AddressSpace, handles: impl IntoIterator<Item = Capability>) -> Self {
        let mut process = Self {
            space,
            handles: HandleTable::new(),
            area: MappingArea::new(MAPPING_START, MAPPING_END, PAGE_SIZE),
            waiting: None,
            stop_requested: false,
        };
        for capability in handles {
            process.handles.insert(capability);
        }
        process
    }

    /// Maps `frames` at a fresh address of the mapping area and returns it.
    /// On failure nothing stays mapped. `caching` comes from the memory
    /// object: write-back for RAM, what the device needs for device memory.
    pub fn map_frames(&mut self, frames: &[u64], writable: bool, caching: Caching) -> Option<u64> {
        self.map_frames_aligned(frames, writable, caching, PAGE_SIZE)
    }

    /// The same, at an address that is a multiple of `align` (a power of two).
    pub fn map_frames_aligned(
        &mut self,
        frames: &[u64],
        writable: bool,
        caching: Caching,
        align: u64,
    ) -> Option<u64> {
        let start = self.area.reserve_aligned(frames.len() as u64, align)?;
        for (page, &frame) in (start..).step_by(PAGE_SIZE as usize).zip(frames) {
            if self
                .space
                .map(page, frame, user_flags(writable, caching))
                .is_err()
            {
                self.release_pages(start, frames.len());
                return None;
            }
        }
        Some(start)
    }

    /// Maps `frames` at `address` again, with new permissions.
    pub fn remap_frames(&mut self, address: u64, frames: &[u64], writable: bool, caching: Caching) {
        self.unmap_pages(address, frames.len());
        for (page, &frame) in (address..).step_by(PAGE_SIZE as usize).zip(frames) {
            // The page tables for these addresses exist already (the pages
            // were mapped before), so nothing needs allocating.
            self.space
                .map(page, frame, user_flags(writable, caching))
                .expect("remapping a page whose tables exist");
        }
    }

    /// Removes the mappings; the frames stay allocated (their memory object
    /// owns them).
    pub fn unmap_pages(&mut self, address: u64, pages: usize) {
        self.space.unmap_range(address, pages, false);
    }

    /// For memory this process is done with for good: also frees the page
    /// tables that become empty. The addresses are never handed out again
    /// (see `MappingArea`), so a stale pointer keeps faulting.
    pub fn release_pages(&mut self, address: u64, pages: usize) {
        self.space.unmap_range(address, pages, true);
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
            // Nobody can ask how this child ended anymore.
            Capability::Child(child) => crate::task::forget_exit(child),
            // Reply handles and receiving ends were dealt with when the
            // program ended (`exit.rs`); the rest needs no cleanup.
            _ => {}
        });
    }
}

fn user_flags(writable: bool, caching: Caching) -> Flags {
    Flags {
        writable,
        executable: false,
        user: true,
        caching,
    }
}
