//! Which task runs next (DECISIONS.md, 017).
//!
//! Round robin: every ready task gets one time slice in turn. Sleeping
//! tasks wake up when their tick comes. If nobody is ready, the idle task
//! runs. Exited tasks are handed back to the caller to free their memory –
//! but never the task that is running, since we may still be on its stack.
//!
//! Pure bookkeeping, generic over what a task carries (`T`), so it's tested
//! on the host; the kernel does the actual switching.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::vec::Vec;

pub type TaskId = u64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ready,
    Sleeping { until_tick: u64 },
    Exited,
}

struct Entry<T> {
    id: TaskId,
    state: State,
    data: T,
}

pub struct RunQueue<T> {
    tasks: Vec<Entry<T>>,
    current: usize,
    idle_id: TaskId,
    next_id: TaskId,
}

impl<T> RunQueue<T> {
    /// `running` is whatever executes right now (it becomes task 0);
    /// `idle` runs only when no other task is ready, and never sleeps.
    pub fn new(running: T, idle: T) -> Self {
        let tasks = alloc::vec![
            Entry {
                id: 0,
                state: State::Ready,
                data: running
            },
            Entry {
                id: 1,
                state: State::Ready,
                data: idle
            },
        ];
        Self {
            tasks,
            current: 0,
            idle_id: 1,
            next_id: 2,
        }
    }

    pub fn spawn(&mut self, data: T) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(Entry {
            id,
            state: State::Ready,
            data,
        });
        id
    }

    pub fn current(&mut self) -> &mut T {
        &mut self.tasks[self.current].data
    }

    pub fn current_id(&self) -> TaskId {
        self.tasks[self.current].id
    }

    /// For sleeping and exiting. Takes effect at the next `switch`.
    pub fn set_current_state(&mut self, state: State) {
        if self.tasks[self.current].id != self.idle_id {
            self.tasks[self.current].state = state;
        }
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Picks the task to run next and makes it current.
    pub fn switch(&mut self, now: u64) {
        for task in &mut self.tasks {
            if let State::Sleeping { until_tick } = task.state
                && until_tick <= now
            {
                task.state = State::Ready;
            }
        }
        // Look at everyone after the current task, wrapping around; the
        // current task itself comes last, so it only keeps running if no
        // one else is ready.
        let count = self.tasks.len();
        let next_ready = (1..=count)
            .map(|step| (self.current + step) % count)
            .find(|&index| {
                self.tasks[index].state == State::Ready && self.tasks[index].id != self.idle_id
            });
        self.current = match next_ready {
            Some(index) => index,
            None => self.index_of(self.idle_id),
        };
    }

    /// Removes one exited task (never the current one) and returns its data.
    pub fn take_exited(&mut self) -> Option<T> {
        let index = (0..self.tasks.len())
            .find(|&index| index != self.current && self.tasks[index].state == State::Exited)?;
        if index < self.current {
            self.current -= 1;
        }
        Some(self.tasks.remove(index).data)
    }

    fn index_of(&self, id: TaskId) -> usize {
        self.tasks
            .iter()
            .position(|task| task.id == id)
            .expect("idle task is always there")
    }
}
