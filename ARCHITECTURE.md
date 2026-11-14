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
              ├─ task::init: boot code becomes task 0, idle task created  kernel/src/task/
              ├─ task::check: 2 busy tasks + 1 sleeper must interleave   task/checks.rs
              ├─ user programs: hello must exit 0, intruder must be      programs/, task/program.rs
              │  stopped; both address spaces freed
              ├─ "Hematite kernel ready."   ← the boot test looks for this line
              └─ heartbeat: boot task sleeps 10 s at a time; idle task runs hlt
```

Address space (upper half = kernel, shared by all; lower half = one per user program):

| Start | What | Defined in |
|---|---|---|
| `0xFFFF_FFFF_8000_0000` | kernel image (W^X) | `boot_info` |
| `0xFFFF_FF00_0000_0000` | kernel heap, 512 GiB, grows on demand | `kernel/src/memory/allocator.rs` |
| `0xFFFF_8000_0000_0000` | physical memory window (`memory::physical_to_pointer`) | `boot_info` |
| `0x0000_7FFF_FFFF_0000` | top of a program's stack (16 KiB, guard page above) | `kernel/src/task/program.rs` |
| `0x0000_0000_0040_0000` | a program's image (W^X, user) | `kernel/src/task/program.rs` |

All interrupts enter through one path: a 16-byte stub per vector → `interrupt_common` saves all
registers into an `InterruptFrame` → `interrupt_dispatch` in Rust (DECISIONS 008). The dispatcher
returns the frame to resume; on a timer tick, `int 0x30` (kernel sleep/exit) or `int 0x80` (system
call) that may be another task's frame on another stack – that is the whole task switch
(DECISIONS 017). Switching to a user program also loads its CR3 and puts its kernel stack into the
TSS, where the CPU finds it on the next interrupt from ring 3 (DECISIONS 018).

System calls (`abi` crate): `int 0x80`, number in `rax`, arguments in `rdi`/`rsi`, result in `rax`.
Today: `exit`, `write`, `sleep`. Programs use `userlib` (wrappers, `println!`, panic handler).

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
| `scheduling` | any + host | Run queue: round robin, sleeping, idle fallback, freeing exited tasks. Unit-tested. |
| `x86` | any + host | x86_64 formats as data (descriptors, exceptions, APIC/PIT registers). Unit-tested. |
| `abi` | any + host | Kernel ↔ program contract: system call vector, numbers, error codes. |
| `userlib` | `x86_64-unknown-none` | What programs link against: system calls, `println!`, panic handler. |
| `programs/*` | `x86_64-unknown-none` | User programs, embedded into the kernel: `hello`, `intruder` (attacks the kernel on purpose). |
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
- [x] Preemptive kernel tasks: spawn, sleep, exit, round robin at 100 Hz (DECISIONS 017)
- [x] User mode: programs in ring 3 with own address spaces, system calls via `int 0x80`,
      faulty programs are stopped, not the kernel (DECISIONS 018)

Next, in order (each one a milestone with its own decision entry):
1. **Capabilities + ownership IPC** – handles, channels, page move/borrow (DECISIONS 012).
   Design first, together with the owner: this is the heart of Hematite.
2. **Framebuffer** (UEFI GOP) – Hematite gets a screen.

## Known limits (on purpose, for now)

- **Single core – to be fixed next** (ground rule "multi-core by design"). Already fine for SMP:
  `sync::SpinLock` (atomic, interrupts off while held), the frame table, heap and page-table locks.
  Not yet: one global TSS/GDT, one scheduler "current task", the APIC timer on one core only, and
  no TLB shootdown when mappings change.
- Heap: size-class pages are never returned, heap addresses never reused, alignment > 4 KiB refused.
- Tasks: 16 KiB kernel stacks without guard pages (an overflow corrupts the heap silently), no
  priorities, no blocking on events yet – only sleeping. An exception in a *kernel* task still
  stops the whole machine (in user programs it only ends the program).
- Programs: one task per program, fixed 16 KiB stack, no heap, embedded in the kernel. `write`
  prints with the kernel's lock held, so one program can't tear another's line apart.
- Upper-half top-level page table entries must all exist before the first program starts
  (they're copied, not shared) – see `kernel/src/memory/paging.rs`.
- The identity map's tables (part of `page tables` memory) aren't freed after removing it – a few KiB.
- The physical memory window maps device memory with normal attributes; the firmware's MTRRs keep
  it uncached. Proper per-device mappings come with the first real drivers.
- Frames are handed out one at a time; nothing allocates physically contiguous blocks yet.
- Only tested in QEMU (q35). Real hardware: untested.
- `cargo xtask` firmware/QEMU discovery is tested on Windows only; Linux/macOS paths are best effort.
