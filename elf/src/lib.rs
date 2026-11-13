//! Minimal loader for position-independent ELF-64 executables
//! (DECISIONS.md, 005).
//!
//! Pure byte work – no allocation, no firmware – so it runs in the boot
//! loader and in host unit tests alike.

#![cfg_attr(not(test), no_std)]

mod bytes;
mod file;
mod relocate;
#[cfg(test)]
mod tests;

pub use file::ElfFile;
use file::{EXECUTABLE, SEGMENT_LOAD, Segment, WRITABLE};

pub type Error = &'static str;

pub const PAGE_SIZE: u64 = 4096;

/// Where a loadable segment sits in the image and what it may do – what
/// the page tables need to map it with the right permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentLayout {
    /// Byte offset from the image start (= `load_span().start`).
    pub offset: u64,
    pub size: u64,
    pub writable: bool,
    pub executable: bool,
}

impl ElfFile<'_> {
    pub fn segment_layouts(
        &self,
    ) -> Result<impl Iterator<Item = Result<SegmentLayout, Error>> + '_, Error> {
        let image_start = self.load_span()?.start;
        Ok(self.segments().filter_map(move |segment| match segment {
            Ok(segment) if segment.kind != SEGMENT_LOAD => None,
            Ok(segment) => Some(Ok(SegmentLayout {
                offset: segment.virtual_address - image_start,
                size: segment.memory_size,
                writable: segment.permissions & WRITABLE != 0,
                executable: segment.permissions & EXECUTABLE != 0,
            })),
            Err(error) => Some(Err(error)),
        }))
    }
}

/// Copies all loadable segments into `image` and applies the relocations
/// for running at `image_address`. Returns the real entry point address.
///
/// `image` must be zero-filled (that's what makes `.bss` zero) and at least
/// as long as `elf.load_span()`.
pub fn load(elf: &ElfFile, image: &mut [u8], image_address: u64) -> Result<u64, Error> {
    let span = elf.load_span()?;
    if (image.len() as u64) < span.end - span.start {
        return Err("image memory is smaller than the ELF needs");
    }
    for segment in elf.segments() {
        let segment = segment?;
        if segment.kind == SEGMENT_LOAD {
            copy_segment(elf, &segment, image, span.start)?;
        }
    }
    // Linked to start at `span.start`, actually starts at `image_address`.
    let load_offset = image_address.wrapping_sub(span.start);
    relocate::apply(elf, image, span.start, load_offset)?;
    Ok(elf.entry.wrapping_add(load_offset))
}

fn copy_segment(
    elf: &ElfFile,
    segment: &Segment,
    image: &mut [u8],
    lowest: u64,
) -> Result<(), Error> {
    let source_start = segment.file_offset as usize;
    let source = source_start
        .checked_add(segment.file_size as usize)
        .and_then(|source_end| elf.bytes.get(source_start..source_end))
        .ok_or("ELF segment lies outside the file")?;
    let target_start = (segment.virtual_address - lowest) as usize;
    image
        .get_mut(target_start..target_start + source.len())
        .ok_or("ELF segment lies outside the image")?
        .copy_from_slice(source);
    Ok(())
}
