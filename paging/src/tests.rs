use super::*;

const KERNEL_CODE: Flags = Flags {
    writable: false,
    executable: true,
    user: false,
};
const HIGH: u64 = 0xFFFF_FFFF_8000_0000;

/// Page tables in host memory. "Physical address" n·4096 = table n−1.
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

#[test]
fn normal_page_translates_with_offset_and_flags() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(HIGH, 0x20_0000, PageSize::Normal, KERNEL_CODE)
        .unwrap();

    let translation = mapper.translate(HIGH + 0x123).unwrap();
    assert_eq!(translation.physical_address, 0x20_0123);
    assert_eq!(translation.flags, KERNEL_CODE);
    assert_eq!(mapper.translate(HIGH + PAGE_SIZE), None);
}

#[test]
fn huge_page_covers_two_megabytes() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0x4000_0000, 0x80_0000, PageSize::Huge, Flags::KERNEL_DATA)
        .unwrap();

    let translation = mapper.translate(0x4000_0000 + 0x1F_FFFF).unwrap();
    assert_eq!(translation.physical_address, 0x80_0000 + 0x1F_FFFF);
    assert_eq!(mapper.translate(0x4000_0000 + HUGE_PAGE_SIZE), None);
}

#[test]
fn neighbouring_pages_share_their_tables() {
    let mut memory = FakeMemory::new(16);
    {
        let mut mapper = mapper(&mut memory);
        mapper
            .map(HIGH, 0x1000, PageSize::Normal, KERNEL_CODE)
            .unwrap();
        mapper
            .map(HIGH + PAGE_SIZE, 0x5000, PageSize::Normal, KERNEL_CODE)
            .unwrap();
    }
    assert_eq!(memory.tables.len(), 4); // root + PDPT + PD + one PT
}

#[test]
fn misaligned_and_non_canonical_addresses_are_refused() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    let flags = Flags::KERNEL_DATA;
    assert_eq!(
        mapper.map(HIGH + 1, 0x1000, PageSize::Normal, flags),
        Err(MapError::Misaligned)
    );
    assert_eq!(
        mapper.map(HIGH, 0x1000, PageSize::Huge, flags),
        Err(MapError::Misaligned)
    );
    assert_eq!(
        mapper.map(0x0000_8000_0000_0000, 0x1000, PageSize::Normal, flags),
        Err(MapError::NotCanonical)
    );
    assert_eq!(mapper.translate(0x0000_8000_0000_0000), None);
}

#[test]
fn mapping_a_page_to_another_frame_is_refused() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, KERNEL_CODE)
        .unwrap();
    assert_eq!(
        mapper.map(HIGH, 0x2000, PageSize::Normal, KERNEL_CODE),
        Err(MapError::AlreadyMapped)
    );
}

#[test]
fn mapping_the_same_frame_again_widens_permissions() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, KERNEL_CODE)
        .unwrap();
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();

    let flags = mapper.translate(HIGH).unwrap().flags;
    assert_eq!(
        flags,
        Flags {
            writable: true,
            executable: true,
            user: false
        }
    );
}

#[test]
fn huge_page_blocks_small_mappings_inside_it() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0, 0, PageSize::Huge, Flags::KERNEL_DATA)
        .unwrap();
    assert_eq!(
        mapper.map(0x1000, 0x1000, PageSize::Normal, Flags::KERNEL_DATA),
        Err(MapError::HugePageInTheWay)
    );
}

#[test]
fn running_out_of_table_memory_is_reported() {
    let mut memory = FakeMemory::new(2); // root + one more
    let mut mapper = mapper(&mut memory);
    assert_eq!(
        mapper.map(HIGH, 0x1000, PageSize::Normal, KERNEL_CODE),
        Err(MapError::OutOfTableMemory)
    );
}

#[test]
fn without_no_execute_support_everything_is_executable() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = Mapper::new(&mut memory, false).unwrap();
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();
    assert!(mapper.translate(HIGH).unwrap().flags.executable);
}

#[test]
fn clearing_the_lower_half_keeps_the_upper_half() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0x1000, 0x1000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, KERNEL_CODE)
        .unwrap();

    mapper.clear_root_entries(LOWER_HALF_ROOT_ENTRIES);

    assert_eq!(mapper.translate(0x1000), None);
    assert!(mapper.translate(HIGH).is_some());
}

#[test]
fn canonical_addresses() {
    assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
    assert!(is_canonical(0xFFFF_8000_0000_0000));
    assert!(!is_canonical(0x0000_8000_0000_0000));
    assert!(!is_canonical(0xFFFF_7FFF_FFFF_FFFF));
}

#[test]
fn unmapping_returns_the_frame_and_removes_the_page() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(HIGH, 0x7000, PageSize::Normal, KERNEL_CODE)
        .unwrap();

    assert_eq!(mapper.unmap(HIGH), Ok(0x7000));
    assert_eq!(mapper.translate(HIGH), None);
    assert_eq!(mapper.unmap(HIGH), Err(UnmapError::NotMapped));
    // The page can be mapped again, even to another frame.
    mapper
        .map(HIGH, 0x8000, PageSize::Normal, KERNEL_CODE)
        .unwrap();
}

#[test]
fn parts_of_huge_pages_cannot_be_unmapped() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0, 0, PageSize::Huge, Flags::KERNEL_DATA)
        .unwrap();
    assert_eq!(mapper.unmap(0x1000), Err(UnmapError::HugePage));
    assert_eq!(mapper.unmap(0x4000_0000), Err(UnmapError::NotMapped));
}

#[test]
fn releasing_the_lower_half_reports_pages_and_tables() {
    const USER: Flags = Flags {
        writable: true,
        executable: false,
        user: true,
    };
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0x40_0000, 0x1000, PageSize::Normal, USER)
        .unwrap();
    mapper
        .map(0x40_1000, 0x2000, PageSize::Normal, USER)
        .unwrap();
    mapper
        .map(0x7FFF_0000_0000, 0x3000, PageSize::Normal, USER)
        .unwrap(); // other PDPT
    mapper
        .map(HIGH, 0x9000, PageSize::Normal, KERNEL_CODE)
        .unwrap();

    let mut released = Vec::new();
    mapper.release_lower_half(|frame| released.push(frame));

    let pages: Vec<_> = released
        .iter()
        .filter(|r| matches!(r, Released::Page(_)))
        .collect();
    let tables = released.len() - pages.len();
    assert_eq!(
        pages,
        [
            &Released::Page(0x1000),
            &Released::Page(0x2000),
            &Released::Page(0x3000)
        ]
    );
    assert_eq!(tables, 6); // two chains of PDPT + PD + PT
    assert_eq!(mapper.translate(0x40_0000), None);
    assert!(mapper.translate(HIGH).is_some(), "upper half must survive");
}
