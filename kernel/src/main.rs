//! The Hematite kernel. Entered from the loader via `_start` (arch/start.rs).

#![no_std]
#![no_main]

// First, so the macros are visible in all modules below.
#[macro_use]
mod print;

mod arch;
mod memory;
mod serial;
mod sync;
mod time;

use arch::cpu;
use boot_info::BootInfo;
use serial::Serial;

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    Serial::init_com1();
    assert_eq!(
        boot_info.magic,
        boot_info::MAGIC,
        "BootInfo pointer is garbage"
    );
    println!("\nHematite kernel: the firmware is gone, the machine is ours.");

    arch::init();
    println!("Own GDT and IDT loaded, legacy PIC silenced.");
    cpu::breakpoint(); // proves an exception can go through our handlers and come back

    let timer = arch::start_timer(time::TICKS_PER_SECOND);
    println!(
        "APIC timer counts {} steps/s, interrupting {} times/s.",
        timer.steps_per_second, timer.interrupts_per_second
    );
    // Hangs here if timer interrupts don't arrive – the boot test would notice.
    time::sleep_ticks(10);
    println!(
        "Timer interrupts arrive: uptime {} ms.",
        time::uptime_milliseconds()
    );

    memory::start(boot_info);

    println!("Hematite kernel ready.");

    if option_env!("HEMATITE_CRASH_TEST").is_some() {
        crash_test();
    }
    idle()
}

/// Nothing to do yet: sleep between ticks, show a sign of life now and then.
fn idle() -> ! {
    const HEARTBEAT_SECONDS: u64 = 10;
    let mut next_heartbeat = HEARTBEAT_SECONDS * 1000;
    loop {
        cpu::wait_for_interrupt();
        let uptime = time::uptime_milliseconds();
        if uptime >= next_heartbeat {
            println!("Still alive, uptime {} s.", uptime / 1000);
            next_heartbeat += HEARTBEAT_SECONDS * 1000;
        }
    }
}

/// Built in only by `cargo xtask boot-test --crash`: touches the lower half,
/// which is empty since the identity map is gone, to check that faults get
/// reported.
fn crash_test() {
    println!("Crash test: reading unmapped memory...");
    let unmapped = 0x1000 as *const u64;
    // SAFETY: not safe at all – that's the point. It must page-fault.
    let _ = unsafe { unmapped.read_volatile() };
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\nKERNEL PANIC: {info}");
    cpu::halt_forever()
}
