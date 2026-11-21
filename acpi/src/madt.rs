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
/// One I/O APIC: where it is, which interrupt lines it serves (§5.2.12.3).
const IO_APIC: u8 = 1;
/// An old ISA interrupt that arrives on another line, or with another
/// polarity/trigger than ISA's "edge, active high" (§5.2.12.5).
const SOURCE_OVERRIDE: u8 = 2;
/// One core with a 32-bit APIC ID, used beyond 255 cores (§5.2.12.12).
const LOCAL_X2APIC: u8 = 9;
/// In both core entry kinds' flags: the core is usable right now.
const ENABLED: u32 = 1 << 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Core {
    /// What inter-processor interrupts are addressed to.
    pub apic_id: u32,
}

/// An I/O APIC: it turns device interrupt lines into messages to cores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IoApic {
    /// Physical address of its two registers (select, window).
    pub address: u32,
    /// The global system interrupt ("GSI") its first input is.
    pub first_line: u32,
}

/// Where an ISA interrupt really arrives (§5.2.12.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourceOverride {
    /// The ISA IRQ (0–15).
    pub irq: u8,
    /// The global system interrupt it arrives on.
    pub line: u32,
    /// MPS INTI flags: bits 0–1 polarity, bits 2–3 trigger mode.
    pub flags: u16,
}

/// How the machine's interrupt lines are wired (the MADT's other entries).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct InterruptWiring {
    pub io_apics: Vec<IoApic>,
    pub overrides: Vec<SourceOverride>,
}

pub fn cores(madt: &[u8]) -> Vec<Core> {
    let mut cores = Vec::new();
    for (kind, entry) in entries(madt) {
        let core = match kind {
            LOCAL_APIC if entry.len() >= 8 => Some((u32::from(entry[3]), read_u32(entry, 4))),
            LOCAL_X2APIC if entry.len() >= 16 => Some((read_u32(entry, 4), read_u32(entry, 8))),
            _ => None,
        };
        if let Some((apic_id, flags)) = core
            && flags & ENABLED != 0
        {
            cores.push(Core { apic_id });
        }
    }
    cores
}

pub fn wiring(madt: &[u8]) -> InterruptWiring {
    let mut wiring = InterruptWiring::default();
    for (kind, entry) in entries(madt) {
        match kind {
            IO_APIC if entry.len() >= 12 => wiring.io_apics.push(IoApic {
                address: read_u32(entry, 4),
                first_line: read_u32(entry, 8),
            }),
            // Bus 0 is ISA, the only bus this entry is defined for.
            SOURCE_OVERRIDE if entry.len() >= 10 && entry[2] == 0 => {
                wiring.overrides.push(SourceOverride {
                    irq: entry[3],
                    line: read_u32(entry, 4),
                    flags: u16::from_le_bytes([entry[8], entry[9]]),
                });
            }
            _ => {}
        }
    }
    wiring
}

/// Every well-formed entry as (type, whole entry), in order.
fn entries(madt: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut offset = ENTRIES_START;
    core::iter::from_fn(move || {
        if offset + 2 > madt.len() {
            return None;
        }
        let (kind, length) = (madt[offset], usize::from(madt[offset + 1]));
        if length < 2 || offset + length > madt.len() {
            return None; // a broken entry: don't trust anything after it
        }
        let entry = &madt[offset..offset + length];
        offset += length;
        Some((kind, entry))
    })
}
