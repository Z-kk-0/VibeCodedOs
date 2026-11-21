//! Giving up on waiting (DECISIONS.md, 021). A timeout covers only the time
//! in an endpoint's queue – a caller whose message no server took yet, a
//! server no message came to – and waiting for a device interrupt (025).
//! Once a server took a call, the caller waits for the reply or for the
//! reply handle's loss – cutting that short would mean tearing lent memory
//! out of a server in the middle of its work.

use super::delivery::fail_call;
use super::interrupt;
use super::process::Waiting;
use super::transfer::process;
use crate::task::Tasks;
use crate::time;
use core::sync::atomic::{AtomicU64, Ordering};
use scheduling::State;

/// The earliest deadline any task may be waiting for (`u64::MAX`: none).
/// Lets every task switch skip the search in the common case; a deadline
/// that went away meanwhile only costs one search too many.
static NEXT_DEADLINE: AtomicU64 = AtomicU64::new(u64::MAX);

/// The tick to give up at for a timeout of `ticks` (0 = never).
pub fn deadline(ticks: u64) -> Option<u64> {
    let deadline = (ticks != 0).then(|| time::ticks().saturating_add(ticks))?;
    NEXT_DEADLINE.fetch_min(deadline, Ordering::Relaxed);
    Some(deadline)
}

/// Wakes every task whose deadline has passed with `ERROR_TIMED_OUT`.
/// Called by the scheduler on every switch, with its lock held – so it
/// neither allocates nor searches unless a deadline is due.
pub fn expire_timeouts(tasks: &mut Tasks, now: u64) {
    if NEXT_DEADLINE.load(Ordering::Relaxed) > now {
        return;
    }
    let mut next = u64::MAX;
    let mut index = 0;
    // Waking tasks adds or removes none, so the indices stay valid.
    while let Some(id) = tasks.id_at(index) {
        index += 1;
        if tasks.state(id) != Some(State::Blocked) {
            continue;
        }
        let Ok(process) = process(tasks, id) else {
            continue;
        };
        let deadline = match &process.waiting {
            Some(
                Waiting::InQueue {
                    deadline: Some(deadline),
                    ..
                }
                | Waiting::ForInterrupt {
                    deadline: Some(deadline),
                    ..
                },
            ) => *deadline,
            _ => continue,
        };
        if deadline > now {
            next = next.min(deadline);
            continue;
        }
        let was_waiting = match &process.waiting {
            // Still in the queue? Then nobody took the call (or sent one) yet.
            Some(Waiting::InQueue {
                endpoint,
                as_caller: true,
                ..
            }) => endpoint.remove_caller(id).is_some(),
            Some(Waiting::InQueue { endpoint, .. }) => endpoint.remove_receiver(id),
            // An interrupt wakes its waiter with the scheduler lock held, as
            // we hold it now: still blocked means it didn't fire.
            Some(Waiting::ForInterrupt { irq, .. }) => {
                interrupt::stop_waiting(*irq, id);
                true
            }
            _ => unreachable!("matched above"),
        };
        if was_waiting {
            fail_call(tasks, id, abi::ERROR_TIMED_OUT);
        }
    }
    NEXT_DEADLINE.store(next, Ordering::Relaxed);
}
