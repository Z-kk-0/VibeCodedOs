//! Kernel stacks with guard pages (DECISIONS.md, 022).
//!
//! Every kernel stack gets a slot of its own in the top 64 GiB of the heap's
//! top-level entry. Only the top of a slot is mapped; the pages below stay
//! unmapped, so a stack that overflows hits them and faults – the CPU then
//! reports a double fault on its separate stack – instead of silently
//! overwriting whatever lay below. Slots of finished tasks are reused.

use super::allocator::HEAP_START;
use super::paging;
use super::{allocate_frame, free_frame, physical_to_pointer};
use crate::sync::SpinLock;
use ::paging::{Flags, PAGE_SIZE};
use alloc::vec::Vec;
use frame_table::FrameNumber;

pub const STACKS_START: u64 = HEAP_START + (448 << 30);
const STACKS_END: u64 = HEAP_START + (512 << 30);
/// 64 KiB per slot: at most 60 KiB of stack, at least 4 KiB of guard.
const SLOT_PAGES: usize = 16;

struct Slots {
    next: u64,
    free: Vec<u64>,
}

static SLOTS: SpinLock<Slots> = SpinLock::new(Slots {
    next: STACKS_START,
    free: Vec::new(),
});

/// A mapped, zeroed kernel stack. Unmapped and freed when dropped.
pub struct KernelStack {
    slot: u64,
    pages: usize,
}

impl KernelStack {
    /// A stack of `pages` pages (at most 15), with guard pages below.
    pub fn new(pages: usize) -> Option<Self> {
        assert!(pages < SLOT_PAGES, "a stack needs room for its guard page");
        let slot = {
            let mut slots = SLOTS.lock();
            match slots.free.pop() {
                Some(slot) => slot,
                None => {
                    let slot = slots.next;
                    let next = slot + SLOT_PAGES as u64 * PAGE_SIZE;
                    if next > STACKS_END {
                        return None;
                    }
                    slots.next = next;
                    slot
                }
            }
        };
        let stack = Self { slot, pages: 0 };
        stack.grow_to(pages)
    }

    /// Maps the pages one by one, so a failure releases what was mapped.
    fn grow_to(mut self, pages: usize) -> Option<Self> {
        while self.pages < pages {
            let frame = allocate_frame()?;
            // SAFETY: a fresh frame we own, reached through the window.
            unsafe {
                physical_to_pointer(frame.start_address()).write_bytes(0, PAGE_SIZE as usize)
            };
            self.pages += 1;
            if paging::map(
                self.lowest_page(),
                frame.start_address(),
                Flags::KERNEL_DATA,
            )
            .is_err()
            {
                self.pages -= 1;
                free_frame(frame);
                return None;
            }
        }
        Some(self)
    }

    /// One past the highest byte (stacks grow down from here).
    pub fn top(&self) -> u64 {
        self.slot + SLOT_PAGES as u64 * PAGE_SIZE
    }

    /// Keeps the stack forever (cores' own stacks) and returns its top.
    pub fn leak(self) -> u64 {
        let top = self.top();
        core::mem::forget(self);
        top
    }

    fn lowest_page(&self) -> u64 {
        self.top() - self.pages as u64 * PAGE_SIZE
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        paging::unmap_range(self.lowest_page(), self.pages, |frame| {
            free_frame(FrameNumber::containing(frame));
        });
        SLOTS.lock().free.push(self.slot);
    }
}
