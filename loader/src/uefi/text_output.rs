use super::Status;
use core::ffi::c_void;

/// The firmware's text console. UEFI spec 2.10, §12.4
/// "Simple Text Output Protocol".
///
/// A UEFI "protocol" is just a struct of function pointers. Every function
/// takes a pointer to the protocol itself as first argument (like `self`).
/// `extern "efiapi"` selects the calling convention UEFI uses
/// (on x86_64 that is the Microsoft x64 convention).
#[repr(C)]
pub struct SimpleTextOutputProtocol {
    pub reset: unsafe extern "efiapi" fn(this: *mut Self, extended_verification: bool) -> Status,
    /// Prints a null-terminated UCS-2 string. Needs "\r\n" for a new line.
    pub output_string: unsafe extern "efiapi" fn(this: *mut Self, string: *const u16) -> Status,
    test_string: *mut c_void,
    query_mode: *mut c_void,
    set_mode: *mut c_void,
    set_attribute: *mut c_void,
    pub clear_screen: unsafe extern "efiapi" fn(this: *mut Self) -> Status,
    set_cursor_position: *mut c_void,
    enable_cursor: *mut c_void,
    mode: *mut c_void,
}
