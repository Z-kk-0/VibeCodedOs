//! The kernel heap: makes `alloc` (Box, Vec, String, …) work in the kernel
//! (DECISIONS.md, 016). The allocator logic is the `heap` crate; this file
//! gives it pages from its own region of the address space.

use super::paging as page_tables;
use super::{allocate_frame, free_frame};
use crate::sync::SpinLock;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use frame_table::FrameNumber;
use heap::{Heap, PAGE_SIZE, PageSource};
use paging::Flags;

/// The heap's region: the first 448 GiB of one top-level entry (512 GiB)
/// below the kernel image. The rest holds kernel stacks (`stacks.rs`).
pub const HEAP_START: u64 = 0xFFFF_FF00_0000_0000;
pub const HEAP_END: u64 = super::stacks::STACKS_START;

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

static HEAP: SpinLock<Heap<HeapPages>> = SpinLock::new(Heap::new(HeapPages { next: HEAP_START }));

struct KernelAllocator;

// SAFETY: the `heap` crate hands out each block once until it's freed.
unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HEAP.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: forwarded from `GlobalAlloc`'s contract.
        unsafe { HEAP.lock().deallocate(pointer, layout) }
    }
}

pub struct HeapStats {
    pub bytes_in_use: usize,
    pub pages_mapped: usize,
}

pub fn stats() -> HeapStats {
    let heap = HEAP.lock();
    HeapStats {
        bytes_in_use: heap.bytes_in_use(),
        pages_mapped: heap.pages_mapped(),
    }
}

/// Fresh frames, mapped at the next free addresses of the heap region.
/// Virtual addresses are never reused – 512 GiB lasts a long time; frames
/// of freed large blocks do go back to the frame allocator.
struct HeapPages {
    next: u64,
}

// SAFETY: every page is a freshly allocated frame mapped writable at an
// address no one else uses; `unmap_pages` returns exactly those frames.
unsafe impl PageSource for HeapPages {
    fn map_pages(&mut self, count: usize, align: usize) -> Option<NonNull<u8>> {
        // Addresses are never reused, so skipping some to align costs nothing.
        let start = self.next.checked_next_multiple_of(align as u64)?;
        let end = start.checked_add((count * PAGE_SIZE) as u64)?;
        if end > HEAP_END {
            return None;
        }
        for page in (start..end).step_by(PAGE_SIZE) {
            let Some(frame) = allocate_frame() else {
                unmap_range(start, page); // undo the part that worked
                return None;
            };
            if page_tables::map(page, frame.start_address(), Flags::KERNEL_DATA).is_err() {
                free_frame(frame);
                unmap_range(start, page);
                return None;
            }
        }
        self.next = end;
        NonNull::new(start as *mut u8)
    }

    fn unmap_pages(&mut self, start: NonNull<u8>, count: usize) {
        let start = start.as_ptr() as u64;
        unmap_range(start, start + (count * PAGE_SIZE) as u64);
    }
}

fn unmap_range(start: u64, end: u64) {
    let pages = ((end - start) / PAGE_SIZE as u64) as usize;
    page_tables::unmap_range(start, pages, |frame| {
        free_frame(FrameNumber::containing(frame));
    });
}
