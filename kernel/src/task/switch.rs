//! The task switch itself (DECISIONS.md, 017 and 019): save the interrupted
//! task's frame, pick the next task for this core, prepare the core for it.

use super::context::TaskContext;
use super::{SCHEDULER, Tasks};
use crate::arch::{self, InterruptFrame, cpu};
use crate::memory::AddressSpace;
use crate::sync::SpinLockGuard;
use crate::time;
use core::sync::atomic::Ordering;
use scheduling::CoreId;

/// Called by the yield interrupt and every voluntary switch (sleep, exit,
/// IPC) with the interrupted task's frame. Returns the frame of the task
/// to continue with.
pub fn switch(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    switch_with(SCHEDULER.lock(), frame)
}

/// Called by the timer: a chance to switch, not a duty. If another core
/// holds the run queue right now, the current task simply keeps running
/// one more tick – with many cores, waiting here would make every tick a
/// queue in front of one lock (DECISIONS.md, 022).
pub fn switch_on_tick(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    match SCHEDULER.try_lock() {
        Some(guard) => switch_with(guard, frame),
        None => frame,
    }
}

fn switch_with(
    mut guard: SpinLockGuard<'_, Option<Tasks>>,
    frame: *mut InterruptFrame,
) -> *mut InterruptFrame {
    let core = arch::core_index();
    let Some(scheduler) = guard.as_mut() else {
        return frame; // scheduler not started yet
    };
    if core >= scheduler.core_count() {
        return frame; // this core hasn't joined yet
    }
    // Tasks that waited too long in an IPC queue give up now.
    crate::ipc::expire_timeouts(scheduler, time::ticks());
    // Free finished tasks *before* picking the next one: never one a core
    // still runs or is still leaving, so we never free a stack – or an
    // address space – that is in use.
    while let Some(finished) = scheduler.take_exited(TaskContext::is_off_every_core) {
        drop(finished);
    }

    let previous = scheduler.current(core);
    previous.saved_frame = frame;
    let previous_space = previous
        .process
        .as_ref()
        .map(|process| &process.space as *const AddressSpace);
    let leaving: *const _ = &*previous.leaving;
    if !scheduler.switch(core, time::ticks(), TaskContext::is_off_every_core) {
        return frame;
    }
    // SAFETY: `leaving` is boxed and its task can't be freed while it's set.
    // Until the stub clears it, no core picks or frees the previous task.
    let leaving = unsafe { &*leaving };
    leaving.store(true, Ordering::Relaxed); // published by the lock's release
    arch::release_after_switch(leaving);

    let next = scheduler.current(core);
    // SAFETY: the run queue doesn't change while we hold the lock, so the
    // previous task's address space is still where it was.
    let previous_space = previous_space.map(|space| unsafe { &*space });
    // SAFETY: interrupts are off; the stack belongs to `next`, and every
    // address space maps the kernel identically, so this code stays mapped.
    unsafe { activate(next, previous_space, core) };
    next.saved_frame
}

/// Prepares this core to run `next`: its kernel stack for interrupts from
/// user mode, its page tables.
///
/// # Safety
/// Interrupts off, scheduler lock held, `next` about to run on `core`.
unsafe fn activate(next: &TaskContext, previous_space: Option<&AddressSpace>, core: CoreId) {
    // SAFETY: forwarded to the caller.
    unsafe {
        if next.kernel_stack_top != 0 {
            arch::set_kernel_stack(next.kernel_stack_top);
        }
        let root = next.page_table_root();
        if root != cpu::page_table_root() {
            // Active before loading (changes now reach us), inactive after
            // leaving (loading CR3 flushed everything old).
            if let Some(process) = &next.process {
                process.space.mark_active(core);
            }
            cpu::set_page_table_root(root);
            if let Some(space) = previous_space {
                space.mark_inactive(core);
            }
        }
    }
}
