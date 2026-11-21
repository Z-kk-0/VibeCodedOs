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
    ]
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
