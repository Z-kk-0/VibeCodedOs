//! Device memory (DECISIONS.md, 023): the kernel gives `init` a `Device`
//! handle per device, `init` moves it to the program that drives the
//! device, and that program opens it – once. Afterwards it is ordinary
//! memory the program owns, only cached the way the device needs.

use crate::ipc::{Attach, Error, Handle, call};
use crate::memory::{Memory, ReceivedMemory};

/// Opens the device behind `device`: its memory, mapped into this
/// program, and the four words the kernel knows about it (for the screen
/// see `abi::PIXEL_RED_GREEN_BLUE`). `ERROR_NO_DEVICE` if the machine
/// doesn't have it. On success the handle now stands for the memory.
pub fn open(device: Handle) -> Result<(Memory, [u64; 4]), Error> {
    let reply = call(device, Attach::Nothing, [0; 4])?;
    match reply.memory {
        Some(ReceivedMemory::Owned(memory)) => Ok((memory, reply.words)),
        _ => Err(Error(abi::ERROR_WRONG_KIND)),
    }
}
