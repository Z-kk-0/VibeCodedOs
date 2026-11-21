//! Creating and looking up mappings.

use super::*;
use crate::*;

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
            user: false,
            caching: Caching::WriteBack,
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
fn canonical_addresses() {
    assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
    assert!(is_canonical(0xFFFF_8000_0000_0000));
    assert!(!is_canonical(0x0000_8000_0000_0000));
    assert!(!is_canonical(0xFFFF_7FFF_FFFF_FFFF));
}

#[test]
fn caching_is_kept_and_must_not_be_mixed() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    let framebuffer = Flags {
        caching: Caching::WriteCombining,
        ..Flags::KERNEL_DATA
    };
    let registers = Flags {
        caching: Caching::Uncached,
        ..Flags::KERNEL_DATA
    };
    mapper
        .map(HIGH, 0x1000, PageSize::Normal, framebuffer)
        .unwrap();
    mapper
        .map(HIGH + 0x20_0000, 0x20_0000, PageSize::Huge, registers)
        .unwrap();
    assert_eq!(mapper.translate(HIGH).unwrap().flags, framebuffer);
    assert_eq!(mapper.translate(HIGH + 0x20_0000).unwrap().flags, registers);
    // The same frame again, but cached differently: two views that disagree.
    assert_eq!(
        mapper.map(HIGH, 0x1000, PageSize::Normal, Flags::KERNEL_DATA),
        Err(MapError::AlreadyMapped)
    );
}
