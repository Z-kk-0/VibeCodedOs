//! Bits of a page table entry (Intel SDM Vol. 3A, Table 4-19 / 4-20).

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
/// Ring 3 (user mode) may access. Needed on *every* level of the walk.
pub const USER: u64 = 1 << 2;
/// "Page-level write-through" and "page-level cache disable". Together with
/// the PAT bit they pick one of the eight entries of the CPU's page
/// attribute table (SDM Vol. 3A, §11.12.3). Same position in 4 KiB and
/// 2 MiB entries – unlike the PAT bit itself, which we never use.
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const CACHE_DISABLE: u64 = 1 << 4;
/// In a PD entry: this maps a 2 MiB page directly, there is no PT below.
pub const HUGE_PAGE: u64 = 1 << 7;
/// Instructions may not be fetched from here. Only valid if EFER.NXE is set.
pub const NO_EXECUTE: u64 = 1 << 63;
/// Bits 12–51 hold the physical address of the next table or the page.
pub const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// How the CPU caches a page – RAM wants caching, device memory doesn't
/// (DECISIONS.md, 023). Encoded in the PWT/PCD bits; which memory type each
/// combination means is set by the page attribute table, see `x86::pat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Caching {
    /// Normal RAM: reads and writes go through the caches (PAT entry 0).
    #[default]
    WriteBack,
    /// For framebuffers: never cached, but writes are collected and sent
    /// in bursts (PAT entry 1 – only once the kernel programmed the PAT).
    WriteCombining,
    /// Device registers: every access goes to the device, in order (PAT entry 3).
    Uncached,
}

impl Caching {
    fn to_bits(self) -> u64 {
        match self {
            Caching::WriteBack => 0,
            Caching::WriteCombining => WRITE_THROUGH,
            Caching::Uncached => WRITE_THROUGH | CACHE_DISABLE,
        }
    }

    fn from_bits(bits: u64) -> Caching {
        match (bits & WRITE_THROUGH != 0, bits & CACHE_DISABLE != 0) {
            (false, false) => Caching::WriteBack,
            (true, false) => Caching::WriteCombining,
            // PAT entry 2 (PCD only) is never used; it is "uncached" too.
            (_, true) => Caching::Uncached,
        }
    }
}

/// What a mapping allows, and how it is cached. Reading is always allowed
/// when present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Flags {
    pub writable: bool,
    pub executable: bool,
    pub user: bool,
    pub caching: Caching,
}

impl Flags {
    pub const KERNEL_DATA: Flags = Flags {
        writable: true,
        executable: false,
        user: false,
        caching: Caching::WriteBack,
    };

    /// Everything either of the two allows. Caching isn't a permission:
    /// both must agree on it (the mapper refuses a mix).
    pub fn union(self, other: Flags) -> Flags {
        Flags {
            writable: self.writable || other.writable,
            executable: self.executable || other.executable,
            user: self.user || other.user,
            caching: self.caching,
        }
    }

    pub(crate) fn to_bits(self, use_no_execute: bool) -> u64 {
        let mut bits = PRESENT | self.caching.to_bits();
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
            caching: Caching::from_bits(bits),
        }
    }
}
