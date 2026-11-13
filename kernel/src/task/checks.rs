//! Boot-time proof that preemptive multitasking works.

use super::{sleep, spawn, task_count};
use crate::sync::SpinLock;
use crate::time;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

static LOG: SpinLock<Vec<u8>> = SpinLock::new(Vec::new());
static FINISHED: AtomicUsize = AtomicUsize::new(0);

/// Two tasks that never give up the CPU voluntarily – they busy-wait – and
/// one that sleeps. If the busy tasks' letters interleave, the timer really
/// takes the CPU away from them. Afterwards all tasks must be gone.
pub fn check() {
    const ROUNDS: usize = 6;
    const TASKS: usize = 3;
    for letter in [b'A', b'B'] {
        spawn(move || {
            for _ in 0..ROUNDS {
                LOG.lock().push(letter);
                busy_wait_for_next_tick();
            }
            FINISHED.fetch_add(1, Ordering::SeqCst);
        });
    }
    spawn(|| {
        sleep(5);
        LOG.lock().push(b'z');
        FINISHED.fetch_add(1, Ordering::SeqCst);
    });

    while FINISHED.load(Ordering::SeqCst) < TASKS {
        sleep(1);
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
    println!("All {TASKS} tasks exited and their stacks were freed.");
}

fn busy_wait_for_next_tick() {
    let start = time::ticks();
    while time::ticks() == start {
        core::hint::spin_loop();
    }
}
