//! The I/O APIC: the chip that turns device interrupt lines into
//! interrupt messages to a core's local APIC (Intel 82093AA datasheet;
//! every PC chipset has a compatible one).
//!
//! It has just two memory-mapped registers: write a register number to
//! `SELECT`, then read or write it through `WINDOW`. Behind them lies one
//! 64-bit "redirection entry" per input line: which vector to raise, on
//! which core, and how the line signals (edge or level, active high or low).

/// Offsets of the two registers from the chip's base address.
pub const SELECT: usize = 0x00;
pub const WINDOW: usize = 0x10;

/// Bits 16–23: the highest redirection entry's index (entries − 1).
pub const VERSION_REGISTER: u32 = 0x01;
/// Entry `n` is registers `FIRST_ENTRY + 2n` (low half) and `+ 2n + 1`.
pub const FIRST_ENTRY_REGISTER: u32 = 0x10;

const ACTIVE_LOW: u64 = 1 << 13;
const LEVEL_TRIGGERED: u64 = 1 << 15;
pub const MASKED: u64 = 1 << 16;
const DESTINATION_SHIFT: u32 = 56;

/// How a line tells that its device wants attention.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signal {
    pub active_low: bool,
    pub level_triggered: bool,
}

impl Signal {
    /// What ISA devices (keyboard, PIT, serial port) do.
    pub const ISA: Self = Self {
        active_low: false,
        level_triggered: false,
    };

    /// From an ACPI interrupt source override's flags (MPS INTI flags,
    /// ACPI spec 6.5, Table 5.50): "conforms to the bus" means ISA's way.
    pub fn from_override_flags(flags: u16) -> Self {
        const LOW: u16 = 0b11;
        const LEVEL: u16 = 0b11 << 2;
        Self {
            active_low: flags & 0b11 == LOW,
            level_triggered: flags & (0b11 << 2) == LEVEL,
        }
    }
}

/// How many redirection entries a chip has, from its version register.
pub fn entry_count(version: u32) -> u32 {
    ((version >> 16) & 0xFF) + 1
}

/// A redirection entry: raise `vector` on the core with APIC ID `apic_id`
/// (physical destination mode, fixed delivery – an 8-bit ID).
pub fn entry(vector: u8, apic_id: u8, signal: Signal) -> u64 {
    let mut value = u64::from(vector) | u64::from(apic_id) << DESTINATION_SHIFT;
    if signal.active_low {
        value |= ACTIVE_LOW;
    }
    if signal.level_triggered {
        value |= LEVEL_TRIGGERED;
    }
    value
}

/// The two register numbers of line `index`'s entry: (low, high).
pub fn entry_registers(index: u32) -> (u32, u32) {
    let low = FIRST_ENTRY_REGISTER + 2 * index;
    (low, low + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isa_keyboard_entry() {
        // Vector 0x51 to APIC ID 3, edge, active high, not masked.
        assert_eq!(entry(0x51, 3, Signal::ISA), 0x0300_0000_0000_0051);
    }

    #[test]
    fn level_and_active_low() {
        let signal = Signal {
            active_low: true,
            level_triggered: true,
        };
        assert_eq!(entry(0x49, 0, signal), 0xA049);
    }

    #[test]
    fn override_flags() {
        assert_eq!(Signal::from_override_flags(0), Signal::ISA);
        assert_eq!(Signal::from_override_flags(0b0101), Signal::ISA);
        assert_eq!(
            Signal::from_override_flags(0b1111),
            Signal {
                active_low: true,
                level_triggered: true
            }
        );
    }

    #[test]
    fn counts_and_registers() {
        assert_eq!(entry_count(0x0017_0011), 24); // QEMU's: 24 lines
        assert_eq!(entry_registers(0), (0x10, 0x11));
        assert_eq!(entry_registers(1), (0x12, 0x13));
    }
}
