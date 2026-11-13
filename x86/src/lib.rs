//! Facts about x86_64 as plain data: descriptor formats, exception numbers,
//! error-code bits. Nothing here executes a CPU instruction – the kernel's
//! `arch` module does that – so all of it can be unit-tested on the host.

#![cfg_attr(not(test), no_std)]

pub mod exceptions;
pub mod gdt;
pub mod idt;
