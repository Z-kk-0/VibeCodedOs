//! I/O ports for drivers in user space (DECISIONS.md, 025).
//!
//! Old PC devices – the keyboard controller among them – aren't reached
//! through memory but through the CPU's separate port space (`in`/`out`).
//! A `Ports` handle names a few ports; `call`ing it reads or writes one
//! byte, and the kernel does the `in`/`out` for the program. That keeps
//! the rule "a program can only use what it holds a handle to" exact to
//! the single port, and needs no per-program port bitmap in the TSS.

use super::message::{Delivery, Message, Received, write_error};
use super::object::Ports;
use crate::arch::{InterruptFrame, port};

/// `call(ports, [op, offset, value, 0])`: answers at once.
pub fn access(ports: Ports, message: &Message, frame: &mut InterruptFrame) {
    match access_one(ports, message) {
        Ok(value) => Delivery {
            badge: 0,
            words: [value, 0, 0, 0],
            received: Received::default(),
            reply: None,
        }
        .write_to(frame),
        Err(error) => write_error(frame, error),
    }
}

fn access_one(ports: Ports, message: &Message) -> Result<u64, u64> {
    if message.attachment.is_some() {
        return Err(abi::ERROR_WRONG_KIND);
    }
    let [op, offset, value, _] = message.words;
    let port = ports.port(offset).ok_or(abi::ERROR_BAD_ARGUMENT)?;
    match op {
        // SAFETY: the kernel handed this handle out for exactly the device
        // at these ports (`ipc::check`); whoever holds it drives the device.
        abi::PORT_READ => Ok(u64::from(unsafe { port::read_u8(port) })),
        abi::PORT_WRITE => {
            let byte = u8::try_from(value).map_err(|_| abi::ERROR_BAD_ARGUMENT)?;
            // SAFETY: as above.
            unsafe { port::write_u8(port, byte) };
            Ok(0)
        }
        _ => Err(abi::ERROR_BAD_ARGUMENT),
    }
}
