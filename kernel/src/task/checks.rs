//! Boot-time proof that multitasking and user mode work.

use super::{Exit, sleep, spawn, spawn_program, task_count, wait};
use crate::sync::SpinLock;
use crate::{memory, programs, time};
use alloc::string::String;
use alloc::vec::Vec;

static LOG: SpinLock<Vec<u8>> = SpinLock::new(Vec::new());

/// Two tasks that never give up the CPU voluntarily – they busy-wait – and
/// one that sleeps. If the busy tasks' letters interleave, the timer really
/// takes the CPU away from them. Afterwards all tasks must be gone.
pub fn check() {
    const ROUNDS: usize = 6;
    let busy = [b'A', b'B'].map(|letter| {
        spawn("busy", move || {
            for _ in 0..ROUNDS {
                LOG.lock().push(letter);
                busy_wait_for_next_tick();
            }
        })
    });
    let sleeper = spawn("sleeper", || {
        sleep(5);
        LOG.lock().push(b'z');
    });

    for task in busy.into_iter().chain([sleeper]) {
        assert_eq!(wait(task), Exit::Code(0));
    }
    sleep(2); // a few more switches, so the finished tasks get freed

    let log = String::from_utf8(LOG.lock().clone()).expect("log is ASCII");
    assert!(
        log.contains("AB") && log.contains("BA"),
        "tasks did not interleave: {log}"
    );
    assert!(log.contains('z'), "the sleeper never woke up: {log}");
    assert_eq!(task_count(), 2, "finished tasks were not freed"); // boot + idle
    println!("Scheduler works: busy tasks preempted every tick, sleeper woke up: {log}");
    println!("All 3 tasks exited and their stacks were freed.");
}

/// Runs a well-behaved and a malicious user program. The first must exit
/// normally; the second must be stopped by the page tables – and the
/// kernel must not notice anything but that.
pub fn check_user_programs() {
    let free_before = memory::free_frame_count();
    let hello = spawn_program("hello", programs::HELLO).expect("hello failed to load");
    let intruder = spawn_program("intruder", programs::INTRUDER).expect("intruder failed to load");

    assert_eq!(wait(hello), Exit::Code(0), "hello did not exit cleanly");
    let intruder_exit = wait(intruder);
    assert!(
        matches!(intruder_exit, Exit::Killed(_)),
        "the intruder was not stopped: {intruder_exit:?}"
    );
    sleep(2); // let the scheduler free both programs

    assert_eq!(task_count(), 2, "finished programs were not freed");
    let still_held = free_before.saturating_sub(memory::free_frame_count());
    println!(
        "User mode works: hello exited with 0, the intruder was stopped, the kernel carries on."
    );
    println!("Both address spaces freed ({still_held} frames stay with the heap's size classes).");
}

fn busy_wait_for_next_tick() {
    let start = time::ticks();
    while time::ticks() == start {
        core::hint::spin_loop();
    }
}
