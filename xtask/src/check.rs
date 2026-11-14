//! `cargo xtask test`: every check we have, fastest first. Stops at the
//! first failure. Green here = safe to commit.

use crate::build::{KERNEL_TARGET, LOADER_TARGET, PROGRAM_TARGET, PROGRAMS};
use crate::{Crash, Options, Result, qemu, run_command};
use std::process::Command;

pub fn everything(options: Options) -> Result {
    let programs: String = PROGRAMS
        .iter()
        .map(|program| format!(" --package {program}"))
        .collect();

    step("Formatting", "fmt --all --check")?;
    // Default workspace members = the host-buildable crates, incl. their tests.
    step("Lints: host crates", "clippy --all-targets -- -D warnings")?;
    step(
        "Lints: user programs",
        &format!("clippy{programs} --package userlib --target {PROGRAM_TARGET} -- -D warnings"),
    )?;
    // Each layer embeds the one inside it, so build before linting the next.
    step(
        "Build: user programs",
        &format!("build{programs} --target {PROGRAM_TARGET}"),
    )?;
    step(
        "Lints: kernel",
        &format!("clippy --package kernel --target {KERNEL_TARGET} -- -D warnings"),
    )?;
    step(
        "Build: kernel",
        &format!("build --package kernel --target {KERNEL_TARGET}"),
    )?;
    step(
        "Lints: loader",
        &format!("clippy --package loader --target {LOADER_TARGET} -- -D warnings"),
    )?;
    step("Unit tests", "test")?;

    if !options.quick {
        for (name, crash) in [
            ("Boot test", Crash::None),
            ("Crash test: page fault", Crash::PageFault),
            ("Crash test: stack overflow", Crash::StackOverflow),
        ] {
            println!("\n=== {name} ===");
            qemu::boot_test(Options { crash, ..options })?;
        }
    }
    println!("\nALL CHECKS PASSED");
    Ok(())
}

/// Runs `cargo <arguments>`; arguments are separated by spaces.
fn step(name: &str, arguments: &str) -> Result {
    println!("\n=== {name} ===");
    run_command(Command::new("cargo").args(arguments.split_whitespace()))
        .map_err(|error| format!("{name}: {error}"))
}
