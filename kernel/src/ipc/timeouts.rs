//! Giving up on waiting (DECISIONS.md, 021). A timeout covers only the time
//! in an endpoint's queue: a caller whose message no server took yet, a
//! server no message came to. Once a server took a call, the caller waits
//! for the reply or for the reply handle's loss – cutting that short would
//! mean tearing lent memory out of a server in the middle of its work.

use super::delivery::fail_call;
use super::process::Waiting;
use super::transfer::process;
use crate::task::Tasks;
use crate::time;
use scheduling::State;

/// The tick to give up at for a timeout of `ticks` (0 = never).
pub fn deadline(ticks: u64) -> Option<u64> {
    (ticks != 0).then(|| time::ticks().saturating_add(ticks))
}

/// Wakes every task whose deadline has passed with `ERROR_TIMED_OUT`.
/// Called by the scheduler on every switch, with its lock held.
pub fn expire_timeouts(tasks: &mut Tasks, now: u64) {
    for id in tasks.ids() {
        if tasks.state(id) != Some(State::Blocked) {
            continue;
        }
        let Ok(process) = process(tasks, id) else {
            continue;
        };
        let Some(Waiting::InQueue {
            endpoint,
            as_caller,
            deadline: Some(deadline),
        }) = &process.waiting
        else {
            continue;
        };
        if *deadline > now {
            continue;
        }
        // Still in the queue? Then nobody took the call (or sent one) yet.
        let was_waiting = if *as_caller {
            endpoint.remove_caller(id).is_some()
        } else {
            endpoint.remove_receiver(id)
        };
        if was_waiting {
            fail_call(tasks, id, abi::ERROR_TIMED_OUT);
        }
    }
}
