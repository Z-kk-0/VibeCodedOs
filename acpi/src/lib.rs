//! Just enough ACPI to find the machine's CPU cores and how its device
//! interrupts are wired.
//!
//! The firmware describes the hardware in a tree of tables in memory:
//! the root pointer (RSDP) leads to a list of tables (XSDT, or the older
//! RSDT), one of which is the MADT ("APIC" table) listing every core's
//! local APIC and its I/O APICs. ACPI spec 6.5: §5.2.5 (RSDP), §5.2.6
//! (header), §5.2.7/8 (RSDT/XSDT), §5.2.12 (MADT).
//!
//! Pure parsing over a `PhysicalMemory` trait, so it's tested on the host.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod bytes;
mod madt;
#[cfg(test)]
mod tests;

use alloc::vec::Vec;
use bytes::{checksum_ok, read_u32, read_u64};

pub use madt::{Core, InterruptWiring, IoApic, SourceOverride};

/// Reads physical memory: the kernel goes through its window, tests
/// through a byte array.
pub trait PhysicalMemory {
    /// `length` bytes starting at physical `address`, or `None` if that
    /// range isn't readable.
    fn bytes(&self, address: u64, length: usize) -> Option<&[u8]>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// The root pointer's signature or checksum is wrong.
    BadRootPointer,
    /// A table (named by its signature) has a wrong checksum or length.
    BadTable([u8; 4]),
    /// No MADT: the firmware tells us nothing about other cores.
    NoMadt,
    /// A table points outside readable memory.
    Unreadable,
}

const ROOT_POINTER_SIGNATURE: &[u8; 8] = b"RSD PTR ";
/// ACPI 1.0 root pointer size; 2.0+ adds a length field and the XSDT.
const ROOT_POINTER_V1_SIZE: usize = 20;
const ROOT_POINTER_V2_SIZE: usize = 36;
const HEADER_SIZE: usize = 36;

/// All enabled cores the MADT lists, in its order (the first one is
/// usually, but not necessarily, the core we boot on).
pub fn find_cores(root_pointer: u64, memory: &impl PhysicalMemory) -> Result<Vec<Core>, Error> {
    let madt = find_table(root_pointer, memory, *b"APIC")?.ok_or(Error::NoMadt)?;
    Ok(madt::cores(madt))
}

/// The I/O APICs and ISA interrupt overrides the MADT lists.
pub fn find_interrupt_wiring(
    root_pointer: u64,
    memory: &impl PhysicalMemory,
) -> Result<InterruptWiring, Error> {
    let madt = find_table(root_pointer, memory, *b"APIC")?.ok_or(Error::NoMadt)?;
    Ok(madt::wiring(madt))
}

/// Walks the root pointer and the table list to the table named `signature`.
fn find_table(
    root_pointer: u64,
    memory: &impl PhysicalMemory,
    signature: [u8; 4],
) -> Result<Option<&[u8]>, Error> {
    let v1 = memory
        .bytes(root_pointer, ROOT_POINTER_V1_SIZE)
        .ok_or(Error::Unreadable)?;
    if &v1[..8] != ROOT_POINTER_SIGNATURE || !checksum_ok(v1) {
        return Err(Error::BadRootPointer);
    }
    let revision = v1[15];
    // Revision 0 = ACPI 1.0 (32-bit RSDT entries), 2+ = XSDT (64-bit entries).
    let (list_address, entry_size) = if revision >= 2 {
        let v2 = memory
            .bytes(root_pointer, ROOT_POINTER_V2_SIZE)
            .ok_or(Error::Unreadable)?;
        if !checksum_ok(v2) {
            return Err(Error::BadRootPointer);
        }
        (read_u64(v2, 24), 8)
    } else {
        (u64::from(read_u32(v1, 16)), 4)
    };

    let list = table(list_address, memory)?;
    for entry in list[HEADER_SIZE..].chunks_exact(entry_size) {
        let address = if entry_size == 8 {
            read_u64(entry, 0)
        } else {
            u64::from(read_u32(entry, 0))
        };
        let candidate = table(address, memory)?;
        if candidate[..4] == signature {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// A whole table (header included), length and checksum checked.
fn table(address: u64, memory: &impl PhysicalMemory) -> Result<&[u8], Error> {
    let header = memory
        .bytes(address, HEADER_SIZE)
        .ok_or(Error::Unreadable)?;
    let signature = [header[0], header[1], header[2], header[3]];
    let length = read_u32(header, 4) as usize;
    if length < HEADER_SIZE {
        return Err(Error::BadTable(signature));
    }
    let whole = memory.bytes(address, length).ok_or(Error::Unreadable)?;
    if !checksum_ok(whole) {
        return Err(Error::BadTable(signature));
    }
    Ok(whole)
}
