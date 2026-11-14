//! Starting a user program in its own address space (DECISIONS.md, 018).

use super::context::TaskContext;
use super::spawn_context;
use crate::memory::{AddressSpace, physical_to_pointer};
use alloc::vec;
use elf::ElfFile;
use paging::{Flags, PAGE_SIZE};
use scheduling::TaskId;

/// Programs are position-independent and moved here: low, but not at 0,
/// so a null pointer still faults.
pub const PROGRAM_BASE: u64 = 0x40_0000;
/// Top of the user stack, near the end of the lower half. The page above
/// it stays unmapped, so running off the top faults instead of corrupting.
const STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;
const STACK_PAGES: u64 = 4;
const USER_DATA: Flags = Flags {
    writable: true,
    executable: false,
    user: true,
};

pub type Error = &'static str;

/// Loads `file` (an ELF executable) as a new user program and schedules it.
pub fn spawn_program(name: &'static str, file: &[u8]) -> Result<TaskId, Error> {
    let elf = ElfFile::parse(file)?;
    let span = elf.load_span()?;
    let mut image = vec![0u8; (span.end - span.start) as usize];
    let entry = elf::load(&elf, &mut image, PROGRAM_BASE)?;

    let mut space = AddressSpace::new().ok_or("out of memory for a new address space")?;
    for layout in elf.segment_layouts()? {
        let layout = layout?;
        // Same W^X rules as for the kernel – plus the user bit.
        let flags = Flags {
            writable: layout.writable,
            executable: layout.executable,
            user: true,
        };
        let first_page = layout.offset / PAGE_SIZE * PAGE_SIZE;
        for offset in (first_page..layout.offset + layout.size).step_by(PAGE_SIZE as usize) {
            map_image_page(&mut space, &image, offset, flags)?;
        }
    }
    for page in 1..=STACK_PAGES {
        space
            .map_new_page(STACK_TOP - page * PAGE_SIZE, USER_DATA)
            .map_err(|_| "out of memory for the program's stack")?;
    }

    // At the entry point the ABI expects rsp + 8 to be 16-byte aligned, as
    // right after a `call`.
    let stack_pointer = STACK_TOP - 8;
    Ok(spawn_context(TaskContext::new_user(
        name,
        entry,
        stack_pointer,
        space,
    )))
}

/// Maps one page of the loaded image. A page that two segments share is
/// mapped once, with the permissions of both.
fn map_image_page(
    space: &mut AddressSpace,
    image: &[u8],
    offset: u64,
    flags: Flags,
) -> Result<(), Error> {
    let address = PROGRAM_BASE + offset;
    if let Some(existing) = space.translate(address) {
        return space
            .map(address, existing.physical_address, flags)
            .map_err(|_| "could not widen a shared page's permissions");
    }
    let frame = space
        .map_new_page(address, flags)
        .map_err(|_| "out of memory for the program")?;
    let start = offset as usize;
    let bytes = &image[start..(start + PAGE_SIZE as usize).min(image.len())];
    // SAFETY: `frame` is a fresh page of this program, reached through the
    // physical memory window; `bytes` is at most one page long.
    unsafe { physical_to_pointer(frame).copy_from_nonoverlapping(bytes.as_ptr(), bytes.len()) };
    Ok(())
}
