//! Puts the embedded kernel into memory (DECISIONS.md, 004 and 005).
//! The ELF work itself happens in the `elf` crate; this module only gets
//! memory from the firmware and, at the end, jumps.

use crate::address_space::AddressSpace;
use crate::uefi::{BootServices, MemoryType};
use boot_info::{KERNEL_BASE, PAGE_SIZE};
use elf::ElfFile;

pub type Error = &'static str;

/// A kernel copied into physical memory, relocated to run at `KERNEL_BASE`.
pub struct LoadedKernel {
    pub elf: ElfFile<'static>,
    pub physical_start: u64,
    pub size: u64,
    /// Virtual address of `_start`.
    pub entry: u64,
}

pub fn load(boot_services: &BootServices, file: &'static [u8]) -> Result<LoadedKernel, Error> {
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

    // Copied to `physical_start`, but relocated for where it will be mapped.
    let entry = elf::load(&elf, image, KERNEL_BASE)?;
    Ok(LoadedKernel {
        elf,
        physical_start,
        size,
        entry,
    })
}

impl LoadedKernel {
    /// Switches to the kernel's address space and jumps in. Contract with
    /// `kernel/src/arch/start.rs`: System V calling convention, first
    /// argument = `BootInfo` address (in the physical memory window).
    ///
    /// # Safety
    /// Only after `ExitBootServices`; `address_space` must map this kernel.
    pub unsafe fn start(&self, address_space: &AddressSpace, boot_info: u64) -> ! {
        type Entry = extern "sysv64" fn(u64) -> !;
        // SAFETY: forwarded to the caller. `entry` is the relocated `_start`,
        // mapped by `address_space`, and follows `Entry`.
        unsafe {
            address_space.activate();
            let entry: Entry = core::mem::transmute(self.entry as usize);
            entry(boot_info)
        }
    }
}
