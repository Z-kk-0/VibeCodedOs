//! The Global Descriptor Table and Task State Segment – one of each per
//! core, inside the core's `PerCore` (which is how a core finds its own
//! data, see `per_core.rs`). The descriptor formats live in the `x86`
//! crate; this module only decides the layout and loads it into the CPU.

use super::per_core::{PerCore, this_core};
use core::arch::asm;
use core::mem::size_of;
use x86::gdt::{
    KERNEL_CODE_DESCRIPTOR, KERNEL_DATA_DESCRIPTOR, USER_CODE_DESCRIPTOR, USER_DATA_DESCRIPTOR,
    USER_REQUESTED_PRIVILEGE, tss_descriptor,
};

/// Selectors = byte offsets into the GDT. User data comes before user code:
/// the order the `sysret` instruction expects, should we use it one day.
pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
pub const USER_DATA: u16 = 0x18 | USER_REQUESTED_PRIVILEGE;
pub const USER_CODE: u16 = 0x20 | USER_REQUESTED_PRIVILEGE;
const TASK_STATE: u16 = 0x28;
/// Null, kernel code + data, user data + code, TSS (takes two slots).
pub const GDT_ENTRIES: usize = 7;

/// Interrupt Stack Table slot (1-based) used for double faults.
pub const DOUBLE_FAULT_STACK_INDEX: u8 = 1;

/// Fills `core`'s GDT and TSS and loads them into this CPU. A double fault
/// often means the normal stack is broken (e.g. overflowed), so its handler
/// runs on `double_fault_stack_top`, a stack of its own.
///
/// # Safety
/// Call once per core, on that core, with interrupts disabled;
/// `double_fault_stack_top` must be the top of a stack that lives forever.
pub unsafe fn init(core: &'static PerCore, double_fault_stack_top: u64) {
    // SAFETY: nothing else touches this core's GDT and TSS yet; they live
    // forever, so the CPU may keep pointing at them.
    unsafe {
        let tss = core.tss.get();
        (*tss).set_interrupt_stack(DOUBLE_FAULT_STACK_INDEX, double_fault_stack_top);

        let [tss_low, tss_high] = tss_descriptor(tss as u64);
        let gdt = core.gdt.get();
        gdt.write([
            0,
            KERNEL_CODE_DESCRIPTOR,
            KERNEL_DATA_DESCRIPTOR,
            USER_DATA_DESCRIPTOR,
            USER_CODE_DESCRIPTOR,
            tss_low,
            tss_high,
        ]);

        load(gdt as u64, size_of::<[u64; GDT_ENTRIES]>());
    }
}

/// Tells the CPU which stack to switch to when an interrupt arrives while
/// user code runs – the kernel stack of the task about to run.
///
/// # Safety
/// Interrupts must be off (the scheduler calls this mid-switch), and
/// `stack_top` must be the top of a valid kernel stack.
pub unsafe fn set_kernel_stack(stack_top: u64) {
    // SAFETY: it's this core's own TSS, and with interrupts off nobody else
    // touches it; the CPU only reads this field on the next ring 3 → ring 0
    // transition.
    unsafe { (*this_core().tss.get()).set_kernel_stack(stack_top) };
}

/// Activates the GDT, reloads every segment register, loads the TSS.
unsafe fn load(gdt_address: u64, gdt_size: usize) {
    #[repr(C, packed)]
    struct Pointer {
        limit: u16,
        base: u64,
    }
    let pointer = Pointer {
        limit: (gdt_size - 1) as u16,
        base: gdt_address,
    };

    // SAFETY: the GDT is valid and lives forever. CS can't be set with `mov`,
    // so we fake a far call return: push selector + address, then `retfq`.
    unsafe {
        asm!(
            "lgdt [{pointer}]",
            "push {code}",
            "lea {scratch}, [rip + 2f]",
            "push {scratch}",
            "retfq",
            "2:",
            "mov ds, {data:x}",
            "mov es, {data:x}",
            "mov fs, {data:x}",
            "mov gs, {data:x}",
            "mov ss, {data:x}",
            "ltr {tss:x}",
            pointer = in(reg) &pointer,
            code = in(reg) u64::from(KERNEL_CODE),
            data = in(reg) KERNEL_DATA,
            tss = in(reg) TASK_STATE,
            // `out`, not `lateout`: it is written while inputs are still needed.
            scratch = out(reg) _,
        );
    }
}
