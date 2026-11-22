//! Capabilities and ownership IPC (DECISIONS.md, 012, 020, 021 and 027).
//!
//! A program can only use what it holds a handle to. Talking is a `call`
//! to an endpoint: the caller blocks until a receiver answers through the
//! reply handle that came with the message. Or it is a `send`: the same
//! call, but the sender runs on, and the answer arrives later as a message
//! in its mailbox – an endpoint it receives on, next to answers from
//! anything else it waits for. A message is four words plus at most one
//! handle; data only travels as memory objects that are moved or lent
//! (`transfer.rs`).
//!
//! Every operation runs with the scheduler lock held (`task::with_tasks`),
//! because it touches several tasks at once – caller, receiver, their
//! states and saved frames. That makes each operation atomic on all cores.

mod checks;
mod console;
mod delivery;
mod device;
mod endpoint;
mod endpoint_calls;
mod exit;
mod interrupt;
mod lending;
mod memory;
mod memory_calls;
mod message;
mod object;
mod ports;
mod process;
mod reply;
mod spawn;
mod stop;
mod timeouts;
mod transfer;

pub use checks::{KEYBOARD_IRQ, check};
pub use endpoint_calls::{endpoint_new, mint, revoke};
pub use exit::on_exit;
pub use interrupt::fired as interrupt_fired;
pub use memory_calls::{drop_handle, memory_info, memory_new};
pub use object::Capability;
pub use process::Process;
pub use reply::reply;
pub use spawn::{finish as finish_spawn, load as load_child};
pub use stop::{any_pending as stop_pending, must_stop};
pub use timeouts::expire_timeouts;

use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use delivery::{block, deliver, fail_call, fail_caller, saved_frame, write_answer};
use endpoint::{Endpoint, Queued, WaitingCall};
use message::{Message, write_error};
use object::{Caller, Mailbox};
use process::Waiting;
use scheduling::{CoreId, State, TaskId};
use transfer::{Phase, carry, process};

/// What the system call does next.
pub enum Next {
    /// Return to the calling task; its registers hold the result.
    Continue,
    /// The task waits now: switch to another one.
    Block,
    /// Load this program with no lock held, then `finish_spawn`.
    Spawn(spawn::Request),
}

/// `call(object, attachment, words, timeout)`.
pub fn call(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let message = match Message::from_frame(frame) {
        Ok(message) => message,
        Err(error) => return fail_now(frame, error),
    };
    let Ok(process) = process(tasks, me) else {
        return fail_now(frame, abi::ERROR_WRONG_KIND);
    };
    let endpoint = match process.handles.get(Handle(frame.rdi)) {
        // Kernel objects answer at once.
        Some(Capability::Console) => {
            let result = console::write(process, &message);
            return fail_now(frame, result.err().unwrap_or(0));
        }
        Some(Capability::Program(program)) => {
            let program = *program;
            return spawn::prepare(tasks, me, program, &message, frame);
        }
        Some(&Capability::Child(child)) => {
            return spawn::child_call(tasks, core, child, &message, frame);
        }
        Some(Capability::Device(_)) => {
            device::open(process, Handle(frame.rdi), &message, frame);
            return Next::Continue;
        }
        Some(&Capability::Interrupt(irq)) => {
            if message.attachment.is_some() {
                return fail_now(frame, abi::ERROR_WRONG_KIND);
            }
            return interrupt::wait(tasks, core, irq, frame);
        }
        Some(&Capability::Ports(ports)) => {
            ports::access(ports, &message, frame);
            return Next::Continue;
        }
        Some(Capability::Endpoint(endpoint)) if endpoint.is_revoked() => {
            return fail_now(frame, abi::ERROR_REVOKED);
        }
        Some(Capability::Endpoint(endpoint)) => endpoint.clone(),
        Some(_) => return fail_now(frame, abi::ERROR_WRONG_KIND),
        None => return fail_now(frame, abi::ERROR_BAD_HANDLE),
    };
    if endpoint.endpoint.is_closed() {
        return fail_now(frame, abi::ERROR_PEER_GONE);
    }
    if let Some(attachment) = message.attachment
        && let Err(error) = transfer::check(process, attachment, Phase::Call)
    {
        return fail_now(frame, error);
    }

    let call = WaitingCall {
        caller: Caller::waiting(me),
        badge: endpoint.badge,
        words: message.words,
        attachment: message.attachment,
        carried: None,
    };
    // From now on the reply may come from another core at any moment, so
    // the frame it will be written into must be known first.
    let waiting = Waiting::InQueue {
        endpoint: endpoint.endpoint.clone(),
        as_caller: true,
        deadline: timeouts::deadline(frame.rcx),
    };
    block(tasks, core, frame, Some(waiting));
    match endpoint.endpoint.pop_receiver() {
        Some(receiver) => {
            let receiver_frame = saved_frame(tasks, receiver);
            if let Err((error, _)) = deliver(tasks, call, receiver, receiver_frame) {
                endpoint.endpoint.return_receiver(receiver);
                fail_call(tasks, me, error);
                return Next::Continue;
            }
            tasks.set_state(receiver, State::Ready);
        }
        None => endpoint.endpoint.push_caller(call),
    }
    Next::Block
}

/// `send(object, attachment, words, mailbox, tag)`: a `call` that doesn't
/// wait (DECISIONS.md, 027). The answer – the reply, or why there is none –
/// arrives in `mailbox` (a receiving end of ours) tagged with `tag`. What
/// is attached leaves us now: moved things at once, lent memory is ours
/// again with the answer. Endpoints take sends, and so do the things one
/// waits for: a child (`CHILD_WAIT`) and an interrupt line. Objects that
/// answer at once are used with `call`.
pub fn send(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let status = send_message(tasks, me, frame).err().unwrap_or(0);
    fail_now(frame, status)
}

fn send_message(tasks: &mut Tasks, me: TaskId, frame: &InterruptFrame) -> Result<(), u64> {
    let message = Message::from_frame(frame)?;
    let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    let mailbox = match process.handles.get(Handle(frame.rcx)) {
        Some(Capability::Endpoint(end)) if end.can_receive => Mailbox {
            endpoint: end.endpoint.clone(),
            tag: frame.r13,
        },
        Some(_) => return Err(abi::ERROR_WRONG_KIND),
        None => return Err(abi::ERROR_BAD_HANDLE),
    };
    let endpoint = match process.handles.get(Handle(frame.rdi)) {
        Some(&Capability::Child(child)) if message.attachment.is_none() => {
            return spawn::child_send(tasks, child, &message, mailbox);
        }
        Some(&Capability::Interrupt(irq)) if message.attachment.is_none() => {
            interrupt::watch(tasks, irq, mailbox);
            return Ok(());
        }
        Some(Capability::Endpoint(endpoint)) if endpoint.is_revoked() => {
            return Err(abi::ERROR_REVOKED);
        }
        Some(Capability::Endpoint(endpoint)) => endpoint.clone(),
        Some(_) => return Err(abi::ERROR_WRONG_KIND),
        None => return Err(abi::ERROR_BAD_HANDLE),
    };
    if endpoint.endpoint.is_closed() {
        return Err(abi::ERROR_PEER_GONE);
    }
    let carried = match message.attachment {
        Some(attachment) => {
            transfer::check(process, attachment, Phase::Call)?;
            Some(carry(tasks, me, attachment, Phase::Call)?)
        }
        None => None,
    };
    let caller = Caller {
        task: me,
        answer_to: Some(mailbox),
    };
    let call = WaitingCall {
        caller,
        badge: endpoint.badge,
        words: message.words,
        attachment: None,
        carried,
    };
    match endpoint.endpoint.pop_receiver() {
        Some(receiver) => {
            let receiver_frame = saved_frame(tasks, receiver);
            match deliver(tasks, call, receiver, receiver_frame) {
                Ok(()) => tasks.set_state(receiver, State::Ready),
                Err((error, caller)) => {
                    // The receiver keeps waiting; the failure is an answer.
                    endpoint.endpoint.return_receiver(receiver);
                    fail_caller(tasks, &caller, error);
                }
            }
        }
        None => endpoint.endpoint.push_caller(call),
    }
    Ok(())
}

/// `receive(endpoint, timeout)`.
pub fn receive(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let endpoint = match receiving_end(tasks, me, frame.rdi) {
        Ok(endpoint) => endpoint,
        Err(error) => return fail_now(frame, error),
    };
    while let Some(queued) = endpoint.pop() {
        match queued {
            Queued::Answer(answer) => {
                write_answer(tasks, me, frame, answer);
                return Next::Continue;
            }
            Queued::Call(call) => match deliver(tasks, call, me, frame) {
                Ok(()) => return Next::Continue,
                // Delivery failed (out of memory): that caller learns why,
                // and we try the next one.
                Err((error, caller)) => fail_caller(tasks, &caller, error),
            },
        }
    }
    let waiting = Waiting::InQueue {
        endpoint: endpoint.clone(),
        as_caller: false,
        deadline: timeouts::deadline(frame.rcx),
    };
    block(tasks, core, frame, Some(waiting));
    endpoint.push_receiver(me);
    Next::Block
}

fn receiving_end(
    tasks: &mut Tasks,
    me: TaskId,
    handle: u64,
) -> Result<alloc::sync::Arc<Endpoint>, u64> {
    let process = process(tasks, me).map_err(|_| abi::ERROR_WRONG_KIND)?;
    match process.handles.get(Handle(handle)) {
        Some(Capability::Endpoint(endpoint)) if endpoint.can_receive => {
            Ok(endpoint.endpoint.clone())
        }
        Some(_) => Err(abi::ERROR_WRONG_KIND),
        None => Err(abi::ERROR_BAD_HANDLE),
    }
}

/// Writes `status` (0 = success) and lets the task continue.
pub(super) fn fail_now(frame: &mut InterruptFrame, status: u64) -> Next {
    write_error(frame, status);
    Next::Continue
}
