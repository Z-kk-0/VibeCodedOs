//! The saved CPU state of an interrupted piece of code.

use crate::arch::gdt::{KERNEL_CODE, KERNEL_DATA, USER_CODE, USER_DATA};

/// Everything the CPU and our stub saved, lowest address first.
/// Must match the push order in `stubs.rs`.
#[repr(C)]
#[derive(Default)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    // Pushed by our stub.
    pub vector: u64,
    pub error_code: u64,
    // Pushed by the CPU.
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// RFLAGS.IF, Intel SDM Vol. 1, §3.4.3.
const INTERRUPT_FLAG: u64 = 1 << 9;
/// RFLAGS with interrupts enabled. Bit 1 is reserved and always set.
const INTERRUPTS_ON: u64 = INTERRUPT_FLAG | 1 << 1;

impl InterruptFrame {
    /// A frame that, when "returned to" with `iretq`, starts
    /// `entry(argument)` on a fresh stack with interrupts enabled – as if the
    /// task had been interrupted right before its first instruction.
    pub fn for_new_kernel_task(
        entry: extern "sysv64" fn(u64) -> !,
        argument: u64,
        stack_pointer: u64,
    ) -> Self {
        Self {
            rdi: argument, // first argument (System V calling convention)
            rip: entry as usize as u64,
            cs: u64::from(KERNEL_CODE),
            rflags: INTERRUPTS_ON,
            rsp: stack_pointer,
            ss: u64::from(KERNEL_DATA),
            ..Self::default()
        }
    }

    /// Same for a user program: `iretq` with user selectors drops the CPU to
    /// ring 3, at `entry`, on the program's own stack.
    pub fn for_new_user_task(entry: u64, stack_pointer: u64) -> Self {
        Self {
            rip: entry,
            cs: u64::from(USER_CODE),
            rflags: INTERRUPTS_ON,
            rsp: stack_pointer,
            ss: u64::from(USER_DATA),
            ..Self::default()
        }
    }

    /// Whether the interrupted code was a user program (privilege level 3).
    pub fn came_from_user_mode(&self) -> bool {
        self.cs & 3 == 3
    }

    /// Whether interrupts were on in the interrupted code (RFLAGS.IF).
    pub fn interrupts_were_on(&self) -> bool {
        self.rflags & INTERRUPT_FLAG != 0
    }
}
