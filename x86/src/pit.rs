//! The 8254 Programmable Interval Timer (PIT). Ancient, but its input clock
//! is fixed at 1.193182 MHz on every PC – a reliable ruler for measuring
//! clocks whose speed we don't know, like the APIC timer.
//! Sources: OSDev wiki "Programmable Interval Timer", Intel 8254 datasheet.

pub const FREQUENCY_HZ: u64 = 1_193_182;

pub const CHANNEL_2_DATA: u16 = 0x42;
pub const COMMAND: u16 = 0x43;

/// Port 0x61, "NMI status and control": channel 2 is wired to the PC speaker
/// and can be started and watched through this port without interrupts.
pub const CONTROL_PORT: u16 = 0x61;
pub const CONTROL_CHANNEL_2_GATE: u8 = 1 << 0;
pub const CONTROL_SPEAKER: u8 = 1 << 1;
pub const CONTROL_CHANNEL_2_OUTPUT: u8 = 1 << 5;

/// Bits 7–6 = 10: channel 2. Bits 5–4 = 11: write low, then high byte.
/// Bits 3–1 = 000: mode 0 ("interrupt on terminal count": the output goes
/// high once the count reaches 0). Bit 0 = 0: binary counting.
pub const CHANNEL_2_ONE_SHOT: u8 = 0b1011_0000;

/// PIT steps for a duration. The counter is 16 bits, so at most ~54.9 ms.
pub fn count_for_microseconds(microseconds: u32) -> u16 {
    let count = FREQUENCY_HZ * u64::from(microseconds) / 1_000_000;
    count.min(u64::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_milliseconds_is_about_eleven_thousand_steps() {
        assert_eq!(count_for_microseconds(10_000), 11_931);
    }

    #[test]
    fn long_durations_are_capped_at_the_counter_size() {
        assert_eq!(count_for_microseconds(100_000), u16::MAX);
    }
}
