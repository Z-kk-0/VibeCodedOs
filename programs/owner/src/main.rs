//! Checks the ownership rules from the lender's side (DECISIONS.md, 020).
//! Exits 0 only if all of them held:
//!
//! 1. Lend a page `&mut` to `shout`: it comes back upper-cased.
//! 2. Lend it `&` to `vandal`, which writes into it and is stopped: the
//!    call fails with "the other side is gone", the page is unchanged and
//!    ours again – writable.
//!
//! Handles: 0 = log, 1 = shout, 2 = vandal.

#![no_std]
#![no_main]

use userlib::{Attach, Error, Handle, Memory, abi, call, exit, println};

const SHOUT: Handle = Handle(1);
const VANDAL: Handle = Handle(2);
const TEXT: &[u8] = b"ownership, enforced by the page tables";
const SHOUTED: &str = "OWNERSHIP, ENFORCED BY THE PAGE TABLES";

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(mut page) = Memory::new(1) else {
        exit(10)
    };
    page.bytes_mut()[..TEXT.len()].copy_from_slice(TEXT);
    let length = [TEXT.len() as u64, 0, 0, 0];

    // 1. `&mut`: while shout has it, it is gone from our address space.
    match call(SHOUT, Attach::LendMut(&mut page), length) {
        Ok(reply) if reply.words[0] == 0 => {}
        _ => exit(2),
    }
    if &page.bytes()[..SHOUTED.len()] != SHOUTED.as_bytes() {
        exit(3);
    }
    println!("Lent my page &mut to shout, got it back: {SHOUTED}");

    // 2. `&`: vandal may only read – it writes and is stopped.
    match call(VANDAL, Attach::Lend(&page), length) {
        Err(Error(abi::ERROR_PEER_GONE)) => {}
        _ => exit(4),
    }
    if &page.bytes()[..SHOUTED.len()] != SHOUTED.as_bytes() {
        exit(5);
    }
    page.bytes_mut()[0] = b'O'; // faults if it hadn't become writable again
    println!("Lent it & to vandal, who was stopped; my page is unchanged and mine again.");
    exit(0)
}
