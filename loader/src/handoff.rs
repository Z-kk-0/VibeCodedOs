//! Leaving the firmware behind (DECISIONS.md, 006).
//!
//! 1. `prepare`: allocate everything the handoff needs while the firmware
//!    can still allocate, and find out how far physical memory reaches.
//! 2. `finish`: read the final memory map, call `ExitBootServices`, and
//!    translate the map into the kernel's own `MemoryRegion` list.
//!
//! After `ExitBootServices` the firmware's services are gone for good: no
//! printing, no allocating – only plain memory work.

use crate::console::Console;
use crate::uefi::{BootServices, Handle, MemoryDescriptor, MemoryType};
use boot_info::{
    BootInfo, Framebuffer, MemoryKind, MemoryRegion, PHYSICAL_MEMORY_WINDOW, RegionList,
};
use core::mem::size_of;

pub type Error = &'static str;

/// Our own allocations after measuring the map add entries to it (page
/// tables, the kernel's memory), so leave some room.
const SPARE_DESCRIPTORS: usize = 32;
const MAX_EXIT_ATTEMPTS: usize = 3;

/// What the kernel needs to know about the machine besides its memory.
pub struct Platform {
    pub acpi_root: u64,
    pub core_startup: u64,
    pub framebuffer: Framebuffer,
}

pub struct Handoff {
    raw_map: &'static mut [u8],
    boot_area: &'static mut [u8],
    capacity: usize,
    /// The first look at the memory map, kept in `raw_map` until `finish`.
    first_map_size: usize,
    descriptor_size: usize,
    /// End of the highest physical address range that isn't "reserved".
    pub physical_memory_end: u64,
}

pub fn prepare(boot_services: &BootServices) -> Result<Handoff, Error> {
    let needed = boot_services
        .memory_map_size()
        .map_err(|_| "cannot query the memory map")?;
    if needed.descriptor_size < size_of::<MemoryDescriptor>() {
        return Err("firmware reports a too small memory descriptor size");
    }
    let capacity = needed.size / needed.descriptor_size + SPARE_DESCRIPTORS;
    let raw_map = boot_services
        .allocate_zeroed(MemoryType::LOADER_DATA, capacity * needed.descriptor_size)
        .map_err(|_| "out of memory for the memory map")?;
    let boot_area = boot_services
        .allocate_zeroed(
            MemoryType::HEMATITE_BOOT_INFO,
            size_of::<BootInfo>() + capacity * size_of::<MemoryRegion>(),
        )
        .map_err(|_| "out of memory for the boot info")?;

    // A first look at the map, only to see how far physical memory reaches.
    let map = boot_services
        .memory_map(raw_map)
        .map_err(|_| "cannot read the memory map")?;
    let physical_memory_end = descriptors(raw_map, map.size, map.descriptor_size)
        .filter(|descriptor| kind_of(descriptor.memory_type) != MemoryKind::Reserved)
        .map(|descriptor| {
            descriptor.physical_start + descriptor.number_of_pages * boot_info::PAGE_SIZE
        })
        .max()
        .ok_or("the memory map is empty")?;

    Ok(Handoff {
        raw_map,
        boot_area,
        capacity,
        first_map_size: map.size,
        descriptor_size: map.descriptor_size,
        physical_memory_end,
    })
}

impl Handoff {
    /// Whether any RAM lies in `start..end` – by the map `prepare` read.
    /// What we allocated since only turned RAM into other kinds of RAM.
    pub fn holds_ram(&self, start: u64, end: u64) -> bool {
        descriptors(self.raw_map, self.first_map_size, self.descriptor_size).any(|descriptor| {
            let region_end =
                descriptor.physical_start + descriptor.number_of_pages * boot_info::PAGE_SIZE;
            kind_of(descriptor.memory_type).is_ram_we_manage()
                && descriptor.physical_start < end
                && start < region_end
        })
    }

    /// Hands the machine from the firmware to us. Returns the `BootInfo` at
    /// its address in the physical memory window (where the kernel sees it).
    ///
    /// Takes the console by value: once boot services are gone, so is the
    /// console – and the compiler guarantees nobody can still use it.
    pub fn finish(
        self,
        boot_services: &BootServices,
        image: Handle,
        _console: Console,
        platform: Platform,
    ) -> Result<u64, Error> {
        // ExitBootServices fails if the map changed since we read it (firmware
        // timers may still allocate). Then: read again, try again (spec §7.4.6).
        for _ in 0..MAX_EXIT_ATTEMPTS {
            let map = boot_services
                .memory_map(self.raw_map)
                .map_err(|_| "cannot read the memory map")?;
            // SAFETY: the console was moved in here and is never used again;
            // nothing else calls into the firmware after this point.
            if unsafe { boot_services.exit_boot_services(image, map.key) }.is_ok() {
                // --- Boot services are gone. ---
                let descriptors = descriptors(self.raw_map, map.size, map.descriptor_size);
                return Ok(write_boot_info(
                    self.boot_area,
                    descriptors,
                    self.capacity,
                    &platform,
                ));
            }
        }
        Err("ExitBootServices keeps failing")
    }
}

fn descriptors(
    raw_map: &[u8],
    size: usize,
    descriptor_size: usize,
) -> impl Iterator<Item = MemoryDescriptor> + '_ {
    raw_map[..size].chunks_exact(descriptor_size).map(|bytes| {
        // SAFETY: chunks are at least as big as a descriptor (checked in
        // `prepare`); `read_unaligned` needs no alignment.
        unsafe { bytes.as_ptr().cast::<MemoryDescriptor>().read_unaligned() }
    })
}

/// Lays out `[BootInfo][MemoryRegion; capacity]` in `area` and returns the
/// window address of the `BootInfo`.
fn write_boot_info(
    area: &'static mut [u8],
    descriptors: impl Iterator<Item = MemoryDescriptor>,
    capacity: usize,
    platform: &Platform,
) -> u64 {
    let area_physical = area.as_ptr() as u64;
    let (info_bytes, region_bytes) = area.split_at_mut(size_of::<BootInfo>());
    // SAFETY: `area` is page-aligned and `BootInfo` is a multiple of 8 bytes,
    // so `region_bytes` is aligned for `MemoryRegion`; it holds `capacity`
    // entries by construction; all-zero bytes are a valid `MemoryRegion`.
    let regions = unsafe {
        core::slice::from_raw_parts_mut(region_bytes.as_mut_ptr().cast::<MemoryRegion>(), capacity)
    };
    let mut list = RegionList::new(regions);
    for descriptor in descriptors {
        // Can't fail: `capacity` covers the whole map. And with the firmware
        // gone there would be no way to report it anyway.
        let _ = list.push(MemoryRegion {
            start: descriptor.physical_start,
            page_count: descriptor.number_of_pages,
            kind: kind_of(descriptor.memory_type),
        });
    }
    let region_count = list.len();
    let regions_in_window = PHYSICAL_MEMORY_WINDOW + area_physical + size_of::<BootInfo>() as u64;

    let info = info_bytes.as_mut_ptr().cast::<BootInfo>();
    // SAFETY: `info` is page-aligned and big enough; the regions live in
    // memory tagged HEMATITE_BOOT_INFO, which nobody overwrites, and the
    // window address is valid once the kernel's page tables are active.
    unsafe {
        info.write(BootInfo::new(
            regions_in_window as *const MemoryRegion,
            region_count,
            platform.acpi_root,
            platform.core_startup,
            platform.framebuffer,
        ))
    };
    PHYSICAL_MEMORY_WINDOW + area_physical
}

fn kind_of(memory_type: MemoryType) -> MemoryKind {
    match memory_type {
        MemoryType::CONVENTIONAL => MemoryKind::Usable,
        // Free once the kernel runs on our page tables, not the firmware's.
        MemoryType::BOOT_SERVICES_CODE | MemoryType::BOOT_SERVICES_DATA => {
            MemoryKind::FirmwareReclaimable
        }
        MemoryType::LOADER_CODE | MemoryType::LOADER_DATA => MemoryKind::LoaderReclaimable,
        MemoryType::HEMATITE_KERNEL => MemoryKind::Kernel,
        MemoryType::HEMATITE_BOOT_INFO => MemoryKind::BootInfo,
        MemoryType::HEMATITE_PAGE_TABLES => MemoryKind::PageTables,
        MemoryType::HEMATITE_CORE_STARTUP => MemoryKind::CoreStartup,
        MemoryType::ACPI_RECLAIM => MemoryKind::AcpiReclaimable,
        MemoryType::ACPI_NVS => MemoryKind::AcpiNvs,
        _ => MemoryKind::Reserved,
    }
}
