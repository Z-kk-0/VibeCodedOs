//! Tests on page tables living in host memory.

mod mapping;
mod removing;

use crate::*;

const KERNEL_CODE: Flags = Flags {
    writable: false,
    executable: true,
    user: false,
    caching: Caching::WriteBack,
};
const HIGH: u64 = 0xFFFF_FFFF_8000_0000;

/// Page tables in host memory. "Physical address" nÂ·4096 = table nâˆ’1.
struct FakeMemory {
    tables: Vec<Box<PageTable>>,
    limit: usize,
}

impl FakeMemory {
    fn new(limit: usize) -> Self {
        Self {
            tables: Vec::new(),
            limit,
        }
    }
}

// SAFETY: each table is a separate zeroed heap allocation, addressed uniquely.
unsafe impl TableMemory for &mut FakeMemory {
    fn allocate_table(&mut self) -> Option<u64> {
        if self.tables.len() == self.limit {
            return None;
        }
        self.tables.push(Box::new([0; ENTRIES_PER_TABLE]));
        Some(self.tables.len() as u64 * PAGE_SIZE)
    }

    fn table(&mut self, physical_address: u64) -> *mut PageTable {
        &mut *self.tables[(physical_address / PAGE_SIZE) as usize - 1]
    }
}

fn mapper(memory: &mut FakeMemory) -> Mapper<&mut FakeMemory> {
    Mapper::new(memory, true).unwrap()
}
