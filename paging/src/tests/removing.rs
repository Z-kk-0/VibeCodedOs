//! Removing mappings: single pages, whole halves.

use super::*;
use crate::*;

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

#[test]
fn pruning_frees_tables_only_when_they_become_empty() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(0x1000, 0x7000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();
    mapper
        .map(0x2000, 0x8000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();
    let mut released = Vec::new();

    assert_eq!(
        mapper.unmap_and_prune(0x1000, |table| released.push(table)),
        Ok(0x7000)
    );
    assert!(released.is_empty(), "the page table still maps 0x2000");

    assert_eq!(
        mapper.unmap_and_prune(0x2000, |table| released.push(table)),
        Ok(0x8000)
    );
    assert_eq!(
        released.len(),
        3,
        "page table, directory and PDPT all empty now"
    );
    assert_eq!(mapper.translate(0x2000), None);
    // Mapping there again builds fresh tables.
    mapper
        .map(0x1000, 0x7000, PageSize::Normal, Flags::KERNEL_DATA)
        .unwrap();
    assert!(mapper.translate(0x1000).is_some());
}

#[test]
fn pruning_keeps_the_shared_upper_half_tables() {
    let mut memory = FakeMemory::new(16);
    let mut mapper = mapper(&mut memory);
    mapper
        .map(HIGH, 0x7000, PageSize::Normal, KERNEL_CODE)
        .unwrap();
    let mut released = Vec::new();
    mapper
        .unmap_and_prune(HIGH, |table| released.push(table))
        .unwrap();
    assert_eq!(released.len(), 2, "page table and directory, not the PDPT");
}
