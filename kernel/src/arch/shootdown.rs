//! TLB shootdown: when a mapping changes, every core that may have the old
//! translation cached must forget it (DECISIONS.md, 019).
//!
//! Each core has its own TLB, and `invlpg` only cleans the TLB of the core
//! that runs it. So the core changing a mapping asks the others with an
//! inter-processor interrupt and waits until all of them have flushed. Only
//! then is the change really done – e.g. may a freed frame be reused.
//!
//! The trap: the waiting core may hold locks (the page tables', the
//! heap's), and a core spinning for one of those has interrupts off – it
//! would never see our IPI, and both would wait forever. So spinning cores
//! serve shootdowns themselves (`sync::SpinLock` calls `serve_pending`).

use super::apic;
use super::cpu;
use super::per_core::{CoreSet, cores_in, online_cores, this_core};
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicU64, Ordering};
use paging::PAGE_SIZE;
use scheduling::cores::AtomicCoreSet;
use x86::apic::Ipi;

pub const SHOOTDOWN_VECTOR: u8 = 0x40;

/// One shootdown at a time. Taking it may serve other cores' requests
/// (it's a `SpinLock`), so two cores can never wait for each other here.
static SENDING: SpinLock<()> = SpinLock::new(());
/// The pages to flush, valid while `PENDING` isn't empty.
static REQUEST_START: AtomicU64 = AtomicU64::new(0);
static REQUEST_PAGES: AtomicU64 = AtomicU64::new(0);
/// From this many pages on, forgetting everything (reloading CR3) is
/// cheaper than `invlpg` page by page.
const FLUSH_ALL_FROM: u64 = 32;
/// How long to wait for a core before sending it the IPI again.
const RESEND_AFTER_SPINS: u64 = 100_000;
/// Cores that still have to flush for the current request.
static PENDING: AtomicCoreSet = AtomicCoreSet::new();
/// How many shootdowns reached other cores – for the boot check.
static SENT: AtomicU64 = AtomicU64::new(0);

/// Makes the cores in `cores` forget the translations of `pages` pages from
/// `start` – one request for the whole range. This core flushes too if
/// it's in `cores`. Returns once every core has done it.
pub fn flush_pages(cores: CoreSet, start: u64, pages: u64) {
    flush(cores, start, pages);
}

/// Same for pages in the kernel's half, which every core shares.
pub fn flush_kernel_pages(start: u64, pages: u64) {
    flush(online_cores(), start, pages);
}

fn flush(cores: CoreSet, start: u64, pages: u64) {
    if pages == 0 {
        return;
    }
    let online = online_cores();
    if online.is_empty() {
        // Early boot: only this core runs, and its per-core data may not
        // even exist yet.
        flush_here(start, pages);
        return;
    }
    let me = this_core().index;
    if cores.contains(me) {
        flush_here(start, pages);
    }
    let others = cores.and(online).without(CoreSet::only(me));
    if others.is_empty() {
        return;
    }

    let _one_at_a_time = SENDING.lock();
    REQUEST_START.store(start, Ordering::Relaxed);
    REQUEST_PAGES.store(pages, Ordering::Relaxed);
    // SeqCst: whoever sees its bit also sees the request.
    PENDING.store(others);
    for core in cores_in(others) {
        let apic_id = core.apic_id.load(Ordering::Relaxed);
        apic::send(Ipi::fixed(apic_id, SHOOTDOWN_VECTOR));
    }
    // An IPI can get lost – e.g. at a core whose APIC is still switching
    // on while it starts. So ask again now and then; serving twice is
    // harmless (`serve_pending` checks the bit).
    let mut spins: u64 = 0;
    while !PENDING.is_empty() {
        spins += 1;
        if spins.is_multiple_of(RESEND_AFTER_SPINS) {
            for core in cores_in(PENDING.load()) {
                let apic_id = core.apic_id.load(Ordering::Relaxed);
                apic::send(Ipi::fixed(apic_id, SHOOTDOWN_VECTOR));
            }
        }
        core::hint::spin_loop();
    }
    SENT.fetch_add(1, Ordering::Relaxed);
}

/// How many shootdowns other cores have answered so far.
pub fn sent_count() -> u64 {
    SENT.load(Ordering::Relaxed)
}

/// Whether some core waits for a flush – cheap enough for a spin loop.
pub fn any_pending() -> bool {
    !PENDING.is_empty()
}

/// Does this core's part of the current shootdown, if it has one. Called
/// by the shootdown interrupt and by spinning locks.
pub fn serve_pending() {
    let me = this_core().index;
    if !PENDING.contains(me) {
        return;
    }
    flush_here(
        REQUEST_START.load(Ordering::Relaxed),
        REQUEST_PAGES.load(Ordering::Relaxed),
    );
    // SeqCst: our flush is done before the sender sees our bit gone.
    PENDING.remove(me);
}

fn flush_here(start: u64, pages: u64) {
    if pages >= FLUSH_ALL_FROM {
        cpu::flush_translation_cache();
    } else {
        for page in (start..).step_by(PAGE_SIZE as usize).take(pages as usize) {
            cpu::flush_translation(page);
        }
    }
}
