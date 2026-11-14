//! Which task runs next, on which core (DECISIONS.md, 017 and 019).
//!
//! One run queue shared by all cores (the kernel guards it with a lock);
//! each core has its own current task and its own idle task. Round robin:
//! every ready task gets one time slice in turn, on whichever core asks
//! first. Sleeping tasks wake up when their tick comes. A core with nobody
//! ready runs its idle task. Exited tasks are handed back to the caller to
//! free their memory – but never one that is still running somewhere.
//!
//! A task a core has just switched away from may still be in use for a
//! moment (that core is still on its stack). The kernel knows when it is
//! really free; it tells us through the `may_run` / `may_free` callbacks.
//!
//! Pure bookkeeping, generic over what a task carries (`T`), so it's tested
//! on the host; the kernel does the actual switching.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::vec::Vec;

pub type TaskId = u64;
/// Cores are numbered 0, 1, 2, … in the order they are added.
pub type CoreId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ready,
    Sleeping {
        until_tick: u64,
    },
    /// Waits for something only another task can do (e.g. send a message);
    /// that task wakes it with `set_state`.
    Blocked,
    Exited,
}

struct Entry<T> {
    id: TaskId,
    state: State,
    /// The core this task is the current task of, if any.
    running_on: Option<CoreId>,
    /// Idle tasks belong to one core and are never picked by another.
    idle: bool,
    data: T,
}

struct Core {
    current: TaskId,
    idle: TaskId,
}

pub struct RunQueue<T> {
    tasks: Vec<Entry<T>>,
    cores: Vec<Core>,
    next_id: TaskId,
}

impl<T> RunQueue<T> {
    /// `running` is whatever executes right now on core 0 (it becomes task
    /// 0); `idle` runs on core 0 only when no other task is ready.
    pub fn new(running: T, idle: T) -> Self {
        let mut queue = Self {
            tasks: Vec::new(),
            cores: Vec::new(),
            next_id: 0,
        };
        let running = queue.push(running, Some(0), false);
        let idle = queue.push(idle, None, true);
        queue.cores.push(Core {
            current: running,
            idle,
        });
        queue
    }

    /// A new core joins. What it runs right now becomes its idle task.
    pub fn add_core(&mut self, running_idle: T) -> CoreId {
        let core = self.cores.len();
        let idle = self.push(running_idle, Some(core), true);
        self.cores.push(Core {
            current: idle,
            idle,
        });
        core
    }

    pub fn core_count(&self) -> usize {
        self.cores.len()
    }

    pub fn spawn(&mut self, data: T) -> TaskId {
        self.push(data, None, false)
    }

    pub fn current(&mut self, core: CoreId) -> &mut T {
        let index = self.current_index(core);
        &mut self.tasks[index].data
    }

    pub fn current_id(&self, core: CoreId) -> TaskId {
        self.cores[core].current
    }

    /// For sleeping and exiting. Takes effect at the next `switch` of this
    /// core. Idle tasks never sleep or exit.
    pub fn set_current_state(&mut self, core: CoreId, state: State) {
        let index = self.current_index(core);
        if !self.tasks[index].idle {
            self.tasks[index].state = state;
        }
    }

    /// Any task's data, e.g. to deliver a message to it.
    pub fn task_mut(&mut self, id: TaskId) -> Option<&mut T> {
        let index = self.tasks.iter().position(|task| task.id == id)?;
        Some(&mut self.tasks[index].data)
    }

    /// Changes any task's state – waking a blocked task, for example. Idle
    /// tasks are always ready.
    pub fn set_state(&mut self, id: TaskId, state: State) {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| task.id == id && !task.idle)
        {
            task.state = state;
        }
    }

    pub fn state(&self, id: TaskId) -> Option<State> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.state)
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Picks the task `core` runs next and makes it current there. Only
    /// tasks no core is running and for which `may_run` agrees qualify.
    /// Returns whether the current task changed.
    pub fn switch(&mut self, core: CoreId, now: u64, may_run: impl Fn(&T) -> bool) -> bool {
        for task in &mut self.tasks {
            if let State::Sleeping { until_tick } = task.state
                && until_tick <= now
            {
                task.state = State::Ready;
            }
        }
        let current = self.current_index(core);
        // Look at everyone after the current task, wrapping around; the
        // current task itself comes last, so it only keeps running if no
        // one else is ready.
        let count = self.tasks.len();
        let next = (1..count)
            .map(|step| (current + step) % count)
            .find(|&index| {
                let task = &self.tasks[index];
                task.state == State::Ready
                    && task.running_on.is_none()
                    && !task.idle
                    && may_run(&task.data)
            })
            .or_else(|| {
                let task = &self.tasks[current];
                (task.state == State::Ready && !task.idle).then_some(current)
            })
            .unwrap_or_else(|| self.index_of(self.cores[core].idle));

        if next == current {
            return false;
        }
        self.tasks[current].running_on = None;
        self.tasks[next].running_on = Some(core);
        self.cores[core].current = self.tasks[next].id;
        true
    }

    /// Removes one exited task that no core runs and `may_free` agrees to,
    /// and returns its data.
    pub fn take_exited(&mut self, may_free: impl Fn(&T) -> bool) -> Option<T> {
        let index = self.tasks.iter().position(|task| {
            task.state == State::Exited && task.running_on.is_none() && may_free(&task.data)
        })?;
        Some(self.tasks.remove(index).data)
    }

    fn push(&mut self, data: T, running_on: Option<CoreId>, idle: bool) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(Entry {
            id,
            state: State::Ready,
            running_on,
            idle,
            data,
        });
        id
    }

    fn current_index(&self, core: CoreId) -> usize {
        self.index_of(self.cores[core].current)
    }

    fn index_of(&self, id: TaskId) -> usize {
        self.tasks
            .iter()
            .position(|task| task.id == id)
            .expect("current and idle tasks of every core always exist")
    }
}
