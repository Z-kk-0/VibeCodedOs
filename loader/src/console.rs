//! Safe wrapper around the firmware's text console, so the rest of the
//! loader can just use `write!`/`writeln!`.

use crate::uefi::{SimpleTextOutputProtocol, Status};
use core::fmt;

/// UEFI prints UCS-2 strings; we convert in small chunks on the stack
/// instead of allocating (we have no heap yet).
const CHUNK_LEN: usize = 128;

pub struct Console {
    protocol: *mut SimpleTextOutputProtocol,
}

impl Console {
    /// # Safety
    /// `protocol` must point to a valid Simple Text Output Protocol that
    /// stays valid as long as this `Console` is used (true until we call
    /// `ExitBootServices`).
    pub unsafe fn new(protocol: *mut SimpleTextOutputProtocol) -> Self {
        Self { protocol }
    }

    pub fn clear(&mut self) -> Status {
        // SAFETY: `new` guarantees `protocol` is valid.
        unsafe { ((*self.protocol).clear_screen)(self.protocol) }
    }

    /// Sends one null-terminated UCS-2 buffer to the firmware.
    fn output(&mut self, ucs2: &[u16]) -> fmt::Result {
        debug_assert_eq!(ucs2.last(), Some(&0));
        // SAFETY: `new` guarantees `protocol` is valid; `ucs2` is
        // null-terminated and lives for the duration of the call.
        let status = unsafe { ((*self.protocol).output_string)(self.protocol, ucs2.as_ptr()) };
        if status.is_error() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let mut buffer = [0u16; CHUNK_LEN];
        let mut len = 0;

        for character in text.chars() {
            // Room for up to two units ("\r\n") plus the null terminator.
            if len + 3 > CHUNK_LEN {
                buffer[len] = 0;
                self.output(&buffer[..=len])?;
                len = 0;
            }
            if character == '\n' {
                buffer[len] = u16::from(b'\r');
                len += 1;
            }
            buffer[len] = to_ucs2(character);
            len += 1;
        }

        buffer[len] = 0;
        self.output(&buffer[..=len])
    }
}

/// UCS-2 only covers U+0000..=U+FFFF; anything above (e.g. emoji) becomes '?'.
fn to_ucs2(character: char) -> u16 {
    u16::try_from(u32::from(character)).unwrap_or(u16::from(b'?'))
}
