//! Opening device memory (DECISIONS.md, 023).
//!
//! The kernel hands `init` a `Device` handle per device it found (today:
//! the screen). Like any handle it can be moved to whoever should drive the
//! device – and only that one program can then open it: `call(device)`
//! turns the handle into an owned memory object, mapped into the caller
//! with the caching the device needs. From then on it follows the normal
//! ownership rules (move, lend), and the kernel never looks at it again.

use super::message::{Delivery, Message, Received, write_error};
use super::object::{Capability, OwnedMemory};
use super::process::Process;
use crate::arch::InterruptFrame;
use capability::Handle;
use paging::PAGE_SIZE;

/// `call(device)`: answers at once.
pub fn open(process: &mut Process, handle: Handle, message: &Message, frame: &mut InterruptFrame) {
    match open_in(process, handle, message) {
        Ok(delivery) => delivery.write_to(frame),
        Err(error) => write_error(frame, error),
    }
}

fn open_in(process: &mut Process, handle: Handle, message: &Message) -> Result<Delivery, u64> {
    if message.attachment.is_some() {
        return Err(abi::ERROR_WRONG_KIND); // nothing to give a device
    }
    let Some(Capability::Device(device)) = process.handles.get(handle) else {
        unreachable!("the caller checked the kind");
    };
    let memory = device.memory.as_ref().ok_or(abi::ERROR_NO_DEVICE)?;
    let (frames, caching, info) = (memory.frames(), memory.caching(), device.info);
    // The only step that can fail; until it worked, nothing changed.
    let address = process
        .map_frames(&frames, true, caching)
        .ok_or(abi::ERROR_OUT_OF_MEMORY)?;
    // The kernel checks its own work, as when lending: the hardware must
    // see the caching the device needs, on every page.
    for page in (address..).step_by(PAGE_SIZE as usize).take(frames.len()) {
        let translation = process.space.translate(page);
        assert!(
            translation.is_some_and(|translation| translation.flags.caching == caching),
            "device memory mapped with the wrong caching at {page:#x}"
        );
    }

    let slot = process.handles.get_mut(handle).expect("checked above");
    let Capability::Device(device) = slot else {
        unreachable!("checked above");
    };
    let memory = device.memory.take().expect("checked above");
    *slot = Capability::Memory(OwnedMemory {
        memory: alloc::sync::Arc::new(memory),
        address,
        lent: false,
    });
    Ok(Delivery {
        badge: 0,
        words: info,
        received: Received {
            handle: Some(handle),
            memory: Some((address, frames.len() as u64, abi::ACCESS_OWNED)),
        },
        reply: None,
        status: None,
    })
}
