//! The screen the firmware set up: a plain block of memory where every
//! pixel is 4 bytes, row after row (UEFI spec 2.10, §12.9 "Graphics
//! Output Protocol"). Writing there changes what is on the screen.

/// Where the screen is and how it is laid out. `address == 0` means none.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Framebuffer {
    /// Physical address of the top-left pixel.
    pub address: u64,
    /// Bytes, as the firmware reports them (at least `stride * height * 4`).
    pub size: u64,
    pub width: u32,
    pub height: u32,
    /// Pixels per row in memory: at least `width`, rows may be padded.
    pub stride: u32,
    pub format: PixelFormat,
}

/// The byte order of one pixel; the fourth byte is unused.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PixelFormat {
    /// Red, green, blue, unused (UEFI `PixelRedGreenBlueReserved8BitPerColor`).
    #[default]
    RedGreenBlue = 0,
    /// Blue, green, red, unused (UEFI `PixelBlueGreenRedReserved8BitPerColor`).
    BlueGreenRed = 1,
}

impl Framebuffer {
    pub const NONE: Framebuffer = Framebuffer {
        address: 0,
        size: 0,
        width: 0,
        height: 0,
        stride: 0,
        format: PixelFormat::RedGreenBlue,
    };

    pub fn exists(&self) -> bool {
        self.address != 0
    }

    /// Whole pages it covers. The loader only passes page-aligned ones.
    pub fn pages(&self) -> u64 {
        self.size.div_ceil(crate::PAGE_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_cover_the_last_partial_page() {
        let screen = Framebuffer {
            address: 0x8000_0000,
            size: 4097,
            ..Framebuffer::NONE
        };
        assert_eq!(screen.pages(), 2);
        assert!(screen.exists());
        assert!(!Framebuffer::NONE.exists());
    }
}
