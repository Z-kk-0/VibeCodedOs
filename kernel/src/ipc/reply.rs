//! `reply(reply_handle, attachment, words)` – answering a call, which uses
//! the reply handle up and ends the call's borrow (DECISIONS.md, 021).

use super::delivery::{fail_call, saved_frame};
use super::lending::end_lend;
use super::message::{Delivery, Message};
use super::object::Capability;
use super::transfer::{self, Phase, nothing, process, transfer};
use super::{Next, fail_now};
use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use scheduling::{CoreId, State};

/// `reply(reply_handle, attachment, words)`.
pub fn reply(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let result = (|| {
        let message = Message::from_frame(frame)?;
        let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
        let handle = Handle(frame.rdi);
        match process.handles.get(handle) {
            Some(Capability::Reply(_)) => {}
            Some(_) => return Err(abi::ERROR_WRONG_KIND),
            None => return Err(abi::ERROR_BAD_HANDLE),
        }
        if let Some(attachment) = message.attachment {
            transfer::check(process, attachment, Phase::NoLending)?;
        }
        let Some(Capability::Reply(pending)) = process.handles.remove(handle) else {
            unreachable!("checked above");
        };
        if let Some(lend) = pending.lend {
            end_lend(tasks, pending.caller, lend);
        }
        let received = match message.attachment {
            Some(attachment) => transfer(tasks, me, pending.caller, attachment, Phase::NoLending)
                .map(|(received, _)| received),
            None => Ok(nothing().0),
        };
        match received {
            Ok(received) => {
                // SAFETY: see `saved_frame`.
                let caller_frame = unsafe { &mut *saved_frame(tasks, pending.caller) };
                let reply = Delivery {
                    badge: 0,
                    words: message.words,
                    received,
                    reply: None,
                };
                reply.write_to(caller_frame);
                tasks.set_state(pending.caller, State::Ready);
                Ok(())
            }
            Err(error) => {
                fail_call(tasks, pending.caller, error);
                Err(error)
            }
        }
    })();
    fail_now(frame, result.err().unwrap_or(0))
}
