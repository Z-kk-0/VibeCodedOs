//! Tearing down the lower half of an address space – what's left to do
//! when a process ends.

use crate::entry::{ADDRESS_MASK, HUGE_PAGE, PRESENT};
use crate::mapper::{Mapper, TableMemory};
use crate::{ENTRIES_PER_TABLE, LOWER_HALF_ROOT_ENTRIES};

/// A frame that the lower half used and that is free now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Released {
    /// A mapped 4 KiB page.
    Page(u64),
    /// A page table below the root.
    Table(u64),
}

impl<M: TableMemory> Mapper<M> {
    /// Unmaps the whole lower half and reports every frame it used – pages
    /// and tables – to `release`. 2 MiB pages are only unmapped, not
    /// reported (we never hand those out as frames). The root and the upper
    /// half stay. Flush the TLB (reload CR3) if this space is active.
    pub fn release_lower_half(&mut self, mut release: impl FnMut(Released)) {
        let root = self.root;
        for index in LOWER_HALF_ROOT_ENTRIES {
            self.release_entry(root, index, 4, &mut release);
        }
    }

    /// Releases what entry `index` of the table at `table` (on `level`) maps.
    fn release_entry(
        &mut self,
        table: u64,
        index: usize,
        level: u32,
        release: &mut impl FnMut(Released),
    ) {
        // SAFETY: `table` is valid (TableMemory contract), index < 512.
        let entry = unsafe { self.memory.table(table).cast::<u64>().add(index) };
        // SAFETY: as above.
        let value = unsafe { *entry };
        if value & PRESENT == 0 {
            return;
        }
        let address = value & ADDRESS_MASK;
        if level == 1 {
            release(Released::Page(address));
        } else if value & HUGE_PAGE == 0 {
            for below in 0..ENTRIES_PER_TABLE {
                self.release_entry(address, below, level - 1, release);
            }
            release(Released::Table(address));
        }
        // SAFETY: as above.
        unsafe { *entry = 0 };
    }
}
