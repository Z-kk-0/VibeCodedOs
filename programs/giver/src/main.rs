//! Moves a page to `shout` – and then reaches for it anyway. After a move
//! the page is the receiver's: unmapped here, so the CPU must fault and
//! the kernel must end giver.
//!
//! Handles: 0 = log, 1 = shout.

#![no_std]
#![no_main]

use userlib::{Attach, Handle, Memory, call, exit, println};

const SHOUT: Handle = Handle(1);
const TEXT: &[u8] = b"a gift";

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(mut page) = Memory::new(1) else {
        exit(10)
    };
    page.bytes_mut()[..TEXT.len()].copy_from_slice(TEXT);
    let address = page.address();
    if call(SHOUT, Attach::Move(page), [TEXT.len() as u64, 0, 0, 0]).is_err() {
        exit(2);
    }
    println!("Moved my page to shout. Reaching for it at {address:p} anyway...");
    // SAFETY: none – this is the attack. The kernel must stop it.
    let byte = unsafe { address.read_volatile() };
    println!("Read {byte} from a page I gave away. This line must never appear!");
    exit(1)
}
