use super::{Handle, SimpleTextOutputProtocol, TableHeader};
use core::ffi::c_void;

/// The root table the firmware hands to our entry point.
/// Everything the firmware offers is reachable from here.
/// UEFI spec 2.10, §4.3 "EFI System Table".
///
/// Fields we don't use yet are typed as `*mut c_void` so we don't have to
/// define their structures; they still need to exist to keep the layout right.
#[repr(C)]
pub struct SystemTable {
    pub header: TableHeader,
    /// Null-terminated UCS-2 string, e.g. "EDK II".
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: Handle,
    pub console_in: *mut c_void,
    pub console_out_handle: Handle,
    pub console_out: *mut SimpleTextOutputProtocol,
    pub standard_error_handle: Handle,
    pub standard_error: *mut SimpleTextOutputProtocol,
    pub runtime_services: *mut c_void,
    pub boot_services: *mut c_void,
    pub number_of_table_entries: usize,
    pub configuration_table: *mut c_void,
}
