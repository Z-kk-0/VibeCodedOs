//! The kernel's page tables (format and logic: the `paging` crate).
//!
//! The kernel starts on the tables the loader built (layout: `boot_info`
//! crate docs). There is no lock yet: only boot code changes mappings.

use super::{allocate_frame, physical_to_pointer};
use crate::arch::cpu;
use paging::{LOWER_HALF_ROOT_ENTRIES, Mapper, PAGE_SIZE, PageTable, TableMemory};

/// New tables come from the frame allocator; all tables are reached through
/// the physical memory window.
struct KernelTables;

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

fn active_tables() -> Mapper<KernelTables> {
    // SAFETY: CR3 holds the root of the valid, active address space.
    unsafe {
        Mapper::from_root(
            cpu::page_table_root(),
            KernelTables,
            cpu::no_execute_enabled(),
        )
    }
}

/// Removes the loader's identity map. Afterwards the whole lower half is
/// empty – reserved for user programs – and a stray low pointer faults.
pub fn remove_identity_map() {
    active_tables().clear_root_entries(LOWER_HALF_ROOT_ENTRIES);
    cpu::flush_translation_cache();
}

/// The physical address a virtual address leads to, if it's mapped.
pub fn translate(virtual_address: u64) -> Option<u64> {
    active_tables()
        .translate(virtual_address)
        .map(|translation| translation.physical_address)
}
