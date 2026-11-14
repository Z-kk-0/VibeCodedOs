//! The first instructions another core runs (DECISIONS.md, 019).
//!
//! A core woken by a startup IPI begins like a PC from 1981: 16-bit real
//! mode, at the start of the page named in the IPI, no paging. This code is
//! copied to such a page below 1 MiB and takes the core straight to 64-bit
//! long mode in one leap: it loads a tiny GDT, the boot core's CR4 and
//! EFER, page tables that map this page and the kernel, then the boot
//! core's CR0 – which switches on protected mode and paging together – and
//! far-jumps into a 64-bit code segment. From there it loads its stack and
//! jumps into the kernel proper (`ap_entry`) in the higher half.
//!
//! The kernel fills in the `startup_*` fields after copying. Real-mode code
//! addresses everything relative to the page start (CS = page / 16), so
//! fields are reached as `label - hematite_core_startup`; the 64-bit part
//! uses `rip`-relative addresses. Both work wherever the page is.

use crate::memory::physical_to_pointer;
use x86::gdt::{KERNEL_CODE_DESCRIPTOR, KERNEL_DATA_DESCRIPTOR};

core::arch::global_asm!(
    ".section .text",
    ".balign 16",
    ".global hematite_core_startup",
    ".global hematite_core_startup_long_mode",
    ".global hematite_startup_gdt, hematite_startup_gdt_pointer, hematite_startup_far_jump",
    ".global hematite_startup_cr0, hematite_startup_cr3, hematite_startup_cr4, hematite_startup_efer",
    ".global hematite_startup_stack, hematite_startup_entry",
    ".global hematite_startup_first_argument, hematite_startup_second_argument",
    "hematite_core_startup:",
    ".code16",
    "    cli",
    "    cld",
    "    movw %cs, %ax",
    "    movw %ax, %ds",
    // The 32-bit form of `lgdt` takes a 32-bit base: our page's address.
    "    lgdtl hematite_startup_gdt_pointer - hematite_core_startup",
    "    movl hematite_startup_cr4 - hematite_core_startup, %eax",
    "    movl %eax, %cr4",
    "    movl hematite_startup_cr3 - hematite_core_startup, %eax",
    "    movl %eax, %cr3",
    "    movl $0xC0000080, %ecx", // IA32_EFER: long mode enable, no-execute
    "    movl hematite_startup_efer - hematite_core_startup, %eax",
    "    xorl %edx, %edx",
    "    wrmsr",
    // Protected mode + paging + write protection at once: long mode is on.
    "    movl hematite_startup_cr0 - hematite_core_startup, %eax",
    "    movl %eax, %cr0",
    // Still running 16-bit code from this page; jump into 64-bit code.
    "    ljmpl *hematite_startup_far_jump - hematite_core_startup",
    ".code64",
    "hematite_core_startup_long_mode:",
    "    movw ${data}, %ax",
    "    movw %ax, %ds",
    "    movw %ax, %es",
    "    movw %ax, %ss",
    "    movq hematite_startup_stack(%rip), %rsp",
    "    movq hematite_startup_first_argument(%rip), %rdi",
    "    movq hematite_startup_second_argument(%rip), %rsi",
    "    movq hematite_startup_entry(%rip), %rax",
    "    jmpq *%rax",
    ".balign 8",
    // Null, 64-bit kernel code at 0x08, data at 0x10 – the kernel's layout.
    "hematite_startup_gdt:",
    "    .quad 0",
    "    .quad {code}",
    "    .quad {data_descriptor}",
    "hematite_startup_gdt_pointer:",
    "    .word 23",
    "    .long 0",
    ".balign 4",
    "hematite_startup_far_jump:",
    "    .long 0",
    "    .word {code_selector}",
    ".balign 4",
    "hematite_startup_cr0: .long 0",
    "hematite_startup_cr3: .long 0",
    "hematite_startup_cr4: .long 0",
    "hematite_startup_efer: .long 0",
    ".balign 8",
    "hematite_startup_stack: .quad 0",
    "hematite_startup_entry: .quad 0",
    "hematite_startup_first_argument: .quad 0",
    "hematite_startup_second_argument: .quad 0",
    ".global hematite_core_startup_end",
    "hematite_core_startup_end:",
    code = const KERNEL_CODE_DESCRIPTOR,
    data_descriptor = const KERNEL_DATA_DESCRIPTOR,
    code_selector = const super::super::gdt::KERNEL_CODE,
    data = const super::super::gdt::KERNEL_DATA,
    options(att_syntax),
);

unsafe extern "C" {
    static hematite_core_startup: u8;
    static hematite_core_startup_end: u8;
    static hematite_core_startup_long_mode: u8;
    static hematite_startup_gdt: u8;
    static hematite_startup_gdt_pointer: u8;
    static hematite_startup_far_jump: u8;
    static hematite_startup_cr0: u8;
    static hematite_startup_cr3: u8;
    static hematite_startup_cr4: u8;
    static hematite_startup_efer: u8;
    static hematite_startup_stack: u8;
    static hematite_startup_entry: u8;
    static hematite_startup_first_argument: u8;
    static hematite_startup_second_argument: u8;
}

/// Control register values a starting core copies from the boot core.
pub struct ControlRegisters {
    pub cr0: u32,
    pub cr4: u32,
    pub efer: u32,
}

/// The startup code copied to `code_page`, with everything filled in that
/// is the same for every core.
pub struct StartupPage {
    code_page: u64,
}

impl StartupPage {
    /// # Safety
    /// `code_page` must be a page below 1 MiB reserved for this, and
    /// `tables` the root of page tables that map it at its own address
    /// plus the kernel – and lie below 4 GiB.
    pub unsafe fn install(code_page: u64, tables: u64, registers: ControlRegisters) -> Self {
        let page = Self { code_page };
        let length = offset_of(&raw const hematite_core_startup_end);
        assert!(length <= 4096, "startup code must fit into one page");
        // SAFETY: the page is reserved for us and reached through the
        // window; the source is our own code, `length` bytes long.
        unsafe {
            let source = &raw const hematite_core_startup;
            physical_to_pointer(code_page).copy_from_nonoverlapping(source, length);
            page.write_u32(&raw const hematite_startup_cr0, registers.cr0);
            page.write_u32(&raw const hematite_startup_cr3, tables as u32);
            page.write_u32(&raw const hematite_startup_cr4, registers.cr4);
            page.write_u32(&raw const hematite_startup_efer, registers.efer);
            // `lgdt` and the far jump need real (linear) addresses.
            let gdt = code_page + offset_of(&raw const hematite_startup_gdt) as u64;
            let long_mode =
                code_page + offset_of(&raw const hematite_core_startup_long_mode) as u64;
            page.write_u32(add(&raw const hematite_startup_gdt_pointer, 2), gdt as u32);
            page.write_u32(&raw const hematite_startup_far_jump, long_mode as u32);
        }
        page
    }

    /// Sets what the next core started from this page runs:
    /// `entry(first, second)` on the stack ending at `stack_top`.
    pub fn prepare(&self, stack_top: u64, entry: u64, first: u64, second: u64) {
        // SAFETY: the fields lie inside our page (checked in `install`).
        unsafe {
            self.write_u64(&raw const hematite_startup_stack, stack_top);
            self.write_u64(&raw const hematite_startup_entry, entry);
            self.write_u64(&raw const hematite_startup_first_argument, first);
            self.write_u64(&raw const hematite_startup_second_argument, second);
        }
    }

    pub fn address(&self) -> u64 {
        self.code_page
    }

    /// # Safety
    /// `field` must be a label inside the startup code.
    unsafe fn write_u32(&self, field: *const u8, value: u32) {
        // SAFETY: forwarded; `read_unaligned`'s twin needs no alignment.
        unsafe { self.field(field).cast::<u32>().write_unaligned(value) };
    }

    /// # Safety
    /// `field` must be a label inside the startup code.
    unsafe fn write_u64(&self, field: *const u8, value: u64) {
        // SAFETY: forwarded.
        unsafe { self.field(field).cast::<u64>().write_unaligned(value) };
    }

    fn field(&self, field: *const u8) -> *mut u8 {
        physical_to_pointer(self.code_page + offset_of(field) as u64)
    }
}

/// Where `label` is, counted from the start of the startup code.
fn offset_of(label: *const u8) -> usize {
    label as usize - (&raw const hematite_core_startup) as usize
}

fn add(label: *const u8, bytes: usize) -> *const u8 {
    label.wrapping_add(bytes)
}
