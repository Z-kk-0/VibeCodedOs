//! The contract between loader and kernel.
//!
//! The loader fills a `BootInfo` and passes a pointer to it to the kernel's
//! entry point. Nothing in here is UEFI-specific: the loader translates
//! firmware data into these plain types first.
//!
//! ## Address space the kernel starts in (DECISIONS.md, 015)
//!
//! ```text
//! 0xFFFF_FFFF_8000_0000  KERNEL_BASE   kernel image, W^X per ELF segment
//! 0xFFFF_8000_0000_0000  PHYSICAL_MEMORY_WINDOW + p = physical address p
//!                        (all RAM, and at least the first 4 GiB incl. devices)
//! 0x0000_0000_0000_0000  identity map, same size as the window – only so the
//!                        loader survives switching page tables. The kernel
//!                        removes it right away; the lower half then belongs
//!                        to user programs.
//! ```
//! All pointers in `BootInfo` point into the physical memory window.

#![cfg_attr(not(test), no_std)]

mod region_list;

pub use region_list::{RegionList, RegionListFull};

pub const PAGE_SIZE: u64 = 4096;

/// Where the kernel image is mapped: the top 2 GiB of the address space.
pub const KERNEL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
/// Start of the upper half; physical memory is mapped here 1:1.
pub const PHYSICAL_MEMORY_WINDOW: u64 = 0xFFFF_8000_0000_0000;

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
    /// What the firmware used while booting ("boot services" memory). Free
    /// once the kernel runs on the loader's page tables.
    FirmwareReclaimable,
    /// The page tables the loader built. In use – never free.
    PageTables,
    /// ACPI tables. Free after the kernel has read them.
    AcpiReclaimable,
    /// Firmware needs this even after boot. Never touch.
    AcpiNvs,
    /// Everything else: firmware runtime services, device memory, broken RAM.
    Reserved,
}

impl MemoryKind {
    pub const ALL: [MemoryKind; 9] = [
        MemoryKind::Usable,
        MemoryKind::Kernel,
        MemoryKind::BootInfo,
        MemoryKind::LoaderReclaimable,
        MemoryKind::FirmwareReclaimable,
        MemoryKind::PageTables,
        MemoryKind::AcpiReclaimable,
        MemoryKind::AcpiNvs,
        MemoryKind::Reserved,
    ];

    /// RAM that will be ours one day (now or after reclaiming). Excludes
    /// ACPI NVS and reserved ranges, which are often device memory.
    pub fn is_ram_we_manage(self) -> bool {
        !matches!(self, MemoryKind::AcpiNvs | MemoryKind::Reserved)
    }

    pub fn name(self) -> &'static str {
        match self {
            MemoryKind::Usable => "usable",
            MemoryKind::Kernel => "kernel",
            MemoryKind::BootInfo => "boot info",
            MemoryKind::LoaderReclaimable => "loader (reclaimable)",
            MemoryKind::FirmwareReclaimable => "firmware (reclaimable)",
            MemoryKind::PageTables => "page tables",
            MemoryKind::AcpiReclaimable => "ACPI (reclaimable)",
            MemoryKind::AcpiNvs => "ACPI NVS",
            MemoryKind::Reserved => "reserved",
        }
    }
}
