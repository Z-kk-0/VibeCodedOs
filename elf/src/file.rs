//! Parsing the ELF header and program headers ("segments").
//! Offsets from "ELF-64 Object File Format" v1.5, §3 and §6.

use super::bytes::{read_u16, read_u32, read_u64};
use super::{Error, PAGE_SIZE};
use core::ops::Range;

const HEADER_SIZE: usize = 64;
const MAGIC: &[u8] = b"\x7fELF";
const CLASS_64_BIT: u8 = 2;
const LITTLE_ENDIAN: u8 = 1;
/// "Shared object" – what a position-independent executable is in ELF terms.
const TYPE_POSITION_INDEPENDENT: u16 = 3;
const MACHINE_X86_64: u16 = 62;

pub const SEGMENT_LOAD: u32 = 1;
pub const SEGMENT_DYNAMIC: u32 = 2;

// Segment permission bits ("p_flags").
pub const EXECUTABLE: u32 = 1 << 0;
pub const WRITABLE: u32 = 1 << 1;

/// One program header: a piece of the file and where it goes in memory.
pub struct Segment {
    pub kind: u32,
    pub permissions: u32,
    pub file_offset: u64,
    pub file_size: u64,
    pub virtual_address: u64,
    /// Larger than `file_size` when zero-filled data (`.bss`) follows.
    pub memory_size: u64,
}

pub struct ElfFile<'a> {
    pub bytes: &'a [u8],
    /// Entry point, as linked (before moving the image).
    pub entry: u64,
    segment_table: usize,
    segment_entry_size: usize,
    segment_count: usize,
}

impl<'a> ElfFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE || &bytes[0..4] != MAGIC {
            return Err("not an ELF file");
        }
        if bytes[4] != CLASS_64_BIT || bytes[5] != LITTLE_ENDIAN {
            return Err("ELF is not 64-bit little endian");
        }
        if read_u16(bytes, 0x12)? != MACHINE_X86_64 {
            return Err("ELF is not for x86_64");
        }
        if read_u16(bytes, 0x10)? != TYPE_POSITION_INDEPENDENT {
            return Err("ELF is not position-independent");
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

    /// Page-aligned range of virtual addresses the loadable segments cover.
    /// Its length is how much memory the image needs.
    pub fn load_span(&self) -> Result<Range<u64>, Error> {
        let mut lowest = u64::MAX;
        let mut highest = 0;
        for segment in self.segments() {
            let segment = segment?;
            if segment.kind != SEGMENT_LOAD {
                continue;
            }
            if segment.file_size > segment.memory_size {
                return Err("ELF segment is bigger in the file than in memory");
            }
            let end = segment
                .virtual_address
                .checked_add(segment.memory_size)
                .ok_or("ELF segment ends beyond the address space")?;
            lowest = lowest.min(segment.virtual_address);
            highest = highest.max(end);
        }
        if lowest >= highest {
            return Err("ELF has nothing to load");
        }
        Ok(lowest / PAGE_SIZE * PAGE_SIZE..highest.div_ceil(PAGE_SIZE) * PAGE_SIZE)
    }

    fn segment(&self, index: usize) -> Result<Segment, Error> {
        let at = index
            .checked_mul(self.segment_entry_size)
            .and_then(|offset| offset.checked_add(self.segment_table))
            .ok_or("ELF program header table is out of range")?;
        Ok(Segment {
            kind: read_u32(self.bytes, at)?,
            permissions: read_u32(self.bytes, at + 0x04)?,
            file_offset: read_u64(self.bytes, at + 0x08)?,
            virtual_address: read_u64(self.bytes, at + 0x10)?,
            file_size: read_u64(self.bytes, at + 0x20)?,
            memory_size: read_u64(self.bytes, at + 0x28)?,
        })
    }
}
