//! Sets of cores, by index (DECISIONS.md, 019, 022): which cores are
//! online, which have an address space loaded, which still owe a TLB flush.
//! A plain value type, and an atomic one for state shared between cores.

use core::sync::atomic::{AtomicU64, Ordering};

/// Enough for any machine QEMU's q35 board can emulate (it stops at 288
/// with interrupt remapping, 255 without).
pub const MAX_CORES: usize = 256;
const WORDS: usize = MAX_CORES / 64;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct CoreSet {
    words: [u64; WORDS],
}

impl CoreSet {
    pub const EMPTY: Self = Self { words: [0; WORDS] };

    pub fn only(core: usize) -> Self {
        let mut set = Self::EMPTY;
        set.insert(core);
        set
    }

    pub fn insert(&mut self, core: usize) {
        self.words[core / 64] |= bit(core);
    }

    pub fn remove(&mut self, core: usize) {
        self.words[core / 64] &= !bit(core);
    }

    pub fn contains(&self, core: usize) -> bool {
        self.words[core / 64] & bit(core) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&word| word == 0)
    }

    pub fn len(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// The cores in both sets.
    pub fn and(self, other: Self) -> Self {
        Self {
            words: core::array::from_fn(|index| self.words[index] & other.words[index]),
        }
    }

    /// The cores in `self` but not in `other`.
    pub fn without(self, other: Self) -> Self {
        Self {
            words: core::array::from_fn(|index| self.words[index] & !other.words[index]),
        }
    }

    /// Core indices, lowest first.
    pub fn iter(self) -> impl Iterator<Item = usize> {
        (0..MAX_CORES).filter(move |&core| self.contains(core))
    }
}

/// A `CoreSet` that several cores change at once. Each core's bit is
/// changed atomically; reading the whole set is a snapshot, word by word.
pub struct AtomicCoreSet {
    words: [AtomicU64; WORDS],
}

impl Default for AtomicCoreSet {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicCoreSet {
    pub const fn new() -> Self {
        Self {
            words: [const { AtomicU64::new(0) }; WORDS],
        }
    }

    pub fn insert(&self, core: usize) {
        self.words[core / 64].fetch_or(bit(core), Ordering::SeqCst);
    }

    pub fn remove(&self, core: usize) {
        self.words[core / 64].fetch_and(!bit(core), Ordering::SeqCst);
    }

    pub fn contains(&self, core: usize) -> bool {
        self.words[core / 64].load(Ordering::SeqCst) & bit(core) != 0
    }

    pub fn load(&self) -> CoreSet {
        CoreSet {
            words: core::array::from_fn(|index| self.words[index].load(Ordering::SeqCst)),
        }
    }

    pub fn store(&self, set: CoreSet) {
        for (word, value) in self.words.iter().zip(set.words) {
            word.store(value, Ordering::SeqCst);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words
            .iter()
            .all(|word| word.load(Ordering::Relaxed) == 0)
    }
}

fn bit(core: usize) -> u64 {
    1 << (core % 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_hold_cores_beyond_64() {
        let mut set = CoreSet::EMPTY;
        for core in [0, 63, 64, 200, 255] {
            set.insert(core);
        }
        assert_eq!(set.len(), 5);
        assert!(set.contains(200) && !set.contains(199));
        assert_eq!(set.iter().collect::<Vec<_>>(), [0, 63, 64, 200, 255]);
        set.remove(64);
        assert!(!set.contains(64));
    }

    #[test]
    fn set_operations() {
        let mut online = CoreSet::EMPTY;
        (0..100).for_each(|core| online.insert(core));
        let me = CoreSet::only(70);
        let others = online.without(me);
        assert_eq!(others.len(), 99);
        assert!(!others.contains(70));
        assert_eq!(online.and(me), me);
        assert!(CoreSet::EMPTY.is_empty() && !me.is_empty());
    }

    #[test]
    fn atomic_sets_snapshot_and_change_bit_by_bit() {
        let set = AtomicCoreSet::new();
        assert!(set.is_empty());
        set.insert(3);
        set.insert(130);
        assert!(set.contains(130));
        assert_eq!(set.load().iter().collect::<Vec<_>>(), [3, 130]);
        set.remove(3);
        set.store(set.load().without(CoreSet::only(130)));
        assert!(set.is_empty());
    }
}
