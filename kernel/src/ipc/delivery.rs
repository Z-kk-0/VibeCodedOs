//! Handing messages to waiting tasks – and telling them when nothing comes.

use super::endpoint::WaitingCall;
use super::lending::end_lend;
use super::message::{Delivery, write_error};
use super::object::{Capability, EndpointCapability, ReplyCapability};
use super::process::Waiting;
use super::transfer::{Phase, nothing, process, transfer};
use crate::arch::InterruptFrame;
use crate::task::{TaskContext, Tasks};
use scheduling::{CoreId, State, TaskId};

/// Hands `call` to `receiver`, writing the message – with a fresh reply
/// handle – into `receiver_frame`. On failure the caller keeps everything.
pub(super) fn deliver(
    tasks: &mut Tasks,
    call: WaitingCall,
    receiver: TaskId,
    receiver_frame: *mut InterruptFrame,
) -> Result<(), (u64, TaskId)> {
    let caller = call.caller;
    let (received, lend) = match call.message.attachment {
        Some(attachment) => transfer(tasks, caller, receiver, attachment, Phase::Call)
            .map_err(|error| (error, caller))?,
        None => nothing(),
    };
    let receiving = process(tasks, receiver).map_err(|error| (error, caller))?;
    receiving.waiting = None;
    let reply = receiving
        .handles
        .insert(Capability::Reply(ReplyCapability { caller, lend }));
    // Delivered: from now on the caller waits for the reply, which no
    // timeout cuts short (DECISIONS.md, 021).
    if let Ok(calling) = process(tasks, caller) {
        calling.waiting = None;
    }
    let delivery = Delivery {
        badge: call.badge,
        words: call.message.words,
        received,
        reply: Some(reply),
    };
    // SAFETY: the frame of a task that is current here or blocked (see
    // `saved_frame`); we hold the scheduler lock.
    delivery.write_to(unsafe { &mut *receiver_frame });
    Ok(())
}

/// Wakes a blocked task with an error instead of a message or reply.
pub(super) fn fail_call(tasks: &mut Tasks, task: TaskId, error: u64) {
    // SAFETY: see `saved_frame`.
    write_error(unsafe { &mut *saved_frame(tasks, task) }, error);
    if let Ok(process) = process(tasks, task) {
        process.waiting = None;
    }
    tasks.set_state(task, State::Ready);
}

/// A reply handle is gone unused (dropped, or its holder ended): the
/// caller gets lent memory back and `error`.
pub(super) fn fail_reply(tasks: &mut Tasks, reply: ReplyCapability, error: u64) {
    if let Some(lend) = reply.lend {
        end_lend(tasks, reply.caller, lend);
    }
    fail_call(tasks, reply.caller, error);
}

/// A receiving end is gone. If it was the endpoint's last, the endpoint is
/// closed and everyone still waiting there hears so.
pub(super) fn release_receiving_end(tasks: &mut Tasks, end: EndpointCapability) {
    for call in end.endpoint.release_receiving_end() {
        fail_call(tasks, call.caller, abi::ERROR_PEER_GONE);
    }
}

/// Marks the current task as waiting, with `frame` as the place its result
/// will be written to, and `waiting` as what may wake it besides that.
pub(super) fn block(
    tasks: &mut Tasks,
    core: CoreId,
    frame: &mut InterruptFrame,
    waiting: Option<Waiting>,
) {
    let current = tasks.current(core);
    current.saved_frame = frame;
    if let Some(process) = current.process.as_mut() {
        process.waiting = waiting;
    }
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
