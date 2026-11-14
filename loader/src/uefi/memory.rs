//! Memory-related UEFI types. UEFI spec 2.10, §7.2.

/// Who a piece of memory belongs to, as the firmware sees it.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemoryType(pub u32);

impl MemoryType {
    pub const LOADER_CODE: Self = Self(1);
    pub const LOADER_DATA: Self = Self(2);
    pub const BOOT_SERVICES_CODE: Self = Self(3);
    pub const BOOT_SERVICES_DATA: Self = Self(4);
    pub const CONVENTIONAL: Self = Self(7);
    pub const ACPI_RECLAIM: Self = Self(9);
    pub const ACPI_NVS: Self = Self(10);

    /// Values from 0x8000_0000 up are free for OS loaders to define (§7.2.1).
    /// We tag our own allocations with them, so they show up in the memory map.
    pub const HEMATITE_KERNEL: Self = Self(0x8000_0000);
    pub const HEMATITE_BOOT_INFO: Self = Self(0x8000_0001);
    pub const HEMATITE_PAGE_TABLES: Self = Self(0x8000_0002);
    pub const HEMATITE_CORE_STARTUP: Self = Self(0x8000_0003);
}

/// Where `AllocatePages` may place an allocation.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct AllocateType(pub u32);

impl AllocateType {
    pub const ANY_PAGES: Self = Self(0);
    /// Anywhere at or below the address passed in.
    pub const MAX_ADDRESS: Self = Self(1);
}

/// One entry of the memory map. The firmware may use a bigger stride than
/// `size_of::<MemoryDescriptor>()` – always step by the reported descriptor size.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryDescriptor {
    pub memory_type: MemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

/// Result of reading the memory map.
pub struct MemoryMapInfo {
    /// Bytes used in the buffer (or needed, when asking for the size).
    pub size: usize,
    /// Proof for `ExitBootServices` that we saw the latest map.
    pub key: usize,
    pub descriptor_size: usize,
}
