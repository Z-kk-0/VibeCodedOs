//! CPU exception numbers and how to read their error codes.
//! Intel SDM Vol. 3A, Table 6-1 and Figure 6-11.

pub const NON_MASKABLE_INTERRUPT: u64 = 2;
pub const BREAKPOINT: u64 = 3;
pub const DOUBLE_FAULT: u64 = 8;
/// The CPU found a hardware error (SDM Vol. 3B, chapter 16).
pub const MACHINE_CHECK: u64 = 18;
pub const PAGE_FAULT: u64 = 14;
/// Vectors 0–31 are reserved for CPU exceptions; 32–255 are free.
pub const FIRST_NON_EXCEPTION: u64 = 32;

const NAMES: [&str; FIRST_NON_EXCEPTION as usize] = [
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

pub fn name(vector: u64) -> &'static str {
    NAMES
        .get(vector as usize)
        .copied()
        .unwrap_or("Not an exception")
}

/// What the page fault error code says went wrong.
/// Whether a page fault hit an unmapped page (rather than a permission).
pub fn page_was_unmapped(error_code: u64) -> bool {
    error_code & 1 == 0
}

pub fn describe_page_fault(error_code: u64) -> &'static str {
    const PAGE_PRESENT: u64 = 1 << 0;
    const WRITE: u64 = 1 << 1;
    const INSTRUCTION_FETCH: u64 = 1 << 4;

    let present = error_code & PAGE_PRESENT != 0;
    let write = error_code & WRITE != 0;
    let fetch = error_code & INSTRUCTION_FETCH != 0;
    match (present, write, fetch) {
        (false, _, true) => "Instruction fetch from an unmapped page",
        (true, _, true) => "Instruction fetch from a non-executable page",
        (false, false, _) => "Read from an unmapped page",
        (false, true, _) => "Write to an unmapped page",
        (true, false, _) => "Read violating page permissions",
        (true, true, _) => "Write violating page permissions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_their_vectors() {
        assert_eq!(name(BREAKPOINT), "Breakpoint");
        assert_eq!(name(DOUBLE_FAULT), "Double Fault");
        assert_eq!(name(PAGE_FAULT), "Page Fault");
        assert_eq!(name(FIRST_NON_EXCEPTION), "Not an exception");
    }

    #[test]
    fn page_fault_error_codes_are_decoded() {
        assert_eq!(describe_page_fault(0b00000), "Read from an unmapped page");
        assert_eq!(describe_page_fault(0b00010), "Write to an unmapped page");
        assert_eq!(
            describe_page_fault(0b00011),
            "Write violating page permissions"
        );
        assert_eq!(
            describe_page_fault(0b10001),
            "Instruction fetch from a non-executable page"
        );
        assert_eq!(
            describe_page_fault(0b10000),
            "Instruction fetch from an unmapped page"
        );
    }
}
