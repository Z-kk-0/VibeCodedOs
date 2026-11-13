//! The very first kernel instructions.
//!
//! The loader jumps to `_start` with `rdi` = pointer to `BootInfo`
//! (System V calling convention). At that moment we still run on the
//! firmware's stack, and the firmware's interrupt handlers are still
//! installed – both live in memory we now consider free.

use boot_info::BootInfo;

const STACK_SIZE: usize = 64 * 1024;

#[repr(C, align(16))]
struct Stack([u8; STACK_SIZE]);

/// Our own stack. Lives in `.bss` (zeroed by the loader) and is only ever
/// touched by the CPU through `rsp` – Rust code never references it.
static mut STACK: Stack = Stack([0; STACK_SIZE]);

core::arch::global_asm!(
    ".global _start",
    "_start:",
    // No interrupts until we have our own handlers.
    "cli",
    // Stacks grow downwards, so start at the top end.
    "lea rsp, [rip + {stack} + {stack_size}]",
    // `rdi` still holds the BootInfo pointer: first argument of `kernel_entry`.
    "call {entry}",
    // `kernel_entry` never returns; trap loudly if it somehow does.
    "ud2",
    stack = sym STACK,
    stack_size = const STACK_SIZE,
    entry = sym kernel_entry,
);

extern "sysv64" fn kernel_entry(boot_info: &'static BootInfo) -> ! {
    crate::kernel_main(boot_info)
}
