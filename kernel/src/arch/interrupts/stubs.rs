//! Assembly entry points for all 256 interrupt vectors.
//!
//! The CPU pushes `ss, rsp, rflags, cs, rip` – and for some exceptions an
//! error code. Each stub evens that out (pushes a fake 0 error code if the
//! CPU didn't), pushes its vector number and jumps to `interrupt_common`.
//! That one saves all registers, so the Rust side gets one uniform
//! `InterruptFrame` – and may answer with a different frame to resume,
//! which is how tasks are switched (DECISIONS.md, 017).
//!
//! Every stub sits in its own 16-byte slot, so stub N lives at
//! `interrupt_stubs + N * 16` – no address table needed.
//! Written in AT&T syntax because it makes `$vector` unambiguously a number.

core::arch::global_asm!(
    ".section .text",
    ".balign 16",
    ".global interrupt_stubs",
    "interrupt_stubs:",
    ".set stub_vector, 0",
    ".rept 256",
    "    .balign 16",
    // Vectors where the CPU pushes an error code itself (Intel SDM Vol. 3A, Table 6-1).
    "    .if (stub_vector == 8) || (stub_vector == 10) || (stub_vector == 11) || (stub_vector == 12) || (stub_vector == 13) || (stub_vector == 14) || (stub_vector == 17) || (stub_vector == 21) || (stub_vector == 29) || (stub_vector == 30)",
    "    .else",
    "        pushq $0",
    "    .endif",
    "    pushq $stub_vector",
    "    jmp interrupt_common",
    "    .set stub_vector, stub_vector + 1",
    ".endr",
    "",
    "interrupt_common:",
    // Save every general-purpose register. Order must match `InterruptFrame`
    // (reversed: the last push ends up at the lowest address = first field).
    "    pushq %rax",
    "    pushq %rcx",
    "    pushq %rdx",
    "    pushq %rbx",
    "    pushq %rbp",
    "    pushq %rsi",
    "    pushq %rdi",
    "    pushq %r8",
    "    pushq %r9",
    "    pushq %r10",
    "    pushq %r11",
    "    pushq %r12",
    "    pushq %r13",
    "    pushq %r14",
    "    pushq %r15",
    // First argument = pointer to the frame we just built.
    // 22 × 8 bytes were pushed onto a 16-byte aligned stack, so the
    // stack is aligned again for the call, as the ABI demands.
    "    movq %rsp, %rdi",
    // The ABI requires the direction flag to be clear on function entry.
    "    cld",
    "    call {dispatch}",
    // The dispatcher returns the frame to resume – usually the same one,
    // after a task switch another task's frame on another stack.
    // Switching the stack pointer to it *is* the task switch.
    "    movq %rax, %rsp",
    "    popq %r15",
    "    popq %r14",
    "    popq %r13",
    "    popq %r12",
    "    popq %r11",
    "    popq %r10",
    "    popq %r9",
    "    popq %r8",
    "    popq %rdi",
    "    popq %rsi",
    "    popq %rbp",
    "    popq %rbx",
    "    popq %rdx",
    "    popq %rcx",
    "    popq %rax",
    // Drop vector number and error code.
    "    addq $16, %rsp",
    "    iretq",
    dispatch = sym super::interrupt_dispatch,
    options(att_syntax),
);
