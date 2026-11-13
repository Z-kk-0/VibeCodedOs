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
