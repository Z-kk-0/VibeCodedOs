//! Tasks: independent flows of execution inside the kernel, switched
//! preemptively by the timer (DECISIONS.md, 017). Which task runs next is
//! decided by the `scheduling` crate; this module does the switching.
//!
//! A task switch is nothing but answering an interrupt with a different
//! `InterruptFrame`: the interrupt stub then continues on that task's stack.

mod checks;
mod context;

pub use checks::check;

use crate::arch::{self, InterruptFrame, cpu};
use crate::sync::SpinLock;
use crate::time;
use alloc::boxed::Box;
use context::TaskContext;
use scheduling::{RunQueue, State};

static SCHEDULER: SpinLock<Option<RunQueue<TaskContext>>> = SpinLock::new(None);

/// Makes the code running now task 0 ("boot") and creates the idle task.
/// From now on, every timer tick may switch tasks. Needs the heap.
pub fn init() {
    let idle = TaskContext::new(idle_entry, 0);
    *SCHEDULER.lock() = Some(RunQueue::new(TaskContext::running(), idle));
}

/// Starts `work` as a new task. When `work` returns, the task exits.
pub fn spawn(work: impl FnOnce() + Send + 'static) {
    // `Box<dyn FnOnce>` is a "fat" pointer (data + vtable, two words). Boxing
    // it once more gives one plain pointer that fits into a register.
    let work: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(work));
    let context = TaskContext::new(task_entry, Box::into_raw(work) as u64);
    with_scheduler(|scheduler| scheduler.spawn(context));
}

/// Sleeps for at least `ticks` timer ticks without using the CPU.
pub fn sleep(ticks: u64) {
    let until_tick = time::ticks() + ticks;
    with_scheduler(|scheduler| scheduler.set_current_state(State::Sleeping { until_tick }));
    arch::yield_now();
}

/// Ends the current task. Its stack is freed at a later switch.
pub fn exit() -> ! {
    with_scheduler(|scheduler| scheduler.set_current_state(State::Exited));
    loop {
        arch::yield_now(); // never comes back: exited tasks aren't scheduled
    }
}

pub fn task_count() -> usize {
    with_scheduler(|scheduler| scheduler.task_count())
}

/// Called by the timer and yield interrupts with the interrupted task's
/// frame. Returns the frame of the task to continue with.
pub fn switch(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    let mut guard = SCHEDULER.lock();
    let Some(scheduler) = guard.as_mut() else {
        return frame; // scheduler not started yet
    };
    scheduler.current().saved_frame = frame;
    // Free finished tasks *before* picking the next one: the current task is
    // never among them, so we never free the stack we're running on.
    while let Some(finished) = scheduler.take_exited() {
        drop(finished);
    }
    scheduler.switch(time::ticks());
    scheduler.current().saved_frame
}

fn with_scheduler<R>(action: impl FnOnce(&mut RunQueue<TaskContext>) -> R) -> R {
    action(SCHEDULER.lock().as_mut().expect("scheduler not started"))
}

/// Where every spawned task begins.
extern "sysv64" fn task_entry(work: u64) -> ! {
    // SAFETY: `spawn` passed exactly this pointer from `Box::into_raw`.
    let work = unsafe { Box::from_raw(work as *mut Box<dyn FnOnce() + Send>) };
    work();
    exit()
}

extern "sysv64" fn idle_entry(_: u64) -> ! {
    loop {
        cpu::wait_for_interrupt();
    }
}
