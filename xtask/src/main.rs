//! Hematite's build tool: `cargo xtask <command>` (alias in .cargo/config.toml).
//!
//! Plain Rust with no dependencies, so it runs the same on Windows, Linux
//! and macOS – no shell scripts to port.

mod build;
mod check;
mod qemu;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <command> [options]

Commands:
  build       Build kernel + loader, lay out the boot partition in build/esp
  run         Build and boot in a QEMU window (serial log in this terminal)
  boot-test   Boot headless and check the serial log
  test        Every check: formatting, clippy, unit tests, boot tests

Options:
  --release   Optimized build (build, run, boot-test)
  --crash     Kernel page-faults on purpose; boot-test then expects the report
  --crash-stack  Kernel overflows its stack; boot-test expects the guard page
              to catch it (reported as a double fault)
  --quick     test: skip the QEMU boot tests

Environment:
  HEMATITE_QEMU   path to qemu-system-x86_64 (default: PATH, usual install dirs)
  HEMATITE_OVMF   path to the UEFI firmware image (default: searched near QEMU)
";

pub type Result<T = ()> = std::result::Result<T, String>;

#[derive(Clone, Copy, Default)]
pub struct Options {
    pub release: bool,
    pub crash: Crash,
    pub quick: bool,
}

/// A kernel that crashes on purpose, to check that crashes are reported.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Crash {
    #[default]
    None,
    PageFault,
    StackOverflow,
}

impl Crash {
    /// The value of `HEMATITE_CRASH_TEST` the kernel is built with.
    pub fn kernel_switch(self) -> Option<&'static str> {
        match self {
            Crash::None => None,
            Crash::PageFault => Some("page-fault"),
            Crash::StackOverflow => Some("stack-overflow"),
        }
    }

    /// What the serial log must show.
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Crash::None => None,
            Crash::PageFault => Some("EXCEPTION: Page Fault"),
            // The guard page faults, and the CPU can't push the page fault's
            // frame onto the broken stack: a double fault, on its own stack.
            Crash::StackOverflow => Some("EXCEPTION: Double Fault"),
        }
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_default();
    let mut options = Options::default();
    for argument in arguments {
        match argument.as_str() {
            "--release" => options.release = true,
            "--crash" => options.crash = Crash::PageFault,
            "--crash-stack" => options.crash = Crash::StackOverflow,
            "--quick" => options.quick = true,
            other => return usage_error(&format!("unknown option '{other}'")),
        }
    }

    let result = match command.as_str() {
        "build" => build::build(options).map(|_| ()),
        "run" => qemu::run(options),
        "boot-test" => qemu::boot_test(options),
        "test" => check::everything(options),
        "" | "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => return usage_error(&format!("unknown command '{other}'")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nerror: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("error: {message}\n\n{USAGE}");
    ExitCode::FAILURE
}

/// The repository root (this crate lives in `<root>/xtask`).
pub fn root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("xtask lives inside the repository")
        .to_path_buf()
}

/// Runs `command` in the repository root and fails if it fails.
pub fn run_command(command: &mut Command) -> Result {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .current_dir(root())
        .status()
        .map_err(|error| format!("cannot start {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed ({status})"))
    }
}
