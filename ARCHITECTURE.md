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
       │  relocates it for 0xFFFF_FFFF_8000_0000 (static PIE)            loader/src/kernel_image.rs, elf/
       ├─ allocates the handoff buffers, peeks at the memory map         loader/src/handoff.rs
       ├─ builds page tables: kernel (W^X), physical memory window,
       │  temporary identity map                                         loader/src/address_space.rs, paging/
       ├─ final memory map → ExitBootServices → BootInfo
       └─ enables NX + write protection, loads CR3, jumps to _start
            kernel _start: cli, own stack                                kernel/src/arch/start.rs
            kernel_main:                                                 kernel/src/main.rs
              ├─ serial port COM1 (all output)                           kernel/src/serial.rs
              ├─ own GDT + TSS, IDT, legacy PIC masked                   kernel/src/arch/
              ├─ int3 round trip (tests the interrupt path)
              ├─ APIC timer calibrated with the PIT, 100 Hz, sti         kernel/src/arch/apic.rs
              ├─ waits 10 ticks
              ├─ memory::start                                           kernel/src/memory/
              │    ├─ frame table: every RAM frame + its state           frame_table/
              │    ├─ removes the identity map → lower half empty
              │    ├─ reclaims firmware + loader memory
              │    └─ self-checks: frames, heap (Vec, Box, String, BTreeMap)  memory/checks.rs
              ├─ "Hematite kernel ready."   ← the boot test looks for this line
              └─ idle: hlt between ticks, heartbeat every 10 s
```

Address space (upper half = kernel, lower half = future user programs):

| Start | What | Defined in |
|---|---|---|
| `0xFFFF_FFFF_8000_0000` | kernel image (W^X) | `boot_info` |
| `0xFFFF_FF00_0000_0000` | kernel heap, 512 GiB, grows on demand | `kernel/src/memory/allocator.rs` |
| `0xFFFF_8000_0000_0000` | physical memory window (`memory::physical_to_pointer`) | `boot_info` |
| `0x0000_0000_0000_0000` | empty – for user programs | |

All interrupts enter through one path: a 16-byte stub per vector → `interrupt_common` saves all
registers into an `InterruptFrame` → `interrupt_dispatch` in Rust (DECISIONS 008).

## Crates

| Crate | Builds for | What it is |
|---|---|---|
| `loader` | `x86_64-unknown-uefi` | UEFI app: loads the kernel, builds its page tables, leaves the firmware. Hand-written UEFI types in `loader/src/uefi/`. |
| `kernel` | `x86_64-unknown-none` | The kernel. Everything CPU-specific is in `kernel/src/arch/`. |
| `boot_info` | both + host | The loader→kernel contract: `BootInfo`, memory regions, address space layout. No UEFI types. |
| `elf` | any + host | Pure ELF parsing/loading/relocation, segment permissions. Unit-tested on the host. |
| `paging` | any + host | 4-level page tables: map 4 KiB/2 MiB pages, translate, clear. Memory access via a trait. Unit-tested. |
| `frame_table` | any + host | Physical frame bookkeeping: state per frame, O(1) free list. Unit-tested. |
| `heap` | any + host | Heap allocator: size classes + page-backed large blocks. Unit-tested. |
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
- [x] Physical frame allocator: frame table with per-frame state (DECISIONS 014)
- [x] Own page tables, higher-half kernel with W^X, physical memory window, boot memory
      reclaimed (DECISIONS 015)
- [x] Kernel heap: `alloc` works – Box, Vec, String, BTreeMap (DECISIONS 016)

Next, in order (each one a milestone with its own decision entry):
1. **Tasks and scheduler** – timer-driven switching by swapping `InterruptFrame`s.
2. **User mode + syscalls** – ring 3, `syscall`/`sysret`, first user program in the lower half.
3. **Capabilities + ownership IPC** – handles, channels, page move/borrow (DECISIONS 012).
4. **Framebuffer** (UEFI GOP) – Hematite gets a screen.

## Known limits (on purpose, for now)

- Single core. `sync::SpinLock` disables interrupts while held, so it's already safe against
  interrupt handlers; it will need no change for multi-core.
- Heap: size-class pages are never returned, heap addresses never reused, alignment > 4 KiB refused.
- The identity map's tables (part of `page tables` memory) aren't freed after removing it – a few KiB.
- The physical memory window maps device memory with normal attributes; the firmware's MTRRs keep
  it uncached. Proper per-device mappings come with the first real drivers.
- Frames are handed out one at a time; nothing allocates physically contiguous blocks yet.
- Only tested in QEMU (q35). Real hardware: untested.
- `cargo xtask` firmware/QEMU discovery is tested on Windows only; Linux/macOS paths are best effort.
