//! Proof from outside the machine that Hematite hears the keyboard: after a
//! normal boot the boot test types through QEMU's monitor (`sendkey`,
//! which presses and releases keys on the emulated PS/2 keyboard – by
//! position, like a real one) and looks for what the programs log. Every
//! layer is real: the controller's interrupt, the I/O APIC, the kernel
//! waking the driver, scancodes, layouts, the call to the terminal.

/// The line the keyboard driver logs once it listens (`programs/keyboard`).
pub const KEYBOARD_READY: &str = "[keyboard] Keyboard controller ready.";

/// One step: the keys to press (QEMU names, "qcodes"; `a-b` = together),
/// and the log line that must then be there `times` times.
pub struct Step {
    pub keys: Vec<String>,
    pub expected: String,
    pub times: usize,
}

/// What the boot test types, in order.
pub fn steps() -> Vec<Step> {
    let german_line = "xz/y1@ö";
    vec![
        // US layout: Shift, a comma, a space and `!`.
        Step {
            keys: keys_for("Hello, Hematite!"),
            expected: typed("Hello, Hematite!"),
            times: 1,
        },
        Step {
            keys: names(&["alt-shift"]),
            expected: "[keyboard] Layout: German (QWERTZ).".into(),
            times: 1,
        },
        // German layout, by key position: Y and Z swapped, Shift+7 is `/`,
        // AltGr+Q is `@`, the key right of L is `ö`. The arrows, Home and
        // End move the cursor while typing.
        Step {
            keys: names(&[
                "y",
                "z",
                "left",
                "shift-7",
                "home",
                "x",
                "end",
                "1",
                "alt_r-q",
                "semicolon",
                "ret",
            ]),
            expected: typed(german_line),
            times: 1,
        },
        // Up brings the last line back.
        Step {
            keys: names(&["up", "ret"]),
            expected: typed(german_line),
            times: 2,
        },
        // Every line went to the shell, which knows none of them.
        shown("xz/y1@ö: no such command or program. 'help' lists them."),
        // The shell (back to the US layout first): what it may start, a
        // program whose output lands on the terminal, one the kernel stops.
        Step {
            keys: names(&["alt-shift"]),
            expected: "[keyboard] Layout: US (QWERTY).".into(),
            times: 1,
        },
        Step {
            keys: keys_for("help"),
            expected: output("Programs I may start: hello, intruder, giver, paint."),
            times: 1,
        },
        Step {
            keys: keys_for("hello"),
            expected: output("hello ended: exit code 0."),
            times: 1,
        },
        shown("Got \"a page from the shell\""),
        Step {
            keys: keys_for("intruder"),
            expected: output("intruder was stopped by the kernel: page fault (14)."),
            times: 1,
        },
    ]
}

/// A line the terminal shows (and logs).
fn output(line: &str) -> String {
    format!("[terminal] {line}")
}

/// A step that types nothing, only looks for a line already due.
fn shown(line: &str) -> Step {
    Step {
        keys: Vec::new(),
        expected: output(line),
        times: 1,
    }
}

fn typed(line: &str) -> String {
    format!("[terminal] typed: {line}")
}

fn names(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| key.to_string()).collect()
}

/// QEMU key names that type `text` on a US layout, then Enter.
fn keys_for(text: &str) -> Vec<String> {
    let mut keys: Vec<String> = text.chars().map(key_name).collect();
    keys.push("ret".into());
    keys
}

fn key_name(character: char) -> String {
    match character {
        ' ' => "spc".into(),
        ',' => "comma".into(),
        '.' => "dot".into(),
        '!' => "shift-1".into(),
        letter if letter.is_ascii_uppercase() => {
            format!("shift-{}", letter.to_ascii_lowercase())
        }
        other if other.is_ascii_alphanumeric() => other.to_string(),
        other => panic!("no QEMU key name for {other:?} yet"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_letters_and_punctuation_get_their_qemu_names() {
        assert_eq!(
            keys_for("Hi, a!"),
            ["shift-h", "i", "comma", "spc", "a", "shift-1", "ret"]
        );
    }
}
