//! The page attribute table (PAT): eight memory types that page table
//! entries pick with their PAT/PCD/PWT bits (Intel SDM Vol. 3A, §11.12).
//!
//! After a reset the table holds WB, WT, UC-, UC, WB, WT, UC-, UC – there is
//! no write-combining in it. Hematite swaps entry 1 (PWT only) for WC and
//! leaves everything else alone: entry 0 stays WB for RAM, entry 3 UC for
//! device registers, and nothing set up before the swap used entry 1.
//! The table must be the same on every core (SDM §11.12.4).

/// The MSR that holds the table (SDM Vol. 4, "IA32_PAT").
pub const MSR: u32 = 0x277;
/// CPUID leaf 1, EDX bit 16: the CPU has a PAT.
pub const CPUID_1_EDX_PAT: u32 = 1 << 16;

/// Memory type encodings (SDM Vol. 3A, Table 11-10).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryType {
    Uncacheable = 0x00,
    WriteCombining = 0x01,
    WriteThrough = 0x04,
    WriteProtected = 0x05,
    WriteBack = 0x06,
    /// "UC-": uncached, but an MTRR may still turn it into WC.
    UncachedMinus = 0x07,
}

/// The table after a reset.
pub const POWER_ON: [MemoryType; 8] = {
    use MemoryType::*;
    [
        WriteBack,
        WriteThrough,
        UncachedMinus,
        Uncacheable,
        WriteBack,
        WriteThrough,
        UncachedMinus,
        Uncacheable,
    ]
};

/// Hematite's table: the power-on one with entry 1 as write-combining. The
/// `paging` crate's `Caching` encodings rely on exactly this layout.
pub const HEMATITE: [MemoryType; 8] = {
    let mut table = POWER_ON;
    table[1] = MemoryType::WriteCombining;
    table
};

/// The MSR value for a table: entry n in byte n.
pub const fn msr_value(table: [MemoryType; 8]) -> u64 {
    let mut value = 0;
    let mut index = 0;
    while index < 8 {
        value |= (table[index] as u64) << (8 * index);
        index += 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_on_value_matches_the_manual() {
        // SDM Vol. 3A, Table 11-12: "0007040600070406H".
        assert_eq!(msr_value(POWER_ON), 0x0007_0406_0007_0406);
    }

    #[test]
    fn only_entry_one_changes() {
        assert_eq!(msr_value(HEMATITE), 0x0007_0406_0007_0106);
    }
}
