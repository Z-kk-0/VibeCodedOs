//! Boot-time proof that multitasking and multi-core work.

use super::{Exit, core_count, sleep, spawn, task_count, wait};
use crate::sync::SpinLock;
use crate::{arch, time};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

static LOG: SpinLock<Vec<u8>> = SpinLock::new(Vec::new());
/// One bit per core a busy task ran on.
static CORES_USED: AtomicU64 = AtomicU64::new(0);

/// Busy tasks that never give up the CPU voluntarily – they busy-wait – and
/// one that sleeps. If the busy tasks' letters interleave, they really run
/// side by side (on one core: the timer takes the CPU away from them). With
/// several cores, they must have run on more than one. Afterwards all
/// tasks must be gone – their stacks freed, which also exercises the TLB
/// shootdown.
pub fn check() {
    const ROUNDS: usize = 6;
    let busy = [b'A', b'B', b'C', b'D'].map(|letter| {
        spawn("busy", move || {
            for _ in 0..ROUNDS {
                CORES_USED.fetch_or(1 << arch::core_index(), Ordering::Relaxed);
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
        log.contains("AB") || log.contains("BA"),
        "tasks did not interleave: {log}"
    );
    assert!(log.contains('z'), "the sleeper never woke up: {log}");
    let cores_used = CORES_USED.load(Ordering::Relaxed).count_ones() as usize;
    assert!(
        cores_used > 1 || core_count() == 1,
        "{} cores, but all busy tasks ran on one: {log}",
        core_count()
    );
    // Left: boot, plus one idle task per core.
    assert_eq!(
        task_count(),
        1 + core_count(),
        "finished tasks were not freed"
    );
    // Freeing a stack unmaps heap pages that other cores may have cached.
    let shootdowns = arch::shootdown_count();
    assert!(
        shootdowns > 0 || core_count() == 1,
        "stacks were freed, but no other core was told to flush its TLB"
    );
    println!(
        "Scheduler works: busy tasks interleaved on {cores_used} cores, sleeper woke up: {log}"
    );
    println!(
        "All 5 tasks exited and their stacks were freed ({shootdowns} TLB shootdowns so far)."
    );
}

fn busy_wait_for_next_tick() {
    let start = time::ticks();
    while time::ticks() == start {
        core::hint::spin_loop();
    }
}
