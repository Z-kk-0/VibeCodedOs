//! Starting programs and waiting for them (DECISIONS.md, 021, 022).

use crate::ipc::{Attach, Error, Handle, call};
use crate::memory::Memory;

/// How a handle goes to a new program.
pub enum Pass {
    /// It gets a copy (endpoints: a sending end; programs).
    Copy(Handle),
    /// It gets ours (any handle but memory).
    Move(Handle),
    /// It gets this memory; it finds it with `Memory::from_start_handle`.
    Memory(Memory),
}

/// How a child ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    Exited(u64),
    /// Stopped by the kernel after this CPU exception (14 = page fault).
    Killed(u64),
}

/// Starts the program behind `program`, with exactly `handles` as its
/// handles 0, 1, 2, … Returns a handle to the child. If the start fails,
/// memory that was to be passed on is freed.
pub fn spawn<const N: usize>(program: Handle, handles: [Pass; N]) -> Result<Handle, Error> {
    let entry = abi::SPAWN_ENTRY_SIZE as usize;
    let pages = (N * entry).div_ceil(abi::PAGE_SIZE as usize).max(1);
    let mut list = Memory::new(pages as u64)?;
    for (slot, pass) in list.bytes_mut().chunks_exact_mut(entry).zip(&handles) {
        let (handle, mode) = match pass {
            Pass::Copy(handle) => (*handle, abi::ATTACH_COPY),
            Pass::Move(handle) => (*handle, abi::ATTACH_MOVE),
            Pass::Memory(memory) => (memory.handle(), abi::ATTACH_MOVE),
        };
        slot[..8].copy_from_slice(&handle.0.to_le_bytes());
        slot[8..].copy_from_slice(&mode.to_le_bytes());
    }
    let reply = call(program, Attach::Lend(&list), [N as u64, 0, 0, 0])?;
    for pass in handles {
        if let Pass::Memory(memory) = pass {
            core::mem::forget(memory); // the child has it now
        }
    }
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
