//! Booting Hematite in QEMU: in a window (`run`) or headless, checking the
//! serial log for an expected line (`boot-test`).

use crate::build::build;
use crate::{Crash, Options, Result, root, run_command};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Printed by the kernel once boot finished and timer interrupts arrive.
const READY_MARKER: &str = "Hematite kernel ready.";
const BOOT_TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Hematite must use every core (DECISIONS.md, ground rules), so QEMU gets
/// several. `HEMATITE_CORES` overrides it, e.g. to try 1 or 8.
const DEFAULT_CORES: usize = 4;

pub fn run(options: Options) -> Result {
    let esp = build(options)?;
    let mut qemu = qemu_command(&esp)?;
    qemu.args(["-serial", "stdio"]);
    run_command(&mut qemu)
}

pub fn boot_test(options: Options) -> Result {
    let esp = build(options)?;
    let expected = options.crash.marker().unwrap_or(READY_MARKER);
    let log_path = root().join("build").join("serial.log");
    let _ = fs::remove_file(&log_path);

    let mut qemu = qemu_command(&esp)?;
    qemu.args(["-display", "none", "-serial"])
        .arg(format!("file:{}", log_path.display()))
        .stdout(Stdio::null());
    let mut child = qemu
        .spawn()
        .map_err(|error| format!("cannot start QEMU: {error}"))?;
    let found = wait_for_line(&log_path, expected, &mut child);
    let _ = child.kill();
    let _ = child.wait();

    let log = read_log(&log_path);
    println!("{log}");
    // A normal boot must also have started every core.
    let cores_marker = format!("{} cores online.", cores()?);
    if found && options.crash == Crash::None && !log.contains(&cores_marker) {
        return Err(format!("boot test failed: no '{cores_marker}'"));
    }
    if found {
        println!("\nBOOT TEST OK ('{expected}')");
        Ok(())
    } else {
        Err(format!(
            "boot test failed: no '{expected}' within {} s",
            BOOT_TEST_TIMEOUT.as_secs()
        ))
    }
}

fn wait_for_line(log_path: &Path, expected: &str, qemu: &mut Child) -> bool {
    let deadline = Instant::now() + BOOT_TEST_TIMEOUT;
    while Instant::now() < deadline {
        if read_log(log_path).contains(expected) {
            sleep(Duration::from_millis(500)); // let the rest of a multi-line report arrive
            return true;
        }
        if let Ok(Some(_)) = qemu.try_wait() {
            return read_log(log_path).contains(expected); // QEMU quit on its own
        }
        sleep(Duration::from_millis(250));
    }
    false
}

/// The serial log as text. Invalid UTF-8 (firmware control codes) becomes '�'.
fn read_log(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn qemu_command(esp: &Path) -> Result<Command> {
    let qemu = find_qemu()?;
    let firmware = find_firmware(&qemu)?;
    let mut command = Command::new(&qemu);
    command
        .args(["-machine", "q35", "-m", "256M", "-net", "none"])
        .args(["-smp", &cores()?.to_string()])
        .arg("-drive")
        .arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            firmware.display()
        ))
        // QEMU presents a plain folder as a FAT disk – no disk image needed.
        .arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{}", esp.display()));
    Ok(command)
}

fn cores() -> Result<usize> {
    match env::var("HEMATITE_CORES") {
        Ok(value) => value
            .parse()
            .ok()
            .filter(|count| (1..=64).contains(count))
            .ok_or_else(|| format!("HEMATITE_CORES must be 1 to 64, not '{value}'")),
        Err(_) => Ok(DEFAULT_CORES),
    }
}

fn find_qemu() -> Result<PathBuf> {
    if let Some(path) = env::var_os("HEMATITE_QEMU") {
        return Ok(path.into());
    }
    let executable = if cfg!(windows) {
        "qemu-system-x86_64.exe"
    } else {
        "qemu-system-x86_64"
    };
    let path_dirs = env::var_os("PATH").map(|paths| env::split_paths(&paths).collect::<Vec<_>>());
    let usual_dirs = [
        r"C:\Program Files\qemu",
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
    ];
    path_dirs
        .unwrap_or_default()
        .into_iter()
        .chain(usual_dirs.map(PathBuf::from))
        .map(|dir| dir.join(executable))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "QEMU not found. Install it (CONTRIBUTING.md) or set HEMATITE_QEMU.".into())
}

/// The UEFI firmware (OVMF, from the EDK II project) ships with QEMU or as a
/// distro package; its location differs per platform.
fn find_firmware(qemu: &Path) -> Result<PathBuf> {
    if let Some(path) = env::var_os("HEMATITE_OVMF") {
        return Ok(path.into());
    }
    let qemu_dir = qemu.parent().unwrap_or(Path::new("."));
    let candidates = [
        qemu_dir.join("share/edk2-x86_64-code.fd"), // Windows installer
        qemu_dir.join("../share/qemu/edk2-x86_64-code.fd"), // Homebrew, source builds
        PathBuf::from("/usr/share/qemu/edk2-x86_64-code.fd"), // most Linux qemu packages
        PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd"), // Debian/Ubuntu "ovmf"
        PathBuf::from("/usr/share/edk2/x64/OVMF_CODE.fd"), // Arch "edk2-ovmf"
        PathBuf::from("/usr/share/edk2/ovmf/OVMF_CODE.fd"), // Fedora "edk2-ovmf"
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "UEFI firmware (OVMF) not found. Install it (CONTRIBUTING.md) or set HEMATITE_OVMF."
                .into()
        })
}
