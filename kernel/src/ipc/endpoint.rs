//! Endpoints: where servers wait for messages and callers for servers
//! (DECISIONS.md, 020 and 021).
//!
//! An endpoint also remembers how many receiving ends exist – when the last
//! one is gone, nobody can ever answer, so it closes and every waiting
//! caller learns it at once – and, per badge, a generation: revoking a
//! badge bumps it, which kills every sending end minted with the old one.

use super::message::Message;
use crate::sync::SpinLock;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec::Vec;
use scheduling::TaskId;

/// Only touched with the scheduler lock held, so the inner lock is never
/// contended – it's there to make sharing between processes sound.
pub struct Endpoint {
    state: SpinLock<State>,
}

#[derive(Default)]
struct State {
    receivers: VecDeque<TaskId>,
    callers: VecDeque<WaitingCall>,
    receiving_ends: usize,
    closed: bool,
    /// Badges that were revoked, with how often; absent means 0.
    generations: BTreeMap<u64, u64>,
}

/// A caller waiting for a receiver. The attachment is still the caller's:
/// it changes hands only when the message is delivered.
pub struct WaitingCall {
    pub caller: TaskId,
    pub badge: u64,
    pub message: Message,
}

impl Endpoint {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(State::default()),
        })
    }

    pub fn add_receiving_end(&self) {
        self.state.lock().receiving_ends += 1;
    }

    /// A receiving end is gone. If it was the last, the endpoint closes and
    /// the callers still waiting are returned – they must be told.
    pub fn release_receiving_end(&self) -> Vec<WaitingCall> {
        let mut state = self.state.lock();
        state.receiving_ends -= 1;
        if state.receiving_ends > 0 {
            return Vec::new();
        }
        state.closed = true;
        state.callers.drain(..).collect()
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().closed
    }

    pub fn generation(&self, badge: u64) -> u64 {
        self.state
            .lock()
            .generations
            .get(&badge)
            .copied()
            .unwrap_or(0)
    }

    pub fn revoke(&self, badge: u64) {
        *self.state.lock().generations.entry(badge).or_insert(0) += 1;
    }

    pub fn push_receiver(&self, task: TaskId) {
        self.state.lock().receivers.push_back(task);
    }

    pub fn pop_receiver(&self) -> Option<TaskId> {
        self.state.lock().receivers.pop_front()
    }

    /// For a receiver that must keep its place (delivery to it failed).
    pub fn return_receiver(&self, task: TaskId) {
        self.state.lock().receivers.push_front(task);
    }

    /// For a receiver that stops waiting (timeout). Whether it was there.
    pub fn remove_receiver(&self, task: TaskId) -> bool {
        let mut state = self.state.lock();
        let before = state.receivers.len();
        state.receivers.retain(|&waiting| waiting != task);
        state.receivers.len() != before
    }

    pub fn push_caller(&self, call: WaitingCall) {
        self.state.lock().callers.push_back(call);
    }

    pub fn pop_caller(&self) -> Option<WaitingCall> {
        self.state.lock().callers.pop_front()
    }

    /// For a caller that stops waiting (timeout).
    pub fn remove_caller(&self, task: TaskId) -> Option<WaitingCall> {
        let mut state = self.state.lock();
        let index = state.callers.iter().position(|call| call.caller == task)?;
        state.callers.remove(index)
    }
}
