//! How tasks ended, kept until someone asks (`wait`).

use super::sleep;
use crate::sync::SpinLock;
use alloc::collections::{BTreeMap, BTreeSet};
use scheduling::TaskId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// The task finished on its own, with this exit code.
    Code(u64),
    /// The kernel stopped it: the CPU exception it caused (e.g. 14 = page fault).
    Killed(u64),
}

/// How tasks ended, until someone asked (`wait`) or nobody can ask anymore
/// (`forget`).
static EXITS: SpinLock<Exits> = SpinLock::new(Exits {
    ended: BTreeMap::new(),
    unwanted: BTreeSet::new(),
});

struct Exits {
    ended: BTreeMap<TaskId, Exit>,
    /// Still running, but nobody will ever ask how they end.
    unwanted: BTreeSet<TaskId>,
}

pub fn record(task: TaskId, exit: Exit) {
    let mut exits = EXITS.lock();
    if !exits.unwanted.remove(&task) {
        exits.ended.insert(task, exit);
    }
}

/// Nobody will ask how `task` ended (its `Child` handle is gone): drop the
/// record, or don't keep one when it ends.
pub fn forget_exit(task: TaskId) {
    let mut exits = EXITS.lock();
    if exits.ended.remove(&task).is_none() {
        exits.unwanted.insert(task);
    }
}

/// How `task` ended, if it has – without forgetting it (unlike `wait`).
pub fn exit_of(task: TaskId) -> Option<Exit> {
    EXITS.lock().ended.get(&task).copied()
}

/// Waits (sleeping) until `task` has ended and returns how.
pub fn wait(task: TaskId) -> Exit {
    loop {
        if let Some(exit) = EXITS.lock().ended.remove(&task) {
            return exit;
        }
        sleep(1);
    }
}
