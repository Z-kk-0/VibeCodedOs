//! Talking to the terminal (`programs/terminal`, DECISIONS.md, 026).
//!
//! - Writing: the terminal speaks the log's protocol – lend it a page `&`
//!   with text, word 0 = its length. So a program whose handle 0 is a
//!   terminal prints onto the screen with plain `println!`; it can't tell
//!   the difference, and doesn't need to.
//! - Reading: `[READ_LINE, 0, 0, 0]` with a page lent `&mut`. The reply
//!   comes when a line was finished with Enter (at once, if one was typed
//!   ahead): it is in the page, word 1 = its length in bytes.
//! - Ctrl+C: `[TAKE_INTERRUPT, 0, 0, 0]` says whether it was pressed since
//!   anyone last asked. Several readers wait for lines in turn.
//!
//! Keys only count from the keyboard driver's handle (badge "keyboard"):
//! whoever else holds the terminal can't type into it.

use crate::ipc::{Attach, Error, Handle, call, check};
use crate::memory::Memory;
use alloc::string::String;

pub const READ_LINE: u64 = 16;
/// `[CLEAR, 0, 0, 0]`: empties the screen's lines.
pub const CLEAR: u64 = 17;

/// `[TAKE_INTERRUPT, 0, 0, 0]`: whether Ctrl+C was pressed since the last
/// time anyone asked (reply word 1: 1 or 0) – and forget it.
pub const TAKE_INTERRUPT: u64 = 18;

/// Whether Ctrl+C was pressed since anyone last asked.
pub fn take_interrupt(terminal: Handle) -> Result<bool, Error> {
    let reply = call(terminal, Attach::Nothing, [TAKE_INTERRUPT, 0, 0, 0])?;
    check(reply.words[0])?;
    Ok(reply.words[1] != 0)
}

/// Empties the terminal's lines on the screen.
pub fn clear(terminal: Handle) -> Result<(), Error> {
    check(call(terminal, Attach::Nothing, [CLEAR, 0, 0, 0])?.words[0])
}

/// Waits for the next line typed into the terminal; `page` is where it
/// arrives (one page is enough – longer lines don't fit on the screen).
pub fn read_line(terminal: Handle, page: &mut Memory) -> Result<String, Error> {
    let reply = call(terminal, Attach::LendMut(page), [READ_LINE, 0, 0, 0])?;
    check(reply.words[0])?;
    let length = (reply.words[1] as usize).min(page.len());
    let text =
        core::str::from_utf8(&page.bytes()[..length]).map_err(|_| Error(abi::ERROR_NOT_UTF8))?;
    Ok(String::from(text))
}
