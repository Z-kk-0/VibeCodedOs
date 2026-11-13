//! CPU exceptions (vectors 0–31): the CPU telling us an instruction went
//! wrong. For now, only a breakpoint is survivable; everything else is
//! reported and stops the machine. Names and error-code meanings come from
//! the `x86` crate.

use super::InterruptFrame;
use crate::arch::cpu;
use x86::exceptions::{BREAKPOINT, PAGE_FAULT, describe_page_fault, name};

pub fn handle(frame: &mut InterruptFrame) {
    if frame.vector == BREAKPOINT {
        // `int3` is one byte and `rip` already points past it: just return.
        println!("Breakpoint at {:#x} - handled, continuing.", frame.rip);
        return;
    }
    report(frame);
    cpu::halt_forever();
}

fn report(frame: &InterruptFrame) {
    println!();
    println!(
        "EXCEPTION: {} (#{}), error code {:#x}",
        name(frame.vector),
        frame.vector,
        frame.error_code
    );
    if frame.vector == PAGE_FAULT {
        println!(
            "  {} at address {:#x}",
            describe_page_fault(frame.error_code),
            cpu::page_fault_address()
        );
    }
    println!(
        "  rip {:#018x}  cs {:#x}  rflags {:#x}",
        frame.rip, frame.cs, frame.rflags
    );
    println!("  rsp {:#018x}  ss {:#x}", frame.rsp, frame.ss);
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
        print!(" ");
        for (name, value) in row {
            print!(" {name} {value:#018x}");
        }
        println!();
    }
}
