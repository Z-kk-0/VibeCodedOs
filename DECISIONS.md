# Hematite – Decisions

Short log of architecture decisions. Per entry: what, why, alternatives.

## Ground rules
- OS from scratch, for learning. One programming language (plus the unavoidable bit of assembly).
- No big external libraries that do the real work (bootloader crates, ready-made paging/interrupt libs, etc.).
- **Not a textbook clone.** The usual path is a reference, not a rule. Where a less common design is better or more interesting, we take it and say why.
- **Handover-ready at every commit.** Anyone – human or AI – must be able to pick this up cold. Docs (`CONTRIBUTING.md`, `ARCHITECTURE.md`, this file) change in the same commit as the code they describe. Tooling is plain Rust and cross-platform (`cargo xtask`).

## Coding rules
- **Readable first, but not slow.** Clear code over clever code; only optimize where it measurably matters, and then explain it in a comment.
- **Small files.** One concept per module, soft limit ~200 lines. Bigger → split into a folder module.
- **Names say what things are.** No cryptic abbreviations, except names from the specs (`efi_main`, `GDT`) – those link to the spec.
- **Comments explain why, not what.** Hardware/spec facts cite their source (e.g. "UEFI spec 2.10, §4.3").
- **Every `unsafe` block has a `// SAFETY:` comment** saying why it is sound.
- **Hardware details stay at the edges.** Raw pointers/registers wrapped in small safe types; the rest of the code uses those.
- `cargo fmt` and `cargo clippy` clean. Code and comments in English.

## 001 – Language
**Decision:** Rust, `#![no_std]`, only `core` – no external crates.
**Why:** The compiler catches most memory bugs; hardware access is visibly marked `unsafe`. Cross-compiling for bare metal (`x86_64-unknown-none`) works on Windows without an extra toolchain. Assembly via `asm!`/`global_asm!` – stays one language.
**Alternatives:** C (more transparent, more tutorials, but painful cross-toolchain on Windows, no safety nets). Zig (still too unstable).

## 002 – Target architecture
**Decision:** x86_64.
**Why:** Runs natively on the dev machine, excellent QEMU support, most documentation (OSDev wiki, Intel/AMD manuals).
**Alternatives:** AArch64, RISC-V (cleaner designs without legacy baggage, but further from our hardware). Cost of x86_64: legacy (real mode, segmentation, GDT) we have to deal with.

## 003 – Boot method
**Decision:** Own UEFI loader in Rust (target `x86_64-unknown-uefi`), UEFI structs defined by hand – no `uefi` crate. Tested in QEMU with OVMF firmware.
**Why:** Stays in one language; firmware already hands us 64-bit long mode with paging, so we skip real-mode legacy and focus on memory map, framebuffer and kernel handoff ourselves.
**Alternatives:** Own legacy BIOS loader (lots of 16/32-bit assembly, obsolete firmware). Limine/GRUB via Multiboot2 (fast, but someone else does the learning part).

## 004 – Kernel lives inside the loader
**Decision:** The kernel ELF is embedded into `BOOTX64.EFI` at compile time (`include_bytes!`). One file boots the whole OS.
**Why:** No UEFI file system protocols needed just to find our own kernel; loader and kernel can never be out of sync.
**Alternatives:** Separate `kernel.elf` on the ESP, read via Simple File System protocol (more firmware code, more ways to fail).

## 005 – Position-independent kernel
**Decision:** Kernel is a static PIE (default of `x86_64-unknown-none`). The loader puts it wherever the firmware has free memory and applies the relocations itself.
**Why:** No hard-coded load address that might collide with firmware memory. Relocation is ~50 lines and teaches how linkers/loaders really work.
**Alternatives:** Fixed link address + linker script (simpler, but breaks if that memory is taken). Higher-half mapping – comes later with our own page tables.

## 006 – Kernel knows nothing about UEFI
**Decision:** Loader translates firmware data into our own `BootInfo` (crate `boot_info`, shared by both). Our allocations are tagged with OS-defined UEFI memory types (`0x8000_0000+`) so the map tells the kernel where it lives.
**Why:** Clean boundary: firmware specifics stay in the loader; the kernel could be booted by something else later.

## 007 – First kernel output: serial port
**Decision:** COM1 (16550 UART), polled. Screen output (GOP framebuffer) comes later.
**Why:** ~40 lines, works without any setup, and QEMU pipes it to the host – so boots can be tested headless (`cargo xtask boot-test`).

## 008 – One door for all interrupts
**Decision:** All 256 vectors get a tiny assembly stub (uniform 16-byte slots, generated with `.rept`) that pushes the vector number and jumps to one common path. That path saves *all* registers into an `InterruptFrame` and calls one Rust function, which may modify the frame before `iretq`.
**Why:** One place where every interrupt arrives, with the full CPU state as a plain struct. Later the scheduler can switch tasks just by swapping that frame – no separate context-switch code. Uniform slots mean the IDT is computed (`base + 16·n`), no address table.
**Alternatives:** `extern "x86-interrupt"` functions (unstable Rust, only saves what each handler uses). One hand-written handler per exception (lots of copies, no common frame).
**Also (008):** Double fault runs on its own stack (TSS IST 1), so a broken kernel stack still gets reported. Every boot runs an `int3` round trip; `cargo xtask boot-test --crash` checks that a real page fault is reported.

## 009 – Testing: pure logic on the host, hardware in QEMU
**Decision:** Everything that is just "bytes in, bytes out" lives in small `no_std` library crates (`elf`, `x86`, `boot_info`) that also build for the host, so `cargo test` runs their unit tests in milliseconds. Code that needs the real CPU stays in `kernel`/`loader` and is checked by booting in QEMU. `cargo xtask test` runs all of it.
**Why:** `no_std` only restricts our code, not the test harness. Splitting out pure logic also enforces "hardware at the edges": the `x86` crate *describes* the CPU's formats, `kernel/src/arch` *uses* them.
**Alternatives:** `custom_test_frameworks` to run tests inside the kernel (nightly-only Rust). Making kernel/loader themselves host-testable (lots of `cfg(test)` noise around asm and entry points).

## 010 – Name: Hematite
**Decision:** The OS is called **Hematite** (the repo keeps its name, VibeCodedOs).
**Why:** Hematite is Fe₂O₃ – iron oxide, the mineral that rust is made of. So it's literally "Rust, crystallized": an OS built from Rust, layer by layer. Iron oxide also stored the bits of early computers (ferrite core memory). And `b"HEMATITE"` is exactly 8 bytes – it fits the boot magic number perfectly.
**Alternatives:** Ferrite (an obvious, common pick for Rust projects), Lodestone, Magnetite.
## 011 – Timer: local APIC, measured with the PIT
**Decision:** Periodic local APIC timer at 100 Hz on vector 0x20. Its speed is unknown, so at boot we let it count for 10 ms of PIT channel 2 (polled via port 0x61, no interrupts needed) and scale. The legacy 8259 PICs are remapped to 0x70+ and fully masked.
**Why:** The APIC timer is per-core – the right base for a scheduler later, and for multi-core. The PIT has a fixed 1.193182 MHz clock, so it's a reliable ruler without parsing ACPI tables first.
**Alternatives:** PIT as the tick source (one shared timer, slow I/O ports). HPET for calibration (needs ACPI parsing – later). TSC deadline mode (also needs a known TSC frequency).
## 012 – Architecture: ownership microkernel
**Decision:** Hematite becomes a microkernel. The kernel only manages memory, switches tasks and delivers messages; drivers and services are ordinary programs in their own hardware-protected address spaces (ring 3). Resources are **capabilities** – unforgeable handles to objects (memory, channels, devices, interrupts). The twist: **Rust's ownership rules apply between processes, enforced by the page tables**:
- *move* – sending a page unmaps it from the sender and maps it into the receiver. Zero copies, and the sender really can't touch it anymore.
- *shared borrow* (`&T`) – the page is mapped read-only in both.
- *exclusive borrow* (`&mut T`) – the page leaves the lender until it's returned.
Handles follow the same rules.
**Why:** Unique, yet we still learn all the fundamentals (page tables, user mode, syscalls, context switches). The `InterruptFrame` design (008) already makes task switching a frame swap.
**Alternatives:** Classic monolithic kernel with "everything is a file" (the textbook clone; one driver bug takes down everything). Single address space with Rust-only programs (fast, but teaches little about hardware isolation and trusts the compiler blindly).
**Consequences:** Next comes a physical page allocator, our own page tables and a higher-half kernel, so every process gets the lower half to itself.
## 013 – Tooling in Rust: `cargo xtask`
**Decision:** Build, run and test commands are a small Rust program (`xtask/`, no dependencies), called via a cargo alias: `cargo xtask build | run | boot-test | test`. It replaces the earlier PowerShell scripts.
**Why:** Handover-readiness – works on Windows, Linux and macOS without porting shell scripts, and stays in our one language. Reading the serial log as bytes also fixed PowerShell's encoding issues.
**Alternatives:** PowerShell (Windows-centric), Makefile/just (extra tools, shell differences per OS).