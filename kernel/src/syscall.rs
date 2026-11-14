//! System calls: the only way user programs reach the kernel (`int 0x80`,
//! calling convention and numbers in the `abi` crate; DECISIONS.md, 018 and
//! 020).
//!
//! Arguments come straight from the program's registers, so every one of
//! them is checked before use – handles against the program's own table.
//! Most of the work is IPC (`crate::ipc`).

use crate::arch::{InterruptFrame, cpu};
use crate::ipc::{self, Next};
use crate::task::{self, Exit};

/// Handles the call described by the frame's registers. Returns the frame
/// to resume – a different task's if the call ended or paused this one.
pub fn handle(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let next = match frame.rax {
        abi::EXIT => return task::end_current(frame, Exit::Code(frame.rdi)),
        abi::SLEEP => return task::sleep_current(frame, frame.rdi),
        abi::CALL => task::with_tasks(|tasks, core| ipc::call(tasks, core, frame)),
        abi::RECEIVE => task::with_tasks(|tasks, core| ipc::receive(tasks, core, frame)),
        abi::REPLY => task::with_tasks(|tasks, core| ipc::reply(tasks, core, frame)),
        abi::MEMORY_NEW => {
            task::with_tasks(|tasks, core| ipc::memory_new(tasks, core, frame));
            Next::Continue
        }
        abi::DROP => {
            task::with_tasks(|tasks, core| ipc::drop_handle(tasks, core, frame));
            Next::Continue
        }
        abi::ENDPOINT_NEW => {
            task::with_tasks(|tasks, core| ipc::endpoint_new(tasks, core, frame));
            Next::Continue
        }
        abi::MINT => {
            task::with_tasks(|tasks, core| ipc::mint(tasks, core, frame));
            Next::Continue
        }
        abi::REVOKE => {
            task::with_tasks(|tasks, core| ipc::revoke(tasks, core, frame));
            Next::Continue
        }
        abi::MEMORY_INFO => {
            task::with_tasks(|tasks, core| ipc::memory_info(tasks, core, frame));
            Next::Continue
        }
        _ => {
            frame.rax = abi::ERROR_UNKNOWN_CALL;
            Next::Continue
        }
    };
    match next {
        Next::Continue => frame,
        // The result will be written into `frame` by whoever wakes us.
        Next::Block => task::switch(frame),
        Next::Spawn(request) => {
            // Loading is slow: do it with no lock held and interrupts on, so
            // every core – this one too – keeps scheduling meanwhile. This
            // task may even move to another core; its frame stays where it
            // is, on its own kernel stack.
            // SAFETY: every interrupt has a handler, and we hold no lock.
            unsafe { cpu::enable_interrupts() };
            let child = ipc::load_child(&request);
            cpu::disable_interrupts();
            task::with_tasks(|tasks, _| ipc::finish_spawn(tasks, request, child, frame));
            frame
        }
    }
}
