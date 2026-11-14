//! The MADT ("Multiple APIC Description Table"): after a fixed part, a
//! list of variable-length entries, each starting with `type, length`.
//! ACPI spec 6.5, §5.2.12.

use super::HEADER_SIZE;
use super::bytes::read_u32;
use alloc::vec::Vec;

/// Header + local APIC address (u32) + flags (u32).
const ENTRIES_START: usize = HEADER_SIZE + 8;
/// One core with an 8-bit APIC ID (§5.2.12.2).
const LOCAL_APIC: u8 = 0;
/// One core with a 32-bit APIC ID, used beyond 255 cores (§5.2.12.12).
const LOCAL_X2APIC: u8 = 9;
/// In both entry kinds' flags: the core is usable right now.
const ENABLED: u32 = 1 << 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Core {
    /// What inter-processor interrupts are addressed to.
    pub apic_id: u32,
}

pub fn cores(madt: &[u8]) -> Vec<Core> {
    let mut cores = Vec::new();
    let mut offset = ENTRIES_START;
    while offset + 2 <= madt.len() {
        let (kind, length) = (madt[offset], usize::from(madt[offset + 1]));
        if length < 2 || offset + length > madt.len() {
            break; // a broken entry: don't trust anything after it
        }
        let entry = &madt[offset..offset + length];
        let core = match kind {
            LOCAL_APIC if length >= 8 => Some((u32::from(entry[3]), read_u32(entry, 4))),
            LOCAL_X2APIC if length >= 16 => Some((read_u32(entry, 4), read_u32(entry, 8))),
            _ => None,
        };
        if let Some((apic_id, flags)) = core
            && flags & ENABLED != 0
        {
            cores.push(Core { apic_id });
        }
        offset += length;
    }
    cores
}
