//! Loads the kernel ELF into memory and prepares it to run
//! (see DECISIONS.md, 004 and 005).

mod bytes;
mod file;
mod relocate;

use crate::uefi::{BootServices, MemoryType};
use boot_info::{BootInfo, PAGE_SIZE};
use file::{ElfFile, SEGMENT_LOAD, Segment};

pub type Error = &'static str;

/// A kernel copied into memory and relocated – ready to jump to.
pub struct LoadedKernel {
    pub physical_start: u64,
    pub size: u64,
    pub entry: u64,
}

pub fn load(boot_services: &BootServices, file: &[u8]) -> Result<LoadedKernel, Error> {
    let elf = ElfFile::parse(file)?;
    let (lowest, highest) = virtual_span(&elf)?;
    let size = highest - lowest;

    let physical_start = boot_services
        .allocate_pages(MemoryType::VIBE_KERNEL, (size / PAGE_SIZE) as usize)
        .map_err(|_| "not enough memory for the kernel")?;
    // SAFETY: the firmware just gave us these pages and nobody else uses
    // them. UEFI identity-maps memory, so the address is a valid pointer.
    // Zeroing first makes the bytes initialized and fills `.bss` on the way.
    let image = unsafe {
        let start = physical_start as *mut u8;
        core::ptr::write_bytes(start, 0, size as usize);
        core::slice::from_raw_parts_mut(start, size as usize)
    };

    for segment in elf.segments() {
        let segment = segment?;
        if segment.kind == SEGMENT_LOAD {
            copy_segment(&elf, &segment, image, lowest)?;
        }
    }

    // Linked to start at `lowest`, actually starts at `physical_start`.
    let load_offset = physical_start.wrapping_sub(lowest);
    relocate::apply(&elf, image, lowest, load_offset)?;

    Ok(LoadedKernel {
        physical_start,
        size,
        entry: elf.entry.wrapping_add(load_offset),
    })
}

impl LoadedKernel {
    /// Jumps into the kernel. Contract with `kernel/src/start.rs`:
    /// System V calling convention, first argument = `&BootInfo`, never returns.
    pub fn start(&self, boot_info: &'static BootInfo) -> ! {
        type Entry = extern "sysv64" fn(&'static BootInfo) -> !;
        // SAFETY: `entry` points to the relocated `_start`, which follows `Entry`.
        let entry: Entry = unsafe { core::mem::transmute(self.entry as usize) };
        entry(boot_info)
    }
}

/// Page-aligned range of virtual addresses the loadable segments cover.
fn virtual_span(elf: &ElfFile) -> Result<(u64, u64), Error> {
    let mut lowest = u64::MAX;
    let mut highest = 0;
    for segment in elf.segments() {
        let segment = segment?;
        if segment.kind != SEGMENT_LOAD {
            continue;
        }
        if segment.file_size > segment.memory_size {
            return Err("kernel ELF segment is bigger in the file than in memory");
        }
        lowest = lowest.min(segment.virtual_address);
        highest = highest.max(segment.virtual_address + segment.memory_size);
    }
    if lowest >= highest {
        return Err("kernel ELF has nothing to load");
    }
    Ok((
        lowest / PAGE_SIZE * PAGE_SIZE,
        highest.div_ceil(PAGE_SIZE) * PAGE_SIZE,
    ))
}

fn copy_segment(
    elf: &ElfFile,
    segment: &Segment,
    image: &mut [u8],
    lowest: u64,
) -> Result<(), Error> {
    let source_start = segment.file_offset as usize;
    let source = elf
        .bytes
        .get(source_start..source_start + segment.file_size as usize)
        .ok_or("kernel ELF segment lies outside the file")?;
    let target_start = (segment.virtual_address - lowest) as usize;
    image
        .get_mut(target_start..target_start + source.len())
        .ok_or("kernel ELF segment lies outside the image")?
        .copy_from_slice(source);
    Ok(())
}
