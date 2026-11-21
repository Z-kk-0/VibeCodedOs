//! What the keyboard driver (`programs/keyboard`) sends to whoever listens:
//! one `call` per key pressed, `[KEY, character, 0, 0]`, the character as
//! a Unicode scalar value. Enter is `'\n'`, Backspace `'\u{8}'`. The
//! listener replies with anything; until it does, the driver waits.

pub const KEY: u64 = 1;
pub const BACKSPACE: char = '\u{8}';
