//! Sharing data inside the kernel.

use crate::arch::{self, cpu};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// A lock that spins until it's free – on another core, the holder keeps
/// running meanwhile. While held, interrupts are off on this core:
/// otherwise an interrupt handler wanting the same lock would wait forever
/// for code that can't continue until the handler returns.
pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: the lock hands out access to `value` to one holder at a time.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// The lock if it's free right now, without waiting.
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        let interrupts_were_on = cpu::disable_interrupts();
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Some(SpinLockGuard {
                lock: self,
                interrupts_were_on,
            });
        }
        if interrupts_were_on {
            // SAFETY: they were on before, so every handler is set up.
            unsafe { cpu::enable_interrupts() };
        }
        None
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let interrupts_were_on = cpu::disable_interrupts();
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // With interrupts off we'd never hear a shootdown request – and
            // its sender may be the one holding this lock (DECISIONS.md, 019).
            if arch::shootdown_pending() {
                arch::serve_shootdown();
            }
            core::hint::spin_loop();
        }
        SpinLockGuard {
            lock: self,
            interrupts_were_on,
        }
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    interrupts_were_on: bool,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the lock, so nobody else accesses the value.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the lock, so nobody else accesses the value.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        if self.interrupts_were_on {
            // SAFETY: they were on before `lock`, so every handler is set up.
            unsafe { cpu::enable_interrupts() };
        }
    }
}
