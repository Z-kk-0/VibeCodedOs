//! Building the memory region list in memory someone else provides
//! (the loader has no heap).

use super::MemoryRegion;

/// Fills caller-provided slots with regions, merging each new region into
/// the previous one when they touch and have the same kind. The firmware
/// reports memory in many small pieces; merged, the list is much shorter.
pub struct RegionList<'a> {
    slots: &'a mut [MemoryRegion],
    len: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RegionListFull;

impl<'a> RegionList<'a> {
    pub fn new(slots: &'a mut [MemoryRegion]) -> Self {
        Self { slots, len: 0 }
    }

    pub fn push(&mut self, region: MemoryRegion) -> Result<(), RegionListFull> {
        if let Some(previous) = self.len.checked_sub(1).map(|last| &mut self.slots[last])
            && previous.kind == region.kind
            && previous.end() == region.start
        {
            previous.page_count += region.page_count;
            return Ok(());
        }
        let slot = self.slots.get_mut(self.len).ok_or(RegionListFull)?;
        *slot = region;
        self.len += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[MemoryRegion] {
        &self.slots[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryKind, PAGE_SIZE};

    fn region(start_page: u64, page_count: u64, kind: MemoryKind) -> MemoryRegion {
        MemoryRegion {
            start: start_page * PAGE_SIZE,
            page_count,
            kind,
        }
    }

    fn empty_slots<const N: usize>() -> [MemoryRegion; N] {
        [region(0, 0, MemoryKind::Usable); N]
    }

    #[test]
    fn merges_touching_regions_of_the_same_kind() {
        let mut slots = empty_slots::<4>();
        let mut list = RegionList::new(&mut slots);
        list.push(region(0, 2, MemoryKind::Usable)).unwrap();
        list.push(region(2, 3, MemoryKind::Usable)).unwrap();

        assert_eq!(list.len(), 1);
        assert_eq!(list.as_slice()[0].page_count, 5);
    }

    #[test]
    fn keeps_regions_with_a_gap_apart() {
        let mut slots = empty_slots::<4>();
        let mut list = RegionList::new(&mut slots);
        list.push(region(0, 2, MemoryKind::Usable)).unwrap();
        list.push(region(3, 1, MemoryKind::Usable)).unwrap();

        assert_eq!(list.len(), 2);
    }

    #[test]
    fn keeps_regions_of_different_kinds_apart() {
        let mut slots = empty_slots::<4>();
        let mut list = RegionList::new(&mut slots);
        list.push(region(0, 2, MemoryKind::Usable)).unwrap();
        list.push(region(2, 1, MemoryKind::Kernel)).unwrap();

        assert_eq!(list.len(), 2);
        assert_eq!(list.as_slice()[1].kind, MemoryKind::Kernel);
    }

    #[test]
    fn reports_when_full_but_still_merges() {
        let mut slots = empty_slots::<1>();
        let mut list = RegionList::new(&mut slots);
        list.push(region(0, 1, MemoryKind::Usable)).unwrap();

        assert_eq!(
            list.push(region(5, 1, MemoryKind::Usable)),
            Err(RegionListFull)
        );
        assert_eq!(list.push(region(1, 1, MemoryKind::Usable)), Ok(()));
        assert_eq!(list.as_slice()[0].page_count, 2);
    }

    #[test]
    fn region_end_is_start_plus_size() {
        let region = region(1, 3, MemoryKind::Usable);
        assert_eq!(region.size(), 3 * PAGE_SIZE);
        assert_eq!(region.end(), 4 * PAGE_SIZE);
    }
}
