//! The kernel's page tables (format and logic: the `paging` crate).
//!
//! The kernel starts on the tables the loader built (layout: `boot_info`
//! crate docs). Every change goes through `LOCK`, so the heap, the
//! scheduler and process setup can map pages without stepping on each other.
//!
//! Invariant: the upper half's *top-level* entries (physical memory window,
//! heap, kernel image) all exist before the first process is created, and
//! never change (`map` checks it). Every process copies them, so they share all kernel
//! mappings below that level – a page the kernel maps later is visible in
//! every address space at once.

use super::frames::reclaim_frame;
use super::{allocate_frame, free_frame, physical_to_pointer};
use crate::arch::{self, cpu};
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
pub fn remove_identity_map() -> u64 {
    let _guard = LOCK.lock();
    let root = cpu::page_table_root();
    KERNEL_ROOT.store(root, Ordering::Relaxed);
    // It's made of 2 MiB pages, so only its tables come back, no pages.
    // At most a few dozen: collect them on the stack (no heap yet).
    let mut tables_freed = [0u64; 64];
    let mut count = 0;
    tables(root).release_lower_half(|released| {
        if let Released::Table(table) = released {
            tables_freed[count] = table;
            count += 1;
        }
    });
    cpu::flush_translation_cache();
    // The loader made them, so the frame table says "unavailable" – hand
    // them over like reclaimed boot memory.
    for &table in &tables_freed[..count] {
        reclaim_frame(table);
    }
    count as u64 * PAGE_SIZE
}

/// Maps one 4 KiB page into the kernel's (upper) half.
pub fn map(virtual_address: u64, physical_address: u64, flags: Flags) -> Result<(), MapError> {
    let _guard = LOCK.lock();
    let root = kernel_root();
    // The invariant above, checked: a new top-level entry now would be
    // missing from every address space that already copied them.
    assert!(
        !PROCESSES_EXIST.load(Ordering::Relaxed) || root_entry_present(root, virtual_address),
        "kernel mapping {virtual_address:#x} needs a new top-level entry after processes exist"
    );
    tables(root).map(virtual_address, physical_address, PageSize::Normal, flags)
}

/// Set by the first `AddressSpace`: from then on the upper half's
/// top-level entries are copied around and must not change.
pub(super) static PROCESSES_EXIST: AtomicBool = AtomicBool::new(false);

fn root_entry_present(root: u64, virtual_address: u64) -> bool {
    let index = (virtual_address >> 39) as usize % ENTRIES_PER_TABLE;
    // SAFETY: `root` is the kernel's valid top-level table, read through
    // the window; `index` < 512.
    unsafe { (*physical_to_pointer(root).cast::<PageTable>())[index] & 1 != 0 }
}

/// Unmaps `pages` 4 KiB kernel pages from `start` and hands each frame they
/// pointed to to `release` – only once every core has forgotten the old
/// translations, so the frames are really free. Page tables that become
/// empty are freed too. One shootdown per 64 pages.
///
/// Allocates nothing: the heap calls this while holding its own lock.
pub fn unmap_range(start: u64, pages: usize, mut release: impl FnMut(u64)) {
    const BATCH: usize = 64;
    // 64 contiguous pages touch at most 2 page tables and 2 directories.
    const MAX_EMPTY_TABLES: usize = 4;
    let _guard = LOCK.lock();
    let mut tables = tables(kernel_root());
    for batch_start in (0..pages).step_by(BATCH) {
        let batch_pages = BATCH.min(pages - batch_start);
        let first = start + (batch_start as u64) * PAGE_SIZE;
        let mut frames = [0u64; BATCH];
        let mut found = 0;
        let mut empty = [0u64; MAX_EMPTY_TABLES];
        let mut emptied = 0;
        for page in (first..).step_by(PAGE_SIZE as usize).take(batch_pages) {
            let unmapped = tables.unmap_and_prune(page, |table| {
                empty[emptied] = table;
                emptied += 1;
            });
            if let Ok(frame) = unmapped {
                frames[found] = frame;
                found += 1;
            }
        }
        arch::flush_kernel_pages_everywhere(first, batch_pages as u64);
        frames[..found].iter().for_each(|&frame| release(frame));
        for &table in &empty[..emptied] {
            free_frame(FrameNumber::containing(table));
        }
    }
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

/// Unmaps one kernel page for good but keeps its frame and its tables – for
/// guard pages carved out of memory the kernel image owns.
pub fn unmap_guard_page(address: u64) {
    let _guard = LOCK.lock();
    let _ = tables(kernel_root()).unmap(address);
    arch::flush_kernel_pages_everywhere(address, 1);
}
