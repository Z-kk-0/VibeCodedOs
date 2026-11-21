//! When the kernel itself fails – a CPU exception in kernel mode, or a
//! `panic!` – how much has to go down with it (DECISIONS.md, 024)?
//!
//! The danger of going on after a kernel bug is shared state: a lock the
//! failed code still holds (every other core would wait for it forever),
//! or a structure it was halfway through changing. Rust can't clean up
//! after it either: the kernel is built with `panic = abort`, there is no
//! unwinding. So the kernel only carries on when it can *prove* that no
//! shared state is affected:
//!
//! - **Interrupts were on** where it failed. Every `SpinLock` switches them
//!   off while held, and interrupt handlers run with them off – so this
//!   core held no lock and wasn't in the middle of handling an interrupt.
//! - The failed task isn't one of the kernel's own (boot, idle): without
//!   those, the kernel itself would be incomplete.
//!
//! Then only that task ends: a kernel task, or the user program whose
//! system call it was. Whatever it owned on its kernel stack is leaked –
//! a leak, but no corruption. Otherwise the machine stops, as before.

use super::SCHEDULER;

pub enum Verdict {
    /// End only the current task.
    Contain,
    /// Stop the machine, for this reason.
    Stop(&'static str),
}

/// Never panics (it runs inside the panic handler).
pub fn judge(interrupts_were_on: bool) -> Verdict {
    if !interrupts_were_on {
        return Verdict::Stop(
            "interrupts were off - a lock may be held, or an interrupt was being handled",
        );
    }
    // Interrupts were on, so this core holds no lock: taking one is safe.
    let mut scheduler = SCHEDULER.lock();
    let core = crate::arch::core_index();
    match scheduler.as_mut() {
        None => Verdict::Stop("too early - there are no tasks yet"),
        Some(tasks) => {
            if tasks.current(core).essential {
                Verdict::Stop("it is the kernel's own task (boot or idle)")
            } else {
                Verdict::Contain
            }
        }
    }
}
