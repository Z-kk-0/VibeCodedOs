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
use task::Exit;

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
    // Device interrupts: all masked, then only the keyboard's let through.
    // It goes to whichever program init gives its handle (DECISIONS.md, 025).
    let io_apics = arch::init_device_interrupts(boot_info.acpi_root);
    if arch::route_isa_interrupt(ipc::KEYBOARD_IRQ) {
        println!(
            "{io_apics} I/O APIC(s), every line masked but IRQ {} (keyboard) - init gets it as a handle.",
            ipc::KEYBOARD_IRQ
        );
    } else {
        println!("No route for the keyboard's interrupt: nobody will hear keys.");
    }
    ipc::check(screen);

    println!("Hematite kernel ready.");

    if option_env!("HEMATITE_CRASH_TEST").is_some() {
        crash_test();
    }
    // The boot task's work is done. From here on the kernel runs no task of
    // its own but the idle tasks (DECISIONS.md, 024): everything else is a
    // user program. Nobody waits for the boot task, so keep no record.
    task::forget_exit(task::current_id());
    task::exit_current(Exit::Code(0))
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

/// Built in only by `cargo xtask boot-test --crash` / `--crash-stack`: kernel
/// bugs that must stop the machine (DECISIONS.md, 024), and be reported.
fn crash_test() {
    if option_env!("HEMATITE_CRASH_TEST") == Some("stack-overflow") {
        println!("Crash test: overflowing the boot stack...");
        overflow(0);
    }
    // A kernel task that faults *while holding a lock*: that can't be
    // contained (every other core might wait for the lock forever).
    static HELD: sync::SpinLock<()> = sync::SpinLock::new(());
    println!("Crash test: a kernel task reads unmapped memory while holding a lock...");
    let crasher = task::spawn("crasher", || {
        let _guard = HELD.lock();
        let unmapped = 0x1000 as *const u64;
        // SAFETY: not safe at all – that's the point. It must page-fault.
        let _ = unsafe { unmapped.read_volatile() };
    });
    task::wait(crasher);
    println!("Crash test failed: the machine is still running.");
}

/// Calls itself until the stack runs into its guard page.
#[allow(unconditional_recursion, reason = "that is the crash test")]
fn overflow(depth: u64) -> u64 {
    let frame = core::hint::black_box([depth; 64]);
    overflow(depth + 1) + frame[0]
}

/// A kernel bug. Like a fault: if it provably touched no shared state, only
/// the task it happened in ends (DECISIONS.md, 024); else the machine stops.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let reason = match task::judge_failure(cpu::interrupts_enabled()) {
        task::Verdict::Contain => {
            println!("[{}] kernel panic contained: {info}", task::current_name());
            println!(
                "  No lock was held: only this task ends. What it had on its stack is leaked."
            );
            task::exit_current(Exit::KernelFailed(abi::KERNEL_PANIC))
        }
        task::Verdict::Stop(reason) => reason,
    };
    arch::stop_other_cores();
    emergency_println!("\nKERNEL PANIC: {info}");
    emergency_println!("  Machine stopped: {reason}.");
    arch::stop_this_core()
}
