//! The heap allocator, used by the kernel and by `userlib` (DECISIONS.md,
//! 016, 022).
//!
//! Small requests (up to 2 KiB) are rounded up to a power of two and served
//! from *slabs*: runs of pages, aligned to their own size, carved into
//! blocks of one size class. A slab's bookkeeping sits in its first
//! block(s), so the slab of any block is found by masking its address –
//! freeing is O(1) and needs no table. Free blocks store the link to the
//! next free block in their own first bytes. A slab that becomes empty goes
//! back to the page source (one empty single-page slab per class is kept, so
//! a loop of allocate/free doesn't map and unmap all the time).
//!
//! Bigger requests get whole pages of their own, aligned as asked, which go
//! back when freed. Where pages come from is up to the caller
//! (`PageSource`), so this runs in the kernel, in programs and in host unit
//! tests alike.

#![cfg_attr(not(test), no_std)]

mod slab;
#[cfg(test)]
mod tests;

use core::alloc::Layout;
use core::ptr::{NonNull, null_mut};
use slab::{Slab, SlabList};

pub const PAGE_SIZE: usize = 4096;
const SMALLEST_BLOCK: usize = 16;
/// Blocks up to this size come from size classes.
pub const LARGEST_SMALL_BLOCK: usize = 2048;
/// 16, 32, 64, 128, 256, 512, 1024, 2048 bytes.
const CLASS_COUNT: usize = 8;

/// Where the heap gets its memory.
///
/// # Safety
/// `map_pages` must return `count` contiguous, writable pages aligned to
/// `align` bytes (a power of two, at least `PAGE_SIZE`) that nothing else
/// uses. `unmap_pages` only ever receives exactly what an earlier
/// `map_pages` call returned.
pub unsafe trait PageSource {
    fn map_pages(&mut self, count: usize, align: usize) -> Option<NonNull<u8>>;
    fn unmap_pages(&mut self, start: NonNull<u8>, count: usize);
}

enum Request {
    Small { class: usize },
    Large { pages: usize, align: usize },
}

pub struct Heap<S> {
    /// Per class: the slabs that still have free blocks.
    partial: [SlabList; CLASS_COUNT],
    source: S,
    bytes_in_use: usize,
    pages_mapped: usize,
}

// SAFETY: the raw pointers only point into memory this heap owns.
unsafe impl<S: Send> Send for Heap<S> {}

impl<S: PageSource> Heap<S> {
    pub const fn new(source: S) -> Self {
        Self {
            partial: [const { SlabList::EMPTY }; CLASS_COUNT],
            source,
            bytes_in_use: 0,
            pages_mapped: 0,
        }
    }

    /// Returns null if out of memory.
    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match classify(layout) {
            Request::Small { class } => {
                if self.partial[class].is_empty() && !self.add_slab(class) {
                    return null_mut();
                }
                let slab = self.partial[class].first();
                // SAFETY: slabs in the list are live and have a free block.
                let block = unsafe { (*slab).take_block() };
                // SAFETY: as above.
                if unsafe { (*slab).is_full() } {
                    self.partial[class].remove(slab);
                }
                self.bytes_in_use += block_size(class);
                block
            }
            Request::Large { pages, align } => match self.source.map_pages(pages, align) {
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
            Request::Small { class } => {
                let slab = Slab::of(pointer, slab_bytes(class));
                // SAFETY: the block came from this live slab (caller contract).
                let was_full = unsafe { (*slab).is_full() };
                // SAFETY: as above; the block is ours again.
                unsafe { (*slab).give_back(pointer) };
                self.bytes_in_use -= block_size(class);
                if was_full {
                    self.partial[class].push(slab);
                }
                // SAFETY: as above.
                let empty = unsafe { (*slab).is_empty() };
                // Keep one small empty slab per class against map/unmap churn;
                // give back any further one, and multi-page ones right away.
                let keep =
                    slab_bytes(class) == PAGE_SIZE && !self.partial[class].has_more_than_one();
                if empty && !keep {
                    self.partial[class].remove(slab);
                    self.release_slab(class, slab);
                }
            }
            Request::Large { pages, .. } => {
                // SAFETY: `allocate` returned a non-null pointer for this layout.
                let start = unsafe { NonNull::new_unchecked(pointer) };
                self.source.unmap_pages(start, pages);
                self.pages_mapped -= pages;
                self.bytes_in_use -= pages * PAGE_SIZE;
            }
        }
    }

    /// Bytes handed out right now (rounded up to block or page size).
    pub fn bytes_in_use(&self) -> usize {
        self.bytes_in_use
    }

    /// Pages taken from the source right now.
    pub fn pages_mapped(&self) -> usize {
        self.pages_mapped
    }

    fn add_slab(&mut self, class: usize) -> bool {
        let bytes = slab_bytes(class);
        let Some(start) = self.source.map_pages(bytes / PAGE_SIZE, bytes) else {
            return false;
        };
        self.pages_mapped += bytes / PAGE_SIZE;
        // SAFETY: fresh pages of exactly `bytes`, aligned to `bytes`, ours.
        let slab = unsafe { Slab::create(start.as_ptr(), bytes, block_size(class)) };
        self.partial[class].push(slab);
        true
    }

    fn release_slab(&mut self, class: usize, slab: *mut Slab) {
        let bytes = slab_bytes(class);
        // SAFETY: a slab pointer is never null.
        let start = unsafe { NonNull::new_unchecked(slab.cast::<u8>()) };
        self.source.unmap_pages(start, bytes / PAGE_SIZE);
        self.pages_mapped -= bytes / PAGE_SIZE;
    }
}

fn classify(layout: Layout) -> Request {
    // Power-of-two blocks inside slabs aligned to their size are aligned to
    // their own size – so rounding up to the alignment satisfies it for free.
    let size = layout
        .size()
        .max(layout.align())
        .max(SMALLEST_BLOCK)
        .next_power_of_two();
    if size <= LARGEST_SMALL_BLOCK {
        Request::Small {
            class: (size / SMALLEST_BLOCK).trailing_zeros() as usize,
        }
    } else {
        Request::Large {
            pages: layout.size().div_ceil(PAGE_SIZE).max(1),
            align: layout.align().max(PAGE_SIZE),
        }
    }
}

fn block_size(class: usize) -> usize {
    SMALLEST_BLOCK << class
}

/// At least 16 blocks per slab, so the bookkeeping block costs ≤ 1/16.
fn slab_bytes(class: usize) -> usize {
    (block_size(class) * 16).max(PAGE_SIZE)
}
