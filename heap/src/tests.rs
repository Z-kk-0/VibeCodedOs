use super::*;
use std::alloc::{alloc, dealloc};

/// Pages from the host allocator, with a limit to test running out.
struct FakePages {
    limit: usize,
    mapped: usize,
    unmapped: usize,
}

// SAFETY: every call returns a fresh, page-aligned host allocation; unmap
// frees exactly that allocation.
unsafe impl PageSource for &mut FakePages {
    fn map_pages(&mut self, count: usize) -> Option<NonNull<u8>> {
        if self.mapped + count > self.limit {
            return None;
        }
        self.mapped += count;
        // SAFETY: non-zero size, valid alignment.
        NonNull::new(unsafe { alloc(page_layout(count)) })
    }

    fn unmap_pages(&mut self, start: NonNull<u8>, count: usize) {
        self.unmapped += count;
        // SAFETY: `start` came from `map_pages(count)`.
        unsafe { dealloc(start.as_ptr(), page_layout(count)) };
    }
}

fn page_layout(count: usize) -> Layout {
    Layout::from_size_align(count * PAGE_SIZE, PAGE_SIZE).unwrap()
}

fn pages(limit: usize) -> FakePages {
    FakePages {
        limit,
        mapped: 0,
        unmapped: 0,
    }
}

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).unwrap()
}

#[test]
fn small_blocks_are_distinct_aligned_and_packed_into_one_page() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    let blocks: Vec<*mut u8> = (0..128).map(|_| heap.allocate(layout(24, 8))).collect();

    assert!(
        blocks
            .iter()
            .all(|block| !block.is_null() && (*block as usize).is_multiple_of(32))
    );
    let mut sorted = blocks.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 128);
    assert_eq!(heap.pages_mapped(), 1); // 128 × 32 bytes = exactly one page
    assert_eq!(heap.bytes_in_use(), 128 * 32);
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
    let _filler = heap.allocate(layout(256, 1)); // makes the next block in that page non-zero
    let block = heap.allocate(layout(8, 256));
    assert_eq!(block as usize % 256, 0);
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
    // Last use of `heap`: `source` can be read again.
    assert_eq!(source.unmapped, 3);
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
    assert!(heap.allocate(layout(32, 8)).is_null()); // another class needs a new page
}

#[test]
fn alignment_beyond_a_page_is_refused() {
    let mut source = pages(10);
    let mut heap = Heap::new(&mut source);
    assert!(heap.allocate(layout(16, 8192)).is_null());
}

#[test]
fn size_classes_cover_the_boundaries() {
    assert!(matches!(
        classify(layout(1, 1)),
        Some(Request::Small { class: 0 })
    ));
    assert!(matches!(
        classify(layout(17, 1)),
        Some(Request::Small { class: 1 })
    ));
    assert!(matches!(
        classify(layout(2048, 8)),
        Some(Request::Small { class: 7 })
    ));
    assert!(matches!(
        classify(layout(2049, 8)),
        Some(Request::Large { pages: 1 })
    ));
    assert!(matches!(
        classify(layout(8193, 8)),
        Some(Request::Large { pages: 3 })
    ));
}
