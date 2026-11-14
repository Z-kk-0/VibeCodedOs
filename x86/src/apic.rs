//! Local APIC: every CPU core's own interrupt controller, including a timer.
//! In xAPIC mode it is controlled through 32-bit registers in a 4 KiB
//! memory window ("MMIO"); in x2APIC mode through model-specific registers,
//! with 32-bit APIC IDs. Intel SDM Vol. 3A, §11.4 (base, Table 11-1
//! register offsets), §11.5.4 (timer), §11.12 (x2APIC).

/// Model-specific register holding the APIC's physical base address.
pub const BASE_MSR: u32 = 0x1B;
pub const BASE_MSR_ENABLE: u64 = 1 << 11;
/// Also set: x2APIC mode (registers as MSRs). Needs `BASE_MSR_ENABLE`.
pub const BASE_MSR_X2APIC: u64 = 1 << 10;
/// CPUID leaf 1, ECX: the CPU has an x2APIC.
pub const CPUID_1_ECX_X2APIC: u32 = 1 << 21;
/// In x2APIC mode the whole interrupt command is one 64-bit MSR.
pub const X2APIC_INTERRUPT_COMMAND_MSR: u32 = 0x830;

/// The MSR an xAPIC register offset becomes in x2APIC mode.
pub fn x2apic_msr(register: usize) -> u32 {
    0x800 + (register >> 4) as u32
}
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

/// In xAPIC mode the ID register keeps the APIC ID in its top byte; in
/// x2APIC mode it is the whole register.
pub fn id_from_register(value: u32, x2apic: bool) -> u32 {
    if x2apic { value } else { value >> 24 }
}

/// An inter-processor interrupt (IPI): the command word (`low`) and the
/// destination APIC ID. SDM Vol. 3A, §11.6.1, Figure 11-12, §11.12.9.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipi {
    pub destination: u32,
    pub low: u32,
}

impl Ipi {
    /// The high half of the command register in xAPIC mode (IDs < 256).
    pub fn xapic_high(self) -> u32 {
        self.destination << 24
    }

    /// The command MSR's value in x2APIC mode.
    pub fn x2apic_value(self) -> u64 {
        u64::from(self.destination) << 32 | u64::from(self.low)
    }
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
const DELIVERY_NMI: u32 = 0b100 << 8;

impl Ipi {
    /// Interrupt `vector` on the core with this APIC ID.
    pub fn fixed(apic_id: u32, vector: u8) -> Self {
        Self::to(apic_id, DELIVERY_FIXED | LEVEL_ASSERT | u32::from(vector))
    }

    /// Interrupt `vector` on every other core.
    pub fn fixed_to_all_others(vector: u8) -> Self {
        Self {
            destination: 0,
            low: ALL_EXCEPT_SELF | DELIVERY_FIXED | LEVEL_ASSERT | u32::from(vector),
        }
    }

    /// A non-maskable interrupt on every other core: arrives even where
    /// interrupts are switched off (the vector field is ignored).
    pub fn nmi_to_all_others() -> Self {
        Self {
            destination: 0,
            low: ALL_EXCEPT_SELF | DELIVERY_NMI | LEVEL_ASSERT,
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
            destination: apic_id,
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
                destination: 1,
                low: 0x4500
            }
        );
        assert_eq!(Ipi::init(1).xapic_high(), 0x0100_0000);
        assert_eq!(Ipi::startup(1, 0x8000).low, 0x4608);
        assert_eq!(Ipi::fixed(3, 0x40).low, 0x4040);
        assert_eq!(Ipi::fixed(3, 0x40).xapic_high(), 0x0300_0000);
        assert_eq!(Ipi::fixed(300, 0x40).x2apic_value(), 300 << 32 | 0x4040);
        assert_eq!(Ipi::fixed_to_all_others(0x41).low, 0xC4041);
        assert_eq!(Ipi::nmi_to_all_others().low, 0xC4400);
        assert_eq!(id_from_register(0x0200_0000, false), 2);
        assert_eq!(id_from_register(300, true), 300);
        assert_eq!(
            x2apic_msr(register::INTERRUPT_COMMAND_LOW),
            X2APIC_INTERRUPT_COMMAND_MSR
        );
        assert_eq!(x2apic_msr(0xB0), 0x80B, "end of interrupt");
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
