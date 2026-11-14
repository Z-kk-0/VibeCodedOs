//! Starting the other CPU cores (DECISIONS.md, 019).
//!
//! At power-on only one core runs: the one we booted on. The others sleep
//! until they get the classic wake-up call over the APIC bus: INIT (reset,
//! then wait), and twice STARTUP ("begin in real mode at this page"). The
//! ACPI tables (MADT) tell us which APIC IDs exist; `startup_code.rs` takes
//! a woken core from real mode to `ap_entry` below.

mod startup_code;

use super::per_core::{self, MAX_CORES, PerCore, this_core};
use super::{apic, cpu, gdt, idt, pit};
use crate::memory::{self, paging};
use alloc::boxed::Box;
use alloc::vec;
use boot_info::{BootInfo, PAGE_SIZE};
use core::sync::atomic::{AtomicBool, Ordering};
use startup_code::{ControlRegisters, StartupPage};
use x86::apic::Ipi;

/// Interrupt that stops a core for good, after a kernel panic elsewhere.
pub const STOP_VECTOR: u8 = 0x41;

const KERNEL_STACK_SIZE: usize = 32 * 1024;
const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;
/// How long a core gets to report back before we give up on it.
const STARTUP_TIMEOUT_MILLISECONDS: u32 = 1000;
/// ACPI tables lie in the low 4 GiB, which the physical memory window
/// always covers (DECISIONS.md, 015).
const ACPI_READABLE_END: u64 = 4 << 30;

/// Set by a started core once it is part of the scheduler.
static READY: [AtomicBool; MAX_CORES] = [const { AtomicBool::new(false) }; MAX_CORES];

/// Starts every core the firmware lists, one after the other. Each one
/// joins the scheduler (via `crate::other_core_main`) before the next one
/// starts. Returns how many cores run now, this one included.
pub fn start_other_cores(boot_info: &BootInfo) -> usize {
    if boot_info.acpi_root == 0 || boot_info.core_startup == 0 {
        println!("No ACPI tables or no startup memory: staying on one core.");
        return 1;
    }
    let cores = match acpi::find_cores(boot_info.acpi_root, &AcpiMemory) {
        Ok(cores) => cores,
        Err(error) => {
            println!("ACPI tables unusable ({error:?}): staying on one core.");
            return 1;
        }
    };
    let code_page = boot_info.core_startup;
    let table_page = code_page + PAGE_SIZE;
    if paging::build_core_startup_tables(table_page, code_page).is_err() {
        println!("Out of memory for the startup page tables: staying on one core.");
        return 1;
    }
    // SAFETY: the loader reserved both pages below 1 MiB for exactly this;
    // the tables map the code page and the kernel.
    let page = unsafe { StartupPage::install(code_page, table_page, control_registers()) };

    let my_apic_id = apic::id();
    let mut next_index = 1;
    for core in cores.iter().filter(|core| core.apic_id != my_apic_id) {
        if next_index == MAX_CORES {
            println!("More than {MAX_CORES} cores - using only the first {MAX_CORES}.");
            break;
        }
        if !start(&page, core.apic_id, next_index) {
            println!(
                "Core with APIC ID {} did not start - not trying further cores.",
                core.apic_id
            );
            break;
        }
        next_index += 1;
    }
    paging::release_core_startup_tables(table_page);
    next_index
}

/// Wakes the core with `apic_id` as core number `index` and waits until it
/// joined the scheduler.
fn start(page: &StartupPage, apic_id: u32, index: usize) -> bool {
    let core: &'static PerCore = Box::leak(Box::new(PerCore::new(index)));
    core.apic_id.store(apic_id, Ordering::Relaxed);
    per_core::register(core);
    let stack_top = leaked_stack(KERNEL_STACK_SIZE);
    let double_fault_stack_top = leaked_stack(DOUBLE_FAULT_STACK_SIZE);
    page.prepare(
        stack_top,
        ap_entry as *const () as u64,
        core as *const PerCore as u64,
        double_fault_stack_top,
    );

    // The wake-up sequence from Intel's MP spec (SDM Vol. 3A, §9.4.4.1):
    // INIT, 10 ms, STARTUP, 200 µs, STARTUP again in case the first got lost.
    apic::send(Ipi::init(apic_id));
    pit::wait_microseconds(10_000);
    for _ in 0..2 {
        apic::send(Ipi::startup(apic_id, page.address()));
        pit::wait_microseconds(200);
        if READY[index].load(Ordering::Acquire) {
            break; // it was quick; a second STARTUP is ignored anyway
        }
    }
    for _ in 0..STARTUP_TIMEOUT_MILLISECONDS {
        if READY[index].load(Ordering::Acquire) {
            return true;
        }
        pit::wait_microseconds(1000);
    }
    false
}

/// Where a started core arrives in the kernel, still on the startup page
/// tables, interrupts off.
extern "sysv64" fn ap_entry(core: &'static PerCore, double_fault_stack_top: u64) -> ! {
    // SAFETY: we run in the higher half, which the kernel's tables map the
    // same way; the GDT and stacks were made for this core and live forever.
    unsafe {
        cpu::set_page_table_root(paging::kernel_root());
        gdt::init(core, double_fault_stack_top);
        idt::load();
    }
    // From here on `this_core()` works, so this core can take part in
    // shootdowns – before it allocates anything.
    per_core::set_online(core);
    crate::other_core_main()
}

/// For `crate::other_core_main`: this core has joined the scheduler.
pub fn report_ready() {
    READY[this_core().index].store(true, Ordering::Release);
}

/// Stops every other core: after a kernel panic or exception they would
/// only keep running on top of broken state.
pub fn stop_other_cores() {
    if apic::is_ready() && per_core::online_count() > 1 {
        apic::send(Ipi::fixed_to_all_others(STOP_VECTOR));
    }
}

/// Leaves scheduling and shootdowns for good.
pub fn stop_this_core() -> ! {
    if per_core::online_cores() != 0 {
        per_core::set_offline(this_core());
    }
    cpu::halt_forever()
}

fn control_registers() -> ControlRegisters {
    // Only the EFER bits a core may set itself: system calls, long mode,
    // no-execute (SDM Vol. 3A, §2.2.1). "Long mode active" is the CPU's.
    const EFER_WRITABLE: u64 = 1 << 0 | 1 << 8 | 1 << 11;
    // PCID can only be switched on once long mode is active.
    const CR4_PCID: u64 = 1 << 17;
    ControlRegisters {
        cr0: cpu::cr0() as u32,
        cr4: (cpu::cr4() & !CR4_PCID) as u32,
        efer: (cpu::efer() & EFER_WRITABLE) as u32,
    }
}

/// A stack nobody ever frees; returns its (16-byte aligned) top.
fn leaked_stack(size: usize) -> u64 {
    let stack = Box::leak(vec![0u8; size].into_boxed_slice());
    (stack.as_mut_ptr() as u64 + size as u64) & !0xF
}

/// ACPI tables, read through the physical memory window.
struct AcpiMemory;

impl acpi::PhysicalMemory for AcpiMemory {
    fn bytes(&self, address: u64, length: usize) -> Option<&[u8]> {
        let end = address.checked_add(length as u64)?;
        if end > ACPI_READABLE_END {
            return None;
        }
        // SAFETY: the window maps the whole range (checked above); firmware
        // tables are never written while we read them.
        Some(unsafe { core::slice::from_raw_parts(memory::physical_to_pointer(address), length) })
    }
}
