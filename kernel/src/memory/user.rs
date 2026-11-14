//! Reading memory a user program points the kernel to.
//!
//! The rule: never trust a pointer from user mode. It might point into the
//! kernel, to nothing, or half here and half there. So we check every page
//! the range touches in the program's *own* page tables, and copy through
//! the physical memory window – the user pointer itself is never
//! dereferenced.

use super::address_space::AddressSpace;
use super::physical_to_pointer;
use alloc::vec::Vec;
use paging::PAGE_SIZE;

/// Everything below this address is the lower half – user territory.
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;

/// Copies `length` bytes from `address` in `space`, if the program is
/// allowed to read all of them. `None` if not.
pub fn copy_from_user(space: &AddressSpace, address: u64, length: usize) -> Option<Vec<u8>> {
    let end = address.checked_add(length as u64)?;
    if end > USER_SPACE_END {
        return None;
    }
    let mut bytes = Vec::with_capacity(length);
    let mut cursor = address;
    while cursor < end {
        let translation = space.translate(cursor)?;
        if !translation.flags.user {
            return None;
        }
        let page_end = (cursor / PAGE_SIZE + 1) * PAGE_SIZE;
        let chunk_length = (page_end.min(end) - cursor) as usize;
        // SAFETY: the page is mapped and user-readable; we read it through
        // the window, and only up to the end of this page.
        let chunk = unsafe {
            core::slice::from_raw_parts(
                physical_to_pointer(translation.physical_address),
                chunk_length,
            )
        };
        bytes.extend_from_slice(chunk);
        cursor += chunk_length as u64;
    }
    Some(bytes)
}
