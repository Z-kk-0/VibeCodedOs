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
       ├─ finds the ACPI root pointer, reserves 2 pages below 1 MiB
       │  where the other cores will start                               loader/src/main.rs
       ├─ allocates the handoff buffers, peeks at the memory map         loader/src/handoff.rs
       ├─ builds page tables: kernel (W^X), physical memory window,
       │  temporary identity map                                         loader/src/address_space.rs, paging/
       ├─ final memory map → ExitBootServices → BootInfo
       └─ enables NX + write protection, loads CR3, jumps to _start
            kernel _start: cli, own stack                                kernel/src/arch/start.rs
            kernel_main:                                                 kernel/src/main.rs
              ├─ serial port COM1 (all output)                           kernel/src/serial.rs
              ├─ own GDT + TSS (in the boot core's PerCore), IDT,
              │  legacy PIC masked                                       kernel/src/arch/
              ├─ int3 round trip (tests the interrupt path)
              ├─ APIC timer calibrated with the PIT, 100 Hz, sti         kernel/src/arch/apic.rs
              ├─ waits 10 ticks
              ├─ memory::start                                           kernel/src/memory/
              │    ├─ frame table: every RAM frame + its state           frame_table/
              │    ├─ removes the identity map (frees its tables), unmaps
              │    │  the boot stack's guard page → lower half empty
              │    ├─ reclaims firmware + loader memory
              │    └─ self-checks: frames, heap (Vec, Box, String, BTreeMap)  memory/checks.rs
              ├─ task::init: boot code becomes task 0, idle task created  kernel/src/task/
              ├─ start_other_cores: ACPI MADT → APIC IDs; for each core   kernel/src/arch/smp/, acpi/
              │    INIT, STARTUP ×2 → startup code: real mode → long mode
              │    → ap_entry: kernel page tables, own GDT/TSS, IDT
              │    → other_core_main: joins scheduler, timer on, idles   kernel/src/main.rs
              ├─ "N cores online."          ← the boot test checks N
              ├─ task::check: 4 busy tasks + 1 sleeper must interleave,
              │  on more than one core; freeing their stacks must have
              │  sent TLB shootdowns                                     task/checks.rs
              ├─ ipc::check: starts only `init` (console + 8 Program     kernel/src/ipc/checks.rs
              │  handles), waits for its exit code                       programs/init
              │    init: endpoints, badged sending ends, starts log,
              │    dir, vandal, hello, owner, giver, shout, intruder;
              │    checks how each test program ended → exit 0 / n
              │  afterwards: log, dir, shout keep running, the rest freed
              ├─ "Hematite kernel ready."   ← the boot test looks for this line
              └─ heartbeat: boot task sleeps 10 s at a time; idle tasks run hlt
```

Physical memory the loader reserves for starting cores (`MemoryKind::CoreStartup`, never freed):
page 1 = the startup code (`kernel/src/arch/smp/startup_code.rs`, copied there), page 2 = the root
of the page tables cores start on.

Address space (upper half = kernel, shared by all; lower half = one per user program):

| Start | What | Defined in |
|---|---|---|
| `0xFFFF_FFFF_8000_0000` | kernel image (W^X) | `boot_info` |
| `0xFFFF_FF00_0000_0000` | kernel heap, 448 GiB, grows on demand | `kernel/src/memory/allocator.rs` |
| `0xFFFF_FF70_0000_0000` | kernel stacks, 64 KiB slots with guard pages | `kernel/src/memory/stacks.rs` |
| `0xFFFF_8000_0000_0000` | physical memory window (`memory::physical_to_pointer`) | `boot_info` |
| `0x0000_7FFF_FFFF_0000` | top of a program's stack (grows on demand up to 8 MiB; guard pages above and below) | `kernel/src/task/program.rs` |
| `0x0000_1000_0000_0000` | a program's memory objects, owned or borrowed (guard page after each) | `kernel/src/ipc/process.rs` |
| `0x0000_0000_0040_0000` | a program's image (W^X, user) | `kernel/src/task/program.rs` |

All interrupts enter through one path: a 16-byte stub per vector → `interrupt_common` saves all
registers into an `InterruptFrame` → `interrupt_dispatch` in Rust (DECISIONS 008). The dispatcher
returns the frame to resume; on a timer tick, `int 0x30` (kernel sleep/exit) or `int 0x80` (system
call) that may be another task's frame on another stack – that is the whole task switch
(DECISIONS 017). Switching to a user program also loads its CR3 and puts its kernel stack into the
TSS, where the CPU finds it on the next interrupt from ring 3 (DECISIONS 018).

Multi-core (DECISIONS 019): every core runs the same path. A core finds its own data
(`arch/per_core.rs`: GDT, TSS, index, APIC ID) with `sgdt`, because its GDT is the first field of
its `PerCore`. All cores share one run queue (one lock), each with its own current and idle task.
Besides the frame, the dispatcher returns the previous task's `leaving` flag; the stub clears it
once it switched stacks, and only then may another core run or free that task. Changing a mapping
sends a TLB shootdown (`arch/shootdown.rs`) to the cores that have that address space loaded.

| Vector | What |
|---|---|
| `0x00`–`0x1F` | CPU exceptions (`0x02` NMI doubles as "stop" after a kernel panic elsewhere) |
| `0x20` | APIC timer (every core; only core 0 advances the clock) |
| `0x30` | yield (`int 0x30`, kernel tasks) |
| `0x40` | TLB shootdown IPI |
| `0x80` | system call (`int 0x80`, callable from ring 3) |
| `0xFF` | APIC spurious interrupt |

System calls (`abi` crate, register table there): `int 0x80`, number in `rax`. `exit`, `sleep`,
and the capability calls (DECISIONS 020, 021): `call`, `receive`, `reply`, `memory_new`, `drop`,
`endpoint_new`, `mint`, `revoke`. Kernel objects answer `call` at once: the console prints, a
`Program` starts a process, a `Child` says how it ended.
Programs use `userlib` (typed wrappers, `println!` via the `log` service, panic handler).

Capabilities and IPC (`kernel/src/ipc/`, DECISIONS 020, 021): each process (`ipc/process.rs`) has
an address space, a handle table and what it waits for. A message is four words plus at most one
handle, moved between the tasks' saved frames under the scheduler lock; every received message
comes with a one-shot reply handle. Memory objects change hands by the rules in `ipc/transfer.rs`:
move (sender loses the mapping), lend `&` (read-only for both), lend `&mut` (lender loses the
mapping) – a borrow ends with its reply handle. Endpoints (`ipc/endpoint.rs`) close when their
last receiving end is gone and keep a generation per badge for `revoke`; queue waits can time out
(`ipc/timeouts.rs`). The kernel starts only `init`, which wires everything else up.

| Program | Kind | Handles it starts with | What it proves |
|---|---|---|---|
| `init` | first program | 0 console, 1–8 programs | user space wires itself up |
| `log` | service | 0 log endpoint (receive), 1 console | the only one that can print |
| `dir` (`directory`) | service | 0 log, 1 directory endpoint (receive) | names; parked lookups (reply handles) |
| `shout` | service | 0 log, 1 directory (registers itself) | `&mut` borrow and move work |
| `vandal` | attacker | 0 log, 1 vandal endpoint (receive) | writing into a `&` borrow faults |
| `hello` | program | 0 log | printing through `log` |
| `owner` | program | 0 log, 1 directory, 2 vandal | lent memory comes back; closed endpoints, timeouts, revoke |
| `giver` | attacker | 0 log, 1 directory | a moved page is gone |
| `intruder` | attacker | 0 log | forged handles and kernel reads fail |

## Crates

| Crate | Builds for | What it is |
|---|---|---|
| `loader` | `x86_64-unknown-uefi` | UEFI app: loads the kernel, builds its page tables, leaves the firmware. Hand-written UEFI types in `loader/src/uefi/`. |
| `kernel` | `x86_64-unknown-none` | The kernel. Everything CPU-specific is in `kernel/src/arch/`. |
| `boot_info` | both + host | The loader→kernel contract: `BootInfo` (memory regions, ACPI root, core startup pages), address space layout. No UEFI types. |
| `acpi` | any + host | Just enough ACPI to find the CPU cores: RSDP → XSDT/RSDT → MADT, checksums. Memory access via a trait. Unit-tested. |
| `elf` | any + host | Pure ELF parsing/loading/relocation, segment permissions. Unit-tested on the host. |
| `paging` | any + host | 4-level page tables: map 4 KiB/2 MiB pages, translate, clear. Memory access via a trait. Unit-tested. |
| `frame_table` | any + host | Physical frame bookkeeping: state per frame, O(1) free list. Unit-tested. |
| `heap` | any + host | Heap allocator: size classes in self-aligned slabs (returned when empty) + page-backed large blocks. Used by the kernel and by `userlib`. Unit-tested. |
| `scheduling` | any + host | Run queue shared by all cores: round robin, a current and an idle task per core, sleeping, freeing exited tasks. Unit-tested. |
| `x86` | any + host | x86_64 formats as data (descriptors, exceptions, APIC/PIT registers, IPIs). Unit-tested. |
| `capability` | any + host | Handle tables (index + generation) and the mapping area for memory objects. Unit-tested. |
| `abi` | any + host | Kernel ↔ program contract: system call vector, numbers, registers, attach modes, error codes, badges. |
| `userlib` | `x86_64-unknown-none` | What programs link against: typed IPC (`Memory`, `Attach`, `call`…), `spawn`/`wait`, a heap (`alloc`), `println!`, panic handler. |
| `programs/*` | `x86_64-unknown-none` | User programs and services, embedded into the kernel (table above). |
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
- [x] Multi-core: every core the ACPI tables list is started and schedules tasks; per-core
      GDT/TSS/timer/idle task, TLB shootdown via IPIs (DECISIONS 019)
- [x] Capabilities + ownership IPC: handle tables, endpoints, `call`/`receive`/`reply`, memory
      moved or lent `&`/`&mut` – enforced by the page tables; `log` service (DECISIONS 020)
- [x] IPC without limits: reply handles, closed endpoints, timeouts, revoke per badge; `init`
      starts and wires up everything in user space, `directory` service (DECISIONS 021)

Next, in order (each one a milestone with its own decision entry):
1. **Framebuffer** (UEFI GOP) – Hematite gets a screen. Design with the owner first: probably the
   first real driver in user space, which needs a device-memory capability.

## Known limits (on purpose, for now)

- Multi-core (DECISIONS 019): at most 64 cores, xAPIC mode only (APIC IDs up to 255; firmware
  that already switched to x2APIC isn't supported). One run queue lock for all cores; no CPU
  affinity. One shootdown request at a time (each covers a whole range).
- Heap addresses are never reused (on purpose, like memory objects; empty page tables are freed).
- Tasks: no priorities. Tasks block only for sleeping and IPC. An exception in a *kernel* task
  still stops the whole machine (in user programs it only ends the program).
- Programs: one task per program, embedded in the kernel (no file system).
- IPC (DECISIONS 020–022): memory objects ≤ 256 MiB; directory names are first come, first served.
- Upper-half top-level page table entries must all exist before the first program starts
  (they're copied, not shared) – `paging::map` panics otherwise; today all do.
- The physical memory window maps device memory with normal attributes; the firmware's MTRRs keep
  it uncached. Proper per-device mappings come with the first real drivers.
- Frames are handed out one at a time; nothing allocates physically contiguous blocks yet.
- Only tested in QEMU (q35). Real hardware: untested.
- `cargo xtask` firmware/QEMU discovery is tested on Windows only; Linux/macOS paths are best effort.
