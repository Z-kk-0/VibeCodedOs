# Hematite – Decisions

Short log of architecture decisions. Per entry: what, why, alternatives.

## Ground rules
- OS from scratch, for learning. One programming language (plus the unavoidable bit of assembly).
- No big external libraries that do the real work (bootloader crates, ready-made paging/interrupt libs, etc.).
- **Not a textbook clone.** The usual path is a reference, not a rule. Where a less common design is better or more interesting, we take it and say why.
- **Multi-core by design.** Hematite must use all CPU cores, and nothing may rely on "only one thing runs at a time" (e.g. "the caller is blocked, so nobody touches its memory"). Exclusivity is enforced with locks, atomics and page tables – on every core.
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
## 014 – Physical memory: a frame table with an embedded free list
**Decision:** Every 4 KiB frame of RAM gets an 8-byte entry with its state (`Unavailable`, `Free`, `Allocated`). Free entries are chained into a list *through the table itself*, so allocate and free are O(1). The table lives in the first usable region big enough (crate `frame_table`, host-tested); access goes through a `SpinLock` that also disables interrupts. Freeing a frame that isn't allocated panics.
**Why:** The ownership model (012) needs to know who each frame belongs to – the state field is where "moved to process X" or "shared by n" will go. Double frees are caught instead of silently corrupting memory. Cost: 0.2 % of RAM.
**Alternatives:** Bitmap (compact, but O(n) search and no room for ownership state). Free list stored inside the free frames (no metadata at all). Buddy allocator (contiguous multi-frame blocks – add later if DMA needs it).
**Also:** Firmware "boot services" memory is a separate `FirmwareReclaimable` kind: it still contains the page tables the CPU uses, so it's only reclaimed after we switch to our own (roadmap step 2). Same for loader memory.
## 015 – Address space: the loader builds it, the kernel starts in the higher half
**Decision:** The loader builds the kernel's page tables before leaving the firmware and switches to them right before the jump. Layout (documented in `boot_info`): kernel image at `0xFFFF_FFFF_8000_0000` (top 2 GiB), all physical memory 1:1 at `0xFFFF_8000_0000_0000` (the "window", 2 MiB pages, at least 4 GiB so device registers are covered), plus a temporary identity map so the loader survives the switch. The kernel deletes the identity map first thing – the whole lower half is then free for user programs – and reclaims firmware and loader memory.
Kernel pages get their ELF segment's permissions: code read-only + executable, data writable + no-execute (W^X), enforced in ring 0 too (CR0.WP, EFER.NXE). The mapping logic is the host-tested `paging` crate, shared by loader and kernel through a small `TableMemory` trait.
**Why:** Our PIE kernel (005) can simply be relocated to the high address – no second position-dependent stage. The loader still has the firmware's allocator, so building tables there is easy; the kernel never runs on firmware page tables. W^X catches whole bug classes (verified once by hand: writing to code → `#PF 0x3`, executing data → `#PF 0x11`).
**Alternatives:** Kernel builds its own tables after starting identity-mapped (needs a self-relocation step). 1 GiB pages for the window (fewer tables, but not every CPU – e.g. QEMU's default model – supports them).
## 016 – Kernel heap: size classes plus whole pages
**Decision:** Requests up to 2 KiB are rounded up to a power of two (16 … 2048 bytes) and served from one free list per size class; free blocks store the list link inside themselves. Bigger requests get whole pages, mapped on demand and returned (frames back to the frame allocator) when freed. The heap has its own 512 GiB region at `0xFFFF_FF00_0000_0000`; logic in the host-tested `heap` crate, `GlobalAlloc` + page mapping in `kernel/src/memory/allocator.rs`. All page-table changes now go through one lock.
**Why:** O(1) allocate/free with no searching, alignment for free (power-of-two blocks in page-aligned pages are aligned to their size), tiny code. Fits a microkernel, whose own allocations are mostly small fixed-size objects (tasks, capabilities, messages).
**Alternatives:** Linked-list first-fit (simple, but O(n) and fragments). Buddy allocator (merges neighbours, but more code for little gain at our sizes). Slab per object type (a refinement of size classes – possible later).
**Limits:** Pages of size classes are kept for reuse, never returned. Heap virtual addresses are not reused (512 GiB of address space is plenty for now). Alignment above 4 KiB is refused.
## 017 – Tasks: a switch is answering an interrupt with another frame
**Decision:** Preemptive round robin, one 10 ms tick per slice. The interrupt dispatcher returns the `InterruptFrame` to resume; the stub loads it into `rsp` before restoring registers. So a task switch is: save the interrupted task's frame pointer, return another task's. Voluntary switches (`sleep`, `exit`) raise `int 0x30` and go through the very same path. A new task gets a 16 KiB heap stack with a hand-made frame on top, so its first "return from interrupt" starts it. The running boot code becomes task 0; an idle task (`hlt`) runs when nobody is ready. Exited tasks are freed at a later switch – never the current one, whose stack we may still be on. Choosing the next task is the host-tested `scheduling` crate.
**Why:** No separate context-switch routine: one path for every way of entering and leaving a task, and the saved state is the same struct exceptions already report. Exactly what 008 prepared.
**Alternatives:** Cooperative switching with a `switch(from, to)` routine saving only callee-saved registers (faster, but no preemption and a second mechanism). Per-priority queues (later, if needed).
**Also:** `println!` now takes a lock so one call is never torn apart by a switch; panic and exception reports use lock-free `emergency_println!`.
## 018 – User mode: own address spaces, one door into the kernel
**Decision:** A user program runs in ring 3 in its own address space: an own lower half (program at `0x40_0000`, stack below `0x7FFF_FFFF_0000` with an unmapped guard page above), the kernel's upper half shared by copying its top-level entries. Programs are position-independent ELFs loaded by the same `elf` crate as the kernel, with W^X plus the user bit, embedded in the kernel until there is a file system. **System calls go through `int 0x80`** – the same interrupt path as everything else: the call is just another `InterruptFrame` (number in `rax`, arguments in `rdi`/`rsi`, result in `rax`), so `sleep` and `exit` switch tasks exactly like the timer. The task switch also loads CR3 and the TSS kernel stack. Pointers from programs are never dereferenced: every page is checked in the program's own tables and copied through the physical memory window. A fault in ring 3 ends that program; only kernel faults stop the machine. Calls: `exit`, `write`, `sleep` (crate `abi`; programs use `userlib`).
**Why:** Keeps the one-door design (008, 017) – no second entry path with its own register conventions and stack switching. Proven at every boot: `programs/intruder` attacks twice (via a system call, then directly) and must be stopped while `programs/hello` keeps running; afterwards both address spaces must be fully freed.
**Alternatives:** `syscall`/`sysret` (much faster, but its own entry path: manual stack switch, `swapgs`, different saved state – worth it once system calls are hot; the GDT order already allows it). One shared address space for all programs (no isolation).
## 019 – Multi-core: the GDT as an ID card, one shared run queue
**Decision:** Hematite starts every core the ACPI MADT lists and schedules tasks on all of them. Decided together with the owner:
- *Finding cores:* the loader only passes the ACPI root pointer (from the UEFI configuration table); the kernel walks RSDP → XSDT/RSDT → MADT itself (host-tested crate `acpi`).
- *Waking cores:* INIT, STARTUP, STARTUP over the APIC. A woken core starts in 16-bit real mode on a page below 1 MiB the loader reserved (`CoreStartup`). The startup code (`arch/smp/startup_code.rs`) leaps from real mode **straight to long mode** – CR4, EFER, then CR0 with protection and paging switched on together, then a far jump into a 64-bit segment – skipping 32-bit protected mode entirely. It copies the boot core's CR0/CR4/EFER, so every core runs with exactly the same settings. Its page tables (second reserved page, below 4 GiB for the 32-bit CR3 load) are the kernel's upper half plus the startup page mapped at its own address; every core switches to the kernel's tables at once, and the extra tables are freed afterwards. Cores start one at a time; each joins the scheduler before the next is woken.
- *Per-core data ("who am I?"):* each core's GDT is the first field of its `PerCore` (GDT, TSS, index, APIC ID). `sgdt` returns the GDT's address = the `PerCore`. No `swapgs`, no "did we come from user mode?" branch in the one interrupt door (008), and ring 3 can't change the GDT register.
- *Scheduler:* one run queue under one lock, but a current task and an idle task per core (`scheduling` crate, generic and host-tested). A started core's own startup code becomes its idle task. Only core 0's timer advances the clock.
- *The handoff problem:* after picking the next task, a core still runs on the *previous* task's stack until the stub loads the new `rsp`. If another core resumed that task meanwhile, both would use one stack. So the previous task gets a `leaving` flag (set under the scheduler lock); the dispatcher returns it next to the frame (`rdx`), and the stub clears it right after switching stacks. Flagged tasks are neither picked nor freed.
- *TLB shootdown:* every address space keeps a bit mask of the cores that have it loaded (set before loading CR3, cleared after leaving). Changing a mapping flushes exactly those cores – for the shared kernel half, all online cores – via an IPI, and waits until all have flushed. Spinning `SpinLock`s serve pending shootdowns, because the waiting sender may hold the very lock a core spins on with interrupts off.
- A kernel panic or exception stops all other cores (IPI to all), so nothing keeps running on broken state.
**Why:** Ground rule "multi-core by design" – and the upcoming ownership IPC relies on page tables enforcing borrows on *every* core, which is exactly what the shootdown provides. The GDT trick is unusual (the textbook uses GS + `swapgs`, a notorious bug and security source, e.g. CVE-2019-1125) and keeps the interrupt path identical for all cores. The direct real→long mode leap needs half the usual startup code.
**Alternatives:** Cores from UEFI's MP Services protocol (less to parse, but teaches nothing we don't need for ACPI anyway). GS base + `swapgs`, or looking up the APIC ID on every access (an MMIO read each time). Per-core run queues with work stealing (scales better, much more logic – later, if the one lock ever shows up in measurements). Broadcasting every shootdown to all cores (simpler, but wakes idle cores for nothing).
**Limits:** At most 64 cores (bit masks). xAPIC mode only, so APIC IDs up to 255. One page per shootdown, one shootdown at a time. If a core doesn't answer within 1 s, no further cores are started.

## 020 – Capabilities and ownership IPC
**Decision:** Programs reach anything only through *handles* – slots in a per-process table in the kernel (index + generation, so a stale handle stays dead when its slot is reused; host-tested crate `capability`). Objects: `Endpoint` (a rendezvous point; handles are *receiving* or *sending*, the latter carry a badge), `Memory` (physical pages with exactly one owner) and `Console` (the kernel's text output). Talking is synchronous: `call` blocks until the receiver's `reply`; `receive` blocks until a call arrives. Decided together with the owner:
- *Messages are registers only* (N1): four words plus at most one attached handle. The kernel never copies payload – data travels only as memory objects. Ownership is therefore the *only* way to share data.
- *Owning means being mapped* (M1): a memory object is always mapped where its owner (or, during a call, its borrower) can use it. The kernel picks the address in a per-process mapping area (`0x1000_0000_0000`, a guard page after each object) and reports it with the message. Rust's rules, enforced by the page tables on every core (shootdowns, 019): *move* – unmapped from the sender, the receiver's now; *lend `&`* – read-only for both until the reply; *lend `&mut`* – unmapped from the lender, writable for the borrower until the reply. Borrowed memory can't be passed on; replies can't lend (nothing would end the borrow).
- *`write` is gone; a `log` service prints* (L1): only `log` holds the `Console` handle. Programs lend `log` a page with their text (`userlib`'s `println!`); `log` prefixes the name from the sender's badge – the kernel badges each program's handles with its name (8 bytes packed into the `u64`), so names can't be faked.
- Every program starts with the handles the kernel gives it as 0, 1, 2, … (handle 0 = `log` for everyone but `log`). The wiring lives in `kernel/src/ipc/checks.rs`.
- All IPC runs under the scheduler lock, since it changes several tasks at once (states, saved frames) – one atomic step on all cores. Results are written straight into the waiting task's saved `InterruptFrame`, which `block` records under that same lock, so a reply from another core can never miss it.
- A program that dies while owing a reply (e.g. after a fault in borrowed memory) hands lent memory back; its caller gets `ERROR_PEER_GONE`. Memory objects are unmapped before their process's address space is freed, so frames are freed exactly once.
- `userlib` mirrors the rules in types: lending takes `&Memory`/`&mut Memory` for the duration of the call, moving takes the `Memory` by value – the compiler refuses what the page tables would stop.
**Why:** The heart of DECISIONS 012. Registers-only messages keep the kernel tiny and make zero-copy the normal case; "owning = mapped" makes the ownership state visible in the one place the hardware enforces. Proven at every boot: `owner` lends `&mut` to `shout` and gets its text back upper-cased, lends `&` to `vandal` (which writes, is stopped, and owner's page is unchanged), `giver` moves a page and is stopped when it reaches for it, `intruder` forges handles. While lending, the kernel also asserts that the lender's own mapping really is gone or read-only – the lender is blocked in `call` and can't try itself.
**Alternatives:** A kernel-copied message buffer (comfortable for small text, but a second data path). Explicit `map(handle, address)` calls (flexible, but owning and accessing come apart). A user-space serial driver with an I/O-port capability (the "pure" way – comes with the first real driver). Asynchronous queues (no blocking, but buffering in the kernel and harder ownership).
**Limits:** One call at a time per server (`reply` before the next `receive`). A caller of an endpoint whose server died waits forever. No timeouts. Memory objects ≤ 1 MiB; mapping addresses are never reused. Capabilities can't be revoked.
