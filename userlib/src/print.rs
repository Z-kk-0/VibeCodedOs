//! `println!` for programs: formats into a buffer on the stack, then sends
//! it with one `write` call (no heap needed, and one line stays one line).

use core::fmt;

pub struct LineBuffer {
    bytes: [u8; 256],
    length: usize,
}

impl LineBuffer {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 256],
            length: 0,
        }
    }

    pub fn flush(&mut self) {
        // Only whole `&str`s are ever copied in, so this is valid UTF-8.
        let text = core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("<invalid UTF-8>");
        let _ = crate::write(text);
        self.length = 0;
    }
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Write for LineBuffer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.length + text.len() > self.bytes.len() {
            self.flush(); // full: send what we have, continue in a fresh buffer
        }
        if text.len() > self.bytes.len() {
            let _ = crate::write(text); // too big for any buffer: send directly
            return Ok(());
        }
        self.bytes[self.length..self.length + text.len()].copy_from_slice(text.as_bytes());
        self.length += text.len();
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    ($($argument:tt)*) => {{
        use core::fmt::Write as _;
        let mut buffer = $crate::print::LineBuffer::new();
        let _ = writeln!(buffer, $($argument)*);
        buffer.flush();
    }};
}
