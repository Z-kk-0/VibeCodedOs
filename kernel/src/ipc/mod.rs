//! Capabilities and ownership IPC (DECISIONS.md, 012 and 020).
//!
//! A program can only use what it holds a handle to. Talking is a
//! synchronous `call` to an endpoint: the caller blocks until a receiver
//! `reply`s. A message is four words plus at most one handle; data only
//! travels as memory objects that are moved or lent (`transfer.rs`).
//!
//! Every operation runs with the scheduler lock held (`task::with_tasks`),
//! because it touches several tasks at once – caller, receiver, their
//! states and saved frames. That makes each operation atomic on all cores.

mod checks;
mod console;
mod delivery;
mod memory_calls;
mod message;
mod object;
mod process;
mod transfer;

pub use checks::check;
pub use memory_calls::{drop_handle, memory_new};
pub use object::Capability;
pub use process::Process;

use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use delivery::{block, deliver, fail_call, saved_frame};
use message::{Delivery, Message, Received, write_error};
use object::WaitingCall;
use scheduling::{CoreId, State};
use transfer::{Phase, end_lend, process, transfer};

/// What the system call does next.
pub enum Next {
    /// Return to the calling task; its registers hold the result.
    Continue,
    /// The task waits now: switch to another one.
    Block,
}

/// `call(endpoint, attachment, words)`.
pub fn call(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let result = (|| {
        let message = Message::from_frame(frame)?;
        let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
        match process.handles.get(Handle(frame.rdi)) {
            Some(Capability::Console) => console::write(process, &message).map(|()| None),
            Some(Capability::Endpoint(endpoint)) => {
                if let Some(attachment) = message.attachment {
                    transfer::check(process, attachment, Phase::Call)?;
                }
                Ok(Some((endpoint.endpoint.clone(), endpoint.badge, message)))
            }
            Some(_) => Err(abi::ERROR_WRONG_KIND),
            None => Err(abi::ERROR_BAD_HANDLE),
        }
    })();
    let (endpoint, badge, message) = match result {
        Ok(Some(call)) => call,
        Ok(None) => {
            write_error(frame, 0); // the console answered at once
            return Next::Continue;
        }
        Err(error) => {
            write_error(frame, error);
            return Next::Continue;
        }
    };
    // From now on the reply may come from another core at any moment, so
    // the frame it will be written into must be known first.
    block(tasks, core, frame);
    let call = WaitingCall {
        caller: me,
        badge,
        message,
    };
    match endpoint.pop_receiver() {
        Some(receiver) => {
            let receiver_frame = saved_frame(tasks, receiver);
            if let Err((error, _)) = deliver(tasks, call, receiver, receiver_frame) {
                endpoint.return_receiver(receiver);
                write_error(frame, error);
                tasks.set_current_state(core, State::Ready);
                return Next::Continue;
            }
            tasks.set_state(receiver, State::Ready);
        }
        None => endpoint.push_caller(call),
    }
    Next::Block
}

/// `receive(endpoint)`.
pub fn receive(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let endpoint = match process(tasks, me) {
        Err(_) => Err(abi::ERROR_WRONG_KIND),
        Ok(process) if process.replying_to.is_some() => Err(abi::ERROR_BUSY),
        Ok(process) => match process.handles.get(Handle(frame.rdi)) {
            Some(Capability::Endpoint(endpoint)) if endpoint.can_receive => {
                Ok(endpoint.endpoint.clone())
            }
            Some(_) => Err(abi::ERROR_WRONG_KIND),
            None => Err(abi::ERROR_BAD_HANDLE),
        },
    };
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(error) => {
            write_error(frame, error);
            return Next::Continue;
        }
    };
    while let Some(call) = endpoint.pop_caller() {
        match deliver(tasks, call, me, frame) {
            Ok(()) => return Next::Continue,
            // Delivery failed (out of memory): that caller learns why, and
            // we try the next one.
            Err((error, caller)) => fail_call(tasks, caller, error),
        }
    }
    block(tasks, core, frame);
    endpoint.push_receiver(me);
    Next::Block
}

/// `reply(attachment, words)`.
pub fn reply(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let result = (|| {
        let message = Message::from_frame(frame)?;
        let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
        if let Some(attachment) = message.attachment {
            transfer::check(process, attachment, Phase::Reply)?;
        }
        let pending = process.replying_to.take().ok_or(abi::ERROR_NO_CALLER)?;
        if let Some(lend) = pending.lend {
            end_lend(tasks, pending.caller, me, lend);
        }
        let received = match message.attachment {
            Some(attachment) => transfer(tasks, me, pending.caller, attachment, Phase::Reply)
                .map(|(received, _)| received),
            None => Ok(Received::default()),
        };
        // SAFETY: see `saved_frame`.
        let caller_frame = unsafe { &mut *saved_frame(tasks, pending.caller) };
        tasks.set_state(pending.caller, State::Ready);
        match received {
            Ok(received) => {
                let reply = Delivery {
                    badge: 0,
                    words: message.words,
                    received,
                };
                reply.write_to(caller_frame);
                Ok(())
            }
            Err(error) => {
                write_error(caller_frame, error);
                Err(error)
            }
        }
    })();
    write_error(frame, result.err().unwrap_or(0));
    Next::Continue
}

/// A program is ending (exit or fault). If it owed a reply, its caller gets
/// lent memory back and `ERROR_PEER_GONE`. Its own memory is freed later,
/// together with the task.
pub fn on_exit(tasks: &mut Tasks, core: CoreId) {
    let me = tasks.current_id(core);
    let Ok(process) = process(tasks, me) else {
        return;
    };
    if let Some(pending) = process.replying_to.take() {
        if let Some(lend) = pending.lend {
            end_lend(tasks, pending.caller, me, lend);
        }
        fail_call(tasks, pending.caller, abi::ERROR_PEER_GONE);
    }
}
