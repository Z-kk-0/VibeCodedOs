//! The kernel heap allocator (DECISIONS.md, 016).
//!
//! Small requests (up to 2 KiB) are rounded up to a power of two and served
//! from one free list per size ("size classes"): allocating and freeing is
//! popping and pushing a list – O(1), no searching, no fragmentation inside
//! a class. A free block stores the link to the next free block in its own
//! first bytes, so the lists cost no extra memory. Bigger requests get whole
//! pages of their own, which go back to the system when freed.
//!
//! Where pages come from is up to the caller (`PageSource`), so this runs
//! in the kernel and in host unit tests alike.

#![cfg_attr(not(test), no_std)]

#[cfg(test)]
mod tests;

use core::alloc::Layout;
use core::ptr::{NonNull, null_mut};

pub const PAGE_SIZE: usize = 4096;
const SMALLEST_BLOCK: usize = 16;
/// Blocks up to this size come from size classes.
pub const LARGEST_SMALL_BLOCK: usize = 2048;
/// 16, 32, 64, 128, 256, 512, 1024, 2048 bytes.
const CLASS_COUNT: usize = 8;

/// Where the heap gets its memory.
///
/// # Safety
/// `map_pages` must return `count` contiguous, writable, page-aligned pages
/// that nothing else uses. `unmap_pages` only ever receives exactly what an
/// earlier `map_pages` call returned.
pub unsafe trait PageSource {
    fn map_pages(&mut self, count: usize) -> Option<NonNull<u8>>;
    fn unmap_pages(&mut self, start: NonNull<u8>, count: usize);
}

/// Lives inside a free block.
struct FreeBlock {
    next: *mut FreeBlock,
}

enum Request {
    Small { class: usize },
    Large { pages: usize },
}

pub struct Heap<S> {
    free_lists: [*mut FreeBlock; CLASS_COUNT],
    source: S,
    bytes_in_use: usize,
    pages_mapped: usize,
}

// SAFETY: the raw pointers only point into memory this heap owns.
unsafe impl<S: Send> Send for Heap<S> {}

impl<S: PageSource> Heap<S> {
    pub const fn new(source: S) -> Self {
        Self {
            free_lists: [null_mut(); CLASS_COUNT],
            source,
            bytes_in_use: 0,
            pages_mapped: 0,
        }
    }

    /// Returns null if out of memory or if the alignment is stricter than a page.
    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match classify(layout) {
            None => null_mut(),
            Some(Request::Small { class }) => {
                if self.free_lists[class].is_null() && !self.refill(class) {
                    return null_mut();
                }
                let block = self.free_lists[class];
                // SAFETY: `block` is a free block in memory we own.
                self.free_lists[class] = unsafe { (*block).next };
                self.bytes_in_use += block_size(class);
                block.cast()
            }
            Some(Request::Large { pages }) => match self.source.map_pages(pages) {
                Some(start) => {
                    self.pages_mapped += pages;
                    self.bytes_in_use += pages * PAGE_SIZE;
                    start.as_ptr()
                }
                None => null_mut(),
            },
        }
    }

    /// # Safety
    /// `pointer` must come from `allocate` with the same `layout` and must not
    /// have been freed already.
    pub unsafe fn deallocate(&mut self, pointer: *mut u8, layout: Layout) {
        match classify(layout) {
            Some(Request::Small { class }) => {
                let block = pointer.cast::<FreeBlock>();
                // SAFETY: the block is ours again (caller contract) and big
                // and aligned enough for a `FreeBlock` (≥ 16 bytes).
                unsafe {
                    block.write(FreeBlock {
                        next: self.free_lists[class],
                    })
                };
                self.free_lists[class] = block;
                self.bytes_in_use -= block_size(class);
            }
            Some(Request::Large { pages }) => {
                // SAFETY: `allocate` returned a non-null pointer for this layout.
                let start = unsafe { NonNull::new_unchecked(pointer) };
                self.source.unmap_pages(start, pages);
                self.pages_mapped -= pages;
                self.bytes_in_use -= pages * PAGE_SIZE;
            }
            None => unreachable!("`allocate` never succeeds for this layout"),
        }
    }

    /// Bytes handed out right now (rounded up to block or page size).
    pub fn bytes_in_use(&self) -> usize {
        self.bytes_in_use
    }

    /// Pages taken from the source. Pages of size classes are kept for reuse.
    pub fn pages_mapped(&self) -> usize {
        self.pages_mapped
    }

    /// Carves a fresh page into blocks of one class.
    fn refill(&mut self, class: usize) -> bool {
        let Some(page) = self.source.map_pages(1) else {
            return false;
        };
        self.pages_mapped += 1;
        // Chained last to first, so blocks are handed out in address order.
        for offset in (0..PAGE_SIZE).step_by(block_size(class)).rev() {
            // SAFETY: `offset` stays inside the fresh page, which we own.
            let block = unsafe { page.as_ptr().add(offset).cast::<FreeBlock>() };
            // SAFETY: as above; blocks are ≥ 16 bytes and 16-byte aligned.
            unsafe {
                block.write(FreeBlock {
                    next: self.free_lists[class],
                })
            };
            self.free_lists[class] = block;
        }
        true
    }
}

fn classify(layout: Layout) -> Option<Request> {
    if layout.align() > PAGE_SIZE {
        return None;
    }
    // Power-of-two blocks inside page-aligned pages are aligned to their own
    // size – so rounding up to the alignment satisfies it for free.
    let size = layout
        .size()
        .max(layout.align())
        .max(SMALLEST_BLOCK)
        .next_power_of_two();
    Some(if size <= LARGEST_SMALL_BLOCK {
        Request::Small {
            class: (size / SMALLEST_BLOCK).trailing_zeros() as usize,
        }
    } else {
        Request::Large {
            pages: layout.size().div_ceil(PAGE_SIZE),
        }
    })
}

fn block_size(class: usize) -> usize {
    SMALLEST_BLOCK << class
}
