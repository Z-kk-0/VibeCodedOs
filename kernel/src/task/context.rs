//! What the kernel keeps per task: its stack and where its CPU state is
//! saved while it isn't running.

use crate::arch::InterruptFrame;
use alloc::boxed::Box;
use alloc::vec;
use core::mem::size_of;
use core::ptr::null_mut;

pub const STACK_SIZE: usize = 16 * 1024;

pub struct TaskContext {
    /// The task's `InterruptFrame`, on its own stack. Null for the boot task
    /// until it is switched away from for the first time.
    pub saved_frame: *mut InterruptFrame,
    /// Freed when the task is. `None` for the boot task, which keeps
    /// running on the kernel's boot stack.
    _stack: Option<Box<[u8]>>,
}

// SAFETY: `saved_frame` points into the task's own stack, which belongs to
// this context and goes wherever it goes.
unsafe impl Send for TaskContext {}

impl TaskContext {
    /// The code that is running right now.
    pub fn running() -> Self {
        Self {
            saved_frame: null_mut(),
            _stack: None,
        }
    }

    /// A task that will start with `entry(argument)` on a fresh stack.
    pub fn new(entry: extern "sysv64" fn(u64) -> !, argument: u64) -> Self {
        let mut stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let top = (stack.as_mut_ptr() as u64 + STACK_SIZE as u64) & !0xF;
        // At a function's first instruction the ABI expects rsp + 8 to be
        // 16-byte aligned, as if a return address had just been pushed.
        // `entry` never returns, so that slot just holds 0.
        let stack_pointer = top - 8;
        let frame = (stack_pointer - size_of::<InterruptFrame>() as u64) as *mut InterruptFrame;
        // SAFETY: both writes land inside `stack`, which we own; the frame
        // sits right below the stack pointer the task will start with.
        unsafe {
            (stack_pointer as *mut u64).write(0);
            frame.write(InterruptFrame::for_new_kernel_task(
                entry,
                argument,
                stack_pointer,
            ));
        }
        Self {
            saved_frame: frame,
            _stack: Some(stack),
        }
    }
}
