# Hematite – Architecture

How Hematite works today, where the code lives, and what comes next.
*Why* things are the way they are: [DECISIONS.md](DECISIONS.md).

## Where we're heading

An **ownership microkernel** (DECISIONS 012): the kernel only manages memory, switches tasks and
delivers messages. Drivers and services run as isolated user-space programs. Resources are
capabilities, and passing them between processes follows Rust's rules – *move*, *shared borrow*,
*exclusive borrow* – enforced by the page tables.

## Boot flow (what happens today)

```
UEFI firmware (OVMF in QEMU)
  └─ loads \EFI\BOOT\BOOTX64.EFI = our loader, calls efi_main          loader/src/main.rs
       ├─ copies the embedded kernel ELF into firmware memory,
       │  applies its relocations (static PIE)                           loader/src/kernel_image.rs, elf/
       ├─ final memory map → ExitBootServices → BootInfo                 loader/src/handoff.rs
       └─ jumps to kernel _start(&BootInfo)
            kernel _start: cli, own stack                                kernel/src/arch/start.rs
            kernel_main:                                                 kernel/src/main.rs
              ├─ serial port COM1 (all output)                           kernel/src/serial.rs
              ├─ own GDT + TSS, IDT, legacy PIC masked                   kernel/src/arch/
              ├─ int3 round trip (tests the interrupt path)
              ├─ APIC timer calibrated with the PIT, 100 Hz, sti         kernel/src/arch/apic.rs
              ├─ waits 10 ticks, prints memory summary
              ├─ "Hematite kernel ready."   ← the boot test looks for this line
              └─ idle: hlt between ticks, heartbeat every 10 s
```

All interrupts enter through one path: a 16-byte stub per vector → `interrupt_common` saves all
registers into an `InterruptFrame` → `interrupt_dispatch` in Rust (DECISIONS 008).

## Crates

| Crate | Builds for | What it is |
|---|---|---|
| `loader` | `x86_64-unknown-uefi` | UEFI app: loads the kernel, leaves the firmware. Hand-written UEFI types in `loader/src/uefi/`. |
| `kernel` | `x86_64-unknown-none` | The kernel. Everything CPU-specific is in `kernel/src/arch/`. |
| `boot_info` | both + host | The loader→kernel contract (`BootInfo`, memory regions). No UEFI types. |
| `elf` | any + host | Pure ELF parsing/loading/relocation. Unit-tested on the host. |
| `x86` | any + host | x86_64 formats as data (descriptors, exceptions, APIC/PIT registers). Unit-tested. |
| `xtask` | host | Build tool: `cargo xtask build / run / boot-test / test`. |

Rule of thumb: logic that is "bytes in, bytes out" goes into a host-testable crate with unit tests;
code that executes privileged instructions stays in `kernel`/`loader` and is covered by the
QEMU boot tests (DECISIONS 009).

## Status and roadmap

Done:
- [x] UEFI loader, embedded relocatable kernel, ExitBootServices, `BootInfo`
- [x] Serial output, GDT/TSS/IDT, exception reports (double fault on its own stack)
- [x] APIC timer at 100 Hz, uptime
- [x] Host unit tests + QEMU boot/crash tests, cross-platform `cargo xtask`

Next, in order (each one a milestone with its own decision entry):
1. **Physical page allocator** – hand out free 4 KiB frames from the `BootInfo` memory map.
2. **Own page tables + higher-half kernel** – kernel moves to the top of the address space;
   the lower half becomes per-process.
3. **Kernel heap** – `alloc` (Box, Vec) on top of the page allocator.
4. **Tasks and scheduler** – timer-driven switching by swapping `InterruptFrame`s.
5. **User mode + syscalls** – ring 3, `syscall`/`sysret`, first user program.
6. **Capabilities + ownership IPC** – handles, channels, page move/borrow (DECISIONS 012).
7. **Framebuffer** (UEFI GOP) – Hematite gets a screen.

## Known limits (on purpose, for now)

- Single core. No locks yet – interrupts just don't touch shared data except atomics.
- Runs on the firmware's identity-mapped page tables until roadmap step 2.
- Only tested in QEMU (q35). Real hardware: untested.
- `cargo xtask` firmware/QEMU discovery is tested on Windows only; Linux/macOS paths are best effort.
