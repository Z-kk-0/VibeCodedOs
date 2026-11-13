//! Memory management. So far: physical frames (DECISIONS.md, 014).

mod frames;

pub use frames::{allocate_frame, free_frame, free_frame_count, init};

use boot_info::{BootInfo, MemoryKind};

/// Turns a physical address into a pointer the kernel can use.
///
/// Today the firmware's page tables map every physical address to the same
/// virtual one ("identity mapping"). When we switch to our own page tables
/// (roadmap step 2), physical memory moves to a fixed window – and only this
/// function has to change.
pub fn physical_to_pointer(address: u64) -> *mut u8 {
    address as *mut u8
}

/// Prints how much memory of each kind the boot memory map reports.
pub fn print_map(boot_info: &BootInfo) {
    let regions = boot_info.memory_regions();
    println!("Physical memory ({} regions):", regions.len());
    for kind in MemoryKind::ALL {
        let bytes: u64 = regions
            .iter()
            .filter(|region| region.kind == kind)
            .map(|region| region.size())
            .sum();
        if bytes > 0 {
            println!("  {:<24}{:>9} KiB", kind.name(), bytes / 1024);
        }
    }
}

/// Allocates a few frames, writes to them, reads back, frees them – a boot
/// time proof that the allocator hands out real, distinct, usable memory.
pub fn self_check() {
    const FRAMES: usize = 3;
    let free_before = free_frame_count();
    let frames = [(); FRAMES].map(|()| allocate_frame().expect("out of memory during self-check"));

    for (index, frame) in frames.iter().enumerate() {
        let pointer = physical_to_pointer(frame.start_address()).cast::<u64>();
        // SAFETY: we own these freshly allocated frames exclusively.
        unsafe { pointer.write_volatile(0x4845_4D41_0000_0000 | index as u64) };
    }
    for (index, frame) in frames.iter().enumerate() {
        let pointer = physical_to_pointer(frame.start_address()).cast::<u64>();
        // SAFETY: as above.
        let value = unsafe { pointer.read_volatile() };
        assert_eq!(
            value,
            0x4845_4D41_0000_0000 | index as u64,
            "frame {index} lost its data"
        );
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
