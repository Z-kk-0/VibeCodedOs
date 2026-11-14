//! The kernel's page tables (format and logic: the `paging` crate).
//!
//! The kernel starts on the tables the loader built (layout: `boot_info`
//! crate docs). Every change goes through `LOCK`, so the heap, the
//! scheduler and process setup can map pages without stepping on each other.
//!
//! Invariant: the upper half's *top-level* entries (physical memory window,
//! heap, kernel image) all exist before the first process is created, and
//! never change. Every process copies them, so they share all kernel
//! mappings below that level – a page the kernel maps later is visible in
//! every address space at once.

use super::{allocate_frame, physical_to_pointer};
use crate::arch::cpu;
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicU64, Ordering};
use paging::{
    Flags, LOWER_HALF_ROOT_ENTRIES, MapError, Mapper, PAGE_SIZE, PageSize, PageTable, TableMemory,
    Translation,
};

pub(super) static LOCK: SpinLock<()> = SpinLock::new(());

/// Root of the kernel's own address space (set when the identity map goes).
static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);

/// New tables come from the frame allocator; all tables are reached through
/// the physical memory window.
pub(super) struct KernelTables;

// SAFETY: frames from the allocator are unused until we use them and get
// zeroed here; the window maps all physical memory.
unsafe impl TableMemory for KernelTables {
    fn allocate_table(&mut self) -> Option<u64> {
        let address = allocate_frame()?.start_address();
        // SAFETY: a fresh frame we own, reached through the window.
        unsafe { physical_to_pointer(address).write_bytes(0, PAGE_SIZE as usize) };
        Some(address)
    }

    fn table(&mut self, physical_address: u64) -> *mut PageTable {
        physical_to_pointer(physical_address).cast()
    }
}

/// The page tables starting at `root`. Hold `LOCK` while using them.
pub(super) fn tables(root: u64) -> Mapper<KernelTables> {
    // SAFETY: callers only pass roots of valid address spaces.
    unsafe { Mapper::from_root(root, KernelTables, cpu::no_execute_enabled()) }
}

pub fn kernel_root() -> u64 {
    KERNEL_ROOT.load(Ordering::Relaxed)
}

/// Removes the loader's identity map. Afterwards the whole lower half is
/// empty – reserved for user programs – and a stray low pointer faults.
pub fn remove_identity_map() {
    let _guard = LOCK.lock();
    let root = cpu::page_table_root();
    KERNEL_ROOT.store(root, Ordering::Relaxed);
    tables(root).clear_root_entries(LOWER_HALF_ROOT_ENTRIES);
    cpu::flush_translation_cache();
}

/// Maps one 4 KiB page into the kernel's (upper) half.
pub fn map(virtual_address: u64, physical_address: u64, flags: Flags) -> Result<(), MapError> {
    let _guard = LOCK.lock();
    tables(kernel_root()).map(virtual_address, physical_address, PageSize::Normal, flags)
}

/// Unmaps one 4 KiB kernel page and returns the frame it pointed to.
pub fn unmap(virtual_address: u64) -> Option<u64> {
    let _guard = LOCK.lock();
    let frame = tables(kernel_root()).unmap(virtual_address).ok();
    cpu::flush_translation(virtual_address);
    frame
}

/// Where a kernel address leads, if it's mapped.
pub fn translate(virtual_address: u64) -> Option<Translation> {
    let _guard = LOCK.lock();
    tables(kernel_root()).translate(virtual_address)
}
