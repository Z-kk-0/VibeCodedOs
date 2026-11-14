//! Handing an attached handle from one process to another – Rust's
//! ownership rules, enforced by the page tables (DECISIONS.md, 012, 020):
//!
//! | Mode            | Sender's mapping       | Receiver's mapping | Ends            |
//! |-----------------|------------------------|--------------------|-----------------|
//! | move            | gone                   | writable, owned    | never           |
//! | lend shared `&` | read-only              | read-only          | with the reply  |
//! | lend `&mut`     | gone                   | writable           | with the reply  |
//!
//! Everything that can fail (mapping into the receiver) happens before the
//! sender's side changes, so a failed transfer leaves both as they were.

use super::message::{Attachment, Mode, Received};
use super::object::{BorrowedMemory, Capability, EndpointCapability, OwnedMemory};
use super::process::{Lend, Process};
use crate::task::Tasks;
use paging::PAGE_SIZE;
use scheduling::TaskId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Lending is allowed: the borrow ends with the reply.
    Call,
    /// A reply can't lend – nothing would end the borrow.
    Reply,
}

/// What the attachment is, checked against the rules but not yet changed.
enum Plan {
    Endpoint(EndpointCapability),
    Memory {
        frames: alloc::vec::Vec<u64>,
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
    if mode.is_lend() && phase == Phase::Reply {
        return Err(abi::ERROR_WRONG_KIND);
    }
    match capability {
        Capability::Endpoint(endpoint) => match mode {
            // A copy is always a sending end: there is one receiving end.
            Mode::Copy => Ok(Plan::Endpoint(EndpointCapability {
                can_receive: false,
                ..endpoint.clone()
            })),
            Mode::Move => Ok(Plan::Endpoint(endpoint.clone())),
            Mode::LendShared | Mode::LendExclusive => Err(abi::ERROR_WRONG_KIND),
        },
        Capability::Memory(owned) if owned.lent => Err(abi::ERROR_BUSY),
        Capability::Memory(owned) => {
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
        // Borrowed memory isn't ours to pass on; the console isn't either.
        Capability::Borrowed(_) | Capability::Console => Err(abi::ERROR_WRONG_KIND),
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
    match plan(process(tasks, from)?, attachment, phase)? {
        Plan::Endpoint(endpoint) => {
            if attachment.mode == Mode::Move {
                process(tasks, from)?.handles.remove(handle);
            }
            let received = process(tasks, to)?
                .handles
                .insert(Capability::Endpoint(endpoint));
            Ok((
                Received {
                    handle: Some(received),
                    memory: None,
                },
                None,
            ))
        }
        Plan::Memory {
            frames,
            receiver_writable,
            access,
        } => {
            let address = process(tasks, to)?
                .map_frames(&frames, receiver_writable)
                .ok_or(abi::ERROR_OUT_OF_MEMORY)?;
            let pages = frames.len() as u64;
            let capability = take_from_sender(process(tasks, from)?, attachment, frames, address);
            let received = process(tasks, to)?.handles.insert(capability);
            let lend = attachment.mode.is_lend().then_some(Lend {
                lender: handle,
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
    }
}

/// The sender's half of a memory transfer (checked by `plan` already):
/// returns the capability the receiver gets, mapped at `address`.
fn take_from_sender(
    sender: &mut Process,
    attachment: Attachment,
    frames: alloc::vec::Vec<u64>,
    address: u64,
) -> Capability {
    let handle = attachment.handle;
    if attachment.mode == Mode::Move {
        let Some(Capability::Memory(owned)) = sender.handles.remove(handle) else {
            unreachable!("checked by plan()");
        };
        sender.unmap_pages(owned.address, frames.len());
        return Capability::Memory(OwnedMemory {
            memory: owned.memory,
            address,
            lent: false,
        });
    }
    let Some(Capability::Memory(owned)) = sender.handles.get_mut(handle) else {
        unreachable!("checked by plan()");
    };
    owned.lent = true;
    let own_address = owned.address;
    let exclusive = attachment.mode == Mode::LendExclusive;
    if exclusive {
        sender.unmap_pages(own_address, frames.len()); // &mut: the owner can't look
    } else {
        sender.remap_frames(own_address, &frames, false); // &: nobody can write
    }
    // The lender is blocked in `call` and can't try for itself, so the
    // kernel checks its own work: the rule must hold in the page tables.
    for page in (own_address..)
        .step_by(PAGE_SIZE as usize)
        .take(frames.len())
    {
        let translation = sender.space.translate(page);
        assert!(
            if exclusive {
                translation.is_none()
            } else {
                translation.is_some_and(|translation| !translation.flags.writable)
            },
            "lent memory still accessible to its lender at {page:#x}"
        );
    }
    Capability::Borrowed(BorrowedMemory { frames, address })
}

/// Ends a borrow: the borrower loses the memory, the lender gets it back
/// with full access. Either side may be gone already.
pub fn end_lend(tasks: &mut Tasks, lender: TaskId, borrower: TaskId, lend: Lend) {
    if let Ok(borrower) = process(tasks, borrower)
        && let Some(Capability::Borrowed(borrowed)) = borrower.handles.remove(lend.borrower)
    {
        borrower.unmap_pages(borrowed.address, borrowed.frames.len());
    }
    if let Ok(lender) = process(tasks, lender)
        && let Some(Capability::Memory(owned)) = lender.handles.get_mut(lend.lender)
    {
        owned.lent = false;
        let (address, frames) = (owned.address, owned.memory.frames());
        lender.remap_frames(address, &frames, true);
    }
}

/// The process of task `id`; `ERROR_PEER_GONE` if it ended (or is a kernel task).
pub fn process(tasks: &mut Tasks, id: TaskId) -> Result<&mut Process, u64> {
    tasks
        .task_mut(id)
        .and_then(|task| task.process.as_mut())
        .ok_or(abi::ERROR_PEER_GONE)
}
