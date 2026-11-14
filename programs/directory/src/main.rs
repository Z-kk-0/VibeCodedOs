//! The directory service (DECISIONS.md, 021): services register a sending
//! end under a name, others look names up and get a copy. Protocol:
//! `userlib::directory`.
//!
//! A lookup for a name nobody registered yet isn't refused – its reply
//! handle is parked until the name shows up. So services and their clients
//! can start in any order. This is what reply handles are for: a server
//! can hold many unanswered calls and answer them in any order.
//!
//! Handles: 0 = log, 1 = the directory endpoint (receive).

#![no_std]
#![no_main]

use userlib::directory::{LOOKUP, REGISTER};
use userlib::{Attach, Handle, Reply, abi, println, receive, reply};

const REQUESTS: Handle = Handle(1);
const SLOTS: usize = 16;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let mut names: [Option<(u64, Handle)>; SLOTS] = [None; SLOTS];
    let mut parked: [Option<(u64, Reply)>; SLOTS] = core::array::from_fn(|_| None);
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
                let status = store(&mut names, name, endpoint);
                let _ = reply(answer, Attach::Nothing, [status, 0, 0, 0]);
                if status == 0 {
                    // Everyone who asked too early gets their answer now.
                    for slot in parked.iter_mut() {
                        if slot.as_ref().is_some_and(|(wanted, _)| *wanted == name)
                            && let Some((_, waiting)) = slot.take()
                        {
                            let _ = reply(waiting, Attach::Copy(endpoint), [0; 4]);
                        }
                    }
                }
            }
            LOOKUP => match find(&names, name) {
                Some(endpoint) => {
                    let _ = reply(answer, Attach::Copy(endpoint), [0; 4]);
                }
                None => match parked.iter_mut().find(|slot| slot.is_none()) {
                    Some(slot) => {
                        print_name("is not registered yet - the lookup waits for it", name);
                        *slot = Some((name, answer));
                    }
                    None => {
                        let _ = reply(answer, Attach::Nothing, [abi::ERROR_OUT_OF_MEMORY, 0, 0, 0]);
                    }
                },
            },
            _ => {
                let _ = reply(answer, Attach::Nothing, [abi::ERROR_UNKNOWN_CALL, 0, 0, 0]);
            }
        }
    }
}

fn store(names: &mut [Option<(u64, Handle)>], name: u64, endpoint: Handle) -> u64 {
    if find(names, name).is_some() {
        return abi::ERROR_BUSY; // first come, first served
    }
    match names.iter_mut().find(|slot| slot.is_none()) {
        Some(slot) => {
            *slot = Some((name, endpoint));
            print_name("is registered", name);
            0
        }
        None => abi::ERROR_OUT_OF_MEMORY,
    }
}

fn find(names: &[Option<(u64, Handle)>], name: u64) -> Option<Handle> {
    names
        .iter()
        .flatten()
        .find(|(known, _)| *known == name)
        .map(|&(_, endpoint)| endpoint)
}

fn print_name(what: &str, name: u64) {
    let name = abi::name_from_badge(name);
    let name = core::str::from_utf8(name.as_bytes()).unwrap_or("?");
    println!("'{name}' {what}");
}
