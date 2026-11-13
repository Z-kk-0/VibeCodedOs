//! Bits of a page table entry (Intel SDM Vol. 3A, Table 4-19 / 4-20).

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
/// Ring 3 (user mode) may access. Needed on *every* level of the walk.
pub const USER: u64 = 1 << 2;
/// In a PD entry: this maps a 2 MiB page directly, there is no PT below.
pub const HUGE_PAGE: u64 = 1 << 7;
/// Instructions may not be fetched from here. Only valid if EFER.NXE is set.
pub const NO_EXECUTE: u64 = 1 << 63;
/// Bits 12–51 hold the physical address of the next table or the page.
pub const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// What a mapping allows. Reading is always allowed when present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Flags {
    pub writable: bool,
    pub executable: bool,
    pub user: bool,
}

impl Flags {
    pub const KERNEL_DATA: Flags = Flags {
        writable: true,
        executable: false,
        user: false,
    };

    /// Everything either of the two allows.
    pub fn union(self, other: Flags) -> Flags {
        Flags {
            writable: self.writable || other.writable,
            executable: self.executable || other.executable,
            user: self.user || other.user,
        }
    }

    pub(crate) fn to_bits(self, use_no_execute: bool) -> u64 {
        let mut bits = PRESENT;
        if self.writable {
            bits |= WRITABLE;
        }
        if self.user {
            bits |= USER;
        }
        if use_no_execute && !self.executable {
            bits |= NO_EXECUTE;
        }
        bits
    }

    pub(crate) fn from_bits(bits: u64) -> Flags {
        Flags {
            writable: bits & WRITABLE != 0,
            executable: bits & NO_EXECUTE == 0,
            user: bits & USER != 0,
        }
    }
}
