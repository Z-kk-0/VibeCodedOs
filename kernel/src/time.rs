//! Kernel time, counted in timer ticks.

use crate::arch::cpu;
use core::sync::atomic::{AtomicU64, Ordering};

/// 100 ticks per second = one every 10 ms. Coarse, but enough until we
/// schedule tasks; a faster tick only costs more interrupts.
pub const TICKS_PER_SECOND: u32 = 100;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Called by the timer interrupt.
pub fn on_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime_milliseconds() -> u64 {
    ticks() * 1000 / u64::from(TICKS_PER_SECOND)
}

/// Sleeps (CPU halted) for at least `count` ticks.
pub fn sleep_ticks(count: u64) {
    let end = ticks() + count;
    while ticks() < end {
        cpu::wait_for_interrupt();
    }
}
