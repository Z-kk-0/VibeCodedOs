//! Proof from outside the machine that Hematite hears the keyboard: after a
//! normal boot the boot test types a line through QEMU's monitor
//! (`sendkey`, which presses and releases a key on the emulated PS/2
//! keyboard) and looks for the terminal's log of it. Every layer is real:
//! the controller's interrupt, the I/O APIC, the kernel waking the driver,
//! scancode decoding, the call to the terminal.

/// What the boot test types: Shift, a comma, a space and a `!` included,
/// so the decoder's harder cases are covered.
pub const TYPED: &str = "Hello, Hematite!";

/// The line the keyboard driver logs once it listens (`programs/keyboard`).
pub const KEYBOARD_READY: &str = "[keyboard] Keyboard controller ready.";

/// What the terminal logs when Enter finishes the line.
pub fn expected_line() -> String {
    format!("[terminal] typed: {TYPED}")
}

/// QEMU key names ("qcodes") that type `text`, then Enter.
pub fn key_names(text: &str) -> Vec<String> {
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
            key_names("Hi, a!"),
            ["shift-h", "i", "comma", "spc", "a", "shift-1", "ret"]
        );
    }
}
