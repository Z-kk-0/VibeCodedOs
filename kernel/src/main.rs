//! The Hematite kernel. Entered from the loader via `_start` (arch/start.rs).

#![no_std]
#![no_main]

// Box, Vec, String & co., backed by our heap (memory/allocator.rs).
extern crate alloc;

// First, so the macros are visible in all modules below.
#[macro_use]
mod print;

mod arch;
mod ipc;
mod memory;
mod programs;
mod serial;
mod sync;
mod syscall;
mod task;
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
    let screen = boot_info.framebuffer;
    println!("\nHematite kernel: the firmware is gone, the machine is ours.");

    arch::init();
    println!("Own GDT and IDT loaded, legacy PIC silenced.");
    cpu::breakpoint(); // proves an exception can go through our handlers and come back

    let timer = arch::start_timer(time::TICKS_PER_SECOND);
    println!(
        "APIC ({}) timer counts {} steps/s, interrupting {} times/s.",
        arch::apic_mode(),
        timer.steps_per_second,
        timer.interrupts_per_second
    );
    // Hangs here if timer interrupts don't arrive – the boot test would notice.
    time::sleep_ticks(10);
    println!(
        "Timer interrupts arrive: uptime {} ms.",
        time::uptime_milliseconds()
    );

    memory::start(boot_info);

    task::init();
    let cores = arch::start_other_cores(boot_info);
    println!("{cores} cores online.");
    task::check();
    if screen.exists() {
        // Its only view will be the one `display` opens (DECISIONS.md, 023).
        let first_page = boot_info::PHYSICAL_MEMORY_WINDOW + screen.address;
        let last_page = first_page + (screen.pages() - 1) * paging::PAGE_SIZE;
        assert!(
            memory::paging::translate(first_page).is_none()
                && memory::paging::translate(last_page).is_none(),
            "the framebuffer is still in the physical memory window"
        );
        println!(
            "Screen: {}x{} pixels, {} KiB framebuffer at {:#x}, not in the kernel's window - init gets it as a device.",
            screen.width,
            screen.height,
            screen.size / 1024,
            screen.address
        );
    } else {
        println!("No screen: the display service will draw nowhere.");
    }
    ipc::check(screen);

    println!("Hematite kernel ready.");

    if option_env!("HEMATITE_CRASH_TEST").is_some() {
        crash_test();
    }
    heartbeat()
}

/// Where every core but the first continues once `arch` set it up: it joins
/// the scheduler, starts its timer and becomes that core's idle task.
fn other_core_main() -> ! {
    task::add_this_core();
    arch::start_timer_on_this_core();
    arch::report_ready();
    loop {
        cpu::wait_for_interrupt();
    }
}

/// Nothing else to do yet: the boot task sleeps (the idle task halts the
/// CPU meanwhile) and shows a sign of life now and then.
fn heartbeat() -> ! {
    const HEARTBEAT_TICKS: u64 = 10 * time::TICKS_PER_SECOND as u64;
    loop {
        task::sleep(HEARTBEAT_TICKS);
        println!(
            "Still alive, uptime {} s.",
            time::uptime_milliseconds() / 1000
        );
    }
}

/// Built in only by `cargo xtask boot-test --crash`: touches the lower half,
/// which is empty since the identity map is gone, to check that faults get
/// reported.
fn crash_test() {
    if option_env!("HEMATITE_CRASH_TEST") == Some("stack-overflow") {
        println!("Crash test: overflowing the boot stack...");
        overflow(0);
    }
    println!("Crash test: reading unmapped memory...");
    let unmapped = 0x1000 as *const u64;
    // SAFETY: not safe at all – that's the point. It must page-fault.
    let _ = unsafe { unmapped.read_volatile() };
}

/// Calls itself until the stack runs into its guard page.
#[allow(unconditional_recursion, reason = "that is the crash test")]
fn overflow(depth: u64) -> u64 {
    let frame = core::hint::black_box([depth; 64]);
    overflow(depth + 1) + frame[0]
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    arch::stop_other_cores();
    emergency_println!("\nKERNEL PANIC: {info}");
    arch::stop_this_core()
}
