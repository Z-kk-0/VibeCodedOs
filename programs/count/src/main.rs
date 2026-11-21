//! Counts slowly, one number every quarter second: `count 5` up to 5,
//! `count` for ever – something to run in the background, and to stop
//! (DECISIONS.md, 026).
//!
//! Its arguments come the way everything comes in Hematite: as a handle –
//! a page with the rest of the command line, from the shell.
//!
//! Handles: 0 = output (the terminal), 1 = the arguments (a page of text).

#![no_std]
#![no_main]

use userlib::{Handle, Memory, exit, println, sleep};

const ARGUMENTS: Handle = Handle(1);
const TICKS_PER_NUMBER: u64 = 25;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(page) = Memory::from_start_handle(ARGUMENTS) else {
        exit(2)
    };
    let bytes = page.bytes();
    let length = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let arguments = core::str::from_utf8(&bytes[..length]).unwrap_or("").trim();
    let last = match arguments {
        "" => None,
        number => match number.parse::<u64>() {
            Ok(last) => Some(last),
            Err(_) => {
                println!("count: '{number}' is not a number.");
                exit(1)
            }
        },
    };
    let mut number = 1;
    while last.is_none_or(|last| number <= last) {
        println!("{number}");
        sleep(TICKS_PER_NUMBER);
        number += 1;
    }
    exit(0)
}
