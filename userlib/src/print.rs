//! `println!` for programs: formats into a buffer on the stack, copies it
//! into a page of our own and lends that page (`&`) to the log service –
//! data only travels in memory objects (DECISIONS.md, 020). One call per
//! line, so one line stays one line.
//!
//! The `log` service itself must not use this: it would wait for itself.

use crate::ipc::{Handle, call_raw};
use crate::memory::Memory;
use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

/// The print page, made on first use and kept for the program's lifetime.
/// Where `println!` sends to: handle 0 unless the program says otherwise.
static LOG: AtomicU64 = AtomicU64::new(0);
static PAGE_HANDLE: AtomicU64 = AtomicU64::new(abi::NO_HANDLE);
static PAGE_ADDRESS: AtomicU64 = AtomicU64::new(0);

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
        send(text);
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
            send(text); // too big for any buffer: send directly
            return Ok(());
        }
        self.bytes[self.length..self.length + text.len()].copy_from_slice(text.as_bytes());
        self.length += text.len();
        Ok(())
    }
}

/// Hands `text` (at most one page) to the log service.
fn send(text: &str) {
    let Some((handle, address)) = print_page() else {
        return; // no memory, no output – nothing else we could do
    };
    let length = text.len().min(abi::PAGE_SIZE as usize);
    // SAFETY: the print page is ours, one page long, mapped writable, and
    // only lent out during the `call` below.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), address, length) };
    let _ = call_raw(
        Handle(LOG.load(Ordering::Relaxed)),
        handle,
        abi::ATTACH_LEND_SHARED,
        [length as u64, 0, 0, 0],
    );
}

fn print_page() -> Option<(Handle, *mut u8)> {
    if PAGE_HANDLE.load(Ordering::Relaxed) == abi::NO_HANDLE {
        let page = Memory::new(1).ok()?;
        PAGE_ADDRESS.store(page.address() as u64, Ordering::Relaxed);
        PAGE_HANDLE.store(page.handle().0, Ordering::Relaxed);
        core::mem::forget(page); // kept forever, through the statics
    }
    Some((
        Handle(PAGE_HANDLE.load(Ordering::Relaxed)),
        PAGE_ADDRESS.load(Ordering::Relaxed) as *mut u8,
    ))
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

/// Makes `println!` use another handle for the log service – for programs
/// that don't get it as handle 0 (`init`, which starts the log itself).
pub fn set_log(handle: Handle) {
    LOG.store(handle.0, Ordering::Relaxed);
}
