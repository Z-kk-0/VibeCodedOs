//! Talking to the terminal (`programs/terminal`, DECISIONS.md, 026, 027).
//!
//! - Writing: the terminal speaks the log's protocol – lend it a page `&`
//!   with text, word 0 = its length. So a program whose handle 0 is a
//!   terminal prints onto the screen with plain `println!`; it can't tell
//!   the difference, and doesn't need to.
//! - Reading: `[READ_LINE, 0, 0, 0]` with a page lent `&mut`. The reply
//!   comes when a line was finished with Enter (at once, if one was typed
//!   ahead): it is in the page, word 1 = its length in bytes. Several
//!   readers wait for lines in turn.
//! - Ctrl+C: `[WAIT_INTERRUPT, 0, 0, 0]`. The reply comes when Ctrl+C is
//!   pressed (at once, if it was pressed while nobody waited). Every
//!   waiter hears the same press.
//!
//! Both replies may take long – `send` them (`request_line`,
//! `watch_interrupt`) to wait for them and other things at once.
//!
//! Keys only count from the keyboard driver's handle (badge "keyboard"):
//! whoever else holds the terminal can't type into it.

use crate::ipc::{Attach, Error, Handle, call, check};
use crate::mailbox::{Mailbox, Pending, Refused, Send};
use crate::memory::Memory;
use crate::message::Received;
use alloc::string::String;

pub const READ_LINE: u64 = 16;
/// `[CLEAR, 0, 0, 0]`: empties the screen's lines.
pub const CLEAR: u64 = 17;
/// `[WAIT_INTERRUPT, 0, 0, 0]`: replies when Ctrl+C is pressed.
pub const WAIT_INTERRUPT: u64 = 18;

/// Empties the terminal's lines on the screen.
pub fn clear(terminal: Handle) -> Result<(), Error> {
    check(call(terminal, Attach::Nothing, [CLEAR, 0, 0, 0])?.words[0])
}

/// Waits for the next line typed into the terminal; `page` is where it
/// arrives (one page is enough – longer lines don't fit on the screen).
pub fn read_line(terminal: Handle, page: &mut Memory) -> Result<String, Error> {
    let reply = call(terminal, Attach::LendMut(page), [READ_LINE, 0, 0, 0])?;
    line_in(&reply, page)
}

/// Asks for the next line without waiting: it arrives in `mailbox` with
/// `tag`; then `Pending::finish` gives `page` back and `line_in` reads it.
pub fn request_line(
    terminal: Handle,
    page: Memory,
    mailbox: &Mailbox,
    tag: u64,
) -> Result<Pending, Refused> {
    mailbox.send(terminal, Send::LendMut(page), [READ_LINE, 0, 0, 0], tag)
}

/// The line a `READ_LINE` reply put into `page`.
pub fn line_in(reply: &Received, page: &Memory) -> Result<String, Error> {
    check(reply.words[0])?;
    let length = (reply.words[1] as usize).min(page.len());
    let text =
        core::str::from_utf8(&page.bytes()[..length]).map_err(|_| Error(abi::ERROR_NOT_UTF8))?;
    Ok(String::from(text))
}

/// Asks to hear of the next Ctrl+C in `mailbox`, with `tag`.
pub fn watch_interrupt(terminal: Handle, mailbox: &Mailbox, tag: u64) -> Result<(), Error> {
    let words = [WAIT_INTERRUPT, 0, 0, 0];
    let pending = mailbox
        .send(terminal, Send::Nothing, words, tag)
        .map_err(|refused| refused.error)?;
    let _ = pending.finish(); // nothing was lent
    Ok(())
}
