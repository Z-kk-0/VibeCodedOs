//! Our Interrupt Descriptor Table: for each of the 256 vectors, where the
//! CPU jumps when it fires. The gate format lives in the `x86` crate.
//!
//! Every vector points at its own stub in `interrupts/stubs.rs`; all stubs
//! end up in one Rust function (DECISIONS.md, 008).

use super::gdt::{DOUBLE_FAULT_STACK_INDEX, KERNEL_CODE};
use super::interrupts::stub_address;
use core::arch::asm;
use core::mem::size_of;
use x86::exceptions::DOUBLE_FAULT;
use x86::idt::Gate;

const VECTOR_COUNT: usize = 256;

static mut IDT: [Gate; VECTOR_COUNT] = [Gate::MISSING; VECTOR_COUNT];

/// # Safety
/// Call once, after the GDT (the gates refer to its selectors and IST).
pub unsafe fn init() {
    let mut gates = [Gate::MISSING; VECTOR_COUNT];
    for (vector, gate) in (0u64..).zip(gates.iter_mut()) {
        let stack = if vector == DOUBLE_FAULT {
            DOUBLE_FAULT_STACK_INDEX
        } else {
            0
        };
        *gate = Gate::new(stub_address(vector), KERNEL_CODE, stack);
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
