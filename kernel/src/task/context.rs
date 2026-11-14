//! What the kernel keeps per task: its stacks, where its CPU state is saved
//! while it isn't running, and – for user programs – its address space.

use crate::arch::InterruptFrame;
use crate::memory::{AddressSpace, paging};
use alloc::boxed::Box;
use alloc::vec;
use core::mem::size_of;
use core::ptr::null_mut;

pub const KERNEL_STACK_SIZE: usize = 16 * 1024;

pub struct TaskContext {
    pub name: &'static str,
    /// The task's `InterruptFrame`, on its kernel stack. Null for the boot
    /// task until it is switched away from for the first time.
    pub saved_frame: *mut InterruptFrame,
    /// Where interrupts from user mode land (TSS ring-0 stack). 0 for the
    /// boot task, which never runs in user mode.
    pub kernel_stack_top: u64,
    /// `Some` for user programs; kernel tasks use the kernel's own.
    pub address_space: Option<AddressSpace>,
    /// Freed together with the task. `None` for the boot task, which keeps
    /// running on the kernel's boot stack.
    _kernel_stack: Option<Box<[u8]>>,
}

// SAFETY: `saved_frame` points into the task's own stack, which belongs to
// this context and goes wherever it goes.
unsafe impl Send for TaskContext {}

impl TaskContext {
    /// The code that is running right now.
    pub fn running(name: &'static str) -> Self {
        Self {
            name,
            saved_frame: null_mut(),
            kernel_stack_top: 0,
            address_space: None,
            _kernel_stack: None,
        }
    }

    /// A kernel task that will start with `entry(argument)`.
    pub fn new_kernel(
        name: &'static str,
        entry: extern "sysv64" fn(u64) -> !,
        argument: u64,
    ) -> Self {
        Self::with_first_frame(name, None, |stack_pointer| {
            InterruptFrame::for_new_kernel_task(entry, argument, stack_pointer)
        })
    }

    /// A user program that will start at `entry` with `user_stack_pointer`,
    /// in `address_space`.
    pub fn new_user(
        name: &'static str,
        entry: u64,
        user_stack_pointer: u64,
        address_space: AddressSpace,
    ) -> Self {
        Self::with_first_frame(name, Some(address_space), |_| {
            InterruptFrame::for_new_user_task(entry, user_stack_pointer)
        })
    }

    /// The page tables to switch to when this task runs.
    pub fn page_table_root(&self) -> u64 {
        self.address_space
            .as_ref()
            .map_or_else(paging::kernel_root, AddressSpace::root)
    }

    /// Allocates the kernel stack and puts the task's first frame on top.
    fn with_first_frame(
        name: &'static str,
        address_space: Option<AddressSpace>,
        first_frame: impl FnOnce(u64) -> InterruptFrame,
    ) -> Self {
        let mut stack = vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        let top = (stack.as_mut_ptr() as u64 + KERNEL_STACK_SIZE as u64) & !0xF;
        // At a function's first instruction the ABI expects rsp + 8 to be
        // 16-byte aligned, as if a return address had just been pushed.
        // Kernel task entries never return, so that slot just holds 0.
        let stack_pointer = top - 8;
        let frame = (stack_pointer - size_of::<InterruptFrame>() as u64) as *mut InterruptFrame;
        // SAFETY: both writes land inside `stack`, which we own; the frame
        // sits right below the stack pointer a kernel task starts with.
        unsafe {
            (stack_pointer as *mut u64).write(0);
            frame.write(first_frame(stack_pointer));
        }
        Self {
            name,
            saved_frame: frame,
            kernel_stack_top: top,
            address_space,
            _kernel_stack: Some(stack),
        }
    }
}
