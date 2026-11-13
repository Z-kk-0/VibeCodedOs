use super::*;

const UNSET: FrameEntry = FrameEntry {
    state: FrameState::Free,
    next_free: 0,
};

/// A table of 16 frames where frames 2..6 and 10..12 are free.
fn table(entries: &mut [FrameEntry; 16]) -> FrameTable<'_> {
    let mut table = FrameTable::new(entries);
    table.mark(2..6, FrameState::Free);
    table.mark(10..12, FrameState::Free);
    table.build_free_list();
    table
}

#[test]
fn new_table_has_nothing_to_give() {
    let mut entries = [UNSET; 16];
    let mut table = FrameTable::new(&mut entries);
    table.build_free_list();
    assert_eq!(table.allocate(), None);
    assert_eq!(table.state(FrameNumber(3)), FrameState::Unavailable);
}

#[test]
fn allocates_lowest_frames_first_until_empty() {
    let mut entries = [UNSET; 16];
    let mut table = table(&mut entries);
    assert_eq!(table.free_count(), 6);

    let allocated: Vec<u64> = core::iter::from_fn(|| table.allocate())
        .map(|frame| frame.0)
        .collect();

    assert_eq!(allocated, [2, 3, 4, 5, 10, 11]);
    assert_eq!(table.free_count(), 0);
    assert_eq!(table.state(FrameNumber(4)), FrameState::Allocated);
}

#[test]
fn freed_frame_is_handed_out_next() {
    let mut entries = [UNSET; 16];
    let mut table = table(&mut entries);
    let first = table.allocate().unwrap();
    let _second = table.allocate().unwrap();

    table.free(first).unwrap();

    assert_eq!(table.state(first), FrameState::Free);
    assert_eq!(table.allocate(), Some(first));
}

#[test]
fn double_free_is_caught() {
    let mut entries = [UNSET; 16];
    let mut table = table(&mut entries);
    let frame = table.allocate().unwrap();
    table.free(frame).unwrap();

    assert_eq!(
        table.free(frame),
        Err(FreeError::NotAllocated(FrameState::Free))
    );
    assert_eq!(table.free_count(), 6);
}

#[test]
fn freeing_unmanaged_frames_is_refused() {
    let mut entries = [UNSET; 16];
    let mut table = table(&mut entries);
    assert_eq!(
        table.free(FrameNumber(0)),
        Err(FreeError::NotAllocated(FrameState::Unavailable))
    );
    assert_eq!(table.free(FrameNumber(99)), Err(FreeError::OutOfRange));
}

#[test]
fn later_marks_override_earlier_ones() {
    let mut entries = [UNSET; 16];
    let mut table = FrameTable::new(&mut entries);
    table.mark(0..16, FrameState::Free);
    table.mark(4..8, FrameState::Allocated); // e.g. where the table itself lives
    table.mark(0..1, FrameState::Unavailable);
    table.build_free_list();

    assert_eq!(table.free_count(), 11);
    assert_eq!(table.allocate(), Some(FrameNumber(1)));
}

#[test]
fn marks_beyond_the_table_are_ignored() {
    let mut entries = [UNSET; 16];
    let mut table = FrameTable::new(&mut entries);
    table.mark(14..1000, FrameState::Free);
    table.build_free_list();
    assert_eq!(table.free_count(), 2);
}

#[test]
fn frame_numbers_and_addresses_convert() {
    assert_eq!(FrameNumber::containing(0x2FFF), FrameNumber(2));
    assert_eq!(FrameNumber(3).start_address(), 0x3000);
    assert_eq!(FrameTable::bytes_needed(512), 4096);
}
