# Decisions

Short log of architecture decisions. Per entry: what, why, alternatives.

## Ground rules
- OS from scratch, for learning. One programming language (plus the unavoidable bit of assembly).
- No big external libraries that do the real work (bootloader crates, ready-made paging/interrupt libs, etc.).
- **Not a textbook clone.** The usual path is a reference, not a rule. Where a less common design is better or more interesting, we take it and say why.

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
