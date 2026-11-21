//! Device interrupts through the I/O APIC (register layout: `x86::io_apic`;
//! DECISIONS.md, 025).
//!
//! At boot every line of every I/O APIC is masked: no device interrupt
//! arrives until a line is routed on purpose. `route_isa` routes one of the
//! 16 old ISA interrupts (the keyboard's is 1) to vector `FIRST_VECTOR +
//! irq` on the boot core. ACPI says which I/O APIC input that IRQ really
//! is – "interrupt source overrides" move some, e.g. the PIT from 0 to 2.
//!
//! What happens when it fires is IPC's business (`crate::ipc::interrupt`):
//! the kernel only wakes the program that holds the line's handle.

use super::per_core::BOOT_CORE;
use super::smp::AcpiMemory;
use crate::sync::SpinLock;
use acpi::InterruptWiring;
use core::sync::atomic::Ordering;
use x86::io_apic::{self, MASKED, SELECT, Signal, WINDOW};

/// ISA IRQ `n` arrives on vector `FIRST_VECTOR + n`.
pub const FIRST_VECTOR: u64 = 0x50;
pub const ISA_IRQS: u8 = 16;

/// What `init` found: the chips, and where each ISA IRQ arrives.
static WIRING: SpinLock<Option<InterruptWiring>> = SpinLock::new(None);

/// The ISA IRQ a vector stands for, if it is one of ours.
pub fn isa_irq(vector: u64) -> Option<u8> {
    let irq = vector.checked_sub(FIRST_VECTOR)?;
    (irq < u64::from(ISA_IRQS)).then_some(irq as u8)
}

/// Reads the wiring from ACPI and masks every line. Returns how many
/// I/O APICs there are (0: no device interrupts on this machine).
pub fn init(acpi_root: u64) -> usize {
    if acpi_root == 0 {
        return 0;
    }
    let Ok(wiring) = acpi::find_interrupt_wiring(acpi_root, &AcpiMemory) else {
        return 0;
    };
    for chip in &wiring.io_apics {
        let lines = io_apic::entry_count(read(chip.address, io_apic::VERSION_REGISTER));
        for index in 0..lines {
            write_entry(chip.address, index, MASKED);
        }
    }
    let chips = wiring.io_apics.len();
    *WIRING.lock() = Some(wiring);
    chips
}

/// Lets ISA IRQ `irq` through, to the boot core. `false` if the machine
/// can't: no I/O APIC serves it, or it is level-triggered (a level line
/// would fire again at once, before its driver could serve the device –
/// that needs masking until the driver is done, which isn't built yet).
pub fn route_isa(irq: u8) -> bool {
    assert!(irq < ISA_IRQS, "ISA has 16 IRQs, not {irq}");
    let wiring = WIRING.lock();
    let Some(wiring) = wiring.as_ref() else {
        return false;
    };
    // Without an override, IRQ n is input n of the system ("identity").
    let (line, signal) = wiring
        .overrides
        .iter()
        .find(|entry| entry.irq == irq)
        .map_or((u32::from(irq), Signal::ISA), |entry| {
            (entry.line, Signal::from_override_flags(entry.flags))
        });
    if signal.level_triggered {
        return false;
    }
    let Some(chip) = wiring.io_apics.iter().find(|chip| {
        let lines = io_apic::entry_count(read(chip.address, io_apic::VERSION_REGISTER));
        (chip.first_line..chip.first_line + lines).contains(&line)
    }) else {
        return false;
    };
    // The I/O APIC addresses cores by 8-bit IDs; beyond that it would
    // need interrupt remapping. The boot core's is 0 on every PC we know.
    let Ok(apic_id) = u8::try_from(BOOT_CORE.apic_id.load(Ordering::Relaxed)) else {
        return false;
    };
    let vector = (FIRST_VECTOR + u64::from(irq)) as u8;
    write_entry(
        chip.address,
        line - chip.first_line,
        io_apic::entry(vector, apic_id, signal),
    );
    true
}

fn write_entry(chip: u32, index: u32, value: u64) {
    let (low, high) = io_apic::entry_registers(index);
    // Mask first, then the destination, then the low half (which unmasks):
    // the line never points half-way somewhere.
    write(chip, low, MASKED as u32);
    write(chip, high, (value >> 32) as u32);
    write(chip, low, value as u32);
}

fn read(chip: u32, register: u32) -> u32 {
    let base = crate::memory::physical_to_pointer(u64::from(chip));
    // SAFETY: `chip` is an I/O APIC's register window from ACPI, below
    // 4 GiB, so the physical memory window maps it (uncached: no RAM
    // there). Select-then-access is the chip's protocol; the `WIRING`
    // lock or boot (one core) keeps two cores from interleaving.
    unsafe {
        base.add(SELECT).cast::<u32>().write_volatile(register);
        base.add(WINDOW).cast::<u32>().read_volatile()
    }
}

fn write(chip: u32, register: u32, value: u32) {
    let base = crate::memory::physical_to_pointer(u64::from(chip));
    // SAFETY: as in `read`.
    unsafe {
        base.add(SELECT).cast::<u32>().write_volatile(register);
        base.add(WINDOW).cast::<u32>().write_volatile(value);
    }
}
