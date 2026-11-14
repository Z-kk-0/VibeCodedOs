//! The kernel's one physical frame table (logic: the `frame_table` crate).

use super::physical_to_pointer;
use crate::sync::SpinLock;
use boot_info::{BootInfo, MemoryKind};
use core::ops::Range;
use frame_table::{FRAME_SIZE, FrameEntry, FrameNumber, FrameState, FrameTable};

static FRAMES: SpinLock<Option<FrameTable<'static>>> = SpinLock::new(None);

pub struct FrameSetup {
    pub frames_tracked: usize,
    pub table_address: u64,
    pub table_bytes: u64,
}

/// Builds the frame table from the boot memory map. Call once.
pub fn init(boot_info: &BootInfo) -> FrameSetup {
    let regions = boot_info.memory_regions();

    // Track every frame up to the end of the highest RAM region.
    let ram_end = regions
        .iter()
        .filter(|region| region.kind.is_ram_we_manage())
        .map(|region| region.end())
        .max()
        .expect("no RAM in the memory map");
    let frame_count = ram_end / FRAME_SIZE;
    let table_bytes = FrameTable::bytes_needed(frame_count);

    // The table needs a home: the first free region big enough for it.
    let table_address = regions
        .iter()
        .filter(|region| region.kind == MemoryKind::Usable && region.start > 0)
        .find(|region| region.size() >= table_bytes)
        .expect("no free region big enough for the frame table")
        .start;

    // SAFETY: the region is usable RAM nobody else uses yet, big enough for
    // `frame_count` entries, and page-aligned (entries need 4-byte alignment).
    let entries = unsafe {
        let start = physical_to_pointer(table_address).cast::<FrameEntry>();
        core::slice::from_raw_parts_mut(start, frame_count as usize)
    };
    // `new` initializes every entry before anything reads them.
    let mut table = FrameTable::new(entries);

    for region in regions
        .iter()
        .filter(|region| region.kind == MemoryKind::Usable)
    {
        table.mark(frame_range(region.start, region.end()), FrameState::Free);
    }
    // The table itself lives in usable memory: take that back out.
    table.mark(
        frame_range(table_address, table_address + table_bytes),
        FrameState::Allocated,
    );
    // Never hand out physical address 0: a pointer to it looks like null.
    table.mark(0..1, FrameState::Unavailable);
    table.build_free_list();

    *FRAMES.lock() = Some(table);
    FrameSetup {
        frames_tracked: frame_count as usize,
        table_address,
        table_bytes,
    }
}

/// Hands the memory the firmware and the loader used to the allocator.
/// Only safe once nothing uses it anymore: we run on the loader's page
/// tables (not the firmware's) and on our own stack. Returns bytes gained.
pub fn reclaim_boot_memory(boot_info: &BootInfo) -> u64 {
    let mut guard = FRAMES.lock();
    let table = guard.as_mut().expect("frame table not initialized");
    let reclaimable = [
        MemoryKind::FirmwareReclaimable,
        MemoryKind::LoaderReclaimable,
    ];
    let frames: usize = boot_info
        .memory_regions()
        .iter()
        .filter(|region| reclaimable.contains(&region.kind))
        .map(|region| table.add_free(frame_range(region.start, region.end())))
        .sum();
    frames as u64 * FRAME_SIZE
}

pub fn allocate_frame() -> Option<FrameNumber> {
    FRAMES
        .lock()
        .as_mut()
        .expect("frame table not initialized")
        .allocate()
}

/// Gives a frame back. Freeing a frame twice is a kernel bug, so it panics.
pub fn free_frame(frame: FrameNumber) {
    let result = FRAMES
        .lock()
        .as_mut()
        .expect("frame table not initialized")
        .free(frame);
    if let Err(error) = result {
        panic!(
            "freeing frame {:#x} failed: {error:?}",
            frame.start_address()
        );
    }
}

pub fn free_frame_count() -> usize {
    FRAMES.lock().as_ref().map_or(0, |table| table.free_count())
}

/// Frames that cover the byte range `start..end` completely or partially.
fn frame_range(start: u64, end: u64) -> Range<u64> {
    start / FRAME_SIZE..end.div_ceil(FRAME_SIZE)
}

/// Hands one frame the loader used (state "unavailable") to the allocator.
pub fn reclaim_frame(address: u64) {
    let mut guard = FRAMES.lock();
    let table = guard.as_mut().expect("frame table not initialized");
    let frame = FrameNumber::containing(address).0;
    table.add_free(frame..frame + 1);
}
