//! The contract between loader and kernel.
//!
//! The loader fills a `BootInfo` and passes a pointer to it to the kernel's
//! entry point. Nothing in here is UEFI-specific: the loader translates
//! firmware data into these plain types first.

#![cfg_attr(not(test), no_std)]

mod region_list;

pub use region_list::{RegionList, RegionListFull};

pub const PAGE_SIZE: u64 = 4096;

/// "HEMATITE" – exactly 8 bytes, as a number; lets the kernel detect a garbage pointer.
pub const MAGIC: u64 = u64::from_le_bytes(*b"HEMATITE");

#[repr(C)]
pub struct BootInfo {
    pub magic: u64,
    memory_regions: *const MemoryRegion,
    memory_region_count: usize,
}

impl BootInfo {
    /// # Safety
    /// `regions` must point to `count` initialized entries that stay valid
    /// and unchanged for as long as this `BootInfo` is used.
    pub unsafe fn new(regions: *const MemoryRegion, count: usize) -> Self {
        Self {
            magic: MAGIC,
            memory_regions: regions,
            memory_region_count: count,
        }
    }

    /// Physical memory as the firmware reported it, translated and with
    /// neighbouring regions of the same kind merged. Not necessarily sorted.
    pub fn memory_regions(&self) -> &[MemoryRegion] {
        // SAFETY: guaranteed by the contract of `new`.
        unsafe { core::slice::from_raw_parts(self.memory_regions, self.memory_region_count) }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    /// Physical address, always page-aligned.
    pub start: u64,
    pub page_count: u64,
    pub kind: MemoryKind,
}

impl MemoryRegion {
    pub fn size(&self) -> u64 {
        self.page_count * PAGE_SIZE
    }

    pub fn end(&self) -> u64 {
        self.start + self.size()
    }
}

/// What a piece of physical memory is used for.
/// `Usable` is deliberately 0, so zeroed memory is a valid (empty) region.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemoryKind {
    /// Free RAM the kernel may use.
    Usable = 0,
    /// The kernel image itself (code, data, stack).
    Kernel,
    /// This `BootInfo` and the region list. Free once the kernel copied what it needs.
    BootInfo,
    /// Loader code and data. Free once we are sure nothing points into it.
    LoaderReclaimable,
    /// ACPI tables. Free after the kernel has read them.
    AcpiReclaimable,
    /// Firmware needs this even after boot. Never touch.
    AcpiNvs,
    /// Everything else: firmware runtime services, device memory, broken RAM.
    Reserved,
}

impl MemoryKind {
    pub const ALL: [MemoryKind; 7] = [
        MemoryKind::Usable,
        MemoryKind::Kernel,
        MemoryKind::BootInfo,
        MemoryKind::LoaderReclaimable,
        MemoryKind::AcpiReclaimable,
        MemoryKind::AcpiNvs,
        MemoryKind::Reserved,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MemoryKind::Usable => "usable",
            MemoryKind::Kernel => "kernel",
            MemoryKind::BootInfo => "boot info",
            MemoryKind::LoaderReclaimable => "loader (reclaimable)",
            MemoryKind::AcpiReclaimable => "ACPI (reclaimable)",
            MemoryKind::AcpiNvs => "ACPI NVS",
            MemoryKind::Reserved => "reserved",
        }
    }
}
