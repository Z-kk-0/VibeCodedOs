//! `reply(reply_handle, attachment, words)` – answering a call, which uses
//! the reply handle up and ends the call's borrow (DECISIONS.md, 021).
//!
//! A caller blocked in `call` gets the reply in its registers. One that
//! used `send` gets it as an answer in its mailbox (027). One that was
//! stopped while waiting gets nothing: the replier hears `ERROR_PEER_GONE`
//! and keeps its attachment.

use super::delivery::{fail_call, post, saved_frame, waits_for_reply};
use super::endpoint::Answer;
use super::lending::end_lend;
use super::message::{Delivery, Message};
use super::object::{Capability, Mailbox};
use super::transfer::{self, Phase, carry, nothing, process, transfer};
use super::{Next, fail_now};
use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use scheduling::{CoreId, State, TaskId};

/// `reply(reply_handle, attachment, words)`.
pub fn reply(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let status = answer(tasks, me, frame).err().unwrap_or(0);
    fail_now(frame, status)
}

fn answer(tasks: &mut Tasks, me: TaskId, frame: &InterruptFrame) -> Result<(), u64> {
    let message = Message::from_frame(frame)?;
    let replier = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    let handle = Handle(frame.rdi);
    match replier.handles.get(handle) {
        Some(Capability::Reply(_)) => {}
        Some(_) => return Err(abi::ERROR_WRONG_KIND),
        None => return Err(abi::ERROR_BAD_HANDLE),
    }
    if let Some(attachment) = message.attachment {
        transfer::check(replier, attachment, Phase::NoLending)?;
    }
    let Some(Capability::Reply(pending)) = replier.handles.remove(handle) else {
        unreachable!("checked above");
    };
    let caller = pending.caller;
    if let Some(lend) = pending.lend {
        end_lend(tasks, caller.task, lend);
    }
    if let Some(mailbox) = caller.answer_to {
        return answer_mailbox(tasks, me, &mailbox, &message);
    }
    if !waits_for_reply(tasks, caller.task) {
        return Err(abi::ERROR_PEER_GONE); // stopped meanwhile
    }
    let received = match message.attachment {
        Some(attachment) => transfer(tasks, me, caller.task, attachment, Phase::NoLending)
            .map(|(received, _)| received),
        None => Ok(nothing().0),
    };
    match received {
        Ok(received) => {
            // SAFETY: see `saved_frame`.
            let caller_frame = unsafe { &mut *saved_frame(tasks, caller.task) };
            let reply = Delivery {
                badge: 0,
                words: message.words,
                received,
                reply: None,
                status: None,
            };
            reply.write_to(caller_frame);
            if let Ok(calling) = process(tasks, caller.task) {
                calling.waiting = None;
            }
            tasks.set_state(caller.task, State::Ready);
            Ok(())
        }
        Err(error) => {
            fail_call(tasks, caller.task, error);
            Err(error)
        }
    }
}

/// The reply to a `send`: an answer in the sender's mailbox.
fn answer_mailbox(
    tasks: &mut Tasks,
    me: TaskId,
    mailbox: &Mailbox,
    message: &Message,
) -> Result<(), u64> {
    let carried = match message.attachment {
        Some(attachment) => match carry(tasks, me, attachment, Phase::NoLending) {
            Ok(carried) => Some(carried),
            Err(error) => {
                post(tasks, mailbox, Answer::failed(mailbox.tag, error));
                return Err(error);
            }
        },
        None => None,
    };
    let answer = Answer {
        tag: mailbox.tag,
        status: 0,
        words: message.words,
        carried,
    };
    post(tasks, mailbox, answer);
    Ok(())
}
