//! Leaving the firmware behind (see DECISIONS.md, 006).
//!
//! 1. Read the final memory map and call `ExitBootServices`.
//! 2. Translate the map into the kernel's own `MemoryRegion` list.
//!
//! After step 1 the firmware's services are gone for good: no printing,
//! no allocating – only plain memory work.

use crate::console::Console;
use crate::uefi::{BootServices, Handle, MemoryDescriptor, MemoryType};
use boot_info::{BootInfo, MemoryKind, MemoryRegion, PAGE_SIZE};
use core::mem::size_of;

pub type Error = &'static str;

/// Our own allocations below add map entries between measuring the map and
/// reading it for real, so leave some room.
const SPARE_DESCRIPTORS: usize = 16;
const MAX_EXIT_ATTEMPTS: usize = 3;

/// Hands the machine from the firmware to us.
///
/// Takes the console by value: once boot services are gone, so is the
/// console – and the compiler guarantees nobody can still use it.
pub fn exit_boot_services(
    boot_services: &BootServices,
    image: Handle,
    _console: Console,
) -> Result<&'static BootInfo, Error> {
    let needed = boot_services
        .memory_map_size()
        .map_err(|_| "cannot query the memory map")?;
    if needed.descriptor_size < size_of::<MemoryDescriptor>() {
        return Err("firmware reports a too small memory descriptor size");
    }
    let capacity = needed.size / needed.descriptor_size + SPARE_DESCRIPTORS;
    let raw_map = allocate(
        boot_services,
        MemoryType::LOADER_DATA,
        capacity * needed.descriptor_size,
    )?;
    let boot_area = allocate(
        boot_services,
        MemoryType::VIBE_BOOT_INFO,
        size_of::<BootInfo>() + capacity * size_of::<MemoryRegion>(),
    )?;

    // ExitBootServices fails if the map changed since we read it (firmware
    // timers may still allocate). Then: read again, try again (spec §7.4.6).
    for _ in 0..MAX_EXIT_ATTEMPTS {
        let map = boot_services
            .memory_map(raw_map)
            .map_err(|_| "cannot read the memory map")?;
        // SAFETY: the console was moved in here and is never used again;
        // nothing else calls into the firmware after this point.
        if unsafe { boot_services.exit_boot_services(image, map.key) }.is_ok() {
            // --- Boot services are gone. ---
            let descriptors = raw_map[..map.size]
                .chunks_exact(map.descriptor_size)
                .map(read_descriptor);
            return Ok(write_boot_info(boot_area, descriptors, capacity));
        }
    }
    Err("ExitBootServices keeps failing")
}

/// Zeroed pages from the firmware, never freed (the kernel reclaims them).
fn allocate(
    boot_services: &BootServices,
    memory_type: MemoryType,
    bytes: usize,
) -> Result<&'static mut [u8], Error> {
    let page_size = PAGE_SIZE as usize;
    let pages = bytes.div_ceil(page_size);
    let address = boot_services
        .allocate_pages(memory_type, pages)
        .map_err(|_| "out of memory while preparing the handoff")?;
    // SAFETY: fresh, identity-mapped pages that only we own; zeroed before
    // the slice is created so every byte is initialized.
    unsafe {
        let start = address as *mut u8;
        core::ptr::write_bytes(start, 0, pages * page_size);
        Ok(core::slice::from_raw_parts_mut(start, pages * page_size))
    }
}

fn read_descriptor(bytes: &[u8]) -> MemoryDescriptor {
    // SAFETY: `chunks_exact(descriptor_size)` yields chunks at least as big as
    // a descriptor (checked above); `read_unaligned` needs no alignment.
    unsafe { bytes.as_ptr().cast::<MemoryDescriptor>().read_unaligned() }
}

/// Lays out `[BootInfo][MemoryRegion; capacity]` in `area`.
fn write_boot_info(
    area: &'static mut [u8],
    descriptors: impl Iterator<Item = MemoryDescriptor>,
    capacity: usize,
) -> &'static BootInfo {
    let (info_bytes, region_bytes) = area.split_at_mut(size_of::<BootInfo>());
    // SAFETY: `area` is page-aligned and `BootInfo` is a multiple of 8 bytes,
    // so `region_bytes` is aligned for `MemoryRegion`; it holds `capacity`
    // entries by construction; all-zero bytes are a valid `MemoryRegion`.
    let regions = unsafe {
        core::slice::from_raw_parts_mut(region_bytes.as_mut_ptr().cast::<MemoryRegion>(), capacity)
    };
    let count = fill_regions(regions, descriptors);

    let info = info_bytes.as_mut_ptr().cast::<BootInfo>();
    // SAFETY: `info` is page-aligned and big enough; the regions live in
    // memory tagged VIBE_BOOT_INFO, which nobody overwrites.
    unsafe {
        info.write(BootInfo::new(regions.as_ptr(), count));
        &*info
    }
}

/// Translates descriptors into regions, merging a region into its
/// predecessor when they touch and have the same kind. Returns the count.
fn fill_regions(
    regions: &mut [MemoryRegion],
    descriptors: impl Iterator<Item = MemoryDescriptor>,
) -> usize {
    let mut count: usize = 0;
    for descriptor in descriptors {
        let region = MemoryRegion {
            start: descriptor.physical_start,
            page_count: descriptor.number_of_pages,
            kind: kind_of(descriptor.memory_type),
        };
        if let Some(previous) = count.checked_sub(1).map(|last| &mut regions[last])
            && previous.kind == region.kind
            && previous.end() == region.start
        {
            previous.page_count += region.page_count;
        } else if count < regions.len() {
            regions[count] = region;
            count += 1;
        }
    }
    count
}

fn kind_of(memory_type: MemoryType) -> MemoryKind {
    match memory_type {
        // Boot services memory is free now that the firmware is gone.
        MemoryType::CONVENTIONAL
        | MemoryType::BOOT_SERVICES_CODE
        | MemoryType::BOOT_SERVICES_DATA => MemoryKind::Usable,
        MemoryType::LOADER_CODE | MemoryType::LOADER_DATA => MemoryKind::LoaderReclaimable,
        MemoryType::VIBE_KERNEL => MemoryKind::Kernel,
        MemoryType::VIBE_BOOT_INFO => MemoryKind::BootInfo,
        MemoryType::ACPI_RECLAIM => MemoryKind::AcpiReclaimable,
        MemoryType::ACPI_NVS => MemoryKind::AcpiNvs,
        _ => MemoryKind::Reserved,
    }
}
