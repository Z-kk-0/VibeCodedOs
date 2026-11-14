//! A process's handle table: slots that hold capabilities.

use alloc::vec::Vec;

/// What a program sees: low 32 bits slot index, high 32 bits generation.
/// `u64::MAX` (`abi::NO_HANDLE`) is never a valid handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle(pub u64);

impl Handle {
    fn new(index: usize, generation: u32) -> Self {
        Self(u64::from(generation) << 32 | index as u64)
    }

    fn index(self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }

    fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

pub struct HandleTable<T> {
    slots: Vec<Slot<T>>,
}

impl<T> Default for HandleTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> HandleTable<T> {
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Stores `value` in the first free slot. The first handles of a new
    /// table are therefore 0, 1, 2, … – which programs rely on at start.
    pub fn insert(&mut self, value: T) -> Handle {
        if let Some(index) = self.slots.iter().position(|slot| slot.value.is_none()) {
            let slot = &mut self.slots[index];
            slot.value = Some(value);
            return Handle::new(index, slot.generation);
        }
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        Handle::new(self.slots.len() - 1, 0)
    }

    pub fn get(&self, handle: Handle) -> Option<&T> {
        let slot = self.slots.get(handle.index())?;
        (slot.generation == handle.generation())
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index())?;
        if slot.generation != handle.generation() {
            return None;
        }
        slot.value.as_mut()
    }

    /// Takes the value out; the handle (and every copy of it) dies.
    pub fn remove(&mut self, handle: Handle) -> Option<T> {
        let slot = self.slots.get_mut(handle.index())?;
        if slot.generation != handle.generation() {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        Some(value)
    }

    /// Empties the table, handing every value to `release` (for cleanup
    /// that needs more than `Drop`, e.g. unmapping from an address space).
    pub fn drain(&mut self, mut release: impl FnMut(T)) {
        for slot in &mut self.slots {
            if let Some(value) = slot.value.take() {
                release(value);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_handles_are_0_1_2() {
        let mut table = HandleTable::new();
        assert_eq!(table.insert("log"), Handle(0));
        assert_eq!(table.insert("console"), Handle(1));
        assert_eq!(table.get(Handle(1)), Some(&"console"));
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_removed_handle_stays_dead_when_the_slot_is_reused() {
        let mut table = HandleTable::new();
        let old = table.insert("old");
        assert_eq!(table.remove(old), Some("old"));
        let new = table.insert("new");
        assert_ne!(old, new, "same slot, new generation");
        assert_eq!(table.get(old), None);
        assert_eq!(table.remove(old), None);
        assert_eq!(table.get(new), Some(&"new"));
    }

    #[test]
    fn made_up_handles_find_nothing() {
        let mut table = HandleTable::new();
        table.insert(1);
        assert_eq!(table.get(Handle(7)), None);
        assert_eq!(table.get(Handle(1 << 32)), None); // right slot, wrong generation
        assert_eq!(table.get_mut(Handle(u64::MAX)), None);
    }

    #[test]
    fn drain_hands_out_everything() {
        let mut table = HandleTable::new();
        table.insert(1);
        let two = table.insert(2);
        table.insert(3);
        table.remove(two);
        let mut released = Vec::new();
        table.drain(|value| released.push(value));
        assert_eq!(released, [1, 3]);
        assert!(table.is_empty());
    }
}
