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
use super::per_core::{CoreMask, cores_in, online_cores, this_core};
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicU64, Ordering};
use x86::apic::Ipi;

pub const SHOOTDOWN_VECTOR: u8 = 0x40;

/// One shootdown at a time. Taking it may serve other cores' requests
/// (it's a `SpinLock`), so two cores can never wait for each other here.
static SENDING: SpinLock<()> = SpinLock::new(());
/// The address to flush, valid while `PENDING` isn't 0.
static REQUEST: AtomicU64 = AtomicU64::new(0);
/// Cores that still have to flush for the current request.
static PENDING: AtomicU64 = AtomicU64::new(0);
/// How many shootdowns reached other cores – for the boot check.
static SENT: AtomicU64 = AtomicU64::new(0);

/// Makes the cores in `cores` forget the translation of the page at
/// `address`. This core flushes too if it's in `cores`. Returns once every
/// core has done it.
pub fn flush_page(cores: CoreMask, address: u64) {
    flush(cores, address);
}

/// Same for the page at `address` in the kernel's half, which every core
/// shares.
pub fn flush_kernel_page(address: u64) {
    flush(online_cores(), address);
}

fn flush(cores: CoreMask, address: u64) {
    let online = online_cores();
    if online == 0 {
        // Early boot: only this core runs, and its per-core data may not
        // even exist yet.
        cpu::flush_translation(address);
        return;
    }
    let me = this_core().bit();
    if cores & me != 0 {
        cpu::flush_translation(address);
    }
    let others = cores & online & !me;
    if others == 0 {
        return;
    }

    let _one_at_a_time = SENDING.lock();
    REQUEST.store(address, Ordering::Relaxed);
    // Release: whoever sees its bit also sees the request.
    PENDING.store(others, Ordering::Release);
    for core in cores_in(others) {
        let apic_id = core.apic_id.load(Ordering::Relaxed);
        apic::send(Ipi::fixed(apic_id, SHOOTDOWN_VECTOR));
    }
    while PENDING.load(Ordering::Acquire) != 0 {
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
    PENDING.load(Ordering::Relaxed) != 0
}

/// Does this core's part of the current shootdown, if it has one. Called
/// by the shootdown interrupt and by spinning locks.
pub fn serve_pending() {
    let me = this_core().bit();
    if PENDING.load(Ordering::Acquire) & me == 0 {
        return;
    }
    cpu::flush_translation(REQUEST.load(Ordering::Relaxed));
    // Release: our flush is done before the sender sees our bit gone.
    PENDING.fetch_and(!me, Ordering::Release);
}
