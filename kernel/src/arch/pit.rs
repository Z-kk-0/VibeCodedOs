//! Busy-waiting with the PIT – only used once, to measure the APIC timer.

use super::port;
use x86::pit::{
    CHANNEL_2_DATA, CHANNEL_2_ONE_SHOT, COMMAND, CONTROL_CHANNEL_2_GATE, CONTROL_CHANNEL_2_OUTPUT,
    CONTROL_PORT, CONTROL_SPEAKER, count_for_microseconds,
};

/// Waits `microseconds` (at most ~54 ms) by letting PIT channel 2 count down
/// and polling its output bit. Needs no interrupts.
pub fn wait_microseconds(microseconds: u32) {
    let [low, high] = count_for_microseconds(microseconds).to_le_bytes();
    // SAFETY: fixed PC ports of the PIT and its control port. We keep the
    // speaker off and only touch channel 2, which nothing else uses.
    unsafe {
        let control = port::read_u8(CONTROL_PORT);
        port::write_u8(
            CONTROL_PORT,
            (control & !CONTROL_SPEAKER) | CONTROL_CHANNEL_2_GATE,
        );
        port::write_u8(COMMAND, CHANNEL_2_ONE_SHOT);
        port::write_u8(CHANNEL_2_DATA, low);
        port::write_u8(CHANNEL_2_DATA, high); // counting starts now
        while port::read_u8(CONTROL_PORT) & CONTROL_CHANNEL_2_OUTPUT == 0 {
            core::hint::spin_loop();
        }
    }
}
