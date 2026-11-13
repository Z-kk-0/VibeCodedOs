//! IDT entries ("gates"). Intel SDM Vol. 3A, §6.14.1.

/// Present, privilege level 0, type "64-bit interrupt gate" (the CPU clears
/// the interrupt flag on entry, so handlers aren't interrupted themselves).
const PRESENT_INTERRUPT_GATE: u8 = 0x8E;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
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
    /// Not present: the CPU raises a General Protection Fault instead.
    pub const MISSING: Gate = Gate {
        handler_low: 0,
        code_selector: 0,
        stack_index: 0,
        attributes: 0,
        handler_middle: 0,
        handler_high: 0,
        reserved: 0,
    };

    pub fn new(handler: u64, code_selector: u16, stack_index: u8) -> Self {
        Gate {
            handler_low: handler as u16,
            code_selector,
            stack_index,
            attributes: PRESENT_INTERRUPT_GATE,
            handler_middle: (handler >> 16) as u16,
            handler_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_is_sixteen_bytes() {
        assert_eq!(size_of::<Gate>(), 16);
    }

    #[test]
    fn handler_address_is_split_into_three_parts() {
        let gate = Gate::new(0x1122_3344_5566_7788, 0x08, 1);
        assert_eq!(gate.handler_low, 0x7788);
        assert_eq!(gate.handler_middle, 0x5566);
        assert_eq!(gate.handler_high, 0x1122_3344);
        assert_eq!(gate.code_selector, 0x08);
        assert_eq!(gate.stack_index, 1);
    }
}
