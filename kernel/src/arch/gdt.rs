//! Global Descriptor Table and Task State Segment.
//!
//! In 64-bit mode segmentation is mostly dead, but the CPU still insists on
//! a code segment (it says "64-bit, privilege level 0") and a TSS, which
//! holds the stacks the CPU switches to on interrupts.
//! Intel SDM Vol. 3A, §3.4.5 (descriptors) and §8.7 (64-bit TSS).

use core::arch::asm;
use core::mem::size_of;

/// Selectors = byte offsets into the GDT.
pub const KERNEL_CODE: u16 = 0x08;
const KERNEL_DATA: u16 = 0x10;
const TASK_STATE: u16 = 0x18;

/// Interrupt Stack Table slot (1-based) used for double faults.
pub const DOUBLE_FAULT_STACK_INDEX: u8 = 1;

// Descriptor bits (Intel SDM Vol. 3A, Figure 3-8).
const ACCESSED: u64 = 1 << 40;
const WRITABLE_OR_READABLE: u64 = 1 << 41;
const EXECUTABLE: u64 = 1 << 43;
const CODE_OR_DATA: u64 = 1 << 44;
const PRESENT: u64 = 1 << 47;
const LONG_MODE: u64 = 1 << 53;
const AVAILABLE_64_BIT_TSS: u64 = 0x9 << 40;

const KERNEL_CODE_DESCRIPTOR: u64 =
    PRESENT | CODE_OR_DATA | EXECUTABLE | WRITABLE_OR_READABLE | LONG_MODE | ACCESSED;
const KERNEL_DATA_DESCRIPTOR: u64 = PRESENT | CODE_OR_DATA | WRITABLE_OR_READABLE | ACCESSED;

#[repr(C, packed(4))]
struct TaskStateSegment {
    reserved_0: u32,
    /// Stacks for switching from user mode (ring 3) to rings 0–2. Unused yet.
    privilege_stacks: [u64; 3],
    reserved_1: u64,
    /// Up to 7 known-good stacks an IDT entry can ask for.
    interrupt_stacks: [u64; 7],
    reserved_2: u64,
    reserved_3: u16,
    /// Points past the end of the TSS = "no I/O permission bitmap".
    io_map_base: u16,
}

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
/// Null, kernel code, kernel data, TSS (takes two slots).
static mut GDT: [u64; 5] = [0; 5];

/// # Safety
/// Call once, with interrupts disabled.
pub unsafe fn init() {
    // SAFETY: single-threaded early boot; nothing else touches these statics.
    // The pointers stay valid forever because the statics never move.
    unsafe {
        let stack_top = (&raw const DOUBLE_FAULT_STACK) as u64 + DOUBLE_FAULT_STACK_SIZE as u64;
        let tss = &raw mut TSS;
        tss.write(TaskStateSegment::empty());
        (*tss).interrupt_stacks[usize::from(DOUBLE_FAULT_STACK_INDEX - 1)] = stack_top;

        let [tss_low, tss_high] = tss_descriptor(tss as u64);
        let gdt = &raw mut GDT;
        gdt.write([
            0,
            KERNEL_CODE_DESCRIPTOR,
            KERNEL_DATA_DESCRIPTOR,
            tss_low,
            tss_high,
        ]);

        load(gdt as u64, size_of::<[u64; 5]>());
    }
}

impl TaskStateSegment {
    const fn empty() -> Self {
        Self {
            reserved_0: 0,
            privilege_stacks: [0; 3],
            reserved_1: 0,
            interrupt_stacks: [0; 7],
            reserved_2: 0,
            reserved_3: 0,
            io_map_base: size_of::<TaskStateSegment>() as u16,
        }
    }
}

/// A TSS descriptor is 16 bytes: the base address is scattered over both halves.
fn tss_descriptor(base: u64) -> [u64; 2] {
    let limit = (size_of::<TaskStateSegment>() - 1) as u64;
    let low = limit
        | (base & 0xFF_FFFF) << 16
        | AVAILABLE_64_BIT_TSS
        | PRESENT
        | (base >> 24 & 0xFF) << 56;
    [low, base >> 32]
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
