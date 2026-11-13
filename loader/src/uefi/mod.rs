//! Hand-written definitions of the UEFI structures we use.
//!
//! Only what we actually need is defined; everything follows the
//! UEFI Specification 2.10. Layouts must match the spec byte for byte,
//! hence `#[repr(C)]` everywhere.

mod system_table;
mod text_output;

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

impl Status {
    pub fn is_error(self) -> bool {
        const ERROR_BIT: usize = 1 << (usize::BITS - 1);
        self.0 & ERROR_BIT != 0
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
