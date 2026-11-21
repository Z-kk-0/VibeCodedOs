//! The Graphics Output Protocol: the screen the firmware set up.
//! UEFI spec 2.10, §12.9.
//!
//! We only read the current mode: the firmware picked one that works with
//! the attached screen, and after `ExitBootServices` the framebuffer it
//! describes stays where it is – the protocol is gone, the memory is not.

use super::configuration_table::Guid;
use core::ffi::c_void;

/// Spec §12.9.2, `EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID`.
pub const GRAPHICS_OUTPUT_PROTOCOL: Guid = Guid {
    data1: 0x9042_a9de,
    data2: 0x23dc,
    data3: 0x4a38,
    data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

type Unused = *mut c_void;

#[repr(C)]
pub struct GraphicsOutputProtocol {
    query_mode: Unused,
    set_mode: Unused,
    blt: Unused,
    pub mode: *const Mode,
}

/// Spec §12.9.2, `EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE`.
#[repr(C)]
pub struct Mode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *const ModeInformation,
    pub size_of_info: usize,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
}

/// Spec §12.9.2, `EFI_GRAPHICS_OUTPUT_MODE_INFORMATION`.
#[repr(C)]
pub struct ModeInformation {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    /// Only meaningful for `PIXEL_BIT_MASK`.
    pub pixel_information: [u32; 4],
    pub pixels_per_scan_line: u32,
}

/// `EFI_GRAPHICS_PIXEL_FORMAT` values with a framebuffer we understand.
/// The other two: arbitrary bit masks (2), and no framebuffer at all (3).
pub const PIXEL_RED_GREEN_BLUE: u32 = 0;
pub const PIXEL_BLUE_GREEN_RED: u32 = 1;
