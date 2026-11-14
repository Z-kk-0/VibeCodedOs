//! Handing an attached handle from one process to another – Rust's
//! ownership rules, enforced by the page tables (DECISIONS.md, 012, 020):
//!
//! | Mode            | Sender's mapping       | Receiver's mapping | Ends            |
//! |-----------------|------------------------|--------------------|-----------------|
//! | move            | gone                   | writable, owned    | never           |
//! | lend shared `&` | read-only              | read-only          | with the reply  |
//! | lend `&mut`     | gone                   | writable           | with the reply  |
//!
//! Other objects can be moved (endpoints, the console, reply handles,
//! programs, children); endpoints and programs can also be copied.
//! Everything that can fail (mapping into the receiver) happens before the
//! sender's side changes, so a failed transfer leaves both as they were.

use super::lending::take_from_sender;
use super::message::{Attachment, Mode, Received};
use super::object::{Capability, Lend};
use super::process::Process;
use crate::task::Tasks;
use alloc::vec::Vec;
use scheduling::TaskId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Lending is allowed: the borrow ends with the reply.
    Call,
    /// A reply (or a program start) can't lend – nothing would end the borrow.
    NoLending,
}

/// What the attachment is, checked against the rules but not yet changed.
enum Plan {
    /// Take the capability out of the sender and give it as it is.
    Move,
    /// Give this new capability; the sender keeps its own.
    Copy(Capability),
    Memory {
        frames: Vec<u64>,
        receiver_writable: bool,
        access: u64,
    },
}

/// Whether `process` may send `attachment` this way. Changes nothing.
pub fn check(process: &Process, attachment: Attachment, phase: Phase) -> Result<(), u64> {
    plan(process, attachment, phase).map(|_| ())
}

fn plan(process: &Process, attachment: Attachment, phase: Phase) -> Result<Plan, u64> {
    let capability = process
        .handles
        .get(attachment.handle)
        .ok_or(abi::ERROR_BAD_HANDLE)?;
    let mode = attachment.mode;
    if mode.is_lend() && phase == Phase::NoLending {
        return Err(abi::ERROR_WRONG_KIND);
    }
    match (capability, mode) {
        (Capability::Memory(owned), _) if owned.lent => Err(abi::ERROR_BUSY),
        (Capability::Memory(owned), _) => {
            let (receiver_writable, access) = match mode {
                Mode::Copy => return Err(abi::ERROR_WRONG_KIND), // memory has one owner
                Mode::Move => (true, abi::ACCESS_OWNED),
                Mode::LendShared => (false, abi::ACCESS_SHARED),
                Mode::LendExclusive => (true, abi::ACCESS_EXCLUSIVE),
            };
            Ok(Plan::Memory {
                frames: owned.memory.frames(),
                receiver_writable,
                access,
            })
        }
        // Borrowed memory isn't ours to pass on.
        (Capability::Borrowed(_), _) => Err(abi::ERROR_WRONG_KIND),
        (Capability::Endpoint(endpoint), Mode::Copy) if endpoint.is_revoked() => {
            Err(abi::ERROR_REVOKED)
        }
        (Capability::Endpoint(endpoint), Mode::Copy) => {
            Ok(Plan::Copy(Capability::Endpoint(endpoint.copy())))
        }
        (Capability::Program(program), Mode::Copy) => Ok(Plan::Copy(Capability::Program(*program))),
        (_, Mode::Copy) => Err(abi::ERROR_WRONG_KIND), // one console, one reply, one child
        (_, Mode::Move) => Ok(Plan::Move),
        (_, Mode::LendShared | Mode::LendExclusive) => Err(abi::ERROR_WRONG_KIND),
    }
}

/// Takes a capability out of `process` for a program it starts (no
/// mapping involved, so memory can't go this way). Check all first with
/// `check_for_child`.
pub fn take_for_child(process: &mut Process, attachment: Attachment) -> Result<Capability, u64> {
    match plan(process, attachment, Phase::NoLending)? {
        Plan::Move => process
            .handles
            .remove(attachment.handle)
            .ok_or(abi::ERROR_BAD_HANDLE),
        Plan::Copy(capability) => Ok(capability),
        Plan::Memory { .. } => Err(abi::ERROR_WRONG_KIND),
    }
}

pub fn check_for_child(process: &Process, attachment: Attachment) -> Result<(), u64> {
    match plan(process, attachment, Phase::NoLending)? {
        Plan::Memory { .. } => Err(abi::ERROR_WRONG_KIND),
        _ => Ok(()),
    }
}

/// Moves, copies or lends the attachment from task `from` to task `to`.
/// Returns what `to` received and, for a lend, the borrow to end later.
pub fn transfer(
    tasks: &mut Tasks,
    from: TaskId,
    to: TaskId,
    attachment: Attachment,
    phase: Phase,
) -> Result<(Received, Option<Lend>), u64> {
    let handle = attachment.handle;
    let (frames, receiver_writable, access) = match plan(process(tasks, from)?, attachment, phase)?
    {
        Plan::Move => {
            let capability = process(tasks, from)?
                .handles
                .remove(handle)
                .ok_or(abi::ERROR_BAD_HANDLE)?;
            return Ok((give(tasks, to, capability)?, None));
        }
        Plan::Copy(capability) => return Ok((give(tasks, to, capability)?, None)),
        Plan::Memory {
            frames,
            receiver_writable,
            access,
        } => (frames, receiver_writable, access),
    };
    let address = process(tasks, to)?
        .map_frames(&frames, receiver_writable)
        .ok_or(abi::ERROR_OUT_OF_MEMORY)?;
    let pages = frames.len() as u64;
    let capability = take_from_sender(process(tasks, from)?, attachment, frames, address);
    let received = process(tasks, to)?.handles.insert(capability);
    let lend = attachment.mode.is_lend().then_some(Lend {
        lender: handle,
        borrower_task: to,
        borrower: received,
    });
    Ok((
        Received {
            handle: Some(received),
            memory: Some((address, pages, access)),
        },
        lend,
    ))
}

fn give(tasks: &mut Tasks, to: TaskId, capability: Capability) -> Result<Received, u64> {
    let handle = process(tasks, to)?.handles.insert(capability);
    Ok(Received {
        handle: Some(handle),
        memory: None,
    })
}

/// The process of task `id`; `ERROR_PEER_GONE` if it ended (or is a kernel task).
pub fn process(tasks: &mut Tasks, id: TaskId) -> Result<&mut Process, u64> {
    tasks
        .task_mut(id)
        .and_then(|task| task.process.as_mut())
        .ok_or(abi::ERROR_PEER_GONE)
}

/// For messages without attachment.
pub fn nothing() -> (Received, Option<Lend>) {
    (Received::default(), None)
}
