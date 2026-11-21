//! The address space the kernel starts in (layout: `boot_info` crate docs,
//! DECISIONS.md 015). Built while the firmware can still hand out memory,
//! switched to right before jumping into the kernel.

use crate::handoff::Handoff;
use crate::kernel_image::LoadedKernel;
use crate::uefi::{BootServices, MemoryType};
use boot_info::{KERNEL_BASE, PHYSICAL_MEMORY_WINDOW};
use core::arch::asm;
use paging::{
    Caching, Flags, HUGE_PAGE_SIZE, MapError, Mapper, PAGE_SIZE, PageSize, PageTable, TableMemory,
};

pub type Error = &'static str;

const GIB: u64 = 1 << 30;
/// Devices such as the local APIC sit just below 4 GiB, so the window
/// always covers at least that much.
const MINIMUM_WINDOW: u64 = 4 * GIB;
const LOADER_IDENTITY: Flags = Flags {
    writable: true,
    executable: true,
    user: false,
    caching: Caching::WriteBack,
};

pub struct AddressSpace {
    /// Physical address of the top-level table (goes into CR3).
    pub root: u64,
    pub window_size: u64,
    /// How much of the window is mapped uncached: every 2 MiB without RAM.
    pub uncached_size: u64,
    pub tables_used: u64,
    use_no_execute: bool,
}

/// All tables come from one pool allocated up front: a single allocation
/// changes the firmware's memory map only once.
struct TablePool {
    next: u64,
    end: u64,
}

// SAFETY: the pool is zeroed memory only we use; the firmware identity-maps
// it, so a physical address is a valid pointer.
unsafe impl TableMemory for &mut TablePool {
    fn allocate_table(&mut self) -> Option<u64> {
        let table = self.next;
        (table < self.end).then(|| {
            self.next += PAGE_SIZE;
            table
        })
    }

    fn table(&mut self, physical_address: u64) -> *mut PageTable {
        physical_address as *mut PageTable
    }
}

pub fn build(
    boot_services: &BootServices,
    kernel: &LoadedKernel,
    handoff: &Handoff,
) -> Result<AddressSpace, Error> {
    let window_size = handoff
        .physical_memory_end
        .max(MINIMUM_WINDOW)
        .next_multiple_of(GIB);
    let pool_bytes = tables_needed(window_size, kernel.size) * PAGE_SIZE;
    let pool_memory = boot_services
        .allocate_zeroed(MemoryType::HEMATITE_PAGE_TABLES, pool_bytes as usize)
        .map_err(|_| "out of memory for page tables")?;
    let pool_start = pool_memory.as_ptr() as u64;
    let mut pool = TablePool {
        next: pool_start,
        end: pool_start + pool_bytes,
    };

    let use_no_execute = cpu_supports_no_execute();
    let mut mapper = Mapper::new(&mut pool, use_no_execute).map_err(describe)?;
    let mut uncached_size = 0;
    for physical in (0..window_size).step_by(HUGE_PAGE_SIZE as usize) {
        // Device memory (no RAM in this 2 MiB) is never cached: a cached
        // device register would answer from the cache, not the device. Until
        // now only the firmware's MTRRs made sure of that (DECISIONS.md, 023).
        let window_flags = if handoff.holds_ram(physical, physical + HUGE_PAGE_SIZE) {
            Flags::KERNEL_DATA
        } else {
            uncached_size += HUGE_PAGE_SIZE;
            Flags {
                caching: Caching::Uncached,
                ..Flags::KERNEL_DATA
            }
        };
        // Identity map: the loader keeps running at its physical address
        // right after the switch. The kernel removes it immediately.
        mapper
            .map(physical, physical, PageSize::Huge, LOADER_IDENTITY)
            .map_err(describe)?;
        mapper
            .map(
                PHYSICAL_MEMORY_WINDOW + physical,
                physical,
                PageSize::Huge,
                window_flags,
            )
            .map_err(describe)?;
    }

    // The kernel, page by page, with each ELF segment's permissions:
    // code is never writable, data never executable ("W^X").
    for layout in kernel.elf.segment_layouts()? {
        let layout = layout?;
        let flags = Flags {
            writable: layout.writable,
            executable: layout.executable,
            user: false,
            caching: Caching::WriteBack,
        };
        let first_page = layout.offset / PAGE_SIZE * PAGE_SIZE;
        for offset in (first_page..layout.offset + layout.size).step_by(PAGE_SIZE as usize) {
            mapper
                .map(
                    KERNEL_BASE + offset,
                    kernel.physical_start + offset,
                    PageSize::Normal,
                    flags,
                )
                .map_err(describe)?;
        }
    }

    // Last use of `mapper`: from here on `pool` can be read again.
    let root = mapper.root();
    Ok(AddressSpace {
        root,
        window_size,
        uncached_size,
        tables_used: (pool.next - pool_start) / PAGE_SIZE,
        use_no_execute,
    })
}

impl AddressSpace {
    /// Switches the CPU to these page tables.
    ///
    /// # Safety
    /// Only after `ExitBootServices` (the firmware relies on its own tables),
    /// and the code calling this must be mapped – the identity map does that.
    pub unsafe fn activate(&self) {
        const EFER: u32 = 0xC000_0080;
        const NO_EXECUTE_ENABLE: u64 = 1 << 11; // Intel SDM Vol. 3A, §2.2.1
        const WRITE_PROTECT: u64 = 1 << 16; // CR0.WP: read-only applies to ring 0 too
        // SAFETY: EFER exists on every x86_64 CPU; NXE is only set if the CPU
        // supports it. WP and CR3 are valid by the contract above.
        unsafe {
            if self.use_no_execute {
                asm!(
                    "rdmsr", "or eax, {bit:e}", "wrmsr",
                    in("ecx") EFER, bit = in(reg) NO_EXECUTE_ENABLE as u32,
                    out("eax") _, out("edx") _,
                );
            }
            asm!("mov {0}, cr0", "or {0}, {1}", "mov cr0, {0}", out(reg) _, in(reg) WRITE_PROTECT);
            asm!("mov cr3, {}", in(reg) self.root);
        }
    }
}

/// Upper bound of tables `build` needs: per GiB one PD for the identity map
/// and one for the window, their PDPTs, the root, and the kernel's tables.
fn tables_needed(window_size: u64, kernel_size: u64) -> u64 {
    let gigabytes = window_size / GIB;
    let pdpts = 2 * gigabytes.div_ceil(512);
    let kernel_tables = 2 + (kernel_size / PAGE_SIZE).div_ceil(512) + 1;
    2 * gigabytes + pdpts + 1 + kernel_tables
}

/// CPUID leaf 0x8000_0001, EDX bit 20 (Intel SDM Vol. 2A, CPUID).
fn cpu_supports_no_execute() -> bool {
    use core::arch::x86_64::__cpuid;
    // `unsafe` is only needed on older toolchains.
    #[allow(unused_unsafe)]
    // SAFETY: CPUID exists on every x86_64 CPU and has no side effects.
    let (highest_extended, features) =
        unsafe { (__cpuid(0x8000_0000).eax, __cpuid(0x8000_0001).edx) };
    highest_extended >= 0x8000_0001 && features & (1 << 20) != 0
}

fn describe(error: MapError) -> Error {
    match error {
        MapError::OutOfTableMemory => "page table pool too small",
        _ => "building the kernel's page tables failed",
    }
}
