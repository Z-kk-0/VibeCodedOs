//! Capabilities and ownership IPC (DECISIONS.md, 012, 020 and 021).
//!
//! A program can only use what it holds a handle to. Talking is a
//! synchronous `call` to an endpoint: the caller blocks until a receiver
//! answers through the reply handle that came with the message. A message
//! is four words plus at most one handle; data only travels as memory
//! objects that are moved or lent (`transfer.rs`).
//!
//! Every operation runs with the scheduler lock held (`task::with_tasks`),
//! because it touches several tasks at once – caller, receiver, their
//! states and saved frames. That makes each operation atomic on all cores.

mod checks;
mod console;
mod delivery;
mod endpoint;
mod endpoint_calls;
mod exit;
mod lending;
mod memory_calls;
mod message;
mod object;
mod process;
mod reply;
mod spawn;
mod timeouts;
mod transfer;

pub use checks::check;
pub use endpoint_calls::{endpoint_new, mint, revoke};
pub use exit::on_exit;
pub use memory_calls::{drop_handle, memory_new};
pub use object::Capability;
pub use process::Process;
pub use reply::reply;
pub use timeouts::expire_timeouts;

use crate::arch::InterruptFrame;
use crate::task::Tasks;
use capability::Handle;
use delivery::{block, deliver, fail_call, saved_frame};
use endpoint::WaitingCall;
use message::{Message, write_error};
use process::Waiting;
use scheduling::{CoreId, State};
use transfer::{Phase, process};

/// What the system call does next.
pub enum Next {
    /// Return to the calling task; its registers hold the result.
    Continue,
    /// The task waits now: switch to another one.
    Block,
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
            return spawn::start(tasks, me, program, &message, frame);
        }
        Some(&Capability::Child(child)) => return spawn::wait_for(tasks, core, child, frame),
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
        caller: me,
        badge: endpoint.badge,
        message,
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

/// `receive(endpoint, timeout)`.
pub fn receive(tasks: &mut Tasks, core: CoreId, frame: &mut InterruptFrame) -> Next {
    let me = tasks.current_id(core);
    let endpoint = match process(tasks, me) {
        Err(_) => Err(abi::ERROR_WRONG_KIND),
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
        Err(error) => return fail_now(frame, error),
    };
    while let Some(call) = endpoint.pop_caller() {
        match deliver(tasks, call, me, frame) {
            Ok(()) => return Next::Continue,
            // Delivery failed (out of memory): that caller learns why, and
            // we try the next one.
            Err((error, caller)) => fail_call(tasks, caller, error),
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

/// Writes `status` (0 = success) and lets the task continue.
pub(super) fn fail_now(frame: &mut InterruptFrame, status: u64) -> Next {
    write_error(frame, status);
    Next::Continue
}
