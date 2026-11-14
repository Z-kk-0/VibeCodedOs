//! The directory service (DECISIONS.md, 021): services register a sending
//! end under a name, others look names up and get a copy. Protocol:
//! `userlib::directory`.
//!
//! A lookup for a name nobody registered yet isn't refused – its reply
//! handle is parked until the name shows up. So services and their clients
//! can start in any order. This is what reply handles are for: a server
//! can hold many unanswered calls and answer them in any order.
//!
//! Both tables grow as needed (the user heap, DECISIONS.md, 022).
//!
//! Handles: 0 = log, 1 = the directory endpoint (receive).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use userlib::directory::{LOOKUP, REGISTER};
use userlib::{Attach, Handle, Reply, abi, println, receive, reply};

const REQUESTS: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let mut names: Vec<(u64, Handle)> = Vec::new();
    let mut parked: Vec<(u64, Reply)> = Vec::new();
    loop {
        let Ok(mut request) = receive(REQUESTS) else {
            continue;
        };
        let Some(answer) = request.reply.take() else {
            continue;
        };
        let name = request.words[1];
        match request.words[0] {
            REGISTER => {
                let Some(endpoint) = request.handle else {
                    let _ = reply(answer, Attach::Nothing, [abi::ERROR_WRONG_KIND, 0, 0, 0]);
                    continue;
                };
                if find(&names, name).is_some() {
                    // First come, first served.
                    let _ = reply(answer, Attach::Nothing, [abi::ERROR_BUSY, 0, 0, 0]);
                    continue;
                }
                names.push((name, endpoint));
                print_name("is registered", name);
                let _ = reply(answer, Attach::Nothing, [0; 4]);
                // Everyone who asked too early gets their answer now.
                let (waiting, rest): (Vec<_>, Vec<_>) =
                    parked.drain(..).partition(|(wanted, _)| *wanted == name);
                parked = rest;
                for (_, waiting) in waiting {
                    let _ = reply(waiting, Attach::Copy(endpoint), [0; 4]);
                }
            }
            LOOKUP => match find(&names, name) {
                Some(endpoint) => {
                    let _ = reply(answer, Attach::Copy(endpoint), [0; 4]);
                }
                None => {
                    print_name("is not registered yet - the lookup waits for it", name);
                    parked.push((name, answer));
                }
            },
            _ => {
                let _ = reply(answer, Attach::Nothing, [abi::ERROR_UNKNOWN_CALL, 0, 0, 0]);
            }
        }
    }
}

fn find(names: &[(u64, Handle)], name: u64) -> Option<Handle> {
    names
        .iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, endpoint)| endpoint)
}

fn print_name(what: &str, name: u64) {
    let name = abi::name_from_badge(name);
    let name = core::str::from_utf8(name.as_bytes()).unwrap_or("?");
    println!("'{name}' {what}");
}
