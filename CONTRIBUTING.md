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

## Commands

| Command | What it does |
|---|---|
| `cargo xtask run` | Build and boot in a QEMU window; kernel log appears in your terminal. |
| `cargo xtask test` | **Every check** – formatting, clippy, unit tests, boot test, crash test. Must be green before a commit. |
| `cargo xtask test --quick` | Same without QEMU (seconds). |
| `cargo xtask boot-test [--crash]` | Headless boot, checks the serial log. `--crash` builds a kernel that page-faults on purpose. |
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
- Pure logic (parsing, encoding, bookkeeping, math) → `elf`, `x86`, `frame_table`, `boot_info` or a
  new small crate (add it to the workspace `members` *and* `default-members`), with unit tests.
- Code that runs privileged instructions → `kernel/src/arch/` (or `loader/`), covered by boot tests.
- New boot-time behaviour → make sure the boot test still reaches `Hematite kernel ready.`;
  if it can fail silently, print something the boot test can check.

## Gotchas

- **Kernel output must be ASCII.** The serial log is shown by various terminals; non-ASCII gets mangled.
- **The loader embeds the kernel at compile time** (`include_bytes!` via `HEMATITE_KERNEL_ELF`).
  Always build through `cargo xtask`, which builds the kernel first.
- **`--crash` is a compile-time switch** (`option_env!("HEMATITE_CRASH_TEST")` in the kernel).
  `xtask` sets/clears it for every build, so a normal build never contains the crash.
- **Kernel and loader can't be built with a plain `cargo build`** – they need `--target`.
  Plain `cargo build/test/clippy` only covers the host-buildable crates (workspace `default-members`).
- **UEFI watchdog:** the firmware resets the machine after ~5 minutes if the loader never calls
  `ExitBootServices` (e.g. it hangs while still in the firmware).
- **Debugging a hang:** `cargo xtask run`, then look at the last serial line. QEMU's `-d int`
  (add it in `xtask/src/qemu.rs`) logs every interrupt and exception.
