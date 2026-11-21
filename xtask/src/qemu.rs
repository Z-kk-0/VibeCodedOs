//! Booting Hematite in QEMU: in a window (`run`) or headless, checking the
//! serial log for an expected line and, after a normal boot, the keyboard
//! and the screen (`boot-test`).

use crate::build::build;
use crate::{Crash, Options, Result, root, run_command};
use crate::{keyboard, screenshot};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Printed by the kernel once boot finished and timer interrupts arrive.
const READY_MARKER: &str = "Hematite kernel ready.";
const BOOT_TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// For each step after boot: the keyboard driver starting, the typed line.
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// Between two keys: `sendkey` holds each one down for 100 ms.
const KEY_INTERVAL: Duration = Duration::from_millis(150);
/// Hematite must use every core (DECISIONS.md, ground rules), so QEMU gets
/// several. `HEMATITE_CORES` overrides it, e.g. to try 1 or 8.
const DEFAULT_CORES: usize = 4;
/// `--many-cores`: more than fit into one 64-bit mask, in x2APIC mode.
const MANY_CORES: usize = 80;

pub fn run(options: Options) -> Result {
    let esp = build(options)?;
    let mut qemu = qemu_command(&esp, options)?;
    qemu.args(["-serial", "stdio"]);
    run_command(&mut qemu)
}

pub fn boot_test(options: Options) -> Result {
    let esp = build(options)?;
    let expected = options.crash.marker().unwrap_or(READY_MARKER);
    let log_path = root().join("build").join("serial.log");
    let screen_path = root().join("build").join("screen.ppm");
    let _ = fs::remove_file(&log_path);
    let _ = fs::remove_file(&screen_path);

    let mut qemu = qemu_command(&esp, options)?;
    qemu.args(["-display", "none", "-serial"])
        .arg(format!("file:{}", log_path.display()))
        .stdout(Stdio::null());
    // QEMU's monitor on our pipe: how we type and ask for a screenshot.
    qemu.args(["-monitor", "stdio"]).stdin(Stdio::piped());
    let mut child = qemu
        .spawn()
        .map_err(|error| format!("cannot start QEMU: {error}"))?;
    let found = wait_for_line(&log_path, expected, &mut child, BOOT_TEST_TIMEOUT);
    let normal_boot = found && options.crash == Crash::None;
    // Type first, so the screenshot shows the terminal with the line.
    let typed = normal_boot.then(|| type_on_the_keyboard(&mut child, &log_path));
    let screenshot = normal_boot.then(|| take_screenshot(&mut child, &screen_path));
    let _ = child.kill();
    let _ = child.wait();

    let log = read_log(&log_path);
    println!("{log}");
    // A normal boot must also have started every core.
    let cores_marker = format!("{} cores online.", cores(options)?);
    if found && options.crash == Crash::None && !log.contains(&cores_marker) {
        return Err(format!("boot test failed: no '{cores_marker}'"));
    }
    if let Some(typed) = typed {
        typed?;
    }
    if let Some(taken) = screenshot {
        taken.and_then(|()| screenshot::check(&screen_path))?;
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

/// Waits for the keyboard driver, then types each of `keyboard::steps()`
/// and waits until its line is in the log (see `keyboard.rs`).
fn type_on_the_keyboard(qemu: &mut Child, log_path: &Path) -> Result {
    if !wait_for_line(log_path, keyboard::KEYBOARD_READY, qemu, STEP_TIMEOUT) {
        return Err(format!(
            "keyboard test failed: no '{}'",
            keyboard::KEYBOARD_READY
        ));
    }
    for step in keyboard::steps() {
        for key in &step.keys {
            monitor(qemu, &format!("sendkey {key}"))?;
            sleep(KEY_INTERVAL);
        }
        if !wait_for_lines(log_path, &step.expected, step.times, qemu, STEP_TIMEOUT) {
            return Err(format!(
                "keyboard test failed: pressed {}, but no '{}' ({} times)",
                step.keys.join(" "),
                step.expected,
                step.times
            ));
        }
    }
    println!(
        "Typed on the keyboard - arrows, layouts, AltGr, history, shell commands: all arrived."
    );
    Ok(())
}

/// Sends one command line to QEMU's monitor.
fn monitor(qemu: &mut Child, command: &str) -> Result {
    let monitor = qemu.stdin.as_mut().ok_or("QEMU has no monitor pipe")?;
    monitor
        .write_all(format!("{command}\n").as_bytes())
        .and_then(|()| monitor.flush())
        .map_err(|error| format!("cannot talk to the QEMU monitor: {error}"))
}

/// Asks QEMU's monitor for a screendump and waits until the file is complete.
fn take_screenshot(qemu: &mut Child, path: &Path) -> Result {
    // Forward slashes: the monitor reads backslashes as escapes; Windows
    // takes either.
    let path_text = path.display().to_string().replace('\\', "/");
    monitor(qemu, &format!("screendump {path_text}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if fs::read(path).is_ok_and(|bytes| screenshot::parse_ppm(&bytes).is_ok()) {
            return Ok(());
        }
        sleep(Duration::from_millis(100));
    }
    Err("QEMU took no screenshot within 5 s".into())
}

fn wait_for_line(log_path: &Path, expected: &str, qemu: &mut Child, timeout: Duration) -> bool {
    wait_for_lines(log_path, expected, 1, qemu, timeout)
}

/// Waits until `expected` is in the log at least `times` times.
fn wait_for_lines(
    log_path: &Path,
    expected: &str,
    times: usize,
    qemu: &mut Child,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let found = || read_log(log_path).matches(expected).count() >= times;
    while Instant::now() < deadline {
        if found() {
            sleep(Duration::from_millis(500)); // let the rest of a multi-line report arrive
            return true;
        }
        if let Ok(Some(_)) = qemu.try_wait() {
            return found(); // QEMU quit on its own
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

fn qemu_command(esp: &Path, options: Options) -> Result<Command> {
    let qemu = find_qemu()?;
    let firmware = find_firmware(&qemu)?;
    let mut command = Command::new(&qemu);
    let memory = if options.many_cores { "512M" } else { "256M" };
    command
        .args(["-machine", "q35", "-m", memory, "-net", "none"])
        .args(["-smp", &cores(options)?.to_string()]);
    if options.many_cores {
        // QEMU's default CPU has no x2APIC; this one does.
        command.args(["-cpu", "qemu64,+x2apic"]);
    }
    command
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

fn cores(options: Options) -> Result<usize> {
    if options.many_cores {
        return Ok(MANY_CORES);
    }
    match env::var("HEMATITE_CORES") {
        Ok(value) => value
            .parse()
            .ok()
            .filter(|count| (1..=255).contains(count))
            .ok_or_else(|| format!("HEMATITE_CORES must be 1 to 255, not '{value}'")),
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
