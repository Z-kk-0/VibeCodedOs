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

use super::{allocate_frame, free_frame, physical_to_pointer};
use crate::arch::{self, cpu};
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicU64, Ordering};
use frame_table::FrameNumber;
use paging::{
    ENTRIES_PER_TABLE, Flags, LOWER_HALF_ROOT_ENTRIES, MapError, Mapper, PAGE_SIZE, PageSize,
    PageTable, Released, TableMemory, Translation,
};

/// Root entries 256–511: the upper half, the kernel's.
pub(super) const UPPER_HALF_ROOT_ENTRIES: core::ops::Range<usize> =
    LOWER_HALF_ROOT_ENTRIES.end..ENTRIES_PER_TABLE;

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

/// Unmaps one 4 KiB kernel page and returns the frame it pointed to. Every
/// core has forgotten the old translation when this returns, so the frame
/// is really free.
pub fn unmap(virtual_address: u64) -> Option<u64> {
    let _guard = LOCK.lock();
    let frame = tables(kernel_root()).unmap(virtual_address).ok();
    arch::flush_kernel_page_everywhere(virtual_address);
    frame
}

/// Builds the page tables the other cores start on (DECISIONS.md, 019):
/// the kernel's upper half plus `code_page` mapped at its own physical
/// address, because a starting core turns paging on while running there.
/// The root is `root_page` itself, which lies below 4 GiB – where a core
/// still in 32-bit addressing can load it into CR3.
pub fn build_core_startup_tables(root_page: u64, code_page: u64) -> Result<(), MapError> {
    let _guard = LOCK.lock();
    let root = physical_to_pointer(root_page).cast::<PageTable>();
    let kernel = physical_to_pointer(kernel_root()).cast::<PageTable>();
    // SAFETY: `root_page` is reserved for exactly this (kind `CoreStartup`);
    // both tables are reached through the physical memory window.
    unsafe {
        root.write_bytes(0, 1);
        for index in UPPER_HALF_ROOT_ENTRIES {
            (*root)[index] = (*kernel)[index];
        }
    }
    let code = Flags {
        writable: false,
        executable: true,
        user: false,
    };
    tables(root_page).map(code_page, code_page, PageSize::Normal, code)
}

/// Frees the tables `build_core_startup_tables` added below the root, once
/// no core uses them anymore (every started core switched to the kernel's
/// own tables). The code page itself stays reserved.
pub fn release_core_startup_tables(root_page: u64) {
    let _guard = LOCK.lock();
    tables(root_page).release_lower_half(|released| {
        if let Released::Table(frame) = released {
            free_frame(FrameNumber::containing(frame));
        }
    });
}

/// Where a kernel address leads, if it's mapped.
pub fn translate(virtual_address: u64) -> Option<Translation> {
    let _guard = LOCK.lock();
    tables(kernel_root()).translate(virtual_address)
}
