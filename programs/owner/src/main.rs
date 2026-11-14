//! Checks the IPC rules from a client's side (DECISIONS.md, 020 and 021).
//! Exits 0 only if all of them held:
//!
//! 1. Finds `shout` in the directory.
//! 2. Lends a page `&mut` to shout: it comes back upper-cased.
//! 3. Lends it `&` to `vandal`, which writes into it and is stopped: the
//!    call fails with "the other side is gone", the page is unchanged and
//!    ours again – writable.
//! 4. Calls vandal's endpoint again: closed now (no receiving end left), so
//!    the call fails at once instead of waiting forever.
//! 5. Timeouts: receiving on and calling an endpoint nobody uses both give
//!    up after a few ticks.
//! 6. Revoking: a minted sending end stops working once its badge is
//!    revoked; a freshly minted one works again.
//!
//! Handles: 0 = log, 1 = directory, 2 = vandal.

#![no_std]
#![no_main]

use userlib::{Attach, Error, Handle, Memory, abi, call, call_timeout, directory, exit, println};
use userlib::{endpoint_new, mint, receive_timeout, revoke};

const DIRECTORY: Handle = Handle(1);
const VANDAL: Handle = Handle(2);
const TEXT: &[u8] = b"ownership, enforced by the page tables";
const SHOUTED: &str = "OWNERSHIP, ENFORCED BY THE PAGE TABLES";
const PATIENCE: u64 = 5;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(shout) = directory::lookup(DIRECTORY, "shout") else {
        exit(1)
    };
    let Ok(mut page) = Memory::new(1) else {
        exit(10)
    };
    page.bytes_mut()[..TEXT.len()].copy_from_slice(TEXT);
    let length = [TEXT.len() as u64, 0, 0, 0];

    // 2. `&mut`: while shout has it, it is gone from our address space.
    match call(shout, Attach::LendMut(&mut page), length) {
        Ok(reply) if reply.words[0] == 0 => {}
        _ => exit(2),
    }
    if &page.bytes()[..SHOUTED.len()] != SHOUTED.as_bytes() {
        exit(3);
    }
    println!("Found shout in the directory, lent it my page &mut, got back: {SHOUTED}");

    // 3. `&`: vandal may only read – it writes and is stopped.
    expect(
        call(VANDAL, Attach::Lend(&page), length),
        abi::ERROR_PEER_GONE,
        4,
    );
    if &page.bytes()[..SHOUTED.len()] != SHOUTED.as_bytes() {
        exit(5);
    }
    page.bytes_mut()[0] = b'O'; // faults if it hadn't become writable again
    println!("Lent it & to vandal, who was stopped; my page is unchanged and mine again.");

    // 4. Nobody can ever answer there now.
    expect(
        call(VANDAL, Attach::Nothing, [0; 4]),
        abi::ERROR_PEER_GONE,
        6,
    );
    println!("Vandal's endpoint is closed: calling it fails at once.");

    // 5. Nobody sends to or receives on our own new endpoint.
    let Ok(mine) = endpoint_new() else { exit(11) };
    expect(receive_timeout(mine, PATIENCE), abi::ERROR_TIMED_OUT, 7);
    expect(
        call_timeout(mine, Attach::Nothing, [0; 4], PATIENCE),
        abi::ERROR_TIMED_OUT,
        8,
    );
    println!("Receiving and calling with a timeout of {PATIENCE} ticks gave up in time.");

    // 6. Revoke a badge: old sending ends die, new ones work.
    let badge = abi::badge_from_name("test");
    let Ok(old) = mint(mine, badge) else { exit(12) };
    if revoke(mine, badge).is_err() {
        exit(13);
    }
    expect(
        call_timeout(old, Attach::Nothing, [0; 4], PATIENCE),
        abi::ERROR_REVOKED,
        9,
    );
    let Ok(new) = mint(mine, badge) else { exit(14) };
    expect(
        call_timeout(new, Attach::Nothing, [0; 4], PATIENCE),
        abi::ERROR_TIMED_OUT,
        15,
    );
    println!("Revoked a badge: its old sending end is dead, a newly minted one works.");
    exit(0)
}

/// Exits with `code` unless `result` is exactly the error `expected`.
fn expect<T>(result: Result<T, Error>, expected: u64, code: u64) {
    match result {
        Err(Error(error)) if error == expected => {}
        Err(error) => {
            println!(
                "check {code}: expected {:?}, got {error:?}",
                Error(expected)
            );
            exit(code)
        }
        Ok(_) => {
            println!(
                "check {code}: expected {:?}, but it worked",
                Error(expected)
            );
            exit(code)
        }
    }
}
