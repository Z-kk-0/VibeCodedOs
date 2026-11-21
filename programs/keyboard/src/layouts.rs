//! Keyboard layouts: which character a key types, by its scancode (set 1).
//! A scancode names a *position* on the keyboard, not a letter – the key
//! right of T sends 0x15 everywhere; a US layout calls it Y, a German one Z.

/// A row of keys with consecutive make codes: the first code, what they
/// type, and what they type with Shift (one character per key).
struct Row {
    first: u8,
    normal: &'static str,
    shifted: &'static str,
}

pub struct Layout {
    pub name: &'static str,
    rows: &'static [Row],
    /// What keys type with AltGr (the right Alt key).
    alt_graph: &'static [(u8, char)],
}

impl Layout {
    /// The character key `code` types, with or without Shift.
    pub fn character(&self, code: u8, shift: bool) -> Option<char> {
        let row = self.rows.iter().find(|row| {
            let keys = row.normal.chars().count() as u8;
            (row.first..row.first + keys).contains(&code)
        })?;
        let index = usize::from(code - row.first);
        let typed = if shift { row.shifted } else { row.normal };
        typed.chars().nth(index)
    }

    /// Whether Caps Lock turns the key into its shifted one: only for
    /// letters whose shifted character is their capital (not for ß).
    pub fn is_letter(&self, code: u8) -> bool {
        let (Some(normal), Some(shifted)) =
            (self.character(code, false), self.character(code, true))
        else {
            return false;
        };
        normal != shifted && normal.to_uppercase().eq(core::iter::once(shifted))
    }

    pub fn alt_graph(&self, code: u8) -> Option<char> {
        self.alt_graph
            .iter()
            .find(|&&(key, _)| key == code)
            .map(|&(_, character)| character)
    }
}

pub const US: Layout = Layout {
    name: "US (QWERTY)",
    rows: &[
        Row {
            first: 0x02,
            normal: "1234567890-=",
            shifted: "!@#$%^&*()_+",
        },
        Row {
            first: 0x10,
            normal: "qwertyuiop[]",
            shifted: "QWERTYUIOP{}",
        },
        Row {
            first: 0x1E,
            normal: "asdfghjkl;'`",
            shifted: "ASDFGHJKL:\"~",
        },
        Row {
            first: 0x2B,
            normal: "\\zxcvbnm,./",
            shifted: "|ZXCVBNM<>?",
        },
        // The extra key left of Z on keyboards with 102 keys.
        Row {
            first: 0x56,
            normal: "\\",
            shifted: "|",
        },
    ],
    alt_graph: &[],
};

/// The accents (´ ` ^) are typed as themselves: no dead keys yet.
pub const GERMAN: Layout = Layout {
    name: "German (QWERTZ)",
    rows: &[
        Row {
            first: 0x02,
            normal: "1234567890ß´",
            shifted: "!\"§$%&/()=?`",
        },
        Row {
            first: 0x10,
            normal: "qwertzuiopü+",
            shifted: "QWERTZUIOPÜ*",
        },
        Row {
            first: 0x1E,
            normal: "asdfghjklöä^",
            shifted: "ASDFGHJKLÖÄ°",
        },
        Row {
            first: 0x2B,
            normal: "#yxcvbnm,.-",
            shifted: "'YXCVBNM;:_",
        },
        Row {
            first: 0x56,
            normal: "<",
            shifted: ">",
        },
    ],
    alt_graph: &[
        (0x03, '²'),
        (0x04, '³'),
        (0x08, '{'),
        (0x09, '['),
        (0x0A, ']'),
        (0x0B, '}'),
        (0x0C, '\\'),
        (0x10, '@'),
        (0x12, '€'),
        (0x1B, '~'),
        (0x32, 'µ'),
        (0x56, '|'),
    ],
};

/// In the order Alt+Shift cycles through them; the first is the default.
pub const ALL: [&Layout; 2] = [&US, &GERMAN];
