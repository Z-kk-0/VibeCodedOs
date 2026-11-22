//! Hardware interrupts as messages (DECISIONS.md, 025, 027).
//!
//! An `Interrupt` handle stands for one device interrupt line. `call`ing it
//! is a call like any other – the "server" that answers is the device: the
//! caller blocks until the line fires, and the reply says how often it
//! fired. So a driver is an ordinary program in an ordinary loop: wait for
//! the interrupt, talk to the device, repeat. The kernel does no device
//! work at all: it acknowledges the interrupt and wakes the driver.
//!
//! `send`ing to it instead asks for the same reply as an answer in a
//! mailbox – so a driver can wait for its device and for its clients in
//! one `receive`.
//!
//! The handle can be moved but not copied: one line, one driver – so the
//! only one that can ever wait on a line is whoever holds its handle.

use super::delivery::{block, post, saved_frame};
use super::endpoint::Answer;
use super::message::{Delivery, Received};
use super::object::Mailbox;
use super::process::Waiting;
use super::transfer::process;
use super::{Next, timeouts};
use crate::arch::{self, InterruptFrame};
use crate::sync::SpinLock;
use crate::task::{self, Tasks};
use scheduling::{CoreId, State, TaskId};

/// The 16 ISA interrupts (`arch::route_isa_interrupt`).
const LINES: usize = 16;

struct Line {
    /// How often it fired since its driver last asked.
    fired: u64,
    /// The driver, while it waits.
    waiter: Option<Waiter>,
}

enum Waiter {
    /// Blocked in `call`.
    Task(TaskId),
    /// After `send`: tell it there.
    Mailbox(Mailbox),
}

/// Only touched with the scheduler lock held (lock order: scheduler, then
/// this), which keeps "fired" and "waits" from crossing on two cores.
static STATE: SpinLock<[Line; LINES]> = SpinLock::new(
    [const {
        Line {
            fired: 0,
            waiter: None,
        }
    }; LINES],
);

/// `call(interrupt)`: answers at once if the line fired meanwhile, else waits.
/// Calling again also says "done with the device": a level-triggered line,
/// masked since it fired, opens again.
pub fn wait(tasks: &mut Tasks, core: CoreId, irq: u8, frame: &mut InterruptFrame) -> Next {
    arch::acknowledge_isa_interrupt(irq);
    let mut lines = STATE.lock();
    let line = &mut lines[usize::from(irq)];
    if line.fired > 0 {
        write_fired(frame, core::mem::take(&mut line.fired));
        return Next::Continue;
    }
    line.waiter = Some(Waiter::Task(tasks.current_id(core)));
    drop(lines);
    let waiting = Waiting::ForInterrupt {
        irq,
        deadline: timeouts::deadline(frame.rcx),
    };
    block(tasks, core, frame, Some(waiting));
    Next::Block
}

/// `send(interrupt, mailbox)`: like `wait`, but the answer goes to the
/// mailbox – at once if the line fired meanwhile.
pub fn watch(tasks: &mut Tasks, irq: u8, mailbox: Mailbox) {
    arch::acknowledge_isa_interrupt(irq);
    let mut lines = STATE.lock();
    let line = &mut lines[usize::from(irq)];
    if line.fired > 0 {
        let fired = core::mem::take(&mut line.fired);
        drop(lines);
        post(tasks, &mailbox, fired_answer(mailbox.tag, fired));
        return;
    }
    line.waiter = Some(Waiter::Mailbox(mailbox));
}

/// Called by the interrupt handler, after the interrupt was acknowledged.
pub fn fired(irq: u8) {
    task::with_tasks(|tasks, _| {
        let mut lines = STATE.lock();
        let line = &mut lines[usize::from(irq)];
        line.fired += 1;
        let Some(waiter) = line.waiter.take() else {
            return; // counted; the driver gets it with its next call
        };
        let fired = core::mem::take(&mut line.fired);
        drop(lines);
        match waiter {
            Waiter::Task(waiter) => {
                let process = process(tasks, waiter).expect("a waiting driver is a live program");
                process.waiting = None;
                // SAFETY: see `saved_frame` – the waiter is blocked.
                write_fired(unsafe { &mut *saved_frame(tasks, waiter) }, fired);
                tasks.set_state(waiter, State::Ready);
            }
            Waiter::Mailbox(mailbox) => post(tasks, &mailbox, fired_answer(mailbox.tag, fired)),
        }
    });
}

/// `task` gave up waiting for `irq` (its timeout ran out, or it was stopped).
pub fn stop_waiting(irq: u8, task: TaskId) {
    let mut lines = STATE.lock();
    let line = &mut lines[usize::from(irq)];
    if matches!(line.waiter, Some(Waiter::Task(waiter)) if waiter == task) {
        line.waiter = None;
    }
}

fn write_fired(frame: &mut InterruptFrame, fired: u64) {
    Delivery {
        badge: 0,
        words: [fired, 0, 0, 0],
        received: Received::default(),
        reply: None,
        status: None,
    }
    .write_to(frame);
}

fn fired_answer(tag: u64, fired: u64) -> Answer {
    Answer {
        tag,
        status: 0,
        words: [fired, 0, 0, 0],
        carried: None,
    }
}
