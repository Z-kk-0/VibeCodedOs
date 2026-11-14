use super::*;
use std::alloc::{alloc, dealloc};
use std::collections::HashMap;

/// Pages from the host allocator, with a limit to test running out.
struct FakePages {
    limit: usize,
    mapped: usize,
    unmapped: usize,
    /// Alignment of every live allocation, to free it with the same layout.
    alignments: HashMap<usize, usize>,
}

// SAFETY: every call returns a fresh host allocation of the asked size and
// alignment; unmap frees exactly that allocation.
unsafe impl PageSource for &mut FakePages {
    fn map_pages(&mut self, count: usize, align: usize) -> Option<NonNull<u8>> {
        if self.mapped + count > self.limit {
            return None;
        }
        self.mapped += count;
        // SAFETY: non-zero size, valid alignment.
        let start = NonNull::new(unsafe { alloc(page_layout(count, align)) })?;
        self.alignments.insert(start.as_ptr() as usize, align);
        Some(start)
    }

    fn unmap_pages(&mut self, start: NonNull<u8>, count: usize) {
        self.unmapped += count;
        self.mapped -= count;
        let align = self.alignments.remove(&(start.as_ptr() as usize)).unwrap();
        // SAFETY: `start` came from `map_pages(count, align)`.
        unsafe { dealloc(start.as_ptr(), page_layout(count, align)) };
    }
}

fn page_layout(count: usize, align: usize) -> Layout {
    Layout::from_size_align(count * PAGE_SIZE, align).unwrap()
}

fn pages(limit: usize) -> FakePages {
    FakePages {
        limit,
        mapped: 0,
        unmapped: 0,
        alignments: HashMap::new(),
    }
}

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).unwrap()
}

#[test]
fn small_blocks_are_distinct_aligned_and_packed_into_slabs() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let blocks: Vec<*mut u8> = (0..126).map(|_| heap.allocate(layout(24, 8))).collect();

    assert!(
        blocks
            .iter()
            .all(|block| !block.is_null() && (*block as usize).is_multiple_of(32))
    );
    let mut sorted = blocks.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 126);
    // 128 × 32 bytes per page; two blocks hold the slab's bookkeeping.
    assert_eq!(heap.pages_mapped(), 1);
    assert!(!heap.allocate(layout(24, 8)).is_null());
    assert_eq!(heap.pages_mapped(), 2);
}

#[test]
fn a_freed_block_is_handed_out_next() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let first = heap.allocate(layout(100, 8));
    let _second = heap.allocate(layout(100, 8));

    unsafe { heap.deallocate(first, layout(100, 8)) };

    assert_eq!(heap.allocate(layout(100, 8)), first);
}

#[test]
fn strict_alignment_picks_a_bigger_class() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let _filler = heap.allocate(layout(256, 1));
    let block = heap.allocate(layout(8, 256));
    assert_eq!(block as usize % 256, 0);
}

#[test]
fn big_blocks_come_from_aligned_multi_page_slabs() {
    let mut source = pages(20);
    let mut heap = Heap::new(&mut source);
    let blocks: Vec<*mut u8> = (0..15).map(|_| heap.allocate(layout(2048, 8))).collect();
    assert!(
        blocks
            .iter()
            .all(|block| (*block as usize).is_multiple_of(2048))
    );
    assert_eq!(heap.pages_mapped(), 8, "one 32 KiB slab holds 15 blocks");
}

#[test]
fn empty_slabs_go_back_but_one_per_class_stays() {
    let mut source = pages(20);
    let mut heap = Heap::new(&mut source);
    // 63 blocks of 64 bytes fit a slab: 300 need 5 slabs.
    let blocks: Vec<*mut u8> = (0..300).map(|_| heap.allocate(layout(64, 8))).collect();
    assert_eq!(heap.pages_mapped(), 5);

    for block in blocks {
        unsafe { heap.deallocate(block, layout(64, 8)) };
    }

    assert_eq!(heap.pages_mapped(), 1, "one empty slab is kept for reuse");
    assert_eq!(heap.bytes_in_use(), 0);
    assert_eq!(source.unmapped, 4);
}

#[test]
fn large_allocations_get_their_own_pages_and_return_them() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let block = heap.allocate(layout(10_000, 8));
    assert!(!block.is_null());
    assert_eq!(heap.pages_mapped(), 3);

    unsafe { heap.deallocate(block, layout(10_000, 8)) };

    assert_eq!(heap.pages_mapped(), 0);
    assert_eq!(heap.bytes_in_use(), 0);
    assert_eq!(source.unmapped, 3);
}

#[test]
fn alignment_beyond_a_page_is_honoured() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let block = heap.allocate(layout(16, 8192));
    assert!(!block.is_null());
    assert_eq!(block as usize % 8192, 0);
    unsafe { heap.deallocate(block, layout(16, 8192)) };
    assert_eq!(heap.pages_mapped(), 0);
}

#[test]
fn memory_survives_until_freed() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let blocks: Vec<*mut u8> = (0..50).map(|_| heap.allocate(layout(64, 8))).collect();
    for (index, block) in blocks.iter().enumerate() {
        unsafe { block.write_bytes(index as u8, 64) };
    }
    for (index, block) in blocks.iter().enumerate() {
        let bytes = unsafe { std::slice::from_raw_parts(*block, 64) };
        assert!(
            bytes.iter().all(|byte| *byte == index as u8),
            "block {index} was overwritten"
        );
    }
}

#[test]
fn running_out_of_pages_returns_null() {
    let mut source = pages(1);
    let mut heap = Heap::new(&mut source);
    assert!(heap.allocate(layout(5_000, 8)).is_null());
    assert!(!heap.allocate(layout(16, 8)).is_null()); // the one page still works
    assert!(heap.allocate(layout(32, 8)).is_null()); // another class needs a new slab
}

#[test]
fn size_classes_cover_the_boundaries() {
    assert!(matches!(
        classify(layout(1, 1)),
        Request::Small { class: 0 }
    ));
    assert!(matches!(
        classify(layout(17, 1)),
        Request::Small { class: 1 }
    ));
    assert!(matches!(
        classify(layout(2048, 8)),
        Request::Small { class: 7 }
    ));
    assert!(matches!(
        classify(layout(2049, 8)),
        Request::Large { pages: 1, .. }
    ));
    assert!(matches!(
        classify(layout(8193, 8)),
        Request::Large { pages: 3, .. }
    ));
}

#[test]
fn empty_multi_page_slabs_go_back_at_once() {
    let mut source = pages(20);
    let mut heap = Heap::new(&mut source);
    let block = heap.allocate(layout(2048, 8));
    assert_eq!(heap.pages_mapped(), 8);
    unsafe { heap.deallocate(block, layout(2048, 8)) };
    assert_eq!(heap.pages_mapped(), 0);
}
