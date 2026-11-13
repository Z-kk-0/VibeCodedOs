//! The legacy 8259 interrupt controllers ("PIC"), from the 1980s PC.
//!
//! We use the local APIC instead, but the PICs are still there and the
//! firmware may have left them active. Out of the box they deliver on
//! vectors 8–15 – the same numbers as CPU exceptions! So we move them to
//! harmless vectors and mask every line. Sequence: OSDev wiki "8259 PIC".

use super::port;

const PRIMARY_COMMAND: u16 = 0x20;
const PRIMARY_DATA: u16 = 0x21;
const SECONDARY_COMMAND: u16 = 0xA0;
const SECONDARY_DATA: u16 = 0xA1;

/// Where the PICs' 2 × 8 lines land if one fires anyway (e.g. a spurious
/// interrupt): far away from exceptions and from our own vectors.
pub const FIRST_VECTOR: u8 = 0x70;

/// "Initialization command word 1": start setup, a 4th word will follow.
const START_INITIALIZATION: u8 = 0x11;
/// The secondary PIC hangs on line 2 of the primary.
const SECONDARY_ON_LINE_2: u8 = 1 << 2;
const SECONDARY_CASCADE_IDENTITY: u8 = 2;
const MODE_8086: u8 = 0x01;
const MASK_ALL_LINES: u8 = 0xFF;

/// # Safety
/// Call once, with interrupts disabled.
pub unsafe fn disable() {
    let steps = [
        (PRIMARY_COMMAND, START_INITIALIZATION),
        (SECONDARY_COMMAND, START_INITIALIZATION),
        (PRIMARY_DATA, FIRST_VECTOR),
        (SECONDARY_DATA, FIRST_VECTOR + 8),
        (PRIMARY_DATA, SECONDARY_ON_LINE_2),
        (SECONDARY_DATA, SECONDARY_CASCADE_IDENTITY),
        (PRIMARY_DATA, MODE_8086),
        (SECONDARY_DATA, MODE_8086),
        (PRIMARY_DATA, MASK_ALL_LINES),
        (SECONDARY_DATA, MASK_ALL_LINES),
    ];
    for (port_number, value) in steps {
        // SAFETY: these are the PICs' fixed ports; the sequence is the
        // documented initialization protocol.
        unsafe {
            port::write_u8(port_number, value);
            port::wait_a_moment();
        }
    }
}
