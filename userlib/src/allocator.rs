//! A heap for programs (DECISIONS.md, 022): `alloc` – `Box`, `Vec`,
//! `String` – on top of memory objects. The allocator logic is the same
//! `heap` crate the kernel uses; its pages are memory objects we ask the
//! kernel for and give back when a large block is freed.
//!
//! Freeing needs the object's handle, but the heap only knows the address.
//! So a registry keeps (address, handle) pairs, sorted by address, in a
//! memory object of its own that doubles when full – it can't live on the
//! heap it serves.

use crate::ipc::{Handle, drop_handle};
use crate::memory::Memory;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use heap::{Heap, PageSource};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator {
    busy: AtomicBool::new(false),
    heap: UnsafeCell::new(Heap::new(ObjectPages {
        registry: Registry::EMPTY,
    })),
};

struct Allocator {
    /// A program is one task, so this never waits – it only catches a
    /// (buggy) re-entry from inside the allocator.
    busy: AtomicBool,
    heap: UnsafeCell<Heap<ObjectPages>>,
}

// SAFETY: `busy` gives one caller at a time access to `heap`.
unsafe impl Sync for Allocator {}

impl Allocator {
    fn with_heap<R>(&self, action: impl FnOnce(&mut Heap<ObjectPages>) -> R) -> R {
        while self
            .busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: `busy` is ours, so nobody else is in here.
        let result = action(unsafe { &mut *self.heap.get() });
        self.busy.store(false, Ordering::Release);
        result
    }
}

// SAFETY: the `heap` crate hands out each block once until it's freed.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.with_heap(|heap| heap.allocate(layout))
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: forwarded from `GlobalAlloc`'s contract.
        self.with_heap(|heap| unsafe { heap.deallocate(pointer, layout) })
    }
}

struct ObjectPages {
    registry: Registry,
}

// SAFETY: each memory object is fresh, writable, page-aligned and ours
// alone; `unmap_pages` only gets back what `map_pages` returned.
unsafe impl PageSource for ObjectPages {
    fn map_pages(&mut self, count: usize) -> Option<NonNull<u8>> {
        let memory = Memory::new(count as u64).ok()?;
        let address = memory.address as u64;
        let handle = memory.into_handle();
        if !self.registry.insert(address, handle) {
            drop_handle(handle);
            return None;
        }
        NonNull::new(address as *mut u8)
    }

    fn unmap_pages(&mut self, start: NonNull<u8>, _count: usize) {
        if let Some(handle) = self.registry.remove(start.as_ptr() as u64) {
            drop_handle(handle);
        }
    }
}

/// (address, handle) pairs sorted by address, in a memory object.
struct Registry {
    storage: Option<Memory>,
    length: usize,
}

const ENTRY: usize = 16;

impl Registry {
    const EMPTY: Self = Self {
        storage: None,
        length: 0,
    };

    fn insert(&mut self, address: u64, handle: Handle) -> bool {
        if self.length == self.capacity() && !self.grow() {
            return false;
        }
        let index = self.position(address).unwrap_or_else(|index| index);
        let entries = self.entries_mut();
        entries.copy_within(index * 2..entries.len() - 2, index * 2 + 2);
        entries[index * 2] = address;
        entries[index * 2 + 1] = handle.0;
        self.length += 1;
        true
    }

    fn remove(&mut self, address: u64) -> Option<Handle> {
        let index = self.position(address).ok()?;
        let length = self.length;
        let entries = self.entries_mut();
        let handle = Handle(entries[index * 2 + 1]);
        entries.copy_within(index * 2 + 2..length * 2, index * 2);
        self.length -= 1;
        Some(handle)
    }

    /// `Ok(index)` if `address` is registered, else where it would go.
    fn position(&self, address: u64) -> Result<usize, usize> {
        let Some(storage) = &self.storage else {
            return Err(0);
        };
        // SAFETY: the storage holds `length` initialized pairs of u64.
        let entries =
            unsafe { core::slice::from_raw_parts(storage.address.cast::<[u64; 2]>(), self.length) };
        entries.binary_search_by_key(&address, |entry| entry[0])
    }

    /// Doubles the storage (first: one page) and copies the entries over.
    fn grow(&mut self) -> bool {
        let pages = self.storage.as_ref().map_or(1, |storage| storage.pages * 2);
        let Ok(mut bigger) = Memory::new(pages) else {
            return false;
        };
        if let Some(old) = &self.storage {
            let used = self.length * ENTRY;
            bigger.bytes_mut()[..used].copy_from_slice(&old.bytes()[..used]);
        }
        self.storage = Some(bigger); // the old one is dropped – given back
        true
    }

    fn capacity(&self) -> usize {
        self.storage
            .as_ref()
            .map_or(0, |storage| storage.len() / ENTRY)
    }

    /// The first `length + 1` pairs as flat words (room for one insert).
    fn entries_mut(&mut self) -> &mut [u64] {
        let words = (self.length + 1).min(self.capacity()) * 2;
        let storage = self.storage.as_mut().expect("insert grows first");
        // SAFETY: the storage is ours, mapped writable and big enough.
        unsafe { core::slice::from_raw_parts_mut(storage.address.cast::<u64>(), words) }
    }
}
