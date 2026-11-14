//! The raw system call: registers in, registers out (layout: `abi` crate).

use core::arch::asm;

/// Every register a system call reads or writes.
#[derive(Clone, Copy, Default)]
pub struct Registers {
    pub rax: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub r10: u64,
    pub r8: u64,
    pub r9: u64,
    pub r12: u64,
}

pub fn system_call(input: Registers) -> Registers {
    let mut registers = input;
    // SAFETY: `int 0x80` enters the kernel, which checks every argument and
    // only changes the registers listed here. It may read or remap memory
    // we hand it (lent and moved memory objects), hence no `nomem`.
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") registers.rax,
            inlateout("rdi") registers.rdi,
            inlateout("rsi") registers.rsi,
            inlateout("rdx") registers.rdx,
            inlateout("rcx") registers.rcx,
            inlateout("r10") registers.r10,
            inlateout("r8") registers.r8,
            inlateout("r9") registers.r9,
            inlateout("r12") registers.r12,
            options(nostack, preserves_flags),
        );
    }
    registers
}
