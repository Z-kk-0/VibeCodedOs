//! The keyboard driver – a user program like any other (DECISIONS.md, 025).
//!
//! It holds the keyboard controller's interrupt line and its two ports, and
//! nobody else does. Its whole life is one loop: wait for the interrupt (a
//! `call` the device answers), read the scancodes the controller has, turn
//! them into characters and hand each one to the listener – which is just
//! another `call`. The kernel knows nothing about keys.
//!
//! Handles: 0 = log, 1 = the controller's ports (0x60 data, 0x64 status /
//! command), 2 = its interrupt, 3 = where keys go (`userlib::keyboard`).

#![no_std]
#![no_main]

mod controller;
mod scancodes;

use controller::Controller;
use scancodes::Decoder;
use userlib::keyboard::KEY;
use userlib::{Attach, Handle, call, exit, interrupt, println};

const PORTS: Handle = Handle(1);
const INTERRUPT: Handle = Handle(2);
const LISTENER: Handle = Handle(3);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let controller = match Controller::start(PORTS) {
        Ok(controller) => controller,
        Err(problem) => {
            println!("no keyboard: {problem}");
            exit(0)
        }
    };
    println!("Keyboard controller ready. I sleep until its interrupt wakes me.");
    let mut decoder = Decoder::default();
    loop {
        if let Err(error) = interrupt::wait(INTERRUPT, 0) {
            println!("cannot wait for the interrupt: {error:?}");
            exit(1);
        }
        // One interrupt per byte, but reading all that wait is just as
        // right: an interrupt that finds nothing left reads nothing.
        while let Some(byte) = controller.next_byte() {
            let Some(key) = decoder.feed(byte) else {
                continue;
            };
            if let Err(error) = call(LISTENER, Attach::Nothing, [KEY, u64::from(key), 0, 0]) {
                println!("nobody listens to keys anymore ({error:?}) - stopping.");
                exit(0);
            }
        }
    }
}
