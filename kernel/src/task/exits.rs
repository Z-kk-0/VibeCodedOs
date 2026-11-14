//! How tasks ended, kept until someone asks (`wait`).

use super::sleep;
use crate::sync::SpinLock;
use alloc::collections::BTreeMap;
use scheduling::TaskId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// The task finished on its own, with this exit code.
    Code(u64),
    /// The kernel stopped it, e.g. after a page fault; names the reason.
    Killed(&'static str),
}

static EXITS: SpinLock<BTreeMap<TaskId, Exit>> = SpinLock::new(BTreeMap::new());

pub fn record(task: TaskId, exit: Exit) {
    EXITS.lock().insert(task, exit);
}

/// Waits (sleeping) until `task` has ended and returns how.
pub fn wait(task: TaskId) -> Exit {
    loop {
        if let Some(exit) = EXITS.lock().remove(&task) {
            return exit;
        }
        sleep(1);
    }
}
