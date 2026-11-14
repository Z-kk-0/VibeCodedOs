//! What every CPU core has for itself – and how a core finds its own
//! (DECISIONS.md, 019).
//!
//! "The GDT as an ID card": each core gets its own GDT, and that GDT is the
//! first field of the core's `PerCore`. `sgdt` tells the CPU's GDT address
//! – so it tells us which core we're on, without `swapgs` and without a
//! special case on the interrupt path. Only ring 0 can change the GDT
//! register (`lgdt`), so a user program can't fake its identity.

use super::gdt::GDT_ENTRIES;
use core::arch::asm;
use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use x86::gdt::TaskStateSegment;

/// A set of cores, one bit per core index. 64 is plenty for now.
pub type CoreMask = u64;
pub const MAX_CORES: usize = CoreMask::BITS as usize;

#[repr(C)]
pub struct PerCore {
    /// Must stay the first field: its address is the `PerCore`'s address.
    pub(super) gdt: UnsafeCell<[u64; GDT_ENTRIES]>,
    pub(super) tss: UnsafeCell<TaskStateSegment>,
    /// 0 for the core we booted on, then 1, 2, … in start order.
    pub index: usize,
    pub apic_id: AtomicU32,
    /// After a task switch: the flag the interrupt stub clears once this
    /// core is off the previous task's stack (see `interrupts/stubs.rs`).
    release_after_switch: AtomicPtr<AtomicBool>,
}

// SAFETY: the `UnsafeCell`s are only touched by their own core, with
// interrupts off (GDT/TSS setup, `set_kernel_stack`); all else is atomic.
unsafe impl Sync for PerCore {}

/// Cleared after every interrupt that didn't switch tasks. Nobody reads it.
static NOTHING_TO_RELEASE: AtomicBool = AtomicBool::new(false);

/// The boot core's data: it's needed before the heap exists.
pub(super) static BOOT_CORE: PerCore = PerCore::new(0);

static CORES: [AtomicPtr<PerCore>; MAX_CORES] = [const { AtomicPtr::new(null_mut()) }; MAX_CORES];
static CORE_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Cores that finished starting and take part in scheduling and shootdowns.
static ONLINE: AtomicU64 = AtomicU64::new(0);

impl PerCore {
    pub const fn new(index: usize) -> Self {
        Self {
            gdt: UnsafeCell::new([0; GDT_ENTRIES]),
            tss: UnsafeCell::new(TaskStateSegment::empty()),
            index,
            apic_id: AtomicU32::new(0),
            release_after_switch: AtomicPtr::new(null_mut()),
        }
    }

    pub fn bit(&self) -> CoreMask {
        1 << self.index
    }
}

/// The core this code runs on. Only valid once this core loaded its own
/// GDT (`gdt::init`) – in practice: anywhere after `arch::init`.
pub fn this_core() -> &'static PerCore {
    #[repr(C, packed)]
    struct Pointer {
        limit: u16,
        base: u64,
    }
    let mut pointer = Pointer { limit: 0, base: 0 };
    // SAFETY: `sgdt` only writes the 10 bytes of `pointer`.
    unsafe { asm!("sgdt [{}]", in(reg) &mut pointer, options(nostack, preserves_flags)) };
    let base = pointer.base;
    // SAFETY: every core's GDT is the first field of a `PerCore` that lives
    // forever (`BOOT_CORE` or leaked), and `#[repr(C)]` puts it at offset 0.
    unsafe { &*(base as *const PerCore) }
}

pub fn this_core_index() -> usize {
    this_core().index
}

/// Makes `core` known (but not yet online). Cores register in index order;
/// a core that failed to start is replaced by the next one under its index.
pub fn register(core: &'static PerCore) {
    let index = core.index;
    let count = CORE_COUNT.load(Ordering::Acquire);
    assert!(index <= count, "cores must register in order");
    assert!(index < MAX_CORES, "more than {MAX_CORES} cores");
    CORES[index].store(core as *const PerCore as *mut PerCore, Ordering::Release);
    if index == count {
        CORE_COUNT.store(count + 1, Ordering::Release);
    }
}

pub fn set_online(core: &PerCore) {
    ONLINE.fetch_or(core.bit(), Ordering::SeqCst);
}

pub fn online_cores() -> CoreMask {
    ONLINE.load(Ordering::SeqCst)
}

pub fn online_count() -> usize {
    online_cores().count_ones() as usize
}

/// The core with this index. Panics for cores that never registered.
pub fn core(index: usize) -> &'static PerCore {
    let pointer = CORES[index].load(Ordering::Acquire);
    assert!(!pointer.is_null(), "core {index} is not registered");
    // SAFETY: registered pointers come from `&'static PerCore`.
    unsafe { &*pointer }
}

/// Every core in `mask`.
pub fn cores_in(mask: CoreMask) -> impl Iterator<Item = &'static PerCore> {
    (0..MAX_CORES)
        .filter(move |index| mask & (1 << index) != 0)
        .map(core)
}

/// Remembers `flag` to be cleared once this core is off the old stack.
pub fn release_after_switch(flag: &AtomicBool) {
    let pointer = flag as *const AtomicBool as *mut AtomicBool;
    this_core()
        .release_after_switch
        .store(pointer, Ordering::Relaxed);
}

/// The flag for the interrupt stub to clear, and forget it. A harmless
/// dummy if no task switch happened.
pub fn take_release_flag() -> *const AtomicBool {
    let pointer = this_core()
        .release_after_switch
        .swap(null_mut(), Ordering::Relaxed);
    if pointer.is_null() {
        &NOTHING_TO_RELEASE
    } else {
        pointer
    }
}

pub fn set_offline(core: &PerCore) {
    ONLINE.fetch_and(!core.bit(), Ordering::SeqCst);
}
