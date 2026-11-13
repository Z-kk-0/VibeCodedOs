//! The single Rust entry point for every interrupt and exception.

mod exceptions;
mod stubs;

use x86::exceptions::FIRST_NON_EXCEPTION;

const STUB_SIZE: u64 = 16;

unsafe extern "C" {
    /// Start of the stub block in `stubs.rs`. Only its address matters.
    static interrupt_stubs: u8;
}

pub fn stub_address(vector: u64) -> u64 {
    (&raw const interrupt_stubs) as u64 + vector * STUB_SIZE
}

/// Everything the CPU and our stub saved, lowest address first.
/// Must match the push order in `stubs.rs`.
#[repr(C)]
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

extern "sysv64" fn interrupt_dispatch(frame: &mut InterruptFrame) {
    if frame.vector < FIRST_NON_EXCEPTION {
        exceptions::handle(frame);
    } else {
        println!(
            "Unexpected interrupt {} - no device is set up for it yet.",
            frame.vector
        );
    }
}
