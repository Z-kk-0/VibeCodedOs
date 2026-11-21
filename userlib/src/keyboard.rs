//! What the keyboard driver (`programs/keyboard`) sends to whoever listens:
//! one `call` per key pressed. The listener replies with anything; until
//! it does, the driver waits.
//!
//! - `[KEY, character, 0, 0]`: a key that types something, the character
//!   as a Unicode scalar value. Enter is `'\n'`, Backspace `BACKSPACE`.
//! - `[EDIT, Edit as u64, 0, 0]`: a key that moves or deletes (`Edit`).

pub const KEY: u64 = 1;
pub const EDIT: u64 = 2;
pub const BACKSPACE: char = '\u{8}';
/// Ctrl+C: "stop what is running". Ctrl with a letter types the old ASCII
/// control character (Ctrl+A = 1, …, Ctrl+Z = 26).
pub const CONTROL_C: char = '\u{3}';

/// Keys that type nothing but edit a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u64)]
pub enum Edit {
    Left = 1,
    Right = 2,
    Up = 3,
    Down = 4,
    Home = 5,
    End = 6,
    Delete = 7,
}

impl Edit {
    pub fn from_word(word: u64) -> Option<Self> {
        Some(match word {
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Up,
            4 => Self::Down,
            5 => Self::Home,
            6 => Self::End,
            7 => Self::Delete,
            _ => return None,
        })
    }
}

/// One key press, as the driver decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Character(char),
    Edit(Edit),
}

impl Key {
    /// The message words that carry it.
    pub fn to_words(self) -> [u64; 4] {
        match self {
            Key::Character(character) => [KEY, u64::from(character), 0, 0],
            Key::Edit(edit) => [EDIT, edit as u64, 0, 0],
        }
    }

    pub fn from_words(words: [u64; 4]) -> Option<Self> {
        match words[0] {
            KEY => char::from_u32(u32::try_from(words[1]).ok()?).map(Key::Character),
            EDIT => Edit::from_word(words[1]).map(Key::Edit),
            _ => None,
        }
    }
}
