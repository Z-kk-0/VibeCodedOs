//! The physical memory window (and, for the loader's last steps, the same
//! range identity-mapped): physical address p at `PHYSICAL_MEMORY_WINDOW + p`.
//!
//! Two rules (DECISIONS.md, 023):
//! - Every 2 MiB without RAM is device memory and mapped *uncached* – a
//!   cached device register would answer from the cache, not the device.
//! - The framebuffer is *left out*. It belongs to whoever opens the screen
//!   (mapped write-combining there); a second, differently cached view in
//!   the kernel is exactly what Intel warns against (SDM Vol. 3A, §11.12.4),
//!   and without it the kernel can't touch the screen even by mistake.
//!
//! 2 MiB pages where possible; where the framebuffer starts or ends inside
//! one, the rest of that 2 MiB is mapped page by page.

use crate::handoff::Handoff;
use boot_info::{Framebuffer, PHYSICAL_MEMORY_WINDOW};
use paging::{Caching, Flags, HUGE_PAGE_SIZE, MapError, Mapper, PAGE_SIZE, PageSize, TableMemory};

/// What `map` did, for the loader's report.
pub struct Window {
    pub uncached_size: u64,
    pub left_out_size: u64,
}

/// Page tables a framebuffer can cost on top of the 2 MiB pages: one
/// table each where it starts and ends mid-2 MiB, for the window and the
/// identity map.
pub const EXTRA_TABLES: u64 = 4;

pub fn map<M: TableMemory>(
    mapper: &mut Mapper<M>,
    size: u64,
    handoff: &Handoff,
    screen: &Framebuffer,
) -> Result<Window, MapError> {
    let left_out = if screen.exists() {
        screen.address..screen.address + screen.pages() * PAGE_SIZE
    } else {
        0..0
    };
    let mut window = Window {
        uncached_size: 0,
        left_out_size: 0,
    };
    for chunk in (0..size).step_by(HUGE_PAGE_SIZE as usize) {
        let flags = if handoff.holds_ram(chunk, chunk + HUGE_PAGE_SIZE) {
            Flags::KERNEL_DATA
        } else {
            window.uncached_size += HUGE_PAGE_SIZE;
            Flags {
                caching: Caching::Uncached,
                ..Flags::KERNEL_DATA
            }
        };
        let overlaps = left_out.start < chunk + HUGE_PAGE_SIZE && chunk < left_out.end;
        if !overlaps {
            map_both(mapper, chunk, PageSize::Huge, flags)?;
            continue;
        }
        for page in (chunk..chunk + HUGE_PAGE_SIZE).step_by(PAGE_SIZE as usize) {
            if left_out.contains(&page) {
                window.left_out_size += PAGE_SIZE;
            } else {
                map_both(mapper, page, PageSize::Normal, flags)?;
            }
        }
    }
    Ok(window)
}

/// Maps `physical` in the window and at its own address. The identity map
/// only lets the loader survive switching page tables (the kernel removes
/// it right away) – executable, otherwise cached like the window.
fn map_both<M: TableMemory>(
    mapper: &mut Mapper<M>,
    physical: u64,
    size: PageSize,
    flags: Flags,
) -> Result<(), MapError> {
    let identity = Flags {
        executable: true,
        ..flags
    };
    mapper.map(physical, physical, size, identity)?;
    mapper.map(PHYSICAL_MEMORY_WINDOW + physical, physical, size, flags)
}
