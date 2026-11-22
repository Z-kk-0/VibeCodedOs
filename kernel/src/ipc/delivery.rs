//! Handing messages to waiting tasks – and telling them when nothing comes.
//!
//! A caller hears back in one of two ways (DECISIONS.md, 027): blocked in
//! `call`, the result is written into its saved registers and it is woken;
//! after `send`, the result is an `Answer` posted to its mailbox, for
//! whoever receives there.

use super::endpoint::{Answer, Queued, WaitingCall};
use super::lending::end_lend;
use super::message::{Delivery, write_error};
use super::object::{Caller, Capability, EndpointCapability, Mailbox, ReplyCapability};
use super::process::Waiting;
use super::transfer::{Phase, discard, nothing, process, transfer, unpack};
use crate::arch::InterruptFrame;
use crate::task::{TaskContext, Tasks};
use scheduling::{CoreId, State, TaskId};

/// Hands `call` to `receiver`, writing the message – with a fresh reply
/// handle – into `receiver_frame`. On failure the caller is returned, to
/// be told: a `call`er keeps everything, what a `send`er attached was
/// given up already (`discard`).
pub(super) fn deliver(
    tasks: &mut Tasks,
    call: WaitingCall,
    receiver: TaskId,
    receiver_frame: *mut InterruptFrame,
) -> Result<(), (u64, Caller)> {
    let WaitingCall {
        caller,
        badge,
        words,
        attachment,
        carried,
    } = call;
    let unpacked = match (attachment, carried) {
        (Some(attachment), _) => transfer(tasks, caller.task, receiver, attachment, Phase::Call),
        (None, Some(carried)) => unpack(tasks, receiver, carried).map_err(|(error, carried)| {
            discard(tasks, carried);
            error
        }),
        (None, None) => Ok(nothing()),
    };
    let (received, lend) = match unpacked {
        Ok(unpacked) => unpacked,
        Err(error) => return Err((error, caller)),
    };
    // Delivered: from now on a `call`er waits for the reply, which no
    // timeout cuts short (DECISIONS.md, 021).
    if caller.answer_to.is_none()
        && let Ok(calling) = process(tasks, caller.task)
    {
        calling.waiting = Some(Waiting::ForReply);
    }
    let receiving = process(tasks, receiver).expect("a receiver is a live program");
    receiving.waiting = None;
    let reply = receiving
        .handles
        .insert(Capability::Reply(ReplyCapability { caller, lend }));
    let delivery = Delivery {
        badge,
        words,
        received,
        reply: Some(reply),
        status: Some(0),
    };
    // SAFETY: the frame of a task that is current here or blocked (see
    // `saved_frame`); we hold the scheduler lock.
    delivery.write_to(unsafe { &mut *receiver_frame });
    Ok(())
}

/// Gives `answer` to whoever receives on `mailbox`: one waiting there gets
/// it now, else it is queued. A closed mailbox has nobody to tell.
pub(super) fn post(tasks: &mut Tasks, mailbox: &Mailbox, answer: Answer) {
    let endpoint = &mailbox.endpoint;
    if endpoint.is_closed() {
        if let Some(carried) = answer.carried {
            discard(tasks, carried);
        }
        return;
    }
    match endpoint.pop_receiver() {
        Some(receiver) => {
            let frame = saved_frame(tasks, receiver);
            write_answer(tasks, receiver, frame, answer);
            tasks.set_state(receiver, State::Ready);
        }
        None => endpoint.push_answer(answer),
    }
}

/// Writes `answer` into the frame of `receiver`, which is in `receive`.
pub(super) fn write_answer(
    tasks: &mut Tasks,
    receiver: TaskId,
    frame: *mut InterruptFrame,
    answer: Answer,
) {
    let mut status = answer.status;
    let (received, _) = match answer.carried {
        None => nothing(),
        Some(carried) => match unpack(tasks, receiver, carried) {
            Ok(unpacked) => unpacked, // answers never lend
            Err((error, carried)) => {
                discard(tasks, carried);
                status = error;
                nothing()
            }
        },
    };
    if let Ok(receiving) = process(tasks, receiver) {
        receiving.waiting = None;
    }
    let delivery = Delivery {
        badge: answer.tag,
        words: answer.words,
        received,
        reply: None,
        status: Some(status),
    };
    // SAFETY: see `deliver`.
    delivery.write_to(unsafe { &mut *frame });
}

/// Tells `caller` its call failed: a woken `call`er, or an answer.
pub(super) fn fail_caller(tasks: &mut Tasks, caller: &Caller, error: u64) {
    match &caller.answer_to {
        Some(mailbox) => post(tasks, mailbox, Answer::failed(mailbox.tag, error)),
        None => fail_call(tasks, caller.task, error),
    }
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

/// Whether `task` is still blocked in `call`, waiting for a reply – not so
/// once it was stopped (DECISIONS.md, 027): then nobody hears the reply.
pub(super) fn waits_for_reply(tasks: &mut Tasks, task: TaskId) -> bool {
    tasks.state(task) == Some(State::Blocked)
        && process(tasks, task)
            .is_ok_and(|caller| matches!(caller.waiting, Some(Waiting::ForReply)))
}

/// A reply handle is gone unused (dropped, or its holder ended): the
/// caller gets lent memory back and `error`.
pub(super) fn fail_reply(tasks: &mut Tasks, reply: ReplyCapability, error: u64) {
    if let Some(lend) = reply.lend {
        end_lend(tasks, reply.caller.task, lend);
    }
    let waiting = reply.caller.answer_to.is_none();
    if !waiting || waits_for_reply(tasks, reply.caller.task) {
        fail_caller(tasks, &reply.caller, error);
    }
}

/// A receiving end is gone. If it was the endpoint's last, the endpoint is
/// closed and everyone still waiting there hears so.
pub(super) fn release_receiving_end(tasks: &mut Tasks, end: EndpointCapability) {
    for queued in end.endpoint.release_receiving_end() {
        match queued {
            Queued::Call(call) => {
                if let Some(carried) = call.carried {
                    discard(tasks, carried);
                }
                fail_caller(tasks, &call.caller, abi::ERROR_PEER_GONE);
            }
            Queued::Answer(answer) => {
                if let Some(carried) = answer.carried {
                    discard(tasks, carried);
                }
            }
        }
    }
}

/// A capability nobody holds anymore – given up as `drop` gives it up.
/// Memory must have been unmapped by whoever held it.
pub(super) fn destroy(tasks: &mut Tasks, capability: Capability) {
    match capability {
        Capability::Reply(reply) => fail_reply(tasks, reply, abi::ERROR_PEER_GONE),
        Capability::Endpoint(end) if end.can_receive => release_receiving_end(tasks, end),
        Capability::Child(child) => crate::task::forget_exit(child),
        _ => {}
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
