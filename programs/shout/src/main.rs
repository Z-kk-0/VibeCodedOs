//! A service that upper-cases text in place – in memory lent to it `&mut`
//! (the caller gets it back with the reply) or moved to it (then it is
//! shout's, and freed afterwards). Proof that `&mut` lending works.
//!
//! Handles: 0 = log, 1 = the shout endpoint (receive).
//! Message: word 0 = text length. Reply: word 0 = 0 or an error code.

#![no_std]
#![no_main]

use userlib::{Attach, Handle, ReceivedMemory, abi, println, receive, reply};

const REQUESTS: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    loop {
        let Ok(mut request) = receive(REQUESTS) else {
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
                let name = abi::name_from_badge(request.badge);
                let name = core::str::from_utf8(name.as_bytes()).unwrap_or("?");
                println!("{name} gave me a page for good - shouted into it, now it is mine.");
                status
            }
            // Shouting needs to write: a shared borrow won't do.
            Some(ReceivedMemory::Shared(_)) | None => abi::ERROR_WRONG_KIND,
        };
        drop(request); // a moved page is freed here
        let _ = reply(Attach::Nothing, [status, 0, 0, 0]);
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
