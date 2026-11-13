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
    pub const TASK_PRIORITY: usize = 0x80;
    pub const END_OF_INTERRUPT: usize = 0xB0;
    pub const SPURIOUS_INTERRUPT: usize = 0xF0;
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
    fn initial_count_never_becomes_zero() {
        // A count of 0 would stop the timer entirely.
        assert_eq!(periodic_initial_count(1, 10_000, 1_000), 1);
        assert_eq!(periodic_initial_count(0, 0, 0), 1);
    }
}
