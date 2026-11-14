//! Our Interrupt Descriptor Table: for each of the 256 vectors, where the
//! CPU jumps when it fires. The gate format lives in the `x86` crate.
//!
//! Every vector points at its own stub in `interrupts/stubs.rs`; all stubs
//! end up in one Rust function (DECISIONS.md, 008). All cores share this
//! one table – each loads it with `load`.

use super::gdt::{DOUBLE_FAULT_STACK_INDEX, KERNEL_CODE};
use super::interrupts::stub_address;
use core::arch::asm;
use core::mem::size_of;
use x86::exceptions::DOUBLE_FAULT;
use x86::idt::Gate;

const VECTOR_COUNT: usize = 256;

static mut IDT: [Gate; VECTOR_COUNT] = [Gate::MISSING; VECTOR_COUNT];

/// Fills the table and loads it on this core.
///
/// # Safety
/// Call once, on the boot core, after its GDT (the gates refer to its
/// selectors and IST) and before any other core starts.
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
    // The only door user programs may knock on themselves: system calls.
    let syscall = usize::from(abi::SYSCALL_VECTOR);
    gates[syscall] = gates[syscall].callable_from_user_mode();

    // SAFETY: no other core runs yet, so nobody reads the table meanwhile;
    // it's a static and never moves.
    unsafe {
        (&raw mut IDT).write(gates);
        load();
    }
}

/// Makes this core use the table.
///
/// # Safety
/// `init` must have run, and this core's GDT must be loaded.
pub unsafe fn load() {
    #[repr(C, packed)]
    struct Pointer {
        limit: u16,
        base: u64,
    }
    let pointer = Pointer {
        limit: (size_of::<[Gate; VECTOR_COUNT]>() - 1) as u16,
        base: (&raw const IDT) as u64,
    };
    // SAFETY: the table is filled and lives forever.
    unsafe { asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags)) };
}
