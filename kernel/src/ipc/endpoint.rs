//! Endpoints: where servers wait for messages and callers for servers
//! (DECISIONS.md, 020, 021 and 027).
//!
//! What waits in an endpoint's queue is either a call – from a caller
//! blocked in `call`, or one that `send` left and went on – or an answer:
//! the reply to a `send` whose mailbox this endpoint is, a child's end, a
//! device interrupt. They come out in the order they went in.
//!
//! An endpoint also remembers how many receiving ends exist – when the last
//! one is gone, nobody can ever answer, so it closes and every waiting
//! caller learns it at once – and, per badge, a generation: revoking a
//! badge bumps it, which kills every sending end minted with the old one.

use super::message::Attachment;
use super::object::Caller;
use super::transfer::Carried;
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
    queue: VecDeque<Queued>,
    receiving_ends: usize,
    closed: bool,
    /// Badges that were revoked, with how often; absent means 0.
    generations: BTreeMap<u64, u64>,
}

/// What waits for a receiver.
pub enum Queued {
    Call(WaitingCall),
    Answer(Answer),
}

/// A call waiting for a receiver.
pub struct WaitingCall {
    pub caller: Caller,
    pub badge: u64,
    pub words: [u64; 4],
    /// From `call`: the attachment is still the caller's – the caller is
    /// blocked, and it changes hands only when the message is delivered.
    pub attachment: Option<Attachment>,
    /// From `send`: the sender runs on, so what it attached was taken from
    /// it at once and waits here.
    pub carried: Option<Carried>,
}

/// An answer for whoever receives on a mailbox: no reply handle comes with
/// it, and its status says whether what it answers went well.
pub struct Answer {
    pub tag: u64,
    pub status: u64,
    pub words: [u64; 4],
    pub carried: Option<Carried>,
}

impl Answer {
    pub fn failed(tag: u64, status: u64) -> Self {
        Self {
            tag,
            status,
            words: [0; 4],
            carried: None,
        }
    }
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
    /// everything still queued is returned – callers must be told, and what
    /// answers carry must be given up.
    pub fn release_receiving_end(&self) -> Vec<Queued> {
        let mut state = self.state.lock();
        state.receiving_ends -= 1;
        if state.receiving_ends > 0 {
            return Vec::new();
        }
        state.closed = true;
        state.queue.drain(..).collect()
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
        self.state.lock().queue.push_back(Queued::Call(call));
    }

    pub fn push_answer(&self, answer: Answer) {
        self.state.lock().queue.push_back(Queued::Answer(answer));
    }

    pub fn pop(&self) -> Option<Queued> {
        self.state.lock().queue.pop_front()
    }

    /// For a caller blocked in `call` that stops waiting (timeout, stop).
    /// What `send` left stays: its sender isn't waiting for it.
    pub fn remove_caller(&self, task: TaskId) -> Option<WaitingCall> {
        let mut state = self.state.lock();
        let index = state.queue.iter().position(|queued| {
            matches!(queued, Queued::Call(call)
                if call.caller.task == task && call.caller.answer_to.is_none())
        })?;
        match state.queue.remove(index) {
            Some(Queued::Call(call)) => Some(call),
            _ => unreachable!("found above"),
        }
    }
}
