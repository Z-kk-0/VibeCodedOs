//! One slab: a run of pages, aligned to its own size, carved into blocks of
//! one size. Its bookkeeping (`Slab`) lives in its first block(s), so the
//! slab of any block is its address with the low bits cleared.

use core::mem::size_of;
use core::ptr::null_mut;

/// Lives inside a free block.
struct FreeBlock {
    next: *mut FreeBlock,
}

/// At the very start of the slab's memory.
pub struct Slab {
    free: *mut FreeBlock,
    free_count: usize,
    capacity: usize,
    /// Neighbours in its class's list of slabs with free blocks.
    next: *mut Slab,
    previous: *mut Slab,
}

impl Slab {
    /// Writes the bookkeeping into `start` and chains every block after it
    /// into the free list.
    ///
    /// # Safety
    /// `start` must be `bytes` of fresh memory we own, aligned to `bytes`.
    pub unsafe fn create(start: *mut u8, bytes: usize, block: usize) -> *mut Slab {
        let header_blocks = size_of::<Slab>().div_ceil(block);
        let slab = start.cast::<Slab>();
        // SAFETY: the first block(s) are ours and big enough for a `Slab`.
        unsafe {
            slab.write(Slab {
                free: null_mut(),
                free_count: 0,
                capacity: bytes / block - header_blocks,
                next: null_mut(),
                previous: null_mut(),
            });
        }
        // Chained last to first, so blocks are handed out in address order.
        for offset in (header_blocks * block..bytes).step_by(block).rev() {
            // SAFETY: inside the fresh memory; blocks are ≥ 16 bytes and
            // aligned to their size.
            unsafe { (*slab).give_back(start.add(offset)) };
        }
        slab
    }

    /// The slab a block of a `bytes`-sized slab lies in.
    pub fn of(block: *mut u8, bytes: usize) -> *mut Slab {
        (block as usize & !(bytes - 1)) as *mut Slab
    }

    /// # Safety
    /// The slab must have a free block.
    pub unsafe fn take_block(&mut self) -> *mut u8 {
        let block = self.free;
        // SAFETY: `block` is a free block of this slab.
        self.free = unsafe { (*block).next };
        self.free_count -= 1;
        block.cast()
    }

    /// # Safety
    /// `block` must be a block of this slab that isn't free.
    pub unsafe fn give_back(&mut self, block: *mut u8) {
        let block = block.cast::<FreeBlock>();
        // SAFETY: the block is ours again and big enough for a `FreeBlock`.
        unsafe { block.write(FreeBlock { next: self.free }) };
        self.free = block;
        self.free_count += 1;
    }

    pub fn is_full(&self) -> bool {
        self.free_count == 0
    }

    pub fn is_empty(&self) -> bool {
        self.free_count == self.capacity
    }
}

/// A doubly linked list of slabs (through their own bookkeeping).
pub struct SlabList {
    head: *mut Slab,
}

impl SlabList {
    pub const EMPTY: Self = Self { head: null_mut() };

    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    pub fn first(&self) -> *mut Slab {
        self.head
    }

    pub fn has_more_than_one(&self) -> bool {
        // SAFETY: slabs in the list are live.
        !self.head.is_null() && unsafe { !(*self.head).next.is_null() }
    }

    pub fn push(&mut self, slab: *mut Slab) {
        // SAFETY: `slab` and the old head are live slabs.
        unsafe {
            (*slab).previous = null_mut();
            (*slab).next = self.head;
            if !self.head.is_null() {
                (*self.head).previous = slab;
            }
        }
        self.head = slab;
    }

    pub fn remove(&mut self, slab: *mut Slab) {
        // SAFETY: `slab` is in this list, so it and its neighbours are live.
        unsafe {
            let (previous, next) = ((*slab).previous, (*slab).next);
            if previous.is_null() {
                self.head = next;
            } else {
                (*previous).next = next;
            }
            if !next.is_null() {
                (*next).previous = previous;
            }
            (*slab).next = null_mut();
            (*slab).previous = null_mut();
        }
    }
}
