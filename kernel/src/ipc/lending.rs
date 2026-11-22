//! The memory half of the ownership rules (DECISIONS.md, 020, 027): what a
//! lender or giver loses when memory changes hands, and getting it back.
//!
//! A borrow keeps the lent frames alive on its own (`Lend::memory`): a
//! lender that ends while its memory is lent – stopped while waiting for
//! the reply, or one that went on after `send` – can't pull the frames out
//! from under its borrower. They are freed when the borrow ends.

use super::message::{Attachment, Mode};
use super::object::{BorrowedMemory, Capability, Lend, Memory, OwnedMemory};
use super::process::Process;
use super::transfer::process;
use crate::task::Tasks;
use alloc::sync::Arc;
use alloc::vec::Vec;
use capability::Handle;
use paging::PAGE_SIZE;
use scheduling::TaskId;

/// The sender's half of a memory transfer (checked by `plan` already):
/// returns the capability the receiver gets, mapped at `address` – and,
/// for a lend, the memory the borrow keeps alive.
pub(super) fn take_from_sender(
    sender: &mut Process,
    attachment: Attachment,
    frames: Vec<u64>,
    address: u64,
) -> (Capability, Option<Arc<Memory>>) {
    let handle = attachment.handle;
    if attachment.mode == Mode::Move {
        let memory = take_moved(sender, handle);
        let owned = OwnedMemory {
            memory,
            address,
            lent: false,
        };
        return (Capability::Memory(owned), None);
    }
    let exclusive = attachment.mode == Mode::LendExclusive;
    let (memory, caching) = restrict_lender(sender, handle, exclusive);
    let access = if exclusive {
        abi::ACCESS_EXCLUSIVE
    } else {
        abi::ACCESS_SHARED
    };
    let borrowed = Capability::Borrowed(BorrowedMemory {
        frames,
        address,
        access,
        caching,
    });
    (borrowed, Some(memory))
}

/// Takes moved memory away from its sender for good.
pub(super) fn take_moved(sender: &mut Process, handle: Handle) -> Arc<Memory> {
    let Some(Capability::Memory(owned)) = sender.handles.remove(handle) else {
        unreachable!("checked by plan()");
    };
    sender.release_pages(owned.address, owned.memory.pages() as usize);
    owned.memory
}

/// The lender's side of a lend: its memory is marked lent and becomes
/// read-only (`&`) or disappears (`&mut`) until the borrow ends.
pub(super) fn restrict_lender(
    lender: &mut Process,
    handle: Handle,
    exclusive: bool,
) -> (Arc<Memory>, paging::Caching) {
    let Some(Capability::Memory(owned)) = lender.handles.get_mut(handle) else {
        unreachable!("checked by plan()");
    };
    owned.lent = true;
    let (address, memory) = (owned.address, owned.memory.clone());
    let (frames, caching) = (memory.frames(), memory.caching());
    if exclusive {
        lender.unmap_pages(address, frames.len()); // &mut: the owner can't look
    } else {
        lender.remap_frames(address, &frames, false, caching); // &: nobody can write
    }
    // The lender is in this very system call – blocked in `call`, or in
    // `send` on this core – so it can't try for itself; the kernel checks
    // its own work: the rule must hold in the page tables.
    for page in (address..).step_by(PAGE_SIZE as usize).take(frames.len()) {
        let translation = lender.space.translate(page);
        assert!(
            if exclusive {
                translation.is_none()
            } else {
                translation.is_some_and(|translation| !translation.flags.writable)
            },
            "lent memory still accessible to its lender at {page:#x}"
        );
    }
    (memory, caching)
}

/// Ends a borrow: the borrower loses the memory, the lender (task
/// `lender`) gets it back with full access. Either side may be gone.
pub fn end_lend(tasks: &mut Tasks, lender: TaskId, lend: Lend) {
    if let Ok(borrower) = process(tasks, lend.borrower_task)
        && let Some(Capability::Borrowed(borrowed)) = borrower.handles.remove(lend.borrower)
    {
        borrower.release_pages(borrowed.address, borrowed.frames.len());
    }
    give_back(tasks, lender, lend.lender);
    // If the lender is gone, this was the last reference: the frames go.
    drop(lend.memory);
}

/// The lender's memory is its own again – if the lender is still there.
pub(super) fn give_back(tasks: &mut Tasks, lender: TaskId, handle: Handle) {
    if let Ok(lender) = process(tasks, lender)
        && let Some(Capability::Memory(owned)) = lender.handles.get_mut(handle)
    {
        owned.lent = false;
        let (address, frames, caching) =
            (owned.address, owned.memory.frames(), owned.memory.caching());
        lender.remap_frames(address, &frames, true, caching);
    }
}
