//! Finding the screen: the firmware's Graphics Output Protocol tells us
//! where its framebuffer is. The kernel gets only the plain facts
//! (`boot_info::Framebuffer`) – the protocol itself dies with the firmware.

use crate::uefi::{
    BootServices, GRAPHICS_OUTPUT_PROTOCOL, GraphicsOutputProtocol, PIXEL_BLUE_GREEN_RED,
    PIXEL_RED_GREEN_BLUE,
};
use boot_info::{Framebuffer, PAGE_SIZE, PixelFormat};

/// The current mode's framebuffer, or `Framebuffer::NONE` if there is no
/// screen, or one we can't draw on directly (bit-mask pixels, "blit only",
/// or not page-aligned – it will be mapped page by page).
pub fn find(boot_services: &BootServices) -> Framebuffer {
    let Some(protocol) = boot_services.locate_protocol(GRAPHICS_OUTPUT_PROTOCOL) else {
        return Framebuffer::NONE;
    };
    // SAFETY: the firmware returned a valid GOP instance; it and its mode
    // structures stay valid until ExitBootServices. Null pointers are checked.
    let (mode, info) = unsafe {
        let protocol = &*protocol.cast::<GraphicsOutputProtocol>();
        if protocol.mode.is_null() || (*protocol.mode).info.is_null() {
            return Framebuffer::NONE;
        }
        (&*protocol.mode, &*(*protocol.mode).info)
    };
    let format = match info.pixel_format {
        PIXEL_RED_GREEN_BLUE => PixelFormat::RedGreenBlue,
        PIXEL_BLUE_GREEN_RED => PixelFormat::BlueGreenRed,
        _ => return Framebuffer::NONE,
    };
    let screen = Framebuffer {
        address: mode.frame_buffer_base,
        size: mode.frame_buffer_size as u64,
        width: info.horizontal_resolution,
        height: info.vertical_resolution,
        stride: info.pixels_per_scan_line,
        format,
    };
    let rows_fit = u64::from(screen.stride) * u64::from(screen.height) * 4 <= screen.size;
    if screen.address.is_multiple_of(PAGE_SIZE) && screen.stride >= screen.width && rows_fit {
        screen
    } else {
        Framebuffer::NONE
    }
}
