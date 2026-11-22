//! Asks for one line and says what it got – something that waits for a
//! *reply* (the terminal answers `READ_LINE` only when Enter is pressed),
//! so the shell can prove that Ctrl+C stops even that (DECISIONS.md, 027).
//!
//! Handles: 0 = output and input (the terminal).

#![no_std]
#![no_main]

use userlib::terminal::read_line;
use userlib::{Handle, Memory, exit, println};

const TERMINAL: Handle = Handle(0);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(mut page) = Memory::new(1) else {
        exit(2)
    };
    println!("What is your name?");
    match read_line(TERMINAL, &mut page) {
        Ok(name) => {
            println!("Hello, {name}!");
            exit(0)
        }
        Err(error) => {
            println!("ask: cannot read a line: {error:?}");
            exit(1)
        }
    }
}
