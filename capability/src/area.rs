//! Where memory objects appear in a process (DECISIONS.md, 020, 022): the
//! kernel picks the addresses, one after the other, with an unmapped guard
//! page after each object so running off its end faults.
//!
//! Addresses are deliberately never reused: a pointer into memory that was
//! moved away or given up keeps faulting forever, instead of one day
//! pointing into some other object (use-after-free). That costs nothing
//! lasting – the page tables of given-up ranges are freed (pruned) – and
//! the area is tens of terabytes: billions of objects.

pub struct MappingArea {
    next: u64,
    end: u64,
    page_size: u64,
}

impl MappingArea {
    pub const fn new(start: u64, end: u64, page_size: u64) -> Self {
        Self {
            next: start,
            end,
            page_size,
        }
    }

    /// The start address for an object of `pages` pages, or `None` if the
    /// area is used up.
    pub fn reserve(&mut self, pages: u64) -> Option<u64> {
        self.reserve_aligned(pages, self.page_size)
    }

    /// The same, starting at a multiple of `align` (a power of two).
    pub fn reserve_aligned(&mut self, pages: u64, align: u64) -> Option<u64> {
        let size = pages.checked_add(1)?.checked_mul(self.page_size)?; // + guard page
        let start = self
            .next
            .checked_next_multiple_of(align.max(self.page_size))?;
        let next = start.checked_add(size)?;
        if next > self.end {
            return None;
        }
        self.next = next;
        Some(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_get_consecutive_addresses_with_a_gap() {
        let mut area = MappingArea::new(0x1000_0000, 0x2000_0000, 0x1000);
        assert_eq!(area.reserve(1), Some(0x1000_0000));
        assert_eq!(area.reserve(3), Some(0x1000_2000));
        assert_eq!(area.reserve(1), Some(0x1000_6000));
    }

    #[test]
    fn aligned_objects_skip_ahead() {
        let mut area = MappingArea::new(0x1000, 0x10_0000, 0x1000);
        assert_eq!(area.reserve(1), Some(0x1000));
        assert_eq!(area.reserve_aligned(2, 0x8000), Some(0x8000));
        assert_eq!(
            area.reserve(1),
            Some(0xB000),
            "after the 2 pages and the guard"
        );
    }

    #[test]
    fn a_full_area_says_no() {
        let mut area = MappingArea::new(0, 0x4000, 0x1000);
        assert_eq!(area.reserve(1), Some(0));
        assert_eq!(area.reserve(2), None, "would need 3 pages, 2 left");
        assert_eq!(area.reserve(1), Some(0x2000));
        assert_eq!(area.reserve(u64::MAX), None);
    }
}
