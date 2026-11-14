//! A service that upper-cases text in place – in memory lent to it `&mut`
//! (the caller gets it back with the reply) or moved to it (then it is
//! shout's, and freed afterwards). Proof that `&mut` lending works.
//!
//! Makes its own endpoint and registers it in the directory as "shout" –
//! clients find it there (DECISIONS.md, 021).
//!
//! Handles: 0 = log, 1 = directory.
//! Message: word 0 = text length. Reply: word 0 = 0 or an error code.

#![no_std]
#![no_main]

use userlib::{Attach, Handle, ReceivedMemory, abi, directory, endpoint_new, exit, println};
use userlib::{receive, reply};

const DIRECTORY: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Ok(requests) = endpoint_new() else {
        exit(1)
    };
    if directory::register(DIRECTORY, "shout", requests).is_err() {
        exit(2);
    }
    loop {
        let Ok(mut request) = receive(requests) else {
            continue;
        };
        let Some(answer) = request.reply.take() else {
            continue;
        };
        let length = request.words[0] as usize;
        let status = match &mut request.memory {
            Some(ReceivedMemory::Exclusive(text)) => match text.bytes_mut() {
                Some(bytes) => shout(bytes, length),
                None => abi::ERROR_WRONG_KIND,
            },
            Some(ReceivedMemory::Owned(text)) => {
                let status = shout(text.bytes_mut(), length);
                println!("Someone gave me a page for good - shouted into it, now it is mine.");
                status
            }
            // Shouting needs to write: a shared borrow won't do.
            Some(ReceivedMemory::Shared(_)) | None => abi::ERROR_WRONG_KIND,
        };
        drop(request); // a moved page is freed here
        let _ = reply(answer, Attach::Nothing, [status, 0, 0, 0]);
    }
}

fn shout(bytes: &mut [u8], length: usize) -> u64 {
    match bytes.get_mut(..length) {
        Some(text) => {
            text.make_ascii_uppercase();
            0
        }
        None => abi::ERROR_TOO_LONG,
    }
}
