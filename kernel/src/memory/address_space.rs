//! A process's address space: its own lower half, plus the kernel's upper
//! half shared with everyone (see the invariant in `paging.rs`).

use super::paging::{LOCK, PROCESSES_EXIST, UPPER_HALF_ROOT_ENTRIES, kernel_root, tables};
use super::{allocate_frame, free_frame, physical_to_pointer};
use crate::arch::{self, CoreMask};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use frame_table::FrameNumber;
use paging::{Flags, MapError, PAGE_SIZE, PageSize, PageTable, Released, Translation};

pub struct AddressSpace {
    root: u64,
    /// The cores that have this space loaded in CR3 right now: the ones
    /// whose TLBs must forget a mapping when it changes (DECISIONS.md, 019).
    active_cores: AtomicU64,
}

impl AddressSpace {
    /// An empty lower half; the upper half is the kernel's.
    pub fn new() -> Option<Self> {
        PROCESSES_EXIST.store(true, Ordering::Relaxed);
        let root = allocate_frame()?.start_address();
        let new_root = physical_to_pointer(root).cast::<PageTable>();
        let kernel = physical_to_pointer(kernel_root()).cast::<PageTable>();
        let _guard = LOCK.lock();
        // SAFETY: `root` is a fresh frame we own; the kernel root is valid.
        // Both are reached through the physical memory window.
        unsafe {
            new_root.write_bytes(0, 1);
            for index in UPPER_HALF_ROOT_ENTRIES {
                (*new_root)[index] = (*kernel)[index];
            }
        }
        Some(Self {
            root,
            active_cores: AtomicU64::new(0),
        })
    }

    /// What goes into CR3 to activate this space.
    pub fn root(&self) -> u64 {
        self.root
    }

    /// The task switch calls this right *before* loading this space on
    /// `core`: from then on, changes reach that core's TLB.
    pub fn mark_active(&self, core: CoreMask) {
        self.active_cores.fetch_or(core, Ordering::SeqCst);
    }

    /// The task switch calls this right *after* loading another space on
    /// `core` – loading CR3 flushed its TLB, so it needs no more flushes.
    pub fn mark_inactive(&self, core: CoreMask) {
        self.active_cores.fetch_and(!core, Ordering::SeqCst);
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
        let mut tables = tables(self.root);
        let was_mapped = tables.translate(virtual_address).is_some();
        tables.map(virtual_address, physical_address, PageSize::Normal, flags)?;
        if was_mapped {
            // A core may still hold the old permissions of this page. SeqCst
            // orders this read after the table write (see `mark_active`).
            let cores = self.active_cores.load(Ordering::SeqCst);
            arch::flush_pages_on(cores, virtual_address, 1);
        }
        Ok(())
    }

    /// Removes the mappings of `pages` pages from `start`. The frames stay
    /// allocated: they belong to whoever mapped them (a memory object).
    /// With `prune`, page tables that become empty are freed too. Every core
    /// using this space has forgotten them when this returns – one
    /// shootdown for the whole range.
    pub fn unmap_range(&mut self, start: u64, pages: usize, prune: bool) {
        let _guard = LOCK.lock();
        let mut tables = tables(self.root);
        let mut empty_tables = Vec::new();
        for page in (start..).step_by(PAGE_SIZE as usize).take(pages) {
            let _ = if prune {
                tables.unmap_and_prune(page, |table| empty_tables.push(table))
            } else {
                tables.unmap(page)
            };
        }
        let cores = self.active_cores.load(Ordering::SeqCst);
        arch::flush_pages_on(cores, start, pages as u64);
        // Only now no core can still be walking them.
        for table in empty_tables {
            free_frame(FrameNumber::containing(table));
        }
    }

    pub fn translate(&self, virtual_address: u64) -> Option<Translation> {
        let _guard = LOCK.lock();
        tables(self.root).translate(virtual_address)
    }
}

impl Drop for AddressSpace {
    /// Gives every page and table of the lower half back, then the root.
    fn drop(&mut self) {
        assert_eq!(
            self.active_cores.load(Ordering::SeqCst),
            0,
            "freeing an address space that a core still uses"
        );
        let _guard = LOCK.lock();
        tables(self.root).release_lower_half(|released| {
            let (Released::Page(frame) | Released::Table(frame)) = released;
            free_frame(FrameNumber::containing(frame));
        });
        free_frame(FrameNumber::containing(self.root));
    }
}
