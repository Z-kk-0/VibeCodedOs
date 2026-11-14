//! Starting programs at run time, and waiting for them (DECISIONS.md, 021).
//!
//! A `Program` handle stands for a program image; `call`ing it starts a new
//! process. Which handles the child gets is data, so it travels in a lent
//! page: pairs of (our handle, copy or move). The child starts with exactly
//! those as 0, 1, 2, … and nothing else. The caller gets a `Child` handle;
//! `call`ing that waits until the child has ended and says how.

use super::console::lent_bytes;
use super::delivery::block;
use super::message::{Attachment, Message, Mode, write_error, write_handle};
use super::object::{Capability, ProgramCapability};
use super::process::Waiting;
use super::transfer::{check_for_child, process, take_for_child};
use super::{Next, fail_now};
use crate::arch::InterruptFrame;
use crate::task::{self, Exit, Tasks};
use alloc::vec::Vec;
use capability::Handle;
use scheduling::{CoreId, TaskId};

const ENTRY_SIZE: usize = 16;

/// `call(program, lend_shared(page), [count, ..])`.
pub fn start(
    tasks: &mut Tasks,
    me: TaskId,
    program: ProgramCapability,
    message: &Message,
    frame: &mut InterruptFrame,
) -> Next {
    match spawn(tasks, me, program, message) {
        Ok(child) => write_handle(frame, child),
        Err(error) => write_error(frame, error),
    }
    Next::Continue
}

fn spawn(
    tasks: &mut Tasks,
    me: TaskId,
    program: ProgramCapability,
    message: &Message,
) -> Result<Handle, u64> {
    let count = message.words[0];
    if count > abi::SPAWN_MAX_HANDLES {
        return Err(abi::ERROR_TOO_LONG);
    }
    let parent = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    let handles = match message.attachment {
        _ if count == 0 => Vec::new(),
        Some(list) if list.mode == Mode::LendShared => {
            parse(&lent_bytes(parent, list, count as usize * ENTRY_SIZE)?)?
        }
        Some(_) => return Err(abi::ERROR_BAD_MODE),
        None => return Err(abi::ERROR_WRONG_KIND),
    };
    // Check everything before taking anything: a failed start changes nothing.
    for (index, handle) in handles.iter().enumerate() {
        if handles[..index]
            .iter()
            .any(|earlier| earlier.handle == handle.handle)
        {
            return Err(abi::ERROR_BUSY); // the same handle twice
        }
        check_for_child(parent, *handle)?;
    }
    let mut child =
        task::load_program(program.name, program.image).map_err(|_| abi::ERROR_BAD_PROGRAM)?;
    let child_process = child.process.as_mut().expect("a program has a process");
    for handle in handles {
        let capability = take_for_child(parent, handle).expect("checked above");
        child_process.handles.insert(capability);
    }
    let id = tasks.spawn(child);
    let parent = process(tasks, me).expect("still running");
    Ok(parent.handles.insert(Capability::Child(id)))
}

/// The handle list from the lent page: (handle, `ATTACH_COPY`/`ATTACH_MOVE`).
fn parse(bytes: &[u8]) -> Result<Vec<Attachment>, u64> {
    bytes
        .chunks_exact(ENTRY_SIZE)
        .map(|entry| {
            let word = |at: usize| {
                let mut bytes = [0; 8];
                bytes.copy_from_slice(&entry[at..at + 8]);
                u64::from_le_bytes(bytes)
            };
            let mode = match word(8) {
                abi::ATTACH_COPY => Mode::Copy,
                abi::ATTACH_MOVE => Mode::Move,
                _ => return Err(abi::ERROR_BAD_MODE),
            };
            Ok(Attachment {
                handle: Handle(word(0)),
                mode,
            })
        })
        .collect()
}

/// `call(child)`: answers at once if the child has ended, else waits.
pub fn wait_for(
    tasks: &mut Tasks,
    core: CoreId,
    child: TaskId,
    frame: &mut InterruptFrame,
) -> Next {
    if let Some(exit) = task::exit_of(child) {
        write_exit(frame, exit);
        return Next::Continue;
    }
    if tasks.state(child).is_none() {
        return fail_now(frame, abi::ERROR_PEER_GONE); // can't happen: exits are kept
    }
    block(tasks, core, frame, Some(Waiting::ForChild(child)));
    Next::Block
}

/// The reply to `call(child)`: word 0 how it ended, word 1 code or exception.
pub fn write_exit(frame: &mut InterruptFrame, exit: Exit) {
    write_error(frame, 0);
    (frame.r10, frame.r8) = match exit {
        Exit::Code(code) => (abi::CHILD_EXITED, code),
        Exit::Killed(vector) => (abi::CHILD_KILLED, vector),
    };
    (frame.r9, frame.r12) = (0, 0);
}
