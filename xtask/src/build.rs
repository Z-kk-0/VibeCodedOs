//! Building: kernel first, then the loader – which embeds the kernel
//! (DECISIONS.md, 004) – then the folder QEMU boots from.

use crate::{Options, Result, root, run_command};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const KERNEL_TARGET: &str = "x86_64-unknown-none";
pub const LOADER_TARGET: &str = "x86_64-unknown-uefi";

/// Builds everything and returns the boot partition folder (`build/esp`).
pub fn build(options: Options) -> Result<PathBuf> {
    let profile = if options.release { "release" } else { "debug" };

    let mut kernel = cargo_build("kernel", KERNEL_TARGET, options.release);
    // Read by the kernel at compile time (`option_env!`), see kernel/src/main.rs.
    if options.crash {
        kernel.env("HEMATITE_CRASH_TEST", "1");
    } else {
        kernel.env_remove("HEMATITE_CRASH_TEST");
    }
    run_command(&mut kernel)?;

    let kernel_elf = root()
        .join("target")
        .join(KERNEL_TARGET)
        .join(profile)
        .join("kernel");
    let mut loader = cargo_build("loader", LOADER_TARGET, options.release);
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

fn cargo_build(package: &str, target: &str, release: bool) -> Command {
    let mut command = Command::new("cargo");
    command.args(["build", "--package", package, "--target", target]);
    if release {
        command.arg("--release");
    }
    command
}
