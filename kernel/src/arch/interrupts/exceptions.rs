//! CPU exceptions (vectors 0–31): the CPU telling us an instruction went
//! wrong. A breakpoint is harmless. A fault in a user program ends that
//! program – the kernel carries on. A fault in the kernel itself is a bug:
//! if it provably touched no shared state, only its task ends (DECISIONS.md,
//! 024, `task/faults.rs`); otherwise report everything and stop the
//! machine. Names and error-code meanings come from the `x86` crate.

use super::InterruptFrame;
use crate::arch::{self, cpu};
use crate::task::{self, Exit, Verdict};
use x86::exceptions::{
    BREAKPOINT, DOUBLE_FAULT, MACHINE_CHECK, NON_MASKABLE_INTERRUPT, PAGE_FAULT,
    describe_page_fault, name, page_was_unmapped,
};

/// Returns the frame to resume (another task's, if a program was ended).
pub fn handle(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    if frame.vector == NON_MASKABLE_INTERRUPT {
        if crate::arch::smp::stopping() {
            // Another core hit a kernel panic or exception and stops us.
            crate::arch::stop_this_core();
        }
        // Not ours (hardware, a watchdog): nothing the interrupted code did.
        emergency_println!("Non-maskable interrupt at {:#x} - ignored.", frame.rip);
        return frame;
    }
    if frame.vector == BREAKPOINT {
        // `int3` is one byte and `rip` already points past it: just return.
        emergency_println!("Breakpoint at {:#x} - handled, continuing.", frame.rip);
        return frame;
    }
    if frame.came_from_user_mode() {
        if frame.vector == PAGE_FAULT
            && page_was_unmapped(frame.error_code)
            && task::grow_stack(cpu::page_fault_address())
        {
            return frame; // the stack grew: run the instruction again
        }
        let name = task::current_name();
        let (vector, error) = (frame.vector, frame.error_code);
        if vector == PAGE_FAULT {
            let address = cpu::page_fault_address();
            let cause = describe_page_fault(error);
            println!(
                "[{name}] stopped: {} - {cause} at {address:#x} (rip {:#x})",
                self::name(vector),
                frame.rip
            );
        } else {
            println!(
                "[{name}] stopped: {} (#{vector}) at rip {:#x}",
                self::name(vector),
                frame.rip
            );
        }
        return task::end_current(frame, Exit::Killed(vector));
    }
    // A kernel bug. Can it be contained to the task it happened in
    // (DECISIONS.md, 024)? Never after a double fault (the stack is broken)
    // or a machine check (the hardware is).
    let verdict = match frame.vector {
        DOUBLE_FAULT | MACHINE_CHECK => Verdict::Stop("this exception is never survivable"),
        _ => task::judge_failure(frame.interrupts_were_on()),
    };
    let reason = match verdict {
        Verdict::Contain => {
            let task = task::current_name();
            let (exception, rip) = (name(frame.vector), frame.rip);
            if frame.vector == PAGE_FAULT {
                let address = cpu::page_fault_address();
                println!(
                    "[{task}] kernel bug contained: {exception} at {address:#x} (rip {rip:#x})"
                );
            } else {
                println!("[{task}] kernel bug contained: {exception} at rip {rip:#x}");
            }
            println!(
                "  No lock was held: only this task ends. What it had on its stack is leaked."
            );
            return task::end_current(frame, Exit::KernelFailed(frame.vector));
        }
        Verdict::Stop(reason) => reason,
    };
    // Report first: if stopping the others goes wrong too, the reason is out.
    report(frame);
    emergency_println!("  Machine stopped: {reason}.");
    arch::stop_other_cores();
    arch::stop_this_core();
}

fn report(frame: &InterruptFrame) {
    emergency_println!();
    emergency_println!(
        "EXCEPTION: {} (#{}), error code {:#x}",
        name(frame.vector),
        frame.vector,
        frame.error_code
    );
    if frame.vector == PAGE_FAULT {
        emergency_println!(
            "  {} at address {:#x}",
            describe_page_fault(frame.error_code),
            cpu::page_fault_address()
        );
    }
    emergency_println!(
        "  rip {:#018x}  cs {:#x}  rflags {:#x}",
        frame.rip,
        frame.cs,
        frame.rflags
    );
    emergency_println!("  rsp {:#018x}  ss {:#x}", frame.rsp, frame.ss);
    let registers = [
        ("rax", frame.rax),
        ("rbx", frame.rbx),
        ("rcx", frame.rcx),
        ("rdx", frame.rdx),
        ("rsi", frame.rsi),
        ("rdi", frame.rdi),
        ("rbp", frame.rbp),
        ("r8 ", frame.r8),
        ("r9 ", frame.r9),
        ("r10", frame.r10),
        ("r11", frame.r11),
        ("r12", frame.r12),
        ("r13", frame.r13),
        ("r14", frame.r14),
        ("r15", frame.r15),
    ];
    for row in registers.chunks(4) {
        emergency_print!(" ");
        for (name, value) in row {
            emergency_print!(" {name} {value:#018x}");
        }
        emergency_println!();
    }
}
