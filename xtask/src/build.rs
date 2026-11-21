//! Building, innermost first: user programs, then the kernel (which embeds
//! them, kernel/src/programs.rs), then the loader (which embeds the kernel,
//! DECISIONS.md 004), then the folder QEMU boots from.

use crate::{Options, Result, root, run_command};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const KERNEL_TARGET: &str = "x86_64-unknown-none";
pub const LOADER_TARGET: &str = "x86_64-unknown-uefi";
/// User programs run on the bare CPU just like the kernel.
pub const PROGRAM_TARGET: &str = KERNEL_TARGET;
/// Every crate in `programs/`; each is embedded into the kernel.
pub const PROGRAMS: [&str; 11] = [
    "init",
    "log",
    "directory",
    "shout",
    "vandal",
    "hello",
    "owner",
    "giver",
    "intruder",
    "display",
    "paint",
];

/// Builds everything and returns the boot partition folder (`build/esp`).
pub fn build(options: Options) -> Result<PathBuf> {
    let profile = if options.release { "release" } else { "debug" };

    let mut programs = cargo_build(&PROGRAMS, PROGRAM_TARGET, options.release);
    run_command(&mut programs)?;

    let mut kernel = cargo_build(&["kernel"], KERNEL_TARGET, options.release);
    kernel.env(
        "HEMATITE_PROGRAMS",
        root().join("target").join(PROGRAM_TARGET).join(profile),
    );
    // Read by the kernel at compile time (`option_env!`), see kernel/src/main.rs.
    match options.crash.kernel_switch() {
        Some(crash) => kernel.env("HEMATITE_CRASH_TEST", crash),
        None => kernel.env_remove("HEMATITE_CRASH_TEST"),
    };
    run_command(&mut kernel)?;

    let kernel_elf = root()
        .join("target")
        .join(KERNEL_TARGET)
        .join(profile)
        .join("kernel");
    let mut loader = cargo_build(&["loader"], LOADER_TARGET, options.release);
    loader.env("HEMATITE_KERNEL_ELF", &kernel_elf);
    run_command(&mut loader)?;

    let esp = root().join("build").join("esp");
    let boot_dir = esp.join("EFI").join("BOOT");
    fs::create_dir_all(&boot_dir)
        .map_err(|error| format!("cannot create {}: {error}", boot_dir.display()))?;
    // UEFI firmware boots this exact path from any FAT partition (UEFI spec 2.10, §3.5.1).
    let loader_efi = root()
        .join("target")
        .join(LOADER_TARGET)
        .join(profile)
        .join("loader.efi");
    fs::copy(&loader_efi, boot_dir.join("BOOTX64.EFI"))
        .map_err(|error| format!("cannot copy {}: {error}", loader_efi.display()))?;
    Ok(esp)
}

fn cargo_build(packages: &[&str], target: &str, release: bool) -> Command {
    let mut command = Command::new("cargo");
    command.arg("build");
    for package in packages {
        command.args(["--package", package]);
    }
    command.args(["--target", target]);
    if release {
        command.arg("--release");
    }
    command
}
