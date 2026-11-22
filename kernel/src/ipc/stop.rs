//! Stopping a program from outside (DECISIONS.md, 026): its parent – who
//! holds its `Child` handle, and nobody else does – asks, and the kernel
//! ends it at a moment where nothing is half done.
//!
//! That moment is the next time the program would return to user mode.
//! Whatever it waits for is cut short now: a queue, a child, an interrupt,
//! sleeping – and a reply (DECISIONS.md, 027). A server may be working on
//! memory the program lent it; it keeps working on it undisturbed, since
//! the borrow keeps the memory alive by itself (`lending.rs`). When the
//! server replies, nobody is waiting anymore: the reply is dropped and the
//! memory freed.

use super::delivery::fail_call;
use super::interrupt;
use super::process::Waiting;
use super::transfer::process;
use crate::task::Tasks;
use core::sync::atomic::{AtomicUsize, Ordering};
use scheduling::{CoreId, State, TaskId};

/// Programs asked to stop that haven't ended yet. While 0, returning to
/// user mode costs one load more than before.
static PENDING: AtomicUsize = AtomicUsize::new(0);

/// `call(child, [CHILD_STOP])`. A child that has ended already is fine too.
pub fn request(tasks: &mut Tasks, child: TaskId) {
    let Ok(program) = process(tasks, child) else {
        return; // ended
    };
    if program.stop_requested {
        return;
    }
    program.stop_requested = true;
    PENDING.fetch_add(1, Ordering::Relaxed);
    match tasks.state(child) {
        Some(State::Sleeping { .. }) => tasks.set_state(child, State::Ready),
        Some(State::Blocked) => cancel_wait(tasks, child),
        _ => {} // running or ready: it is caught on its way to user mode
    }
}

/// Wakes a blocked program from whatever it waits for.
fn cancel_wait(tasks: &mut Tasks, child: TaskId) {
    let Ok(program) = process(tasks, child) else {
        return;
    };
    let cancelled = match &program.waiting {
        None => false,
        // The server keeps the reply handle and any memory lent with the
        // call (the borrow keeps it alive); its reply will find nobody.
        Some(Waiting::ForReply) => true,
        Some(Waiting::InQueue {
            endpoint,
            as_caller: true,
            ..
        }) => endpoint.remove_caller(child).is_some(),
        Some(Waiting::InQueue { endpoint, .. }) => endpoint.remove_receiver(child),
        Some(Waiting::ForChild { .. }) => true,
        Some(Waiting::ForInterrupt { irq, .. }) => {
            interrupt::stop_waiting(*irq, child);
            true
        }
    };
    if cancelled {
        // What it reads there doesn't matter: it never gets to read it.
        fail_call(tasks, child, abi::ERROR_PEER_GONE);
    }
}

/// Whether the task about to return to user mode on `core` must end
/// instead. Cheap while no stop is pending anywhere.
pub fn must_stop(tasks: &mut Tasks, core: CoreId) -> bool {
    if PENDING.load(Ordering::Relaxed) == 0 {
        return false;
    }
    tasks
        .current(core)
        .process
        .as_ref()
        .is_some_and(|program| program.stop_requested)
}

/// Whether any stop is pending – checked before taking the scheduler lock.
pub fn any_pending() -> bool {
    PENDING.load(Ordering::Relaxed) != 0
}

/// A program asked to stop has ended (one way or another).
pub fn ended(stop_requested: bool) {
    if stop_requested {
        PENDING.fetch_sub(1, Ordering::Relaxed);
    }
}
