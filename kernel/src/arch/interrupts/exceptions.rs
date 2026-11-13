//! CPU exceptions (vectors 0–31): the CPU telling us an instruction went wrong.
//! For now, only a breakpoint is survivable; everything else is reported and
//! stops the machine. Names: Intel SDM Vol. 3A, Table 6-1.

use super::InterruptFrame;
use crate::arch::cpu;

const BREAKPOINT: u64 = 3;
pub const DOUBLE_FAULT: usize = 8;
const PAGE_FAULT: u64 = 14;

const NAMES: [&str; 32] = [
    "Divide Error",
    "Debug",
    "Non-Maskable Interrupt",
    "Breakpoint",
    "Overflow",
    "Bound Range Exceeded",
    "Invalid Opcode",
    "Device Not Available",
    "Double Fault",
    "Coprocessor Segment Overrun",
    "Invalid TSS",
    "Segment Not Present",
    "Stack-Segment Fault",
    "General Protection Fault",
    "Page Fault",
    "Reserved",
    "x87 Floating-Point Error",
    "Alignment Check",
    "Machine Check",
    "SIMD Floating-Point Exception",
    "Virtualization Exception",
    "Control Protection Exception",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Hypervisor Injection Exception",
    "VMM Communication Exception",
    "Security Exception",
    "Reserved",
];

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
        NAMES[frame.vector as usize], frame.vector, frame.error_code
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

/// Page fault error code bits, Intel SDM Vol. 3A, Figure 6-11.
fn describe_page_fault(error_code: u64) -> &'static str {
    const PAGE_PRESENT: u64 = 1 << 0;
    const WRITE: u64 = 1 << 1;
    const INSTRUCTION_FETCH: u64 = 1 << 4;

    match (
        error_code & PAGE_PRESENT != 0,
        error_code & WRITE != 0,
        error_code & INSTRUCTION_FETCH != 0,
    ) {
        (_, _, true) => "Instruction fetch",
        (false, false, _) => "Read from an unmapped page",
        (false, true, _) => "Write to an unmapped page",
        (true, false, _) => "Read violating page permissions",
        (true, true, _) => "Write violating page permissions",
    }
}
