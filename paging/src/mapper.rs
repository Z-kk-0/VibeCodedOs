//! Creating mappings and looking them up.

use crate::entry::{ADDRESS_MASK, Flags, HUGE_PAGE, PRESENT, USER, WRITABLE};
use crate::{ENTRIES_PER_TABLE, HUGE_PAGE_SIZE, PAGE_SIZE, is_canonical};
use core::ops::Range;

pub type PageTable = [u64; ENTRIES_PER_TABLE];

/// How the mapper gets new tables and reaches existing ones.
///
/// # Safety
/// `allocate_table` must return the physical address of a zeroed, unused
/// 4 KiB frame. `table` must return a valid pointer to the table at that
/// physical address that nothing else accesses while the mapper uses it.
pub unsafe trait TableMemory {
    fn allocate_table(&mut self) -> Option<u64>;
    fn table(&mut self, physical_address: u64) -> *mut PageTable;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageSize {
    /// 4 KiB, mapped by a page table (level 1) entry.
    Normal,
    /// 2 MiB, mapped directly by a page directory (level 2) entry.
    Huge,
}

impl PageSize {
    pub fn bytes(self) -> u64 {
        match self {
            PageSize::Normal => PAGE_SIZE,
            PageSize::Huge => HUGE_PAGE_SIZE,
        }
    }

    fn leaf_level(self) -> u32 {
        match self {
            PageSize::Normal => 1,
            PageSize::Huge => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapError {
    Misaligned,
    NotCanonical,
    OutOfTableMemory,
    /// The page is already mapped to a different frame (or with another
    /// size, or other caching).
    AlreadyMapped,
    /// A huge page already covers this address.
    HugePageInTheWay,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Translation {
    pub physical_address: u64,
    pub flags: Flags,
}

pub struct Mapper<M> {
    pub(crate) root: u64,
    pub(crate) memory: M,
    use_no_execute: bool,
}

impl<M: TableMemory> Mapper<M> {
    /// A new, empty address space. Set `use_no_execute` only if the CPU
    /// supports it and EFER.NXE is (or will be) on – otherwise the NX bit
    /// is a reserved bit and every access faults.
    pub fn new(mut memory: M, use_no_execute: bool) -> Result<Self, MapError> {
        let root = memory.allocate_table().ok_or(MapError::OutOfTableMemory)?;
        Ok(Self {
            root,
            memory,
            use_no_execute,
        })
    }

    /// Works on an existing address space.
    ///
    /// # Safety
    /// `root` must be the physical address of a valid top-level table
    /// reachable through `memory`.
    pub unsafe fn from_root(root: u64, memory: M, use_no_execute: bool) -> Self {
        Self {
            root,
            memory,
            use_no_execute,
        }
    }

    /// Physical address of the top-level table – what goes into CR3.
    pub fn root(&self) -> u64 {
        self.root
    }

    /// Maps one page. Mapping the same page to the same frame again is
    /// allowed and widens its permissions (segments can share a page).
    pub fn map(
        &mut self,
        virtual_address: u64,
        physical_address: u64,
        size: PageSize,
        flags: Flags,
    ) -> Result<(), MapError> {
        if !virtual_address.is_multiple_of(size.bytes())
            || !physical_address.is_multiple_of(size.bytes())
        {
            return Err(MapError::Misaligned);
        }
        if !is_canonical(virtual_address) {
            return Err(MapError::NotCanonical);
        }

        let mut table = self.root;
        for level in (size.leaf_level() + 1..=4).rev() {
            let entry = self.entry(table, level, virtual_address);
            // SAFETY: `entry` points into a valid table (TableMemory contract).
            let value = unsafe { *entry };
            table = if value & PRESENT == 0 {
                let new_table = self
                    .memory
                    .allocate_table()
                    .ok_or(MapError::OutOfTableMemory)?;
                // Upper levels allow everything; the leaf entry decides.
                // (Effective rights are the AND over all levels.)
                // SAFETY: as above.
                unsafe { *entry = new_table | PRESENT | WRITABLE | USER };
                new_table
            } else if value & HUGE_PAGE != 0 {
                return Err(MapError::HugePageInTheWay);
            } else {
                value & ADDRESS_MASK
            };
        }

        let leaf = self.entry(table, size.leaf_level(), virtual_address);
        let huge_bit = if size == PageSize::Huge { HUGE_PAGE } else { 0 };
        // SAFETY: as above.
        let existing = unsafe { *leaf };
        let flags = if existing & PRESENT == 0 {
            flags
        } else if existing & ADDRESS_MASK == physical_address
            && existing & HUGE_PAGE == huge_bit
            && Flags::from_bits(existing).caching == flags.caching
        {
            flags.union(Flags::from_bits(existing))
        } else {
            return Err(MapError::AlreadyMapped);
        };
        // SAFETY: as above.
        unsafe { *leaf = physical_address | huge_bit | flags.to_bits(self.use_no_execute) };
        Ok(())
    }

    /// Where a virtual address leads, if anywhere.
    pub fn translate(&mut self, virtual_address: u64) -> Option<Translation> {
        if !is_canonical(virtual_address) {
            return None;
        }
        let mut table = self.root;
        for level in (1..=4).rev() {
            // SAFETY: `entry` points into a valid table (TableMemory contract).
            let value = unsafe { *self.entry(table, level, virtual_address) };
            if value & PRESENT == 0 {
                return None;
            }
            if level == 1 || value & HUGE_PAGE != 0 {
                let page_size = 1u64 << (12 + 9 * (level - 1));
                let page_start = value & ADDRESS_MASK & !(page_size - 1);
                return Some(Translation {
                    physical_address: page_start + (virtual_address & (page_size - 1)),
                    flags: Flags::from_bits(value),
                });
            }
            table = value & ADDRESS_MASK;
        }
        None
    }

    /// Removes whole top-level entries – e.g. the entire lower half at once.
    /// The tables below them are not freed. Flush the TLB afterwards.
    pub fn clear_root_entries(&mut self, entries: Range<usize>) {
        let root = self.memory.table(self.root);
        for index in entries {
            // SAFETY: root is valid (contract), index < 512 by `Range` users.
            unsafe { (*root)[index] = 0 };
        }
    }

    pub(crate) fn entry(&mut self, table: u64, level: u32, virtual_address: u64) -> *mut u64 {
        let index = (virtual_address >> (12 + 9 * (level - 1))) as usize % ENTRIES_PER_TABLE;
        // SAFETY: `table` is valid per the TableMemory contract; index < 512.
        unsafe { self.memory.table(table).cast::<u64>().add(index) }
    }
}
