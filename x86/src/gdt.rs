//! Segment descriptors and the Task State Segment.
//!
//! In 64-bit mode segmentation is mostly dead, but the CPU still insists on
//! a code segment (it says "64-bit, privilege level 0") and a TSS, which
//! holds the stacks the CPU switches to on interrupts.
//! Intel SDM Vol. 3A, §3.4.5 (descriptors) and §8.7 (64-bit TSS).

use core::mem::size_of;

// Descriptor bits (Intel SDM Vol. 3A, Figure 3-8).
const ACCESSED: u64 = 1 << 40;
const WRITABLE_OR_READABLE: u64 = 1 << 41;
const EXECUTABLE: u64 = 1 << 43;
const CODE_OR_DATA: u64 = 1 << 44;
const PRESENT: u64 = 1 << 47;
const LONG_MODE: u64 = 1 << 53;
const AVAILABLE_64_BIT_TSS: u64 = 0x9 << 40;
/// Descriptor privilege level 3: usable from user mode.
const USER_PRIVILEGE: u64 = 3 << 45;

/// Base and limit are ignored in 64-bit mode, so they stay 0. The accessed
/// bit is preset so the CPU never needs to write to the GDT.
pub const KERNEL_CODE_DESCRIPTOR: u64 =
    PRESENT | CODE_OR_DATA | EXECUTABLE | WRITABLE_OR_READABLE | LONG_MODE | ACCESSED;
pub const KERNEL_DATA_DESCRIPTOR: u64 = PRESENT | CODE_OR_DATA | WRITABLE_OR_READABLE | ACCESSED;
pub const USER_CODE_DESCRIPTOR: u64 = KERNEL_CODE_DESCRIPTOR | USER_PRIVILEGE;
pub const USER_DATA_DESCRIPTOR: u64 = KERNEL_DATA_DESCRIPTOR | USER_PRIVILEGE;

/// A selector's lowest two bits: the privilege level it is used with.
pub const USER_REQUESTED_PRIVILEGE: u16 = 3;

#[repr(C, packed(4))]
pub struct TaskStateSegment {
    reserved_0: u32,
    /// Stacks the CPU switches to when an interrupt arrives in user mode
    /// (ring 3); index = target ring. We only use ring 0.
    privilege_stacks: [u64; 3],
    reserved_1: u64,
    /// Up to 7 known-good stacks an IDT entry can ask for (IST 1–7).
    interrupt_stacks: [u64; 7],
    reserved_2: u64,
    reserved_3: u16,
    /// Points past the end of the TSS = "no I/O permission bitmap".
    io_map_base: u16,
}

impl TaskStateSegment {
    pub const fn empty() -> Self {
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

    /// The kernel stack to use when an interrupt arrives in user mode.
    pub fn set_kernel_stack(&mut self, stack_top: u64) {
        self.privilege_stacks[0] = stack_top;
    }

    /// `index` is 1-based, as in IDT entries (0 there means "no switch").
    pub fn set_interrupt_stack(&mut self, index: u8, stack_top: u64) {
        assert!((1..=7).contains(&index), "IST index must be 1..=7");
        self.interrupt_stacks[usize::from(index - 1)] = stack_top;
    }
}

/// A TSS descriptor is 16 bytes: the base address is scattered over both halves.
pub fn tss_descriptor(tss_address: u64) -> [u64; 2] {
    let limit = (size_of::<TaskStateSegment>() - 1) as u64;
    let low = limit
        | (tss_address & 0xFF_FFFF) << 16
        | AVAILABLE_64_BIT_TSS
        | PRESENT
        | (tss_address >> 24 & 0xFF) << 56;
    [low, tss_address >> 32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_data_descriptors_match_the_well_known_values() {
        // Access byte 0x9B / 0x93, flags nibble 0x2 (= long mode) for code.
        assert_eq!(KERNEL_CODE_DESCRIPTOR, 0x0020_9B00_0000_0000);
        assert_eq!(KERNEL_DATA_DESCRIPTOR, 0x0000_9300_0000_0000);
        // Same with privilege level 3: access bytes 0xFB / 0xF3.
        assert_eq!(USER_CODE_DESCRIPTOR, 0x0020_FB00_0000_0000);
        assert_eq!(USER_DATA_DESCRIPTOR, 0x0000_F300_0000_0000);
    }

    #[test]
    fn kernel_stack_goes_into_the_ring_0_slot() {
        let mut tss = TaskStateSegment::empty();
        tss.set_kernel_stack(0x1234_5000);
        assert_eq!({ tss.privilege_stacks }[0], 0x1234_5000);
    }

    #[test]
    fn task_state_segment_has_the_hardware_size() {
        assert_eq!(size_of::<TaskStateSegment>(), 104);
        assert_eq!({ TaskStateSegment::empty().io_map_base }, 104);
    }

    #[test]
    fn tss_descriptor_scatters_the_base_address() {
        let [low, high] = tss_descriptor(0x1122_3344_5566_7788);
        assert_eq!(low, 0x5500_8966_7788_0067);
        assert_eq!(high, 0x1122_3344);
    }

    #[test]
    fn interrupt_stack_index_is_one_based() {
        let mut tss = TaskStateSegment::empty();
        tss.set_interrupt_stack(1, 0xABCD);
        assert_eq!({ tss.interrupt_stacks }[0], 0xABCD);
    }

    #[test]
    #[should_panic]
    fn interrupt_stack_index_zero_is_rejected() {
        TaskStateSegment::empty().set_interrupt_stack(0, 0xABCD);
    }
}
