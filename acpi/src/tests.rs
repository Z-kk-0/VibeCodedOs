use super::*;
use alloc::vec;

/// Fake physical memory: one byte array starting at physical address 0.
struct Memory(Vec<u8>);

impl PhysicalMemory for Memory {
    fn bytes(&self, address: u64, length: usize) -> Option<&[u8]> {
        let start = usize::try_from(address).ok()?;
        self.0.get(start..start.checked_add(length)?)
    }
}

impl Memory {
    fn put(&mut self, address: usize, bytes: &[u8]) {
        if self.0.len() < address + bytes.len() {
            self.0.resize(address + bytes.len(), 0);
        }
        self.0[address..address + bytes.len()].copy_from_slice(bytes);
    }
}

/// Makes all bytes add up to 0 by adjusting the byte at `checksum_offset`.
fn fix_checksum(bytes: &mut [u8], checksum_offset: usize) {
    bytes[checksum_offset] = 0;
    let sum = bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    bytes[checksum_offset] = 0u8.wrapping_sub(sum);
}

/// A table with a standard header and `body` after it.
fn table(signature: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[..4].copy_from_slice(signature);
    bytes.extend_from_slice(body);
    let length = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&length.to_le_bytes());
    fix_checksum(&mut bytes, 9);
    bytes
}

fn madt(entries: &[&[u8]]) -> Vec<u8> {
    let mut body = vec![0; 8]; // local APIC address + flags
    for entry in entries {
        body.extend_from_slice(entry);
    }
    table(b"APIC", &body)
}

fn local_apic(apic_id: u8, flags: u32) -> Vec<u8> {
    let mut entry = vec![0, 8, apic_id, apic_id];
    entry.extend_from_slice(&flags.to_le_bytes());
    entry
}

fn root_pointer_v2(xsdt: u64) -> Vec<u8> {
    let mut bytes = vec![0; ROOT_POINTER_V2_SIZE];
    bytes[..8].copy_from_slice(ROOT_POINTER_SIGNATURE);
    bytes[15] = 2;
    bytes[20..24].copy_from_slice(&(ROOT_POINTER_V2_SIZE as u32).to_le_bytes());
    bytes[24..32].copy_from_slice(&xsdt.to_le_bytes());
    fix_checksum(&mut bytes[..ROOT_POINTER_V1_SIZE], 8);
    fix_checksum(&mut bytes, 32);
    bytes
}

fn root_pointer_v1(rsdt: u32) -> Vec<u8> {
    let mut bytes = vec![0; ROOT_POINTER_V1_SIZE];
    bytes[..8].copy_from_slice(ROOT_POINTER_SIGNATURE);
    bytes[16..20].copy_from_slice(&rsdt.to_le_bytes());
    fix_checksum(&mut bytes, 8);
    bytes
}

/// RSDP at 0x100, XSDT at 0x200 listing a dummy table (0x300) and the MADT (0x400).
fn machine_v2(madt: &[u8]) -> Memory {
    let mut memory = Memory(Vec::new());
    memory.put(0x100, &root_pointer_v2(0x200));
    let mut list = Vec::new();
    list.extend_from_slice(&0x300u64.to_le_bytes());
    list.extend_from_slice(&0x400u64.to_le_bytes());
    memory.put(0x200, &table(b"XSDT", &list));
    memory.put(0x300, &table(b"FACP", &[1, 2, 3]));
    memory.put(0x400, madt);
    memory
}

#[test]
fn finds_enabled_cores_through_the_xsdt() {
    let madt = madt(&[
        &local_apic(0, 1),
        &local_apic(1, 1),
        &local_apic(2, 0),                      // disabled: must be skipped
        &[1, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // an I/O APIC: not a core
        &local_apic(3, 1),
    ]);
    let cores = find_cores(0x100, &machine_v2(&madt)).unwrap();
    let ids: Vec<u32> = cores.iter().map(|core| core.apic_id).collect();
    assert_eq!(ids, [0, 1, 3]);
}

#[test]
fn reads_x2apic_entries() {
    let mut x2apic = vec![9, 16, 0, 0];
    x2apic.extend_from_slice(&300u32.to_le_bytes()); // APIC ID
    x2apic.extend_from_slice(&1u32.to_le_bytes()); // enabled
    x2apic.extend_from_slice(&0u32.to_le_bytes()); // processor UID
    let cores = find_cores(0x100, &machine_v2(&madt(&[&x2apic]))).unwrap();
    assert_eq!(cores, [Core { apic_id: 300 }]);
}

#[test]
fn finds_cores_through_the_old_rsdt() {
    let mut memory = Memory(Vec::new());
    memory.put(0x100, &root_pointer_v1(0x200));
    memory.put(0x200, &table(b"RSDT", &0x400u32.to_le_bytes()));
    memory.put(0x400, &madt(&[&local_apic(7, 1)]));
    assert_eq!(find_cores(0x100, &memory).unwrap(), [Core { apic_id: 7 }]);
}

#[test]
fn rejects_a_broken_root_pointer() {
    let mut memory = machine_v2(&madt(&[]));
    memory.0[0x100 + 9] ^= 0xFF; // corrupt an OEM ID byte
    assert_eq!(find_cores(0x100, &memory), Err(Error::BadRootPointer));
    assert_eq!(find_cores(0x0, &memory), Err(Error::BadRootPointer));
}

#[test]
fn rejects_a_table_with_a_wrong_checksum() {
    let mut memory = machine_v2(&madt(&[&local_apic(0, 1)]));
    memory.0[0x400 + HEADER_SIZE] ^= 1;
    assert_eq!(find_cores(0x100, &memory), Err(Error::BadTable(*b"APIC")));
}

#[test]
fn reports_a_missing_madt() {
    let mut memory = machine_v2(&table(b"HPET", &[]));
    memory.put(0x400, &table(b"HPET", &[]));
    assert_eq!(find_cores(0x100, &memory), Err(Error::NoMadt));
}

#[test]
fn stops_at_a_broken_entry() {
    let madt = madt(&[&local_apic(0, 1), &[0, 0], &local_apic(1, 1)]);
    let cores = find_cores(0x100, &machine_v2(&madt)).unwrap();
    assert_eq!(cores, [Core { apic_id: 0 }]);
}

#[test]
fn reads_io_apics_and_isa_overrides() {
    let mut io_apic = vec![1, 12, 0, 0];
    io_apic.extend_from_slice(&0xFEC0_0000u32.to_le_bytes());
    io_apic.extend_from_slice(&0u32.to_le_bytes());
    // ISA IRQ 0 (the PIT) arrives on line 2; IRQ 9 is level, active low.
    let mut timer = vec![2, 10, 0, 0];
    timer.extend_from_slice(&2u32.to_le_bytes());
    timer.extend_from_slice(&0u16.to_le_bytes());
    let mut acpi = vec![2, 10, 0, 9];
    acpi.extend_from_slice(&9u32.to_le_bytes());
    acpi.extend_from_slice(&0x000Fu16.to_le_bytes());
    let mut other_bus = vec![2, 10, 1, 4]; // not ISA: ignored
    other_bus.extend_from_slice(&20u32.to_le_bytes());
    other_bus.extend_from_slice(&0u16.to_le_bytes());
    let memory = machine_v2(&madt(&[
        &local_apic(0, 1),
        &io_apic,
        &timer,
        &acpi,
        &other_bus,
    ]));
    let wiring = find_interrupt_wiring(0x100, &memory).unwrap();
    assert_eq!(
        wiring.io_apics,
        [IoApic {
            address: 0xFEC0_0000,
            first_line: 0
        }]
    );
    assert_eq!(
        wiring.overrides,
        [
            SourceOverride {
                irq: 0,
                line: 2,
                flags: 0
            },
            SourceOverride {
                irq: 9,
                line: 9,
                flags: 0x000F
            },
        ]
    );
}
