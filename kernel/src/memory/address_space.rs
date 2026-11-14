//! A process's address space: its own lower half, plus the kernel's upper
//! half shared with everyone (see the invariant in `paging.rs`).

use super::paging::{LOCK, kernel_root, tables};
use super::{allocate_frame, free_frame, physical_to_pointer};
use frame_table::FrameNumber;
use paging::{
    ENTRIES_PER_TABLE, Flags, MapError, PAGE_SIZE, PageSize, PageTable, Released, Translation,
};

/// Root entries 256–511: the upper half.
const UPPER_HALF: core::ops::Range<usize> = 256..ENTRIES_PER_TABLE;

pub struct AddressSpace {
    root: u64,
}

impl AddressSpace {
    /// An empty lower half; the upper half is the kernel's.
    pub fn new() -> Option<Self> {
        let root = allocate_frame()?.start_address();
        let new_root = physical_to_pointer(root).cast::<PageTable>();
        let kernel = physical_to_pointer(kernel_root()).cast::<PageTable>();
        let _guard = LOCK.lock();
        // SAFETY: `root` is a fresh frame we own; the kernel root is valid.
        // Both are reached through the physical memory window.
        unsafe {
            new_root.write_bytes(0, 1);
            for index in UPPER_HALF {
                (*new_root)[index] = (*kernel)[index];
            }
        }
        Some(Self { root })
    }

    /// What goes into CR3 to activate this space.
    pub fn root(&self) -> u64 {
        self.root
    }

    /// Maps a fresh, zeroed frame at `virtual_address`; returns the frame.
    pub fn map_new_page(&mut self, virtual_address: u64, flags: Flags) -> Result<u64, MapError> {
        let frame = allocate_frame().ok_or(MapError::OutOfTableMemory)?;
        let physical = frame.start_address();
        // SAFETY: a fresh frame we own, reached through the window.
        unsafe { physical_to_pointer(physical).write_bytes(0, PAGE_SIZE as usize) };
        if let Err(error) = self.map(virtual_address, physical, flags) {
            free_frame(frame);
            return Err(error);
        }
        Ok(physical)
    }

    /// Maps `physical_address` again (e.g. with wider permissions).
    pub fn map(
        &mut self,
        virtual_address: u64,
        physical_address: u64,
        flags: Flags,
    ) -> Result<(), MapError> {
        let _guard = LOCK.lock();
        tables(self.root).map(virtual_address, physical_address, PageSize::Normal, flags)
    }

    pub fn translate(&self, virtual_address: u64) -> Option<Translation> {
        let _guard = LOCK.lock();
        tables(self.root).translate(virtual_address)
    }
}

impl Drop for AddressSpace {
    /// Gives every page and table of the lower half back, then the root.
    /// Must not be the active address space anymore.
    fn drop(&mut self) {
        let _guard = LOCK.lock();
        tables(self.root).release_lower_half(|released| {
            let (Released::Page(frame) | Released::Table(frame)) = released;
            free_frame(FrameNumber::containing(frame));
        });
        free_frame(FrameNumber::containing(self.root));
    }
}
