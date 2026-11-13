//! The local APIC and its timer (register layout: `x86::apic`).
//!
//! The timer counts down at an unknown speed (bus clock / 16), so we first
//! measure it against the PIT, then let it fire periodically.

use super::{cpu, pit};
use core::sync::atomic::{AtomicU64, Ordering};
use x86::apic::{
    BASE_ADDRESS_MASK, BASE_MSR, BASE_MSR_ENABLE, LVT_MASKED, LVT_TIMER_PERIODIC, SOFTWARE_ENABLE,
    TIMER_DIVIDE_BY_16, periodic_initial_count, register,
};

pub const TIMER_VECTOR: u64 = 0x20;
/// The APIC raises this when an interrupt vanished before delivery.
pub const SPURIOUS_VECTOR: u64 = 0xFF;

const CALIBRATION_MICROSECONDS: u32 = 10_000;

/// Address of the register window, set once by `start_timer`.
static BASE: AtomicU64 = AtomicU64::new(0);

pub struct TimerSetup {
    /// How fast the timer counts down, in steps per second.
    pub steps_per_second: u64,
    pub interrupts_per_second: u32,
}

/// Enables the APIC and starts a periodic timer interrupt on `TIMER_VECTOR`.
///
/// # Safety
/// Call once, with interrupts disabled and the IDT installed.
pub unsafe fn start_timer(interrupts_per_second: u32) -> TimerSetup {
    // SAFETY: IA32_APIC_BASE exists on every x86_64 CPU.
    let base_msr = unsafe { cpu::read_msr(BASE_MSR) };
    // SAFETY: setting the enable bit keeps the base address unchanged.
    unsafe { cpu::write_msr(BASE_MSR, base_msr | BASE_MSR_ENABLE) };
    BASE.store(base_msr & BASE_ADDRESS_MASK, Ordering::Relaxed);

    write(register::TASK_PRIORITY, 0); // accept every interrupt
    write(
        register::SPURIOUS_INTERRUPT,
        SOFTWARE_ENABLE | SPURIOUS_VECTOR as u32,
    );
    write(register::TIMER_DIVIDE, TIMER_DIVIDE_BY_16);

    // Measure: count down from the maximum while the PIT waits 10 ms.
    write(register::LVT_TIMER, LVT_MASKED);
    write(register::TIMER_INITIAL_COUNT, u32::MAX);
    pit::wait_microseconds(CALIBRATION_MICROSECONDS);
    let counted = u32::MAX - read(register::TIMER_CURRENT_COUNT);

    let initial = periodic_initial_count(counted, CALIBRATION_MICROSECONDS, interrupts_per_second);
    write(
        register::LVT_TIMER,
        TIMER_VECTOR as u32 | LVT_TIMER_PERIODIC,
    );
    write(register::TIMER_INITIAL_COUNT, initial); // starts the timer

    TimerSetup {
        steps_per_second: u64::from(counted) * 1_000_000 / u64::from(CALIBRATION_MICROSECONDS),
        interrupts_per_second,
    }
}

/// Tells the APIC the current interrupt is handled, so it can deliver the next.
pub fn end_of_interrupt() {
    write(register::END_OF_INTERRUPT, 0);
}

fn write(register: usize, value: u32) {
    // SAFETY: `BASE` is the APIC's physical register window, which the
    // physical memory window covers (it's below 4 GiB); `register` is one
    // of the documented offsets. The firmware's MTRRs make it uncached.
    unsafe { register_pointer(register).write_volatile(value) }
}

fn read(register: usize) -> u32 {
    // SAFETY: as in `write`.
    unsafe { register_pointer(register).read_volatile() }
}

fn register_pointer(register: usize) -> *mut u32 {
    let physical = BASE.load(Ordering::Relaxed) + register as u64;
    crate::memory::physical_to_pointer(physical).cast()
}
