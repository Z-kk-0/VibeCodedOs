//! Starting programs at run time, and waiting for them (DECISIONS.md, 021,
//! 022).
//!
//! A `Program` handle stands for a program image; `call`ing it starts a new
//! process. Which handles the child gets is data, so it travels in lent
//! memory: pairs of (our handle, copy or move) – memory objects included.
//! The child starts with exactly those as 0, 1, 2, … and nothing else. The
//! caller gets a `Child` handle; `call`ing that waits until the child has
//! ended and says how.
//!
//! Loading is the slow part, so it happens in three steps and only the
//! short ones hold the scheduler lock: `prepare` reads and checks the
//! request, `load` builds the address space with no lock held (other cores
//! schedule and talk on meanwhile), `finish` checks again, hands over the
//! handles and schedules the child.

use super::console::lent_bytes;
use super::delivery::block;
use super::message::{Attachment, Message, Mode, write_error, write_handle};
use super::object::{Capability, ProgramCapability};
use super::process::Waiting;
use super::transfer::{self, Phase, give_to_child, process};
use super::{Next, fail_now};
use crate::arch::InterruptFrame;
use crate::task::{self, Exit, TaskContext, Tasks};
use alloc::vec::Vec;
use capability::Handle;
use scheduling::{CoreId, TaskId};

const ENTRY_SIZE: usize = abi::SPAWN_ENTRY_SIZE as usize;

/// A checked request to start a program, waiting to be loaded.
pub struct Request {
    parent: TaskId,
    program: ProgramCapability,
    handles: Vec<Attachment>,
}

/// Step 1, lock held: `call(program, lend_shared(list), [count, ..])`.
pub fn prepare(
    tasks: &mut Tasks,
    me: TaskId,
    program: ProgramCapability,
    message: &Message,
    frame: &mut InterruptFrame,
) -> Next {
    match read_request(tasks, me, message) {
        Ok(handles) => Next::Spawn(Request {
            parent: me,
            program,
            handles,
        }),
        Err(error) => fail_now(frame, error),
    }
}

fn read_request(tasks: &mut Tasks, me: TaskId, message: &Message) -> Result<Vec<Attachment>, u64> {
    let count = message.words[0] as usize;
    let parent = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    let handles = match message.attachment {
        _ if count == 0 => Vec::new(),
        Some(list) if list.mode == Mode::LendShared => {
            let length = count.checked_mul(ENTRY_SIZE).ok_or(abi::ERROR_TOO_LONG)?;
            parse(&lent_bytes(parent, list, length)?)?
        }
        Some(_) => return Err(abi::ERROR_BAD_MODE),
        None => return Err(abi::ERROR_WRONG_KIND),
    };
    for (index, handle) in handles.iter().enumerate() {
        if handles[..index]
            .iter()
            .any(|earlier| earlier.handle == handle.handle)
        {
            return Err(abi::ERROR_BUSY); // the same handle twice
        }
        transfer::check(parent, *handle, Phase::NoLending)?;
    }
    Ok(handles)
}

/// Step 2, no lock held: the slow part.
pub fn load(request: &Request) -> Result<TaskContext, u64> {
    task::load_program(request.program.name, request.program.image)
        .map_err(|_| abi::ERROR_BAD_PROGRAM)
}

/// Step 3, lock held: hand the handles over and schedule the child. The
/// parent was busy in this very call meanwhile, so its handles are almost
/// certainly unchanged – `give_to_child` checks them again all the same.
pub fn finish(
    tasks: &mut Tasks,
    request: Request,
    child: Result<TaskContext, u64>,
    frame: &mut InterruptFrame,
) {
    let result = child.and_then(|mut child| {
        let parent = process(tasks, request.parent)?;
        let child_process = child.process.as_mut().expect("a program has a process");
        give_to_child(parent, child_process, &request.handles)?;
        let id = tasks.spawn(child);
        Ok(process(tasks, request.parent)?
            .handles
            .insert(Capability::Child(id)))
    });
    match result {
        Ok(child) => write_handle(frame, child),
        Err(error) => write_error(frame, error),
    }
}

/// The handle list from lent memory: (handle, `ATTACH_COPY`/`ATTACH_MOVE`).
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
