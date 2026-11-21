//! Scancode set 1 (what the controller's translation produces). A key sends
//! its "make" code when pressed and the same code + 0x80 when released.
//! Keys added after the original PC keyboard (arrows, right Alt, …) send
//! 0xE0 first. Which character a key types is the layout's business
//! (`layouts.rs`); here we track the modifiers: Shift, Caps Lock, Alt, and
//! AltGr (right Alt). Left Alt + Shift switches the layout, as on Windows.

use crate::layouts::{self, Layout};
use userlib::keyboard::{BACKSPACE, Edit, Key};

const EXTENDED: u8 = 0xE0;
const RELEASED: u8 = 0x80;
const LEFT_SHIFT: u8 = 0x2A;
const RIGHT_SHIFT: u8 = 0x36;
const CAPS_LOCK: u8 = 0x3A;
const ALT: u8 = 0x38; // left Alt; with 0xE0 before it: AltGr
const BACKSPACE_KEY: u8 = 0x0E;
const TAB: u8 = 0x0F;
const ENTER: u8 = 0x1C; // with 0xE0: the keypad's Enter
const SPACE: u8 = 0x39;
/// After 0xE0.
const KEYPAD_SLASH: u8 = 0x35;
const EDITS: [(u8, Edit); 7] = [
    (0x4B, Edit::Left),
    (0x4D, Edit::Right),
    (0x48, Edit::Up),
    (0x50, Edit::Down),
    (0x47, Edit::Home),
    (0x4F, Edit::End),
    (0x53, Edit::Delete),
];

/// What a byte from the keyboard meant.
pub enum Event {
    Key(Key),
    /// Alt + Shift switched to this layout.
    Layout(&'static str),
}

#[derive(Default)]
pub struct Decoder {
    shift: bool,
    caps_lock: bool,
    alt: bool,
    alt_graph: bool,
    /// The last byte was 0xE0: this one belongs to an extended key.
    extended: bool,
    /// Index into `layouts::ALL`.
    layout: usize,
}

impl Decoder {
    /// Takes the next byte from the keyboard.
    pub fn feed(&mut self, byte: u8) -> Option<Event> {
        if byte == EXTENDED {
            self.extended = true;
            return None;
        }
        let extended = core::mem::take(&mut self.extended);
        let (code, pressed) = (byte & !RELEASED, byte & RELEASED == 0);
        if extended {
            return self.extended_key(code, pressed).map(Event::Key);
        }
        match code {
            LEFT_SHIFT | RIGHT_SHIFT => {
                let switch = pressed && !self.shift && self.alt;
                self.shift = pressed;
                switch.then(|| self.next_layout())
            }
            ALT => {
                let switch = pressed && !self.alt && self.shift;
                self.alt = pressed;
                switch.then(|| self.next_layout())
            }
            CAPS_LOCK if pressed => {
                self.caps_lock = !self.caps_lock;
                None
            }
            _ if !pressed => None,
            BACKSPACE_KEY => Some(Event::Key(Key::Character(BACKSPACE))),
            TAB => Some(Event::Key(Key::Character('\t'))),
            ENTER => Some(Event::Key(Key::Character('\n'))),
            SPACE => Some(Event::Key(Key::Character(' '))),
            _ => self.typed(code).map(|key| Event::Key(Key::Character(key))),
        }
    }

    fn extended_key(&mut self, code: u8, pressed: bool) -> Option<Key> {
        if code == ALT {
            self.alt_graph = pressed;
            return None;
        }
        if !pressed {
            return None;
        }
        match code {
            ENTER => Some(Key::Character('\n')),
            KEYPAD_SLASH => Some(Key::Character('/')),
            // Everything else that isn't an edit key (right Ctrl, the fake
            // Shifts some keyboards send around Print Screen, …): ignored.
            _ => EDITS
                .iter()
                .find(|&&(key, _)| key == code)
                .map(|&(_, edit)| Key::Edit(edit)),
        }
    }

    fn typed(&self, code: u8) -> Option<char> {
        let layout = self.layout();
        if self.alt_graph {
            return layout.alt_graph(code);
        }
        // Caps Lock shifts letters only; Shift on top of it unshifts them.
        let shift = if layout.is_letter(code) {
            self.shift != self.caps_lock
        } else {
            self.shift
        };
        layout.character(code, shift)
    }

    fn layout(&self) -> &'static Layout {
        layouts::ALL[self.layout]
    }

    fn next_layout(&mut self) -> Event {
        self.layout = (self.layout + 1) % layouts::ALL.len();
        Event::Layout(self.layout().name)
    }
}
