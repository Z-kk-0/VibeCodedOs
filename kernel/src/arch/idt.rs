//! Interrupt Descriptor Table: for each of the 256 vectors, where the CPU
//! jumps when it fires. Intel SDM Vol. 3A, §6.14.1 (64-bit IDT gates).
//!
//! Every vector points at its own stub in `interrupts/stubs.rs`; all stubs
//! end up in one Rust function (DECISIONS.md, 008).

use super::gdt::{DOUBLE_FAULT_STACK_INDEX, KERNEL_CODE};
use super::interrupts::{DOUBLE_FAULT, stub_address};
use core::arch::asm;
use core::mem::size_of;

const VECTOR_COUNT: usize = 256;

/// Present, privilege level 0, type "64-bit interrupt gate" (the CPU clears
/// the interrupt flag on entry, so handlers aren't interrupted themselves).
const PRESENT_INTERRUPT_GATE: u8 = 0x8E;

#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    handler_low: u16,
    code_selector: u16,
    /// Interrupt Stack Table index; 0 = keep the current stack.
    stack_index: u8,
    attributes: u8,
    handler_middle: u16,
    handler_high: u32,
    reserved: u32,
}

impl Gate {
    const MISSING: Gate = Gate {
        handler_low: 0,
        code_selector: 0,
        stack_index: 0,
        attributes: 0,
        handler_middle: 0,
        handler_high: 0,
        reserved: 0,
    };

    fn new(handler: u64, stack_index: u8) -> Self {
        Gate {
            handler_low: handler as u16,
            code_selector: KERNEL_CODE,
            stack_index,
            attributes: PRESENT_INTERRUPT_GATE,
            handler_middle: (handler >> 16) as u16,
            handler_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

static mut IDT: [Gate; VECTOR_COUNT] = [Gate::MISSING; VECTOR_COUNT];

/// # Safety
/// Call once, after the GDT (the gates refer to its selectors and IST).
pub unsafe fn init() {
    let mut gates = [Gate::MISSING; VECTOR_COUNT];
    for (vector, gate) in gates.iter_mut().enumerate() {
        let stack = if vector == DOUBLE_FAULT {
            DOUBLE_FAULT_STACK_INDEX
        } else {
            0
        };
        *gate = Gate::new(stub_address(vector), stack);
    }

    #[repr(C, packed)]
    struct Pointer {
        limit: u16,
        base: u64,
    }

    // SAFETY: single-threaded early boot; the IDT is a static and never moves.
    unsafe {
        let idt = &raw mut IDT;
        idt.write(gates);
        let pointer = Pointer {
            limit: (size_of::<[Gate; VECTOR_COUNT]>() - 1) as u16,
            base: idt as u64,
        };
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }
}
