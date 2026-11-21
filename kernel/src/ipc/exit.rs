//! What happens to a program's promises when it ends (exit or fault):
//!
//! - every reply handle it holds is given up: those callers get their lent
//!   memory back and `ERROR_PEER_GONE`;
//! - its receiving ends are released – an endpoint left without any is
//!   closed, and its waiting callers learn it at once;
//! - whoever waits for it as a child is told how it ended.
//!
//! Its memory is freed later, together with the task (`Process::drop`).

use super::delivery::{fail_reply, release_receiving_end, saved_frame};
use super::object::Capability;
use super::process::Waiting;
use super::spawn::write_exit;
use super::stop;
use super::transfer::process;
use crate::task::{Exit, Tasks};
use scheduling::{CoreId, State};

pub fn on_exit(tasks: &mut Tasks, core: CoreId, exit: Exit) {
    let me = tasks.current_id(core);
    let Ok(dying) = process(tasks, me) else {
        return; // a kernel task
    };
    stop::ended(dying.stop_requested);
    let promises = dying.handles.extract(|capability| {
        matches!(capability, Capability::Reply(_)) || capability.is_receiving_end()
    });
    for promise in promises {
        match promise {
            Capability::Reply(reply) => fail_reply(tasks, reply, abi::ERROR_PEER_GONE),
            Capability::Endpoint(end) => release_receiving_end(tasks, end),
            _ => unreachable!("extracted only those two"),
        }
    }
    for waiter in tasks.ids() {
        let waits_for_me = process(tasks, waiter).is_ok_and(
            |waiter| matches!(waiter.waiting, Some(Waiting::ForChild { child, .. }) if child == me),
        );
        if waits_for_me {
            process(tasks, waiter).expect("checked").waiting = None;
            // SAFETY: see `saved_frame` – the waiter is blocked.
            write_exit(unsafe { &mut *saved_frame(tasks, waiter) }, exit);
            tasks.set_state(waiter, State::Ready);
        }
    }
}
