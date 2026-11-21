//! Memory objects: physical pages with exactly one owner (DECISIONS.md,
//! 020). Usually RAM; since DECISIONS.md 023 also *device memory* – a
//! framebuffer, later device registers – which follows the very same
//! ownership rules but is never zeroed, never freed (it isn't RAM, the
//! frame table doesn't know it) and is mapped with the device's caching.

use crate::memory::{allocate_frame, free_frame, physical_to_pointer};
use alloc::vec::Vec;
use frame_table::FrameNumber;
use paging::{Caching, PAGE_SIZE};

pub struct Memory {
    backing: Backing,
}

enum Backing {
    /// Zeroed when made, freed when dropped.
    Ram(Vec<FrameNumber>),
    /// A fixed physical range of a device.
    Device {
        start: u64,
        pages: u64,
        caching: Caching,
    },
}

impl Memory {
    pub fn new(pages: u64) -> Option<Self> {
        let mut frames = Vec::new();
        for _ in 0..pages {
            let Some(frame) = allocate_frame() else {
                frames.into_iter().for_each(free_frame);
                return None;
            };
            // SAFETY: a fresh frame we own, reached through the window.
            unsafe {
                physical_to_pointer(frame.start_address()).write_bytes(0, PAGE_SIZE as usize)
            };
            frames.push(frame);
        }
        Some(Self {
            backing: Backing::Ram(frames),
        })
    }

    /// The device memory at `start` (page-aligned). Only the kernel makes
    /// these, from what the firmware reported – a program holding one may
    /// write to the device, so it must really be that device's memory.
    pub fn device(start: u64, pages: u64, caching: Caching) -> Self {
        assert!(
            start.is_multiple_of(PAGE_SIZE),
            "device memory not page-aligned"
        );
        Self {
            backing: Backing::Device {
                start,
                pages,
                caching,
            },
        }
    }

    /// Physical addresses of its pages, in order.
    pub fn frames(&self) -> Vec<u64> {
        match &self.backing {
            Backing::Ram(frames) => frames.iter().map(|frame| frame.start_address()).collect(),
            Backing::Device { start, pages, .. } => {
                (0..*pages).map(|page| start + page * PAGE_SIZE).collect()
            }
        }
    }

    pub fn pages(&self) -> u64 {
        match &self.backing {
            Backing::Ram(frames) => frames.len() as u64,
            Backing::Device { pages, .. } => *pages,
        }
    }

    /// How every mapping of it must be cached.
    pub fn caching(&self) -> Caching {
        match &self.backing {
            Backing::Ram(_) => Caching::WriteBack,
            Backing::Device { caching, .. } => *caching,
        }
    }
}

impl Drop for Memory {
    /// Must not be mapped anywhere anymore (the owner unmaps it first).
    fn drop(&mut self) {
        if let Backing::Ram(frames) = &mut self.backing {
            frames.drain(..).for_each(free_frame);
        }
    }
}

/// A device's memory before anyone opened it, plus four words the kernel
/// knows about the device (for the screen: width, height, stride, pixel
/// format – see `abi::DEVICE_*`). `call`ing it opens it: the handle
/// becomes the memory, mapped into the caller. `memory` is `None` if the
/// machine doesn't have the device.
pub struct Device {
    pub memory: Option<Memory>,
    pub info: [u64; 4],
}
