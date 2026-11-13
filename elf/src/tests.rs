//! Tests against a tiny hand-built ELF file.
//!
//! Layout of the test file (0x200 bytes, loaded at virtual address 0):
//!   0x000  ELF header
//!   0x040  program header: LOAD     file 0x000..0x200 -> memory 0x000..0x300
//!   0x078  program header: DYNAMIC  at 0x140
//!   0x100  u64 slot that the relocation fills in
//!   0x140  dynamic entries pointing at the relocation table
//!   0x180  one relocation: slot 0x100 = load offset + 0x42
//!   0x1F0  marker byte 0xAB (checks that bytes get copied)

use super::{ElfFile, PAGE_SIZE, load};

const ENTRY: u64 = 0x10;
const SLOT: usize = 0x100;
const ADDEND: u64 = 0x42;
const MARKER_AT: usize = 0x1F0;
const MARKER: u8 = 0xAB;
const RELATIVE: u64 = 8;
const POSITION_INDEPENDENT: u16 = 3;

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn build_elf(elf_type: u16, relocation_kind: u64) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x200];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2; // 64-bit
    bytes[5] = 1; // little endian
    bytes[6] = 1; // ELF version
    put_u16(&mut bytes, 0x10, elf_type);
    put_u16(&mut bytes, 0x12, 62); // x86_64
    put_u32(&mut bytes, 0x14, 1);
    put_u64(&mut bytes, 0x18, ENTRY);
    put_u64(&mut bytes, 0x20, 0x40); // program headers start
    put_u16(&mut bytes, 0x34, 64); // header size
    put_u16(&mut bytes, 0x36, 56); // program header size
    put_u16(&mut bytes, 0x38, 2); // program header count

    put_program_header(&mut bytes, 0x40, 1, 0x000, 0x200, 0x300);
    put_program_header(&mut bytes, 0x78, 2, 0x140, 0x40, 0x40);

    let dynamic = [(7, 0x180), (8, 24), (9, 24), (0, 0)];
    for (index, (tag, value)) in dynamic.into_iter().enumerate() {
        put_u64(&mut bytes, 0x140 + index * 16, tag);
        put_u64(&mut bytes, 0x148 + index * 16, value);
    }

    put_u64(&mut bytes, 0x180, SLOT as u64);
    put_u64(&mut bytes, 0x188, relocation_kind);
    put_u64(&mut bytes, 0x190, ADDEND);

    bytes[MARKER_AT] = MARKER;
    bytes
}

/// File and virtual offsets are identical in our test file.
fn put_program_header(
    bytes: &mut [u8],
    at: usize,
    kind: u32,
    offset: u64,
    file_size: u64,
    memory_size: u64,
) {
    put_u32(bytes, at, kind);
    put_u64(bytes, at + 0x08, offset);
    put_u64(bytes, at + 0x10, offset);
    put_u64(bytes, at + 0x20, file_size);
    put_u64(bytes, at + 0x28, memory_size);
}

fn read_slot(image: &[u8]) -> u64 {
    u64::from_le_bytes(image[SLOT..SLOT + 8].try_into().unwrap())
}

#[test]
fn load_span_is_rounded_to_whole_pages() {
    let file = build_elf(POSITION_INDEPENDENT, RELATIVE);
    let elf = ElfFile::parse(&file).unwrap();
    assert_eq!(elf.load_span().unwrap(), 0..PAGE_SIZE);
}

#[test]
fn loads_copies_and_relocates() {
    let file = build_elf(POSITION_INDEPENDENT, RELATIVE);
    let elf = ElfFile::parse(&file).unwrap();
    let mut image = vec![0u8; PAGE_SIZE as usize];

    let entry = load(&elf, &mut image, 0x20_0000).unwrap();

    assert_eq!(entry, 0x20_0000 + ENTRY);
    assert_eq!(read_slot(&image), 0x20_0000 + ADDEND);
    assert_eq!(image[MARKER_AT], MARKER);
}

#[test]
fn relocation_follows_the_load_address() {
    let file = build_elf(POSITION_INDEPENDENT, RELATIVE);
    let elf = ElfFile::parse(&file).unwrap();
    let mut image = vec![0u8; PAGE_SIZE as usize];

    load(&elf, &mut image, 0).unwrap();

    assert_eq!(read_slot(&image), ADDEND);
}

#[test]
fn rejects_files_that_are_not_elf() {
    assert!(
        ElfFile::parse(b"definitely not an ELF file, just some text....................").is_err()
    );
    assert!(ElfFile::parse(b"short").is_err());
}

#[test]
fn rejects_fixed_address_executables() {
    let file = build_elf(2, RELATIVE); // 2 = ET_EXEC
    assert_eq!(
        ElfFile::parse(&file).err(),
        Some("ELF is not position-independent")
    );
}

#[test]
fn rejects_unknown_relocation_types() {
    let file = build_elf(POSITION_INDEPENDENT, 1); // 1 = R_X86_64_64, needs a symbol table
    let elf = ElfFile::parse(&file).unwrap();
    let mut image = vec![0u8; PAGE_SIZE as usize];
    assert_eq!(
        load(&elf, &mut image, 0).err(),
        Some("ELF uses an unsupported relocation type")
    );
}

#[test]
fn rejects_too_small_image_memory() {
    let file = build_elf(POSITION_INDEPENDENT, RELATIVE);
    let elf = ElfFile::parse(&file).unwrap();
    let mut image = vec![0u8; 0x100];
    assert!(load(&elf, &mut image, 0).is_err());
}
