//! x86_64 4-level page tables (DECISIONS.md, 015).
//!
//! A virtual address is split into four 9-bit indices – one per table level
//! (PML4 → PDPT → PD → PT) – plus a 12-bit offset into the 4 KiB page.
//! Format: Intel SDM Vol. 3A, §4.5 "4-Level Paging", Tables 4-14 to 4-20.
//!
//! The mapper only does the bookkeeping. *Where* table memory comes from
//! and how to reach it is up to the caller (`TableMemory`), so the same
//! code runs in the loader, in the kernel, and in host unit tests.

#![cfg_attr(not(test), no_std)]

mod entry;
mod mapper;
#[cfg(test)]
mod tests;
mod unmap;

pub use entry::Flags;
pub use mapper::{MapError, Mapper, PageSize, PageTable, TableMemory, Translation};
pub use unmap::UnmapError;

pub const PAGE_SIZE: u64 = 4096;
pub const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
/// The top-level table has 512 entries; 0–255 cover the lower half of the
/// address space, 256–511 the upper ("higher") half.
pub const ENTRIES_PER_TABLE: usize = 512;
pub const LOWER_HALF_ROOT_ENTRIES: core::ops::Range<usize> = 0..256;

/// Addresses must be "canonical": bits 48–63 copy bit 47. That leaves a huge
/// unusable gap in the middle and splits the space into a lower half (user
/// programs) and an upper half (the kernel).
pub fn is_canonical(address: u64) -> bool {
    let top_bits = address >> 47;
    top_bits == 0 || top_bits == 0x1_FFFF
}
