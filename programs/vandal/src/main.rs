//! A service that breaks the rules on purpose: it gets memory lent `&`
//! (read-only) and writes into it anyway. The page tables must stop it –
//! the kernel ends vandal, and the lender gets its memory back unchanged.
//!
//! Handles: 0 = log, 1 = the vandal endpoint (receive).

#![no_std]
#![no_main]

use userlib::{Handle, ReceivedMemory, exit, println, receive};

const REQUESTS: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    if let Ok(request) = receive(REQUESTS)
        && let Some(ReceivedMemory::Shared(text)) = request.memory
    {
        println!("Got a shared borrow (&) - writing into it anyway...");
        // SAFETY: none – this is the attack. The kernel must stop it.
        unsafe { text.address().write_volatile(b'X') };
        println!("Wrote into borrowed memory. This line must never appear!");
    }
    exit(1)
}
