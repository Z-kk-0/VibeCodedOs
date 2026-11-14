//! Bookkeeping for capabilities (DECISIONS.md, 020), generic and pure so
//! it's tested on the host:
//!
//! - `HandleTable`: what a process holds. A handle is a slot index plus the
//!   slot's generation, so a handle to something given up stays invalid
//!   even after the slot is reused.
//! - `MappingArea`: where in a process's address space the kernel maps the
//!   memory objects it owns or borrows.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod area;
mod table;

pub use area::MappingArea;
pub use table::{Handle, HandleTable};
