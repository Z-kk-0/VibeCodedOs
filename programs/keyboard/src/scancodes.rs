//! Scancode set 1 (what the controller's translation produces), US layout.
//! A key sends its "make" code when pressed and the same code + 0x80 when
//! released; some keys send 0xE0 first (arrows, right Ctrl, …) – those are
//! skipped for now. Only Shift and Caps Lock change what a key means.

const EXTENDED: u8 = 0xE0;
const RELEASED: u8 = 0x80;
const LEFT_SHIFT: u8 = 0x2A;
const RIGHT_SHIFT: u8 = 0x36;
const CAPS_LOCK: u8 = 0x3A;
const BACKSPACE: u8 = 0x0E;
const TAB: u8 = 0x0F;
const ENTER: u8 = 0x1C;
const SPACE: u8 = 0x39;

/// A row of keys with consecutive make codes: the first code, what they
/// type, and what they type with Shift.
struct Row {
    first: u8,
    normal: &'static [u8],
    shifted: &'static [u8],
}

const ROWS: [Row; 4] = [
    Row {
        first: 0x02,
        normal: b"1234567890-=",
        shifted: b"!@#$%^&*()_+",
    },
    Row {
        first: 0x10,
        normal: b"qwertyuiop[]",
        shifted: b"QWERTYUIOP{}",
    },
    Row {
        first: 0x1E,
        normal: b"asdfghjkl;'`",
        shifted: b"ASDFGHJKL:\"~",
    },
    Row {
        first: 0x2B,
        normal: b"\\zxcvbnm,./",
        shifted: b"|ZXCVBNM<>?",
    },
];

#[derive(Default)]
pub struct Decoder {
    shift: bool,
    caps_lock: bool,
    /// The last byte was 0xE0: this one belongs to an extended key.
    extended: bool,
}

impl Decoder {
    /// Takes the next byte from the keyboard; returns the character of a
    /// key that was just pressed, if it has one.
    pub fn feed(&mut self, byte: u8) -> Option<char> {
        if byte == EXTENDED {
            self.extended = true;
            return None;
        }
        if core::mem::take(&mut self.extended) {
            return None; // arrows & co.: nothing to type yet
        }
        let (code, pressed) = (byte & !RELEASED, byte & RELEASED == 0);
        match code {
            LEFT_SHIFT | RIGHT_SHIFT => {
                self.shift = pressed;
                None
            }
            CAPS_LOCK if pressed => {
                self.caps_lock = !self.caps_lock;
                None
            }
            _ if !pressed => None,
            BACKSPACE => Some(userlib::keyboard::BACKSPACE),
            TAB => Some('\t'),
            ENTER => Some('\n'),
            SPACE => Some(' '),
            _ => self.printable(code),
        }
    }

    fn printable(&self, code: u8) -> Option<char> {
        let row = ROWS
            .iter()
            .find(|row| (row.first..row.first + row.normal.len() as u8).contains(&code))?;
        let index = usize::from(code - row.first);
        // Caps Lock shifts letters only; Shift on top of it unshifts them.
        let shift = if row.normal[index].is_ascii_alphabetic() {
            self.shift != self.caps_lock
        } else {
            self.shift
        };
        let typed = if shift { row.shifted } else { row.normal };
        Some(char::from(typed[index]))
    }
}
