//! The log service (DECISIONS.md, 020): the only program holding the
//! console. Everyone else prints by lending it a page with their text
//! (`println!` in `userlib`); it adds the sender's name – taken from the
//! badge the kernel put on the sender's handle, so it can't be faked.
//!
//! Handles: 0 = the log endpoint (receive), 1 = the console.
//! Must not use `println!` itself: that would be a call to itself.

#![no_std]
#![no_main]

use userlib::{Attach, Error, Handle, Memory, Received, ReceivedMemory, abi, exit, receive, reply};

const REQUESTS: Handle = Handle(0);
const CONSOLE: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    // One page of text plus room for the "[name] " in front.
    let Ok(mut page) = Memory::new(2) else {
        exit(1)
    };
    loop {
        let Ok(mut request) = receive(REQUESTS) else {
            continue;
        };
        let Some(answer) = request.reply.take() else {
            continue;
        };
        let status = print(&mut page, &request).map_or_else(|error| error.0, |()| 0);
        let _ = reply(answer, Attach::Nothing, [status, 0, 0, 0]);
    }
}

/// Writes "[name] text" into our page and lends it to the console.
fn print(page: &mut Memory, request: &Received) -> Result<(), Error> {
    let text = match &request.memory {
        Some(ReceivedMemory::Shared(text) | ReceivedMemory::Exclusive(text)) => text.bytes(),
        Some(ReceivedMemory::Owned(text)) => text.bytes(),
        None => return Err(Error(abi::ERROR_WRONG_KIND)),
    };
    let length = request.words[0] as usize;
    let text = text.get(..length).ok_or(Error(abi::ERROR_TOO_LONG))?;

    let name = abi::name_from_badge(request.badge);
    let mut line = Line {
        bytes: page.bytes_mut(),
        length: 0,
    };
    line.push(b"[");
    line.push(name.as_bytes());
    line.push(b"] ");
    line.push(text);
    let total = line.length as u64;
    userlib::call(CONSOLE, Attach::Lend(page), [total, 0, 0, 0]).map(|_| ())
}

/// Appends parts to a byte buffer, cutting off what doesn't fit.
struct Line<'a> {
    bytes: &'a mut [u8],
    length: usize,
}

impl Line<'_> {
    fn push(&mut self, part: &[u8]) {
        let end = (self.length + part.len()).min(self.bytes.len());
        self.bytes[self.length..end].copy_from_slice(&part[..end - self.length]);
        self.length = end;
    }
}
