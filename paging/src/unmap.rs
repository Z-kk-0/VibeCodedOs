//! Removing single mappings.

use crate::entry::{ADDRESS_MASK, HUGE_PAGE, PRESENT};
use crate::is_canonical;
use crate::mapper::{Mapper, TableMemory};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnmapError {
    NotMapped,
    /// The address is part of a 2 MiB page; those are unmapped as a whole.
    HugePage,
}

impl<M: TableMemory> Mapper<M> {
    /// Removes a 4 KiB mapping and returns the physical frame it led to.
    /// Tables that become empty are kept. The caller must flush the page
    /// from the CPU's translation cache (`invlpg`) afterwards.
    pub fn unmap(&mut self, virtual_address: u64) -> Result<u64, UnmapError> {
        if !is_canonical(virtual_address) {
            return Err(UnmapError::NotMapped);
        }
        let mut table = self.root;
        for level in (2..=4).rev() {
            // SAFETY: `entry` points into a valid table (TableMemory contract).
            let value = unsafe { *self.entry(table, level, virtual_address) };
            if value & PRESENT == 0 {
                return Err(UnmapError::NotMapped);
            }
            if value & HUGE_PAGE != 0 {
                return Err(UnmapError::HugePage);
            }
            table = value & ADDRESS_MASK;
        }
        let leaf = self.entry(table, 1, virtual_address);
        // SAFETY: as above.
        let value = unsafe { *leaf };
        if value & PRESENT == 0 {
            return Err(UnmapError::NotMapped);
        }
        // SAFETY: as above.
        unsafe { *leaf = 0 };
        Ok(value & ADDRESS_MASK)
    }
}

impl<M: TableMemory> Mapper<M> {
    /// Like `unmap`, but tables left empty are taken out too and handed to
    /// `released` – free them only after flushing the TLB, which also
    /// forgets cached table entries. In the upper half the level-3 tables
    /// stay: every address space shares them through copied root entries.
    pub fn unmap_and_prune(
        &mut self,
        virtual_address: u64,
        mut released: impl FnMut(u64),
    ) -> Result<u64, UnmapError> {
        let frame = self.unmap(virtual_address)?;
        // `unmap` succeeded, so the whole path exists: root, PDPT, PD, PT.
        let mut path = [self.root; 5]; // index = level
        for level in (2..=4).rev() {
            // SAFETY: `entry` points into a valid table (TableMemory contract).
            let value = unsafe { *self.entry(path[level], level as u32, virtual_address) };
            path[level - 1] = value & ADDRESS_MASK;
        }
        let upper_half = virtual_address >> 47 & 1 == 1;
        let highest_prunable = if upper_half { 2 } else { 3 };
        for level in 1..=highest_prunable {
            if !self.is_empty(path[level]) {
                break;
            }
            let parent = self.entry(path[level + 1], level as u32 + 1, virtual_address);
            // SAFETY: as above; the table it pointed to is empty and unused now.
            unsafe { *parent = 0 };
            released(path[level]);
        }
        Ok(frame)
    }

    fn is_empty(&mut self, table: u64) -> bool {
        // SAFETY: `table` is valid per the TableMemory contract.
        let entries = unsafe { &*self.memory.table(table) };
        entries.iter().all(|&entry| entry == 0)
    }
}
