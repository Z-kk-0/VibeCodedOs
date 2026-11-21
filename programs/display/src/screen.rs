//! The screen's memory and how to put pixels into it.
//!
//! The framebuffer is mapped *write-combining* (DECISIONS.md, 023): the
//! CPU doesn't cache it, but gathers writes in its write-combining buffers
//! and sends them to the device in bursts – much faster than one bus
//! transaction per pixel, and fine for memory nobody reads. Those buffers
//! drain in their own time, so `draw` ends with `sfence`: once we reply,
//! the pixels are on the screen.

use userlib::display::Area;
use userlib::{Error, Handle, Memory, abi, device};

pub struct Screen {
    memory: Memory,
    width: u32,
    height: u32,
    /// Pixels per row in memory (rows may be padded).
    stride: u32,
    /// Byte order red, green, blue (else blue, green, red – like `0x00RRGGBB`).
    red_first: bool,
}

impl Screen {
    /// `None` if the machine has no screen.
    pub fn open(handle: Handle) -> Result<Option<Self>, Error> {
        let (memory, [width, height, stride, format]) = match device::open(handle) {
            Ok(opened) => opened,
            Err(Error(abi::ERROR_NO_DEVICE)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let screen = Self {
            memory,
            width: width as u32,
            height: height as u32,
            stride: stride as u32,
            red_first: format == abi::PIXEL_RED_GREEN_BLUE,
        };
        let needed = screen.stride as usize * screen.height as usize * 4;
        if screen.stride < screen.width || needed > screen.memory.len() {
            return Err(Error(abi::ERROR_TOO_LONG)); // the kernel's facts don't add up
        }
        Ok(Some(screen))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Copies `pixels` (rows of `area.width` `0x00RRGGBB` values) to
    /// `area`, cut to the screen. Returns 0 or an error code.
    pub fn draw(&mut self, pixels: &[u8], area: Area) -> u64 {
        let (width, height) = (area.width as usize, area.height as usize);
        if width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            > Some(pixels.len())
        {
            return abi::ERROR_TOO_LONG;
        }
        let (first_column, end_column) = clip(area.x, area.width, self.width);
        let (first_row, end_row) = clip(area.y, area.height, self.height);
        let screen = self.memory.bytes_mut().as_mut_ptr().cast::<u32>();
        for row in first_row..end_row {
            let source_row = (row - area.y as usize) * width;
            for column in first_column..end_column {
                let at = (source_row + column - area.x as usize) * 4;
                let pixel = u32::from_le_bytes([
                    pixels[at],
                    pixels[at + 1],
                    pixels[at + 2],
                    pixels[at + 3],
                ]);
                let pixel = if self.red_first {
                    swap_red_and_blue(pixel)
                } else {
                    pixel
                };
                // SAFETY: row < height and column < width <= stride, and
                // `open` checked that stride * height pixels fit.
                unsafe { screen.add(row * self.stride as usize + column).write(pixel) };
            }
        }
        // SAFETY: `sfence` only waits until earlier stores (the WC buffers
        // included) are visible; it changes no register or memory itself.
        unsafe { core::arch::asm!("sfence", options(nostack, preserves_flags)) };
        0
    }
}

/// `0x00RRGGBB` read as bytes is blue, green, red; RGB screens want them
/// the other way round.
/// The part of `start..start + length` that lies inside `0..limit`.
fn clip(start: u32, length: u32, limit: u32) -> (usize, usize) {
    let end = (u64::from(start) + u64::from(length)).min(u64::from(limit));
    (start.min(limit) as usize, end as usize)
}

fn swap_red_and_blue(pixel: u32) -> u32 {
    (pixel & 0xFF) << 16 | (pixel & 0xFF00) | (pixel >> 16) & 0xFF
}
