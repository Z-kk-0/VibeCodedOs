//! Our Global Descriptor Table and Task State Segment.
//! The descriptor formats live in the `x86` crate; this module only decides
//! the layout and loads it into the CPU.

use core::arch::asm;
use core::mem::size_of;
use x86::gdt::{
    KERNEL_CODE_DESCRIPTOR, KERNEL_DATA_DESCRIPTOR, TaskStateSegment, USER_CODE_DESCRIPTOR,
    USER_DATA_DESCRIPTOR, USER_REQUESTED_PRIVILEGE, tss_descriptor,
};

/// Selectors = byte offsets into the GDT. User data comes before user code:
/// the order the `sysret` instruction expects, should we use it one day.
pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
pub const USER_DATA: u16 = 0x18 | USER_REQUESTED_PRIVILEGE;
pub const USER_CODE: u16 = 0x20 | USER_REQUESTED_PRIVILEGE;
const TASK_STATE: u16 = 0x28;
const GDT_ENTRIES: usize = 7;

/// Interrupt Stack Table slot (1-based) used for double faults.
pub const DOUBLE_FAULT_STACK_INDEX: u8 = 1;

const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
#[allow(
    dead_code,
    reason = "only the CPU uses this memory, Rust never reads it"
)]
struct Stack([u8; DOUBLE_FAULT_STACK_SIZE]);

/// A double fault often means our normal stack is broken (e.g. overflowed),
/// so its handler gets a stack of its own.
static mut DOUBLE_FAULT_STACK: Stack = Stack([0; DOUBLE_FAULT_STACK_SIZE]);
static mut TSS: TaskStateSegment = TaskStateSegment::empty();
/// Null, kernel code + data, user data + code, TSS (takes two slots).
static mut GDT: [u64; GDT_ENTRIES] = [0; GDT_ENTRIES];

/// # Safety
/// Call once, with interrupts disabled.
pub unsafe fn init() {
    // SAFETY: single-threaded early boot; nothing else touches these statics.
    // The pointers stay valid forever because the statics never move.
    unsafe {
        let stack_top = (&raw const DOUBLE_FAULT_STACK) as u64 + DOUBLE_FAULT_STACK_SIZE as u64;
        let tss = &raw mut TSS;
        (*tss).set_interrupt_stack(DOUBLE_FAULT_STACK_INDEX, stack_top);

        let [tss_low, tss_high] = tss_descriptor(tss as u64);
        let gdt = &raw mut GDT;
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
    // SAFETY: with interrupts off nobody else touches the TSS; the CPU only
    // reads this field on the next ring 3 → ring 0 transition.
    let tss = &raw mut TSS;
    // SAFETY: see above.
    unsafe { (*tss).set_kernel_stack(stack_top) };
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
