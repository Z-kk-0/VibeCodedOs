//! Talking to the directory service (`programs/directory`), where services
//! register under a name and others look them up (DECISIONS.md, 021).
//! Message: word 0 = `REGISTER` or `LOOKUP`, word 1 = the name as a badge
//! (`abi::badge_from_name`). Reply word 0 = 0 or an error code.

use crate::ipc::{Attach, Error, Handle, call};

pub const REGISTER: u64 = 1;
pub const LOOKUP: u64 = 2;

/// Makes `endpoint` (a copy of it) findable under `name`.
pub fn register(directory: Handle, name: &str, endpoint: Handle) -> Result<(), Error> {
    let reply = call(
        directory,
        Attach::Copy(endpoint),
        [REGISTER, abi::badge_from_name(name), 0, 0],
    )?;
    crate::ipc::check(reply.words[0])
}

/// A sending end of the service registered as `name`. Waits until it is
/// registered, if it isn't yet.
pub fn lookup(directory: Handle, name: &str) -> Result<Handle, Error> {
    let reply = call(
        directory,
        Attach::Nothing,
        [LOOKUP, abi::badge_from_name(name), 0, 0],
    )?;
    crate::ipc::check(reply.words[0])?;
    reply.handle.ok_or(Error(abi::ERROR_WRONG_KIND))
}
