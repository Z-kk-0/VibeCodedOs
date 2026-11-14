//! Tasks: independent flows of execution, switched preemptively by the
//! timer (DECISIONS.md, 017) on every core (019). A task is either a kernel
//! task or a user program in its own address space (018). Which task runs
//! next is decided by the `scheduling` crate; this module does the switching.
//!
//! A task switch is nothing but answering an interrupt with a different
//! `InterruptFrame`: the interrupt stub then continues on that task's stack.

mod checks;
mod context;
mod exits;
mod program;
mod switch;

pub use checks::check;
pub use context::TaskContext;
pub use exits::{Exit, exit_of, forget_exit, wait};
pub use program::{grow_stack, load_program, spawn_program};
pub use switch::{switch, switch_on_tick};

use crate::arch::{self, InterruptFrame, cpu};
use crate::sync::SpinLock;
use crate::time;
use alloc::boxed::Box;
use scheduling::{CoreId, RunQueue, State, TaskId};

/// All tasks of all cores, with their per-core bookkeeping.
pub type Tasks = RunQueue<TaskContext>;

static SCHEDULER: SpinLock<Option<RunQueue<TaskContext>>> = SpinLock::new(None);

/// Makes the code running now task 0 ("boot") and creates core 0's idle
/// task. From now on, every timer tick may switch tasks. Needs the heap.
pub fn init() {
    let idle = TaskContext::new_kernel("idle", idle_entry, 0);
    *SCHEDULER.lock() = Some(RunQueue::new(TaskContext::running("boot"), idle));
}

/// A freshly started core joins: what it runs now becomes its idle task.
pub fn add_this_core() {
    let core = arch::core_index();
    let added = with_scheduler(|scheduler, _| scheduler.add_core(TaskContext::running("idle")));
    assert_eq!(added, core, "cores must join the scheduler in order");
}

/// Starts `work` as a new kernel task. When `work` returns, the task exits.
pub fn spawn(name: &'static str, work: impl FnOnce() + Send + 'static) -> TaskId {
    // `Box<dyn FnOnce>` is a "fat" pointer (data + vtable, two words). Boxing
    // it once more gives one plain pointer that fits into a register.
    let work: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(work));
    spawn_context(TaskContext::new_kernel(
        name,
        task_entry,
        Box::into_raw(work) as u64,
    ))
}

/// Sleeps for at least `ticks` timer ticks without using the CPU.
pub fn sleep(ticks: u64) {
    let until_tick = time::ticks() + ticks;
    with_scheduler(|scheduler, core| {
        scheduler.set_current_state(core, State::Sleeping { until_tick })
    });
    arch::yield_now();
}

/// Ends the current kernel task. Its stack is freed at a later switch.
fn exit() -> ! {
    with_scheduler(|scheduler, core| {
        exits::record(scheduler.current_id(core), Exit::Code(0));
        scheduler.set_current_state(core, State::Exited);
    });
    loop {
        arch::yield_now(); // never comes back: exited tasks aren't scheduled
    }
}

pub fn task_count() -> usize {
    with_scheduler(|scheduler, _| scheduler.task_count())
}

/// How many cores take part in scheduling.
pub fn core_count() -> usize {
    with_scheduler(|scheduler, _| scheduler.core_count())
}

pub fn current_name() -> &'static str {
    with_scheduler(|scheduler, core| scheduler.current(core).name)
}

/// Runs `action` with every task and the core we're on, atomically on all
/// cores (scheduler lock held, interrupts off) – for IPC, which changes
/// several tasks at once.
pub fn with_tasks<R>(action: impl FnOnce(&mut Tasks, CoreId) -> R) -> R {
    with_scheduler(action)
}

/// For use inside an interrupt (system call, fault): ends the current task
/// and returns the frame of the task to continue with.
pub fn end_current(frame: *mut InterruptFrame, exit: Exit) -> *mut InterruptFrame {
    with_scheduler(|scheduler, core| {
        // A program that owed a reply hands lent memory back first.
        crate::ipc::on_exit(scheduler, core, exit);
        exits::record(scheduler.current_id(core), exit);
        scheduler.set_current_state(core, State::Exited);
    });
    switch(frame)
}

/// For use inside an interrupt (system call): puts the current task to
/// sleep and returns the frame of the task to continue with.
pub fn sleep_current(frame: *mut InterruptFrame, ticks: u64) -> *mut InterruptFrame {
    let until_tick = time::ticks() + ticks;
    with_scheduler(|scheduler, core| {
        scheduler.set_current_state(core, State::Sleeping { until_tick })
    });
    switch(frame)
}

fn with_scheduler<R>(action: impl FnOnce(&mut RunQueue<TaskContext>, CoreId) -> R) -> R {
    let mut guard = SCHEDULER.lock();
    // Inside the lock interrupts are off, so we can't move to another core.
    let core = arch::core_index();
    action(guard.as_mut().expect("scheduler not started"), core)
}

fn spawn_context(context: TaskContext) -> TaskId {
    with_scheduler(|scheduler, _| scheduler.spawn(context))
}

/// Where every spawned kernel task begins.
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
