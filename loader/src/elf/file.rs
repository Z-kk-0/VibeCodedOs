//! Parsing the ELF header and program headers ("segments").
//! Offsets from "ELF-64 Object File Format" v1.5, §3 and §6.

use super::Error;
use super::bytes::{read_u16, read_u32, read_u64};

const HEADER_SIZE: usize = 64;
const MAGIC: &[u8] = b"\x7fELF";
const CLASS_64_BIT: u8 = 2;
const LITTLE_ENDIAN: u8 = 1;
/// "Shared object" – what a position-independent executable is in ELF terms.
const TYPE_POSITION_INDEPENDENT: u16 = 3;
const MACHINE_X86_64: u16 = 62;

pub const SEGMENT_LOAD: u32 = 1;
pub const SEGMENT_DYNAMIC: u32 = 2;

/// One program header: a piece of the file and where it goes in memory.
pub struct Segment {
    pub kind: u32,
    pub file_offset: u64,
    pub file_size: u64,
    pub virtual_address: u64,
    /// Larger than `file_size` when zero-filled data (`.bss`) follows.
    pub memory_size: u64,
}

pub struct ElfFile<'a> {
    pub bytes: &'a [u8],
    pub entry: u64,
    segment_table: usize,
    segment_entry_size: usize,
    segment_count: usize,
}

impl<'a> ElfFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE || &bytes[0..4] != MAGIC {
            return Err("kernel is not an ELF file");
        }
        if bytes[4] != CLASS_64_BIT || bytes[5] != LITTLE_ENDIAN {
            return Err("kernel ELF is not 64-bit little endian");
        }
        if read_u16(bytes, 0x12)? != MACHINE_X86_64 {
            return Err("kernel ELF is not for x86_64");
        }
        if read_u16(bytes, 0x10)? != TYPE_POSITION_INDEPENDENT {
            return Err("kernel ELF is not position-independent");
        }
        Ok(Self {
            bytes,
            entry: read_u64(bytes, 0x18)?,
            segment_table: read_u64(bytes, 0x20)? as usize,
            segment_entry_size: usize::from(read_u16(bytes, 0x36)?),
            segment_count: usize::from(read_u16(bytes, 0x38)?),
        })
    }

    pub fn segments(&self) -> impl Iterator<Item = Result<Segment, Error>> + '_ {
        (0..self.segment_count).map(|index| self.segment(index))
    }

    fn segment(&self, index: usize) -> Result<Segment, Error> {
        let at = self.segment_table + index * self.segment_entry_size;
        Ok(Segment {
            kind: read_u32(self.bytes, at)?,
            file_offset: read_u64(self.bytes, at + 0x08)?,
            virtual_address: read_u64(self.bytes, at + 0x10)?,
            file_size: read_u64(self.bytes, at + 0x20)?,
            memory_size: read_u64(self.bytes, at + 0x28)?,
        })
    }
}
