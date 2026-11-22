//! The keyboard driver – a user program like any other (DECISIONS.md, 025).
//!
//! It holds the keyboard controller's interrupt line and its two ports, and
//! nobody else does. Its whole life is one loop: wait for the interrupt (an
//! answer in its mailbox, DECISIONS.md, 027 – where a driver with clients
//! would hear from them too), read the scancodes the controller has, turn
//! them into characters and hand each one to the listener – which is just
//! a `call`. The kernel knows nothing about keys.
//!
//! Layouts: US (the default) and German; Alt+Shift switches (`layouts.rs`).
//!
//! Handles: 0 = log, 1 = the controller's ports (0x60 data, 0x64 status /
//! command), 2 = its interrupt, 3 = where keys go (`userlib::keyboard`).

#![no_std]
#![no_main]

mod controller;
mod layouts;
mod scancodes;

use controller::Controller;
use scancodes::{Decoder, Event};
use userlib::{Attach, Handle, Incoming, Mailbox, call, exit, interrupt, println};

const PORTS: Handle = Handle(1);
const INTERRUPT: Handle = Handle(2);
const LISTENER: Handle = Handle(3);
/// The tag of the interrupt's answers in our mailbox.
const FIRED: u64 = 1;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let controller = match Controller::start(PORTS) {
        Ok(controller) => controller,
        Err(problem) => {
            println!("no keyboard: {problem}");
            exit(0)
        }
    };
    println!(
        "Keyboard controller ready. I sleep until its interrupt wakes me. Alt+Shift switches the layout."
    );
    let Ok(mailbox) = Mailbox::new() else {
        println!("no mailbox for the interrupt");
        exit(1)
    };
    let mut decoder = Decoder::default();
    loop {
        if let Err(error) = interrupt::watch(INTERRUPT, &mailbox, FIRED) {
            println!("cannot wait for the interrupt: {error:?}");
            exit(1);
        }
        match mailbox.receive() {
            Ok(Incoming::Answer(answer)) if answer.tag == FIRED => {}
            _ => continue, // nothing else can come: nobody else may send here
        }
        // One interrupt per byte, but reading all that wait is just as
        // right: an interrupt that finds nothing left reads nothing.
        while let Some(byte) = controller.next_byte() {
            match decoder.feed(byte) {
                None => {}
                Some(Event::Layout(name)) => println!("Layout: {name}."),
                Some(Event::Key(key)) => {
                    if let Err(error) = call(LISTENER, Attach::Nothing, key.to_words()) {
                        println!("nobody listens to keys anymore ({error:?}) - stopping.");
                        exit(0);
                    }
                }
            }
        }
    }
}
