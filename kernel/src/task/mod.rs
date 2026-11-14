//! Tasks: independent flows of execution, switched preemptively by the
//! timer (DECISIONS.md, 017). A task is either a kernel task or a user
//! program in its own address space (018). Which task runs next is decided
//! by the `scheduling` crate; this module does the switching.
//!
//! A task switch is nothing but answering an interrupt with a different
//! `InterruptFrame`: the interrupt stub then continues on that task's stack.

mod checks;
mod context;
mod exits;
mod program;

pub use checks::{check, check_user_programs};
pub use exits::{Exit, wait};
pub use program::spawn_program;

use crate::arch::{self, InterruptFrame, cpu};
use crate::memory::AddressSpace;
use crate::sync::SpinLock;
use crate::time;
use alloc::boxed::Box;
use context::TaskContext;
use scheduling::{RunQueue, State, TaskId};

static SCHEDULER: SpinLock<Option<RunQueue<TaskContext>>> = SpinLock::new(None);

/// Makes the code running now task 0 ("boot") and creates the idle task.
/// From now on, every timer tick may switch tasks. Needs the heap.
pub fn init() {
    let idle = TaskContext::new_kernel("idle", idle_entry, 0);
    *SCHEDULER.lock() = Some(RunQueue::new(TaskContext::running("boot"), idle));
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
    with_scheduler(|scheduler| scheduler.set_current_state(State::Sleeping { until_tick }));
    arch::yield_now();
}

/// Ends the current kernel task. Its stack is freed at a later switch.
fn exit() -> ! {
    with_scheduler(|scheduler| {
        exits::record(scheduler.current_id(), Exit::Code(0));
        scheduler.set_current_state(State::Exited);
    });
    loop {
        arch::yield_now(); // never comes back: exited tasks aren't scheduled
    }
}

pub fn task_count() -> usize {
    with_scheduler(|scheduler| scheduler.task_count())
}

pub fn current_name() -> &'static str {
    with_scheduler(|scheduler| scheduler.current().name)
}

/// Runs `action` with the current program's address space (`None` for
/// kernel tasks). Interrupts stay off meanwhile.
pub fn with_current_address_space<R>(action: impl FnOnce(Option<&AddressSpace>) -> R) -> R {
    with_scheduler(|scheduler| action(scheduler.current().address_space.as_ref()))
}

/// For use inside an interrupt (system call, fault): ends the current task
/// and returns the frame of the task to continue with.
pub fn end_current(frame: *mut InterruptFrame, exit: Exit) -> *mut InterruptFrame {
    with_scheduler(|scheduler| {
        exits::record(scheduler.current_id(), exit);
        scheduler.set_current_state(State::Exited);
    });
    switch(frame)
}

/// For use inside an interrupt (system call): puts the current task to
/// sleep and returns the frame of the task to continue with.
pub fn sleep_current(frame: *mut InterruptFrame, ticks: u64) -> *mut InterruptFrame {
    let until_tick = time::ticks() + ticks;
    with_scheduler(|scheduler| scheduler.set_current_state(State::Sleeping { until_tick }));
    switch(frame)
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
    // never among them, so we never free the stack – or the address space –
    // we're running on.
    while let Some(finished) = scheduler.take_exited() {
        drop(finished);
    }
    scheduler.switch(time::ticks());

    let next = scheduler.current();
    // SAFETY: interrupts are off; the stack belongs to `next`, and every
    // address space maps the kernel identically, so this code stays mapped.
    unsafe {
        if next.kernel_stack_top != 0 {
            arch::set_kernel_stack(next.kernel_stack_top);
        }
        let root = next.page_table_root();
        if root != cpu::page_table_root() {
            cpu::set_page_table_root(root);
        }
    }
    next.saved_frame
}

fn with_scheduler<R>(action: impl FnOnce(&mut RunQueue<TaskContext>) -> R) -> R {
    action(SCHEDULER.lock().as_mut().expect("scheduler not started"))
}

fn spawn_context(context: TaskContext) -> TaskId {
    with_scheduler(|scheduler| scheduler.spawn(context))
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
