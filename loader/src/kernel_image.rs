//! Puts the embedded kernel into memory (DECISIONS.md, 004 and 005).
//! The ELF work itself happens in the `elf` crate; this module only gets
//! memory from the firmware and, at the end, jumps.

use crate::uefi::{BootServices, MemoryType};
use boot_info::{BootInfo, PAGE_SIZE};
use elf::ElfFile;

pub type Error = &'static str;

/// A kernel copied into memory and relocated – ready to jump to.
pub struct LoadedKernel {
    pub physical_start: u64,
    pub size: u64,
    pub entry: u64,
}

pub fn load(boot_services: &BootServices, file: &[u8]) -> Result<LoadedKernel, Error> {
    let elf = ElfFile::parse(file)?;
    let span = elf.load_span()?;
    let size = span.end - span.start;

    let physical_start = boot_services
        .allocate_pages(MemoryType::HEMATITE_KERNEL, (size / PAGE_SIZE) as usize)
        .map_err(|_| "not enough memory for the kernel")?;
    // SAFETY: the firmware just gave us these pages and nobody else uses
    // them. UEFI identity-maps memory, so the address is a valid pointer.
    // Zeroing first makes the bytes initialized and gives `.bss` its zeros.
    let image = unsafe {
        let start = physical_start as *mut u8;
        core::ptr::write_bytes(start, 0, size as usize);
        core::slice::from_raw_parts_mut(start, size as usize)
    };

    let entry = elf::load(&elf, image, physical_start)?;
    Ok(LoadedKernel {
        physical_start,
        size,
        entry,
    })
}

impl LoadedKernel {
    /// Jumps into the kernel. Contract with `kernel/src/arch/start.rs`:
    /// System V calling convention, first argument = `&BootInfo`, never returns.
    pub fn start(&self, boot_info: &'static BootInfo) -> ! {
        type Entry = extern "sysv64" fn(&'static BootInfo) -> !;
        // SAFETY: `entry` points to the relocated `_start`, which follows `Entry`.
        let entry: Entry = unsafe { core::mem::transmute(self.entry as usize) };
        entry(boot_info)
    }
}
