//! Starting programs and waiting for them (DECISIONS.md, 021).

use crate::ipc::{Attach, Error, Handle, call};
use crate::memory::Memory;

/// How a handle goes to a new program.
#[derive(Clone, Copy)]
pub enum Pass {
    /// It gets a copy (endpoints: a sending end; programs).
    Copy(Handle),
    /// It gets ours.
    Move(Handle),
}

/// How a child ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    Exited(u64),
    /// Stopped by the kernel after this CPU exception (14 = page fault).
    Killed(u64),
}

/// Starts the program behind `program`, with exactly `handles` as its
/// handles 0, 1, 2, … Returns a handle to the child.
pub fn spawn(program: Handle, handles: &[Pass]) -> Result<Handle, Error> {
    let mut list = Memory::new(1)?;
    for (entry, pass) in list.bytes_mut().chunks_exact_mut(16).zip(handles) {
        let (handle, mode) = match *pass {
            Pass::Copy(handle) => (handle, abi::ATTACH_COPY),
            Pass::Move(handle) => (handle, abi::ATTACH_MOVE),
        };
        entry[..8].copy_from_slice(&handle.0.to_le_bytes());
        entry[8..].copy_from_slice(&mode.to_le_bytes());
    }
    let reply = call(
        program,
        Attach::Lend(&list),
        [handles.len() as u64, 0, 0, 0],
    )?;
    reply.handle.ok_or(Error(abi::ERROR_WRONG_KIND))
}

/// Waits until the child has ended.
pub fn wait(child: Handle) -> Result<Ended, Error> {
    let reply = call(child, Attach::Nothing, [0; 4])?;
    Ok(match reply.words[0] {
        abi::CHILD_EXITED => Ended::Exited(reply.words[1]),
        _ => Ended::Killed(reply.words[1]),
    })
}
