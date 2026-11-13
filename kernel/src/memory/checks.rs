//! Boot-time self-checks: each one proves a memory layer really works on
//! this machine, beyond what the host unit tests can show.

use super::{allocate_frame, allocator, free_frame, free_frame_count, physical_to_pointer};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Allocates a few frames, writes to them, reads back, frees them – proof
/// that the frame allocator hands out real, distinct, usable memory.
pub fn frames() {
    const FRAMES: usize = 3;
    const PATTERN: u64 = u64::from_le_bytes(*b"\0\0\0\0HEMA");
    let free_before = free_frame_count();
    let frames = [(); FRAMES].map(|()| allocate_frame().expect("out of memory during self-check"));

    for (index, frame) in frames.iter().enumerate() {
        let pointer = physical_to_pointer(frame.start_address()).cast::<u64>();
        // SAFETY: we own these freshly allocated frames exclusively.
        unsafe { pointer.write_volatile(PATTERN | index as u64) };
    }
    for (index, frame) in frames.iter().enumerate() {
        let pointer = physical_to_pointer(frame.start_address()).cast::<u64>();
        // SAFETY: as above.
        let value = unsafe { pointer.read_volatile() };
        assert_eq!(value, PATTERN | index as u64, "frame {index} lost its data");
    }
    for frame in frames {
        free_frame(frame);
    }
    assert_eq!(free_frame_count(), free_before, "self-check leaked frames");
    println!(
        "Frame allocator works: used and returned frames at {:#x}, {:#x}, {:#x}.",
        frames[0].start_address(),
        frames[1].start_address(),
        frames[2].start_address()
    );
}

/// Uses the standard collections – small blocks, growing vectors (which
/// reallocate through every size), a big page-backed block – then checks
/// that everything was given back.
pub fn heap() {
    let before = allocator::stats();
    {
        let numbers: Vec<u64> = (1..=10_000).collect();
        assert_eq!(numbers.iter().sum::<u64>(), 50_005_000);

        let boxed = Box::new([0xABu8; 100]);
        assert!(boxed.iter().all(|byte| *byte == 0xAB));

        let mut text = String::new();
        for word in ["Rust", "crystallized:", "Hematite"] {
            text.push_str(word);
            text.push(' ');
        }
        assert_eq!(
            format!("{}!", text.trim_end()),
            "Rust crystallized: Hematite!"
        );

        let squares: BTreeMap<u32, u32> = (0..200).map(|n| (n, n * n)).collect();
        assert_eq!(squares[&12], 144);

        let peak = allocator::stats();
        println!(
            "Heap works: Vec, Box, String, BTreeMap; peak {} KiB in use, {} pages mapped.",
            peak.bytes_in_use / 1024,
            peak.pages_mapped
        );
    }
    let after = allocator::stats();
    assert_eq!(
        after.bytes_in_use, before.bytes_in_use,
        "heap self-check leaked memory"
    );
    println!(
        "Everything freed again; {} pages stay mapped for small blocks.",
        after.pages_mapped
    );
}
