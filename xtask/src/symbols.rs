//! `cargo xtask symbolize <address>...`: which kernel function an address
//! lies in – for a hang, read the cores' `RIP`s with QEMU's monitor
//! (`info registers -a`) and paste them here. Uses the symbol table of the
//! last debug build of the kernel; the kernel runs at `KERNEL_BASE`, so a
//! runtime address is that plus the symbol's value.

use crate::{Result, root};
use std::fs;

const KERNEL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
const SECTION_SYMBOL_TABLE: u32 = 2;

struct Symbol {
    start: u64,
    size: u64,
    name: String,
}

pub fn symbolize(addresses: &[String]) -> Result {
    let path = root().join("target/x86_64-unknown-none/debug/kernel");
    let elf = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let symbols = read_symbols(&elf).ok_or("the kernel ELF has no readable symbol table")?;
    for text in addresses {
        let address = u64::from_str_radix(text.trim_start_matches("0x"), 16)
            .map_err(|_| format!("'{text}' is not a hex address"))?;
        let offset = address.wrapping_sub(KERNEL_BASE);
        let best = symbols
            .iter()
            .filter(|symbol| symbol.start <= offset && offset < symbol.start + symbol.size.max(1))
            .min_by_key(|symbol| symbol.size);
        match best {
            Some(symbol) => println!("{text}: {} +{:#x}", symbol.name, offset - symbol.start),
            None => println!("{text}: not in any kernel symbol"),
        }
    }
    Ok(())
}

fn read_symbols(elf: &[u8]) -> Option<Vec<Symbol>> {
    let section_headers = read_u64(elf, 0x28)? as usize;
    let header_size = read_u16(elf, 0x3A)? as usize;
    let count = read_u16(elf, 0x3C)? as usize;
    let mut symbols = Vec::new();
    for index in 0..count {
        let header = section_headers + index * header_size;
        if read_u32(elf, header + 4)? != SECTION_SYMBOL_TABLE {
            continue;
        }
        let offset = read_u64(elf, header + 0x18)? as usize;
        let size = read_u64(elf, header + 0x20)? as usize;
        let strings_index = read_u32(elf, header + 0x28)? as usize;
        let entry_size = (read_u64(elf, header + 0x38)? as usize).max(1);
        let strings = read_u64(elf, section_headers + strings_index * header_size + 0x18)? as usize;
        for entry in (offset..offset + size).step_by(entry_size) {
            let name_at = strings + read_u32(elf, entry)? as usize;
            let name_end = name_at + elf.get(name_at..)?.iter().position(|&byte| byte == 0)?;
            let start = read_u64(elf, entry + 8)?;
            if start != 0 {
                symbols.push(Symbol {
                    start,
                    size: read_u64(elf, entry + 16)?,
                    name: String::from_utf8_lossy(&elf[name_at..name_end]).into_owned(),
                });
            }
        }
    }
    Some(symbols)
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
