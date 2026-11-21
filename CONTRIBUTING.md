# Working on Hematite

Everything you need to pick this project up – setup, commands, workflow, conventions.
How it works: [ARCHITECTURE.md](ARCHITECTURE.md). Why: [DECISIONS.md](DECISIONS.md).

## Setup

1. **Rust** via [rustup](https://rustup.rs). The exact toolchain, components and targets are
   pinned in `rust-toolchain.toml` and installed automatically on the first `cargo` call.
2. **QEMU** (x86_64 system emulator) with the **OVMF/EDK II UEFI firmware**:
   - Windows: `winget install SoftwareFreedomConservancy.QEMU` (firmware included)
   - Debian/Ubuntu: `apt install qemu-system-x86 ovmf`
   - Fedora: `dnf install qemu-system-x86 edk2-ovmf` · Arch: `pacman -S qemu-desktop edk2-ovmf`
   - macOS: `brew install qemu` (firmware included)

   If `cargo xtask` can't find them, set `HEMATITE_QEMU` / `HEMATITE_OVMF` to the full paths.
   QEMU gets 4 cores; `HEMATITE_CORES=1` (up to 255) changes that for `run` and `boot-test`;
   `--many-cores` boots 80 cores on a CPU with x2APIC.

## Commands

| Command | What it does |
|---|---|
| `cargo xtask run` | Build and boot in a QEMU window; kernel log appears in your terminal. |
| `cargo xtask test` | **Every check** – formatting, clippy, unit tests, boot tests (4 cores, and 80 cores with x2APIC), crash tests. Must be green before a commit. |
| `cargo xtask test --quick` | Same without QEMU (seconds). |
| `cargo xtask boot-test [--crash] [--crash-stack]` | Headless boot, checks the serial log – and after a normal boot types "Hello, Hematite!" + Enter on the emulated keyboard (QEMU monitor `sendkey`; the terminal must log the line) and takes a screenshot (QEMU `screendump` to `build/screen.ppm`; pixels of `paint`'s picture and of the terminal strip must be right). `--crash` builds a kernel in which a task page-faults while holding a lock, `--crash-stack` one that overflows its stack (the guard page must catch it) – kernel bugs that must stop the machine (DECISIONS 024). |
| `cargo xtask symbolize <address>...` | Names the kernel function each hex address lies in (debug build). |
| `cargo xtask build [--release]` | Only build; boot folder ends up in `build/esp`. |
| `cargo test` | Unit tests of the host-testable crates only. |

## Workflow

1. Branch per milestone (`git switch -c <topic>`).
2. Work in small steps; run `cargo xtask test` before every commit.
3. A design choice → new numbered entry in `DECISIONS.md` (what, why, alternatives – short).
4. Changed how things work or finished a roadmap step → update `ARCHITECTURE.md` in the same commit.
5. Fast-forward merge into `main`, push.

`README.md` belongs to the project owner – don't edit it.

## Conventions

The coding rules are in [DECISIONS.md → Coding rules](DECISIONS.md#coding-rules). In short:
readable but not slow, files under ~200 lines, names that say what things are, comments explain
*why* and cite specs, every `unsafe` block has a `// SAFETY:` comment, hardware details stay at
the edges, code and docs in English.

**Where does new code go?**
- Pure logic (parsing, encoding, bookkeeping, math) → `elf`, `x86`, `acpi`, `capability`, `frame_table`, `paging`, `heap`, `scheduling`, `boot_info` or a
  new small crate (add it to the workspace `members` *and* `default-members`), with unit tests.
- Code that runs privileged instructions → `kernel/src/arch/` (or `loader/`), covered by boot tests.
- A new user program → a crate in `programs/` using `userlib`, plus: workspace `members` (not
  `default-members`), `PROGRAMS` in `xtask/src/build.rs`, a `program!` line in
  `kernel/src/programs.rs`, an entry in `PROGRAMS` in `kernel/src/ipc/checks.rs` (init gets a
  `Program` handle for it), and its start in `programs/init` with the handles it gets (document
  them at the top of its `main.rs`). The device handles (screen, keyboard ports, keyboard
  interrupt) come right after the programs, so their numbers in `programs/init` move up by one.
- New boot-time behaviour → make sure the boot test still reaches `Hematite kernel ready.`;
  if it can fail silently, print something the boot test can check.

## Gotchas

- **Kernel output must be ASCII.** The serial log is shown by various terminals; non-ASCII gets mangled.
- **The loader embeds the kernel at compile time** (`include_bytes!` via `HEMATITE_KERNEL_ELF`).
  Always build through `cargo xtask`, which builds the kernel first.
- **`--crash` and `--crash-stack` are compile-time switches** (`option_env!("HEMATITE_CRASH_TEST")`
  in the kernel).
  `xtask` sets/clears it for every build, so a normal build never contains the crash.
- **Kernel and loader can't be built with a plain `cargo build`** – they need `--target`.
  Plain `cargo build/test/clippy` only covers the host-buildable crates (workspace `default-members`).
- **UEFI watchdog:** the firmware resets the machine after ~5 minutes if the loader never calls
  `ExitBootServices` (e.g. it hangs while still in the firmware).
- **Physical addresses aren't pointers.** The kernel runs in the higher half; reach physical memory
  only through `memory::physical_to_pointer` (the window). Low addresses fault – on purpose.
- **`println!` takes a lock.** In panic, exception or other "something is already broken" paths use
  `emergency_println!`, which can't deadlock.
- **Locks switch interrupts off** (`sync::SpinLock`), so a task can't be preempted while holding one
  – keep critical sections short, and never call `sleep` or `exit` while holding a lock.
  The kernel also relies on it when it fails (DECISIONS 024): a bug where interrupts are on is
  taken as "no lock held" and only ends that task. So never switch interrupts on while holding a
  lock, and keep them off in multi-step lock-free code that must not be cut off halfway.
- **Everything runs on several cores at once** (DECISIONS 019). Shared data needs a lock or atomics;
  "the other task is blocked" is no protection. Never busy-wait for another core with interrupts
  off except inside `SpinLock::lock` – its spin loop answers TLB shootdowns, anything else would
  deadlock the core waiting for your answer.
- **`arch::core_index()` / `this_core()` need the core's own GDT** (it's how a core recognises
  itself). It's loaded in `arch::init` (boot core) and first thing in `smp::ap_entry`.
- **Changing a mapping needs a shootdown**: kernel pages via `paging::unmap`, process pages via
  `AddressSpace::map` – both do it. Don't call `invlpg` directly for shared mappings.
- **Device memory isn't RAM** (DECISIONS 023): never reach it through `physical_to_pointer` (it
  may lie outside the window, and only its owner may touch it), never free its frames, and map it
  only with its memory object's `caching()`.
- **Device interrupts are IPC** (DECISIONS 025): a new device line needs `arch::route_isa_interrupt`
  at boot and an `Interrupt` handle for `init` – nothing else in the kernel. Only edge-triggered
  ISA lines work (a level-triggered line would fire again before its driver ran). ISA IRQ `n` is
  vector `0x50 + n`; keep other vectors out of `0x50`–`0x5F`.
- **Programs get nothing for free** (DECISIONS 020): a program can only use the handles it was
  started with (by `init`) or received. Data goes in memory objects, never in messages. The `log` service
  must not use `println!` – that would be a call to itself.
- **Debugging a hang:** `cargo xtask run`, then look at the last serial line. QEMU's `-d int`
  (add it in `xtask/src/qemu.rs`) logs every interrupt and exception. If it hangs silently, add
  `-monitor stdio` to QEMU, type `info registers -a`, and pass the cores' `RIP`s (and any register
  that looks like a kernel address, e.g. the lock in `RDI`) to `cargo xtask symbolize`. Many
  cores at one `SpinLock::lock` and one elsewhere is a lock someone never let go.
