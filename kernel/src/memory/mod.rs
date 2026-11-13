//! Memory management: physical frames (DECISIONS.md, 014) and page tables
//! (DECISIONS.md, 015).

mod frames;
pub mod paging;

pub use frames::{allocate_frame, free_frame, free_frame_count};

use boot_info::{BootInfo, MemoryKind, PHYSICAL_MEMORY_WINDOW};

/// Turns a physical address into a pointer the kernel can use: all of
/// physical memory is mapped 1:1 into a window in the upper half.
pub fn physical_to_pointer(address: u64) -> *mut u8 {
    (PHYSICAL_MEMORY_WINDOW + address) as *mut u8
}

/// Takes over memory management from the loader, step by step, and checks
/// each step at boot.
pub fn start(boot_info: &BootInfo) {
    print_map(boot_info);

    let frames = frames::init(boot_info);
    println!(
        "Frame table: {} frames tracked, {} KiB table at {:#x}.",
        frames.frames_tracked,
        frames.table_bytes / 1024,
        frames.table_address
    );

    paging::remove_identity_map();
    let code = start as fn(&BootInfo) as usize as u64;
    let code_physical = paging::translate(code).expect("kernel code is not mapped");
    assert_eq!(paging::translate(0x1000), None, "identity map still there");
    println!(
        "Identity map removed, lower half empty. Kernel code {code:#x} -> physical {code_physical:#x}."
    );

    let reclaimed = frames::reclaim_boot_memory(boot_info);
    println!(
        "Reclaimed {} KiB of firmware and loader memory, {} MiB free.",
        reclaimed / 1024,
        free_frame_count() * 4 / 1024
    );

    check_frames();
}

/// Prints how much memory of each kind the boot memory map reports.
fn print_map(boot_info: &BootInfo) {
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
fn check_frames() {
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
