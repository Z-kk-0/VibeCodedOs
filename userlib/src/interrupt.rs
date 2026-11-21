//! Waiting for a device interrupt (DECISIONS.md, 025). The kernel gives
//! `init` an `Interrupt` handle per device line it routed; whoever holds it
//! is that device's driver.

use crate::ipc::{Attach, Error, Handle, call_timeout};

/// Waits until the line fired – at most `ticks` (0 = no limit). Returns how
/// often it fired since the last wait (at least 1).
pub fn wait(interrupt: Handle, ticks: u64) -> Result<u64, Error> {
    Ok(call_timeout(interrupt, Attach::Nothing, [0; 4], ticks)?.words[0])
}
