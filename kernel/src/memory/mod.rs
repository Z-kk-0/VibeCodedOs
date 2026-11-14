//! Memory management: physical frames (DECISIONS.md, 014), page tables
//! (015), the kernel heap (016) and process address spaces (018).

mod address_space;
mod allocator;
mod checks;
mod frames;
pub mod paging;
pub mod stacks;

pub use address_space::AddressSpace;
pub use frames::{allocate_frame, free_frame, free_frame_count};

use boot_info::{BootInfo, MemoryKind, PHYSICAL_MEMORY_WINDOW};

/// Turns a physical address into a pointer the kernel can use: all of
/// physical memory is mapped 1:1 into a window in the upper half.
pub fn physical_to_pointer(address: u64) -> *mut u8 {
    (PHYSICAL_MEMORY_WINDOW + address) as *mut u8
}

/// Takes over memory management from the loader, step by step, and checks
/// each step at boot. The heap (`alloc`) works only after this.
pub fn start(boot_info: &BootInfo) {
    print_map(boot_info);

    let frames = frames::init(boot_info);
    println!(
        "Frame table: {} frames tracked, {} KiB table at {:#x}.",
        frames.frames_tracked,
        frames.table_bytes / 1024,
        frames.table_address
    );

    let identity_tables = paging::remove_identity_map();
    paging::unmap_guard_page(crate::arch::start::boot_stack_guard_page());
    let code = start as fn(&BootInfo) as usize as u64;
    let code_physical = paging::translate(code)
        .expect("kernel code is not mapped")
        .physical_address;
    assert_eq!(paging::translate(0x1000), None, "identity map still there");
    println!(
        "Identity map removed ({} KiB of its tables freed), lower half empty. Kernel code {code:#x} -> physical {code_physical:#x}.",
        identity_tables / 1024
    );

    let reclaimed = frames::reclaim_boot_memory(boot_info);
    println!(
        "Reclaimed {} KiB of firmware and loader memory, {} MiB free.",
        reclaimed / 1024,
        free_frame_count() * 4 / 1024
    );

    checks::frames();
    checks::heap();
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
