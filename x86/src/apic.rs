//! Local APIC: every CPU core's own interrupt controller, including a timer.
//! It is controlled through 32-bit registers in a 4 KiB memory window
//! ("MMIO"). Intel SDM Vol. 3A, §11.4 (base, Table 11-1 register offsets)
//! and §11.5.4 (timer).

/// Model-specific register holding the APIC's physical base address.
pub const BASE_MSR: u32 = 0x1B;
pub const BASE_MSR_ENABLE: u64 = 1 << 11;
/// Bits 12..52 of the MSR are the page-aligned base address.
pub const BASE_ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Byte offsets of the registers from the base address.
pub mod register {
    pub const ID: usize = 0x20;
    pub const TASK_PRIORITY: usize = 0x80;
    pub const END_OF_INTERRUPT: usize = 0xB0;
    pub const SPURIOUS_INTERRUPT: usize = 0xF0;
    /// Interrupt command register: writing the low half sends the IPI.
    pub const INTERRUPT_COMMAND_LOW: usize = 0x300;
    pub const INTERRUPT_COMMAND_HIGH: usize = 0x310;
    pub const LVT_TIMER: usize = 0x320;
    pub const TIMER_INITIAL_COUNT: usize = 0x380;
    pub const TIMER_CURRENT_COUNT: usize = 0x390;
    pub const TIMER_DIVIDE: usize = 0x3E0;
}

/// In the spurious-interrupt register: turns the APIC on.
pub const SOFTWARE_ENABLE: u32 = 1 << 8;
/// In a "local vector table" (LVT) entry: don't deliver this interrupt.
pub const LVT_MASKED: u32 = 1 << 16;
/// In the timer's LVT entry: restart automatically instead of firing once.
pub const LVT_TIMER_PERIODIC: u32 = 1 << 17;
/// The timer counts down at (bus clock / 16). Encoding from SDM Figure 11-10.
pub const TIMER_DIVIDE_BY_16: u32 = 0b0011;

/// The ID register keeps the APIC ID in its top byte (xAPIC mode).
pub fn id_from_register(value: u32) -> u32 {
    value >> 24
}

/// An inter-processor interrupt (IPI), as the two halves of the interrupt
/// command register. SDM Vol. 3A, §11.6.1 and Figure 11-12.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipi {
    pub high: u32,
    pub low: u32,
}

/// In the low half: the APIC hasn't sent the last IPI yet.
pub const DELIVERY_PENDING: u32 = 1 << 12;

const DELIVERY_FIXED: u32 = 0b000 << 8;
const DELIVERY_INIT: u32 = 0b101 << 8;
const DELIVERY_STARTUP: u32 = 0b110 << 8;
/// Must be set for everything but an INIT de-assert.
const LEVEL_ASSERT: u32 = 1 << 14;
/// "Destination shorthand": every core except the sender, ignoring `high`.
const ALL_EXCEPT_SELF: u32 = 0b11 << 18;

impl Ipi {
    /// Interrupt `vector` on the core with this APIC ID.
    pub fn fixed(apic_id: u32, vector: u8) -> Self {
        Self::to(apic_id, DELIVERY_FIXED | LEVEL_ASSERT | u32::from(vector))
    }

    /// Interrupt `vector` on every other core.
    pub fn fixed_to_all_others(vector: u8) -> Self {
        Self {
            high: 0,
            low: ALL_EXCEPT_SELF | DELIVERY_FIXED | LEVEL_ASSERT | u32::from(vector),
        }
    }

    /// Resets the core into its "wait for startup" state.
    pub fn init(apic_id: u32) -> Self {
        Self::to(apic_id, DELIVERY_INIT | LEVEL_ASSERT)
    }

    /// Starts a waiting core in real mode at physical `page_address`, which
    /// must be page-aligned and below 1 MiB: the vector is its page number.
    pub fn startup(apic_id: u32, page_address: u64) -> Self {
        assert!(
            page_address.is_multiple_of(4096) && page_address < 0x10_0000,
            "startup code must be on a page below 1 MiB"
        );
        Self::to(
            apic_id,
            DELIVERY_STARTUP | LEVEL_ASSERT | (page_address >> 12) as u32,
        )
    }

    fn to(apic_id: u32, low: u32) -> Self {
        Self {
            high: apic_id << 24,
            low,
        }
    }
}

/// The timer's start value so that it fires `hz` times per second, given it
/// counted down `measured` steps in `measured_microseconds`.
pub fn periodic_initial_count(measured: u32, measured_microseconds: u32, hz: u32) -> u32 {
    let per_second = u64::from(measured) * 1_000_000 / u64::from(measured_microseconds.max(1));
    (per_second / u64::from(hz.max(1))).clamp(1, u64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_count_scales_to_the_wanted_frequency() {
        // 62_500 steps in 10 ms = 6.25 MHz.
        assert_eq!(periodic_initial_count(62_500, 10_000, 100), 62_500);
        assert_eq!(periodic_initial_count(62_500, 10_000, 1_000), 6_250);
    }

    #[test]
    fn ipis_match_the_values_every_os_sends() {
        // The classic INIT-SIPI-SIPI sequence to APIC ID 1, startup at 0x8000.
        assert_eq!(
            Ipi::init(1),
            Ipi {
                high: 0x0100_0000,
                low: 0x4500
            }
        );
        assert_eq!(Ipi::startup(1, 0x8000).low, 0x4608);
        assert_eq!(Ipi::fixed(3, 0x40).low, 0x4040);
        assert_eq!(Ipi::fixed(3, 0x40).high, 0x0300_0000);
        assert_eq!(Ipi::fixed_to_all_others(0x41).low, 0xC4041);
        assert_eq!(id_from_register(0x0200_0000), 2);
    }

    #[test]
    #[should_panic(expected = "below 1 MiB")]
    fn startup_code_above_1_mib_is_refused() {
        Ipi::startup(1, 0x10_0000);
    }

    #[test]
    fn initial_count_never_becomes_zero() {
        // A count of 0 would stop the timer entirely.
        assert_eq!(periodic_initial_count(1, 10_000, 1_000), 1);
        assert_eq!(periodic_initial_count(0, 0, 0), 1);
    }
}
