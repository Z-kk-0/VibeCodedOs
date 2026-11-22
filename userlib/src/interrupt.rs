//! Waiting for a device interrupt (DECISIONS.md, 025, 027). The kernel
//! gives `init` an `Interrupt` handle per device line it routed; whoever
//! holds it is that device's driver.

use crate::ipc::{Attach, Error, Handle, call_timeout};
use crate::mailbox::{Mailbox, Send};

/// Waits until the line fired – at most `ticks` (0 = no limit). Returns how
/// often it fired since the last wait (at least 1).
pub fn wait(interrupt: Handle, ticks: u64) -> Result<u64, Error> {
    Ok(call_timeout(interrupt, Attach::Nothing, [0; 4], ticks)?.words[0])
}

/// Asks to hear of the line's next interrupt in `mailbox`, with `tag`: the
/// answer's word 0 says how often it fired (like `wait`). Asking again also
/// says "done with the device", as `wait` does.
pub fn watch(interrupt: Handle, mailbox: &Mailbox, tag: u64) -> Result<(), Error> {
    let pending = mailbox
        .send(interrupt, Send::Nothing, [0; 4], tag)
        .map_err(|refused| refused.error)?;
    let _ = pending.finish(); // nothing was lent
    Ok(())
}
