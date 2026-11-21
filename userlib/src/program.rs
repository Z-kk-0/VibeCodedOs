//! Starting programs and waiting for them (DECISIONS.md, 021, 022).

use crate::ipc::{Attach, Error, Handle, call, call_timeout};
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
    /// The kernel failed while working for it and ended only it (a kernel
    /// bug, contained): the CPU exception, or `abi::KERNEL_PANIC`.
    KernelFailed(u64),
    /// Its parent stopped it (`stop`).
    Stopped,
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
    wait_timeout(child, 0)
}

/// Waits at most `ticks` (0 = no limit) for the child to end;
/// `ERROR_TIMED_OUT` if it is still running.
pub fn wait_timeout(child: Handle, ticks: u64) -> Result<Ended, Error> {
    let reply = call_timeout(child, Attach::Nothing, [abi::CHILD_WAIT, 0, 0, 0], ticks)?;
    Ok(ended(reply.words))
}

/// How the child ended, if it has – without waiting.
pub fn poll(child: Handle) -> Result<Option<Ended>, Error> {
    match call(child, Attach::Nothing, [abi::CHILD_POLL, 0, 0, 0]) {
        Ok(reply) => Ok(Some(ended(reply.words))),
        Err(Error(abi::ERROR_TIMED_OUT)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Asks the kernel to end the child (DECISIONS.md, 026). It ends the next
/// time it would run its own code; `wait` then says `Ended::Stopped`.
pub fn stop(child: Handle) -> Result<(), Error> {
    call(child, Attach::Nothing, [abi::CHILD_STOP, 0, 0, 0]).map(|_| ())
}

fn ended(words: [u64; 4]) -> Ended {
    match words[0] {
        abi::CHILD_EXITED => Ended::Exited(words[1]),
        abi::CHILD_KERNEL_FAILED => Ended::KernelFailed(words[1]),
        abi::CHILD_STOPPED => Ended::Stopped,
        _ => Ended::Killed(words[1]),
    }
}
