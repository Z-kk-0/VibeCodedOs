//! The physical frame table (DECISIONS.md, 014).
//!
//! RAM is managed in 4 KiB "frames". Every frame gets an 8-byte entry with
//! its state – so the kernel always knows who a frame belongs to, which the
//! ownership model (DECISIONS.md, 012) will build on. Free frames are
//! additionally chained into a list through their entries, so allocating
//! and freeing are both O(1).
//!
//! Pure bookkeeping on a slice: the kernel provides the memory, the logic
//! here is unit-tested on the host.

#![cfg_attr(not(test), no_std)]

#[cfg(test)]
mod tests;

use core::ops::Range;

pub const FRAME_SIZE: u64 = 4096;

/// Number of a 4 KiB physical frame: frame `n` starts at address `n * 4096`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FrameNumber(pub u64);

impl FrameNumber {
    pub fn containing(address: u64) -> Self {
        Self(address / FRAME_SIZE)
    }

    pub fn start_address(self) -> u64 {
        self.0 * FRAME_SIZE
    }
}

/// What a frame is used for. Zero = `Unavailable`, so zeroed memory is a
/// valid table where nothing can be allocated yet.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameState {
    /// Not RAM we manage (a hole, device memory, firmware data) – or not yet.
    Unavailable = 0,
    Free,
    Allocated,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameEntry {
    state: FrameState,
    /// Next free frame, if this one is free. `END_OF_LIST` ends the chain.
    next_free: u32,
}

const END_OF_LIST: u32 = u32::MAX;

#[derive(Debug, PartialEq, Eq)]
pub enum FreeError {
    /// The frame lies beyond the table.
    OutOfRange,
    /// The frame wasn't allocated – a double free or a bogus address.
    NotAllocated(FrameState),
}

pub struct FrameTable<'a> {
    entries: &'a mut [FrameEntry],
    first_free: u32,
    free_count: usize,
}

impl<'a> FrameTable<'a> {
    /// Bytes of table needed to describe `frame_count` frames.
    pub const fn bytes_needed(frame_count: u64) -> u64 {
        frame_count * size_of::<FrameEntry>() as u64
    }

    /// Takes over `entries` (one per frame, starting at frame 0); every frame
    /// starts as `Unavailable`. Then: `mark` ranges, then `build_free_list`.
    pub fn new(entries: &'a mut [FrameEntry]) -> Self {
        assert!(
            entries.len() < END_OF_LIST as usize,
            "too many frames for 32-bit links"
        );
        entries.fill(FrameEntry {
            state: FrameState::Unavailable,
            next_free: END_OF_LIST,
        });
        Self {
            entries,
            first_free: END_OF_LIST,
            free_count: 0,
        }
    }

    /// Sets the state of a range of frames. Parts beyond the table are ignored.
    /// Only for setting the table up – it doesn't touch the free list.
    pub fn mark(&mut self, frames: Range<u64>, state: FrameState) {
        let end = frames.end.min(self.entries.len() as u64);
        for frame in frames.start..end {
            self.entries[frame as usize].state = state;
        }
    }

    /// Chains all `Free` frames together, lowest address first.
    pub fn build_free_list(&mut self) {
        self.first_free = END_OF_LIST;
        self.free_count = 0;
        for index in (0..self.entries.len()).rev() {
            if self.entries[index].state == FrameState::Free {
                self.entries[index].next_free = self.first_free;
                self.first_free = index as u32;
                self.free_count += 1;
            }
        }
    }

    /// Hands frames that were `Unavailable` to the allocator while it's
    /// running – for memory that becomes free later (reclaiming). Frames in
    /// any other state are left alone. Returns how many became free.
    pub fn add_free(&mut self, frames: Range<u64>) -> usize {
        let end = frames.end.min(self.entries.len() as u64);
        let mut added = 0;
        for frame in frames.start..end {
            let entry = &mut self.entries[frame as usize];
            if entry.state == FrameState::Unavailable {
                entry.state = FrameState::Free;
                entry.next_free = self.first_free;
                self.first_free = frame as u32;
                added += 1;
            }
        }
        self.free_count += added;
        added
    }

    pub fn allocate(&mut self) -> Option<FrameNumber> {
        if self.first_free == END_OF_LIST {
            return None;
        }
        let entry = &mut self.entries[self.first_free as usize];
        let frame = FrameNumber(u64::from(self.first_free));
        self.first_free = entry.next_free;
        entry.state = FrameState::Allocated;
        entry.next_free = END_OF_LIST;
        self.free_count -= 1;
        Some(frame)
    }

    pub fn free(&mut self, frame: FrameNumber) -> Result<(), FreeError> {
        let entry = self
            .entries
            .get_mut(frame.0 as usize)
            .ok_or(FreeError::OutOfRange)?;
        if entry.state != FrameState::Allocated {
            return Err(FreeError::NotAllocated(entry.state));
        }
        entry.state = FrameState::Free;
        entry.next_free = self.first_free;
        self.first_free = frame.0 as u32;
        self.free_count += 1;
        Ok(())
    }

    pub fn state(&self, frame: FrameNumber) -> FrameState {
        self.entries
            .get(frame.0 as usize)
            .map_or(FrameState::Unavailable, |entry| entry.state)
    }

    pub fn free_count(&self) -> usize {
        self.free_count
    }

    pub fn frame_count(&self) -> usize {
        self.entries.len()
    }
}
