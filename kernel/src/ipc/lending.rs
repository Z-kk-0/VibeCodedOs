//! The memory half of the ownership rules (DECISIONS.md, 020): what a
//! lender or giver loses when memory changes hands, and getting it back.

use super::message::{Attachment, Mode};
use super::object::{BorrowedMemory, Capability, Lend, OwnedMemory};
use super::process::Process;
use super::transfer::process;
use crate::task::Tasks;
use alloc::vec::Vec;
use paging::PAGE_SIZE;
use scheduling::TaskId;

/// The sender's half of a memory transfer (checked by `plan` already):
/// returns the capability the receiver gets, mapped at `address`.
pub(super) fn take_from_sender(
    sender: &mut Process,
    attachment: Attachment,
    frames: Vec<u64>,
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

/// Ends a borrow: the borrower loses the memory, the lender (task
/// `lender`) gets it back with full access. Either side may be gone.
pub fn end_lend(tasks: &mut Tasks, lender: TaskId, lend: Lend) {
    if let Ok(borrower) = process(tasks, lend.borrower_task)
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
