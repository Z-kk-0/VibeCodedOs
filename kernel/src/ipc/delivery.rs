//! Helpers for handing messages to blocked tasks.

use super::message::{Delivery, Received, write_error};
use super::object::WaitingCall;
use super::process::PendingCall;
use super::transfer::{Phase, process, transfer};
use crate::arch::InterruptFrame;
use crate::task::{TaskContext, Tasks};
use scheduling::{CoreId, State, TaskId};

/// Hands `call` to `receiver`, writing the message into `receiver_frame`.
/// On failure the caller keeps everything and gets the call back.
pub(super) fn deliver(
    tasks: &mut Tasks,
    call: WaitingCall,
    receiver: TaskId,
    receiver_frame: *mut InterruptFrame,
) -> Result<(), (u64, TaskId)> {
    let (received, lend) = match call.message.attachment {
        Some(attachment) => transfer(tasks, call.caller, receiver, attachment, Phase::Call)
            .map_err(|error| (error, call.caller))?,
        None => (Received::default(), None),
    };
    let process = process(tasks, receiver).map_err(|error| (error, call.caller))?;
    process.replying_to = Some(PendingCall {
        caller: call.caller,
        lend,
    });
    let delivery = Delivery {
        badge: call.badge,
        words: call.message.words,
        received,
    };
    // SAFETY: the frame of a task that is current here or blocked (see
    // `saved_frame`); we hold the scheduler lock.
    delivery.write_to(unsafe { &mut *receiver_frame });
    Ok(())
}

/// Wakes a blocked caller with an error instead of a reply.
pub(super) fn fail_call(tasks: &mut Tasks, caller: TaskId, error: u64) {
    // SAFETY: see `saved_frame`.
    write_error(unsafe { &mut *saved_frame(tasks, caller) }, error);
    tasks.set_state(caller, State::Ready);
}

/// Marks the current task as waiting, with `frame` as the place its result
/// will be written to.
pub(super) fn block(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) {
    tasks.current(core).saved_frame = frame;
    tasks.set_current_state(core, State::Blocked);
}

/// The saved frame of a blocked task. Writing through it is sound while we
/// hold the scheduler lock: the task is blocked, so it doesn't run; the
/// frame lies on its own kernel stack, which lives as long as the task, and
/// no core touches it until the task runs again. `block` set it while
/// holding the same lock.
pub(super) fn saved_frame(tasks: &mut Tasks, task: TaskId) -> *mut InterruptFrame {
    let context: &mut TaskContext = tasks.task_mut(task).expect("blocked tasks don't vanish");
    context.saved_frame
}
