//! Hand-written definitions of the UEFI structures we use.
//!
//! Only what we actually need is defined; everything follows the
//! UEFI Specification 2.10. Layouts must match the spec byte for byte,
//! hence `#[repr(C)]` everywhere.

mod boot_services;
mod configuration_table;
mod memory;
mod system_table;
mod text_output;

pub use boot_services::BootServices;
pub use configuration_table::{ACPI_10_TABLE, ACPI_20_TABLE, ConfigurationTable};
pub use memory::{MemoryDescriptor, MemoryType};
pub use system_table::SystemTable;
pub use text_output::SimpleTextOutputProtocol;

/// Opaque pointer the firmware uses to identify things (images, devices…).
/// UEFI spec 2.10, §2.3.1 "Data Types".
pub type Handle = *mut core::ffi::c_void;

/// Result code of every UEFI call. Zero means success; the highest bit set
/// means error. UEFI spec 2.10, Appendix D "Status Codes".
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Status(pub usize);

const ERROR_BIT: usize = 1 << (usize::BITS - 1);

impl Status {
    pub const BUFFER_TOO_SMALL: Status = Status(ERROR_BIT | 5);

    pub fn is_error(self) -> bool {
        self.0 & ERROR_BIT != 0
    }

    /// Warnings (non-zero without the error bit) count as success.
    pub fn into_result(self) -> Result<(), Status> {
        if self.is_error() { Err(self) } else { Ok(()) }
    }
}

/// Common header in front of every UEFI table. UEFI spec 2.10, §4.2.
#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}
