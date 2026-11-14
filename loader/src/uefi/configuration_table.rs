//! The firmware's list of extra tables (ACPI, SMBIOS, …), each tagged with a
//! GUID. UEFI spec 2.10, §4.6 "EFI Configuration Table".

use super::SystemTable;
use core::ffi::c_void;

/// 128-bit identifier in the firmware's mixed-endian layout (spec Appendix A).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// ACPI 2.0+ root pointer (RSDP with an XSDT). Spec §4.6.1.
pub const ACPI_20_TABLE: Guid = Guid {
    data1: 0x8868_e871,
    data2: 0xe4f1,
    data3: 0x11d3,
    data4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};
/// ACPI 1.0 root pointer (RSDP with only an RSDT), for very old firmware.
pub const ACPI_10_TABLE: Guid = Guid {
    data1: 0xeb9d_2d30,
    data2: 0x2d88,
    data3: 0x11d3,
    data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};

#[repr(C)]
pub struct ConfigurationTable {
    pub vendor_guid: Guid,
    pub vendor_table: *mut c_void,
}

impl SystemTable {
    /// The address of the table tagged `guid`, if the firmware has one.
    pub fn find_configuration_table(&self, guid: Guid) -> Option<u64> {
        if self.configuration_table.is_null() {
            return None;
        }
        // SAFETY: the firmware guarantees `number_of_table_entries` entries
        // at `configuration_table` until ExitBootServices.
        let tables = unsafe {
            core::slice::from_raw_parts(self.configuration_table, self.number_of_table_entries)
        };
        tables
            .iter()
            .find(|table| table.vendor_guid == guid)
            .map(|table| table.vendor_table as u64)
    }
}
