//! The local APIC – every core has its own, at the same address – with its
//! timer and inter-processor interrupts (register layout: `x86::apic`).
//!
//! The timer counts down at an unknown speed (bus clock / 16), so the boot
//! core first measures it against the PIT; the other cores share the bus
//! clock and simply reuse that measurement.
//!
//! Two ways to reach it: xAPIC mode (registers in a memory window, 8-bit
//! APIC IDs) and x2APIC mode (registers as MSRs, 32-bit IDs – needed beyond
//! 255 cores). We use x2APIC whenever the CPU has it; firmware that already
//! switched to it leaves us no choice anyway (DECISIONS.md, 022).

use super::{cpu, pit};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use x86::apic::{
    BASE_ADDRESS_MASK, BASE_MSR, BASE_MSR_ENABLE, BASE_MSR_X2APIC, DELIVERY_PENDING, Ipi,
    LVT_MASKED, LVT_TIMER_PERIODIC, SOFTWARE_ENABLE, TIMER_DIVIDE_BY_16,
    X2APIC_INTERRUPT_COMMAND_MSR, id_from_register, periodic_initial_count, register, x2apic_msr,
};

pub const TIMER_VECTOR: u64 = 0x20;
/// The APIC raises this when an interrupt vanished before delivery.
pub const SPURIOUS_VECTOR: u64 = 0xFF;

const CALIBRATION_MICROSECONDS: u32 = 10_000;

/// Address of the register window, set once by `start_timer`.
static BASE: AtomicU64 = AtomicU64::new(0);
/// Decided once by the boot core, the same on every core.
static X2APIC: AtomicBool = AtomicBool::new(false);
/// The measured start value for the periodic timer, shared by all cores.
static TIMER_INITIAL_COUNT: AtomicU32 = AtomicU32::new(0);

pub struct TimerSetup {
    /// How fast the timer counts down, in steps per second.
    pub steps_per_second: u64,
    pub interrupts_per_second: u32,
}

/// Enables the boot core's APIC, measures its timer and starts a periodic
/// timer interrupt on `TIMER_VECTOR`.
///
/// # Safety
/// Call once, on the boot core, with interrupts disabled and the IDT installed.
pub unsafe fn start_timer(interrupts_per_second: u32) -> TimerSetup {
    // SAFETY: IA32_APIC_BASE exists on every x86_64 CPU.
    let base_msr = unsafe { cpu::read_msr(BASE_MSR) };
    BASE.store(base_msr & BASE_ADDRESS_MASK, Ordering::Relaxed);
    X2APIC.store(
        cpu::has_x2apic() || base_msr & BASE_MSR_X2APIC != 0,
        Ordering::Relaxed,
    );
    // SAFETY: forwarded to the caller.
    unsafe { enable_on_this_core() };

    // Measure: count down from the maximum while the PIT waits 10 ms.
    write(register::LVT_TIMER, LVT_MASKED);
    write(register::TIMER_INITIAL_COUNT, u32::MAX);
    pit::wait_microseconds(CALIBRATION_MICROSECONDS);
    let counted = u32::MAX - read(register::TIMER_CURRENT_COUNT);

    let initial = periodic_initial_count(counted, CALIBRATION_MICROSECONDS, interrupts_per_second);
    TIMER_INITIAL_COUNT.store(initial, Ordering::Relaxed);
    start_periodic_timer();

    TimerSetup {
        steps_per_second: u64::from(counted) * 1_000_000 / u64::from(CALIBRATION_MICROSECONDS),
        interrupts_per_second,
    }
}

/// Enables a started core's APIC – in the mode the boot core chose – so it
/// can send and receive IPIs. Before anything else on that core: once it
/// counts as online it may send a shootdown, and an x2APIC MSR access while
/// its APIC is still in xAPIC mode would fault.
///
/// # Safety
/// Call once per core, after `start_timer` ran on the boot core, with
/// interrupts disabled.
pub unsafe fn enable_on_started_core() {
    // SAFETY: forwarded to the caller.
    unsafe { enable_on_this_core() };
}

/// Starts this core's timer with the boot core's measurement.
///
/// # Safety
/// `enable_on_started_core` ran on this core, and its IDT is loaded.
pub unsafe fn start_timer_on_this_core() {
    start_periodic_timer();
}

/// This core's APIC ID: what IPIs to it are addressed to.
pub fn id() -> u32 {
    id_from_register(read(register::ID), x2apic())
}

/// "x2APIC" or "xAPIC", for the boot log.
pub fn mode() -> &'static str {
    if x2apic() { "x2APIC" } else { "xAPIC" }
}

fn x2apic() -> bool {
    X2APIC.load(Ordering::Relaxed)
}

/// Sends an inter-processor interrupt and waits until the APIC took it.
/// Interrupts should be off, so nothing on this core sends in between.
pub fn send(ipi: Ipi) {
    if x2apic() {
        // One write sends it; there is no "still sending" bit to wait for.
        // SAFETY: the x2APIC is on, so its command MSR exists.
        unsafe { cpu::write_msr(X2APIC_INTERRUPT_COMMAND_MSR, ipi.x2apic_value()) };
        return;
    }
    write(register::INTERRUPT_COMMAND_HIGH, ipi.xapic_high());
    write(register::INTERRUPT_COMMAND_LOW, ipi.low); // this write sends it
    while read(register::INTERRUPT_COMMAND_LOW) & DELIVERY_PENDING != 0 {
        core::hint::spin_loop();
    }
}

/// Whether `start_timer` ran, i.e. IPIs can be sent at all.
pub fn is_ready() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

/// Tells the APIC the current interrupt is handled, so it can deliver the next.
pub fn end_of_interrupt() {
    write(register::END_OF_INTERRUPT, 0);
}

/// # Safety
/// `BASE` must be set and interrupts disabled.
unsafe fn enable_on_this_core() {
    // SAFETY: IA32_APIC_BASE exists on every x86_64 CPU; setting the enable
    // bits keeps the base address unchanged. x2APIC mode may only be entered
    // from an enabled APIC (SDM Vol. 3A, §11.12.5), hence two steps.
    unsafe {
        let base_msr = cpu::read_msr(BASE_MSR) | BASE_MSR_ENABLE;
        cpu::write_msr(BASE_MSR, base_msr);
        if x2apic() {
            cpu::write_msr(BASE_MSR, base_msr | BASE_MSR_X2APIC);
        }
    }
    write(register::TASK_PRIORITY, 0); // accept every interrupt
    write(
        register::SPURIOUS_INTERRUPT,
        SOFTWARE_ENABLE | SPURIOUS_VECTOR as u32,
    );
    write(register::TIMER_DIVIDE, TIMER_DIVIDE_BY_16);
}

fn start_periodic_timer() {
    write(
        register::LVT_TIMER,
        TIMER_VECTOR as u32 | LVT_TIMER_PERIODIC,
    );
    // Writing the start value starts the timer.
    write(
        register::TIMER_INITIAL_COUNT,
        TIMER_INITIAL_COUNT.load(Ordering::Relaxed),
    );
}

fn write(register: usize, value: u32) {
    if x2apic() {
        // SAFETY: in x2APIC mode every documented register is an MSR.
        unsafe { cpu::write_msr(x2apic_msr(register), u64::from(value)) };
        return;
    }
    // SAFETY: `BASE` is the APIC's physical register window, which the
    // physical memory window covers (it's below 4 GiB); `register` is one
    // of the documented offsets. The firmware's MTRRs make it uncached.
    // Each core reaches its own APIC at this same address.
    unsafe { register_pointer(register).write_volatile(value) }
}

fn read(register: usize) -> u32 {
    if x2apic() {
        // SAFETY: as in `write`.
        return unsafe { cpu::read_msr(x2apic_msr(register)) } as u32;
    }
    // SAFETY: as in `write`.
    unsafe { register_pointer(register).read_volatile() }
}

fn register_pointer(register: usize) -> *mut u32 {
    let physical = BASE.load(Ordering::Relaxed) + register as u64;
    crate::memory::physical_to_pointer(physical).cast()
}
