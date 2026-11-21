//! Device interrupts through the I/O APIC (register layout: `x86::io_apic`;
//! DECISIONS.md, 025).
//!
//! At boot every line of every I/O APIC is masked: no device interrupt
//! arrives until a line is routed on purpose. `route_isa` routes one of the
//! 16 old ISA interrupts (the keyboard's is 1) to vector `FIRST_VECTOR +
//! irq`. ACPI says which I/O APIC input that IRQ really is – "interrupt
//! source overrides" move some, e.g. the PIT from 0 to 2 – and whether it
//! is edge- or level-triggered.
//!
//! Each routed line goes to another core, round robin, starting with the
//! second: device interrupts don't pile up on the core that keeps the clock.
//!
//! A level-triggered line keeps firing as long as its device wants
//! attention – which it does until its driver has served it. So when it
//! fires it is masked (`on_fire`), and unmasked when the driver says it is
//! done (`acknowledge`, on its next wait). Edge-triggered lines stay open:
//! masked, the chip would drop an edge for good, and a keyboard controller
//! whose byte nobody saw would never send another.
//!
//! What happens when it fires is IPC's business (`crate::ipc::interrupt`):
//! the kernel only wakes the program that holds the line's handle.

use super::per_core::{core, online_cores};
use super::smp::AcpiMemory;
use crate::sync::SpinLock;
use acpi::InterruptWiring;
use core::sync::atomic::Ordering;
use x86::io_apic::{self, MASKED, SELECT, Signal, WINDOW};

/// ISA IRQ `n` arrives on vector `FIRST_VECTOR + n`.
pub const FIRST_VECTOR: u64 = 0x50;
pub const ISA_IRQS: u8 = 16;

/// Everything about the chips; the lock also keeps two cores from
/// interleaving their select-then-access register steps.
static CHIPS: SpinLock<Chips> = SpinLock::new(Chips {
    wiring: None,
    routes: [None; ISA_IRQS as usize],
    next_core: 1,
});

struct Chips {
    wiring: Option<InterruptWiring>,
    routes: [Option<Route>; ISA_IRQS as usize],
    /// Which online core (by position) the next routed line goes to.
    next_core: usize,
}

/// Where a routed ISA IRQ is, and its redirection entry while unmasked.
#[derive(Clone, Copy)]
struct Route {
    chip: u32,
    index: u32,
    entry: u64,
    level_triggered: bool,
}

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
    CHIPS.lock().wiring = Some(wiring);
    chips
}

/// How a routed line arrives.
pub struct Routed {
    /// The core it interrupts (its index, 0 = the boot core).
    pub core: usize,
    pub level_triggered: bool,
}

/// Lets ISA IRQ `irq` through, to the next core in turn. `None` if the
/// machine can't: no I/O APIC serves it.
pub fn route_isa(irq: u8) -> Option<Routed> {
    assert!(irq < ISA_IRQS, "ISA has 16 IRQs, not {irq}");
    let mut chips = CHIPS.lock();
    let wiring = chips.wiring.as_ref()?;
    // Without an override, IRQ n is input n of the system ("identity").
    let (line, signal) = wiring
        .overrides
        .iter()
        .find(|entry| entry.irq == irq)
        .map_or((u32::from(irq), Signal::ISA), |entry| {
            (entry.line, Signal::from_override_flags(entry.flags))
        });
    let chip = wiring.io_apics.iter().find(|chip| {
        let lines = io_apic::entry_count(read(chip.address, io_apic::VERSION_REGISTER));
        (chip.first_line..chip.first_line + lines).contains(&line)
    })?;
    let (chip, index) = (chip.address, line - chip.first_line);
    let (core_index, apic_id) = next_core(&mut chips.next_core);
    let vector = (FIRST_VECTOR + u64::from(irq)) as u8;
    let route = Route {
        chip,
        index,
        entry: io_apic::entry(vector, apic_id, signal),
        level_triggered: signal.level_triggered,
    };
    write_entry(chip, index, route.entry);
    chips.routes[usize::from(irq)] = Some(route);
    Some(Routed {
        core: core_index,
        level_triggered: signal.level_triggered,
    })
}

/// The interrupt handler calls this first: a level-triggered line is
/// masked until its driver is done (`acknowledge`).
pub fn on_fire(irq: u8) {
    let chips = CHIPS.lock();
    if let Some(route) = chips.routes[usize::from(irq)]
        && route.level_triggered
    {
        let (low, _) = io_apic::entry_registers(route.index);
        write(route.chip, low, (route.entry | MASKED) as u32);
    }
}

/// The driver of `irq` is done with its device: a level-triggered line is
/// unmasked again (if the device still wants attention, it fires at once).
pub fn acknowledge(irq: u8) {
    let chips = CHIPS.lock();
    if let Some(route) = chips.routes[usize::from(irq)]
        && route.level_triggered
    {
        let (low, _) = io_apic::entry_registers(route.index);
        write(route.chip, low, route.entry as u32);
    }
}

/// The next online core in turn whose APIC ID the I/O APIC can address
/// (8 bits; beyond that it would need interrupt remapping) – with one
/// core, or none such, the boot core. Returns (index, APIC ID).
fn next_core(next: &mut usize) -> (usize, u8) {
    let online = online_cores();
    let count = online.len();
    for _ in 0..count {
        let position = *next % count;
        *next += 1;
        let Some(index) = online.iter().nth(position) else {
            continue;
        };
        if let Ok(apic_id) = u8::try_from(core(index).apic_id.load(Ordering::Relaxed)) {
            return (index, apic_id);
        }
    }
    let boot_id = core(0).apic_id.load(Ordering::Relaxed);
    // The boot core's APIC ID is 0 on every PC we know.
    (0, u8::try_from(boot_id).unwrap_or(0))
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
    // there). Select-then-access is the chip's protocol; the `CHIPS`
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
