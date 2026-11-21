//! Facts about x86_64 as plain data: descriptor formats, exception numbers,
//! error-code bits, device register layouts. Nothing here executes a CPU
//! instruction – the kernel's `arch` module does that – so all of it can be
//! unit-tested on the host.

#![cfg_attr(not(test), no_std)]

pub mod apic;
pub mod exceptions;
pub mod gdt;
pub mod idt;
pub mod io_apic;
pub mod pat;
pub mod pit;
