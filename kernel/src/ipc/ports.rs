//! I/O ports for drivers in user space (DECISIONS.md, 025).
//!
//! Old PC devices – the keyboard controller among them – aren't reached
//! through memory but through the CPU's separate port space (`in`/`out`).
//! A `Ports` handle names a few ports; `call`ing it runs up to four
//! byte-sized reads and writes (`abi::PORT_READ`), and the kernel does the
//! `in`/`out` for the program. That keeps the rule "a program can only use
//! what it holds a handle to" exact to the single port, and needs no
//! per-program port bitmap in the TSS.

use super::message::{Delivery, Message, Received, write_error};
use super::object::Ports;
use crate::arch::{InterruptFrame, port};

/// `call(ports, words)`: answers at once.
pub fn access(ports: Ports, message: &Message, frame: &mut InterruptFrame) {
    let result = if message.attachment.is_some() {
        Err(abi::ERROR_WRONG_KIND)
    } else {
        abi::run_port_ops(
            message.words,
            |offset| ports.port(u64::from(offset)).is_some(),
            |op| {
                let port = ports.port(u64::from(op.offset)).expect("checked first");
                // SAFETY: the kernel handed this handle out for exactly the
                // device at these ports (`ipc::check`); whoever holds it
                // drives the device.
                unsafe {
                    if op.op == abi::PORT_WRITE {
                        port::write_u8(port, op.argument);
                        0
                    } else {
                        port::read_u8(port)
                    }
                }
            },
        )
    };
    match result {
        Ok(words) => Delivery {
            badge: 0,
            words,
            received: Received::default(),
            reply: None,
        }
        .write_to(frame),
        Err(error) => write_error(frame, error),
    }
}
