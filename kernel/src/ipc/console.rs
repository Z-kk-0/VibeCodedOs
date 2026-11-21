//! The `Console` object: the kernel's text output, as a capability. Only
//! the `log` service holds one; everyone else talks to `log` (DECISIONS.md,
//! 020). `call(console, lend_shared(memory), [length, ..])` prints that
//! many bytes of the memory – and returns at once, no reply needed.

use super::message::{Attachment, Message, Mode};
use super::object::Capability;
use super::process::Process;
use crate::memory::physical_to_pointer;
use alloc::vec::Vec;
use paging::{Caching, PAGE_SIZE};

pub fn write(process: &Process, message: &Message) -> Result<(), u64> {
    let attachment = message.attachment.ok_or(abi::ERROR_WRONG_KIND)?;
    if attachment.mode != Mode::LendShared {
        return Err(abi::ERROR_BAD_MODE); // the console only ever reads
    }
    let bytes = lent_bytes(process, attachment, message.words[0] as usize)?;
    let text = core::str::from_utf8(&bytes).map_err(|_| abi::ERROR_NOT_UTF8)?;
    print!("{text}");
    Ok(())
}

/// The first `length` bytes of memory `process` lends a kernel object –
/// read straight from the frames, so nothing needs mapping. RAM only.
pub fn lent_bytes(
    process: &Process,
    attachment: Attachment,
    length: usize,
) -> Result<Vec<u8>, u64> {
    // Device memory may lie outside the physical memory window, and reading
    // a device behind its owner's back is no business of ours anyway.
    let frames = match process.handles.get(attachment.handle) {
        Some(Capability::Memory(owned)) if owned.memory.caching() != Caching::WriteBack => {
            return Err(abi::ERROR_WRONG_KIND);
        }
        Some(Capability::Borrowed(borrowed)) if borrowed.caching != Caching::WriteBack => {
            return Err(abi::ERROR_WRONG_KIND);
        }
        Some(Capability::Memory(owned)) if !owned.lent => owned.memory.frames(),
        Some(Capability::Borrowed(borrowed)) => borrowed.frames.clone(),
        Some(_) => return Err(abi::ERROR_WRONG_KIND),
        None => return Err(abi::ERROR_BAD_HANDLE),
    };
    if length as u64 > frames.len() as u64 * PAGE_SIZE {
        return Err(abi::ERROR_TOO_LONG);
    }
    let mut bytes = Vec::with_capacity(length);
    for &frame in &frames {
        let chunk = (length - bytes.len()).min(PAGE_SIZE as usize);
        // SAFETY: a frame of memory this process holds, read through the
        // window; `chunk` stays inside the page.
        let page = unsafe { core::slice::from_raw_parts(physical_to_pointer(frame), chunk) };
        bytes.extend_from_slice(page);
    }
    Ok(bytes)
}
