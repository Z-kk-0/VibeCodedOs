//! Fixing up absolute addresses inside a loaded image.
//!
//! The kernel is linked as if it started at address 0. Wherever it needs an
//! absolute address (pointers in statics, vtables, …) the linker leaves a
//! "relocation": *at this spot, put load address + this value*. The list is
//! found via the DYNAMIC segment. Formats: ELF-64 v1.5 §5 and §8,
//! relocation types: System V AMD64 psABI §4.4.

use super::Error;
use super::bytes::{read_u64, write_u64};
use super::file::{ElfFile, SEGMENT_DYNAMIC};

// Tags in the DYNAMIC segment.
const DYNAMIC_END: u64 = 0;
const DYNAMIC_RELA_TABLE: u64 = 7;
const DYNAMIC_RELA_TABLE_SIZE: u64 = 8;
const DYNAMIC_RELA_ENTRY_SIZE: u64 = 9;
const DYNAMIC_ENTRY_SIZE: usize = 16;

const RELOCATION_NONE: u64 = 0;
/// value = load offset + addend. The only kind a static PIE needs.
const RELOCATION_RELATIVE: u64 = 8;

/// `image` holds the memory starting at virtual address `lowest`;
/// `load_offset` is how far the image was moved from where it was linked.
pub fn apply(elf: &ElfFile, image: &mut [u8], lowest: u64, load_offset: u64) -> Result<(), Error> {
    let mut dynamic = None;
    for segment in elf.segments() {
        let segment = segment?;
        if segment.kind == SEGMENT_DYNAMIC {
            dynamic = Some(segment.virtual_address);
        }
    }
    let Some(dynamic_address) = dynamic else {
        return Ok(()); // nothing to relocate
    };

    // Where in `image` a virtual address lives.
    let offset = |address: u64| {
        address
            .checked_sub(lowest)
            .map(|offset| offset as usize)
            .ok_or("ELF address lies below the image")
    };

    let mut table = 0;
    let mut table_size = 0;
    let mut entry_size = 0;
    let mut at = offset(dynamic_address)?;
    loop {
        let tag = read_u64(image, at)?;
        let value = read_u64(image, at + 8)?;
        match tag {
            DYNAMIC_END => break,
            DYNAMIC_RELA_TABLE => table = offset(value)?,
            DYNAMIC_RELA_TABLE_SIZE => table_size = value as usize,
            DYNAMIC_RELA_ENTRY_SIZE => entry_size = value as usize,
            _ => {}
        }
        at += DYNAMIC_ENTRY_SIZE;
    }
    if table_size == 0 {
        return Ok(());
    }
    if entry_size == 0 {
        return Err("ELF relocation table has no entry size");
    }

    for entry in (table..table + table_size).step_by(entry_size) {
        let target = read_u64(image, entry)?;
        let kind = read_u64(image, entry + 8)? & 0xFFFF_FFFF;
        let addend = read_u64(image, entry + 16)? as i64;
        match kind {
            RELOCATION_NONE => {}
            RELOCATION_RELATIVE => write_u64(
                image,
                offset(target)?,
                load_offset.wrapping_add_signed(addend),
            )?,
            _ => return Err("ELF uses an unsupported relocation type"),
        }
    }
    Ok(())
}
