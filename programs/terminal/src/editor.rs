//! The line being typed: characters, a cursor in them, and the lines typed
//! before (Up and Down bring them back, as in a shell).

use alloc::string::String;
use alloc::vec::Vec;
use userlib::keyboard::{BACKSPACE, Edit, Key};

/// How many finished lines Up can bring back.
const HISTORY: usize = 32;

#[derive(Default)]
pub struct Editor {
    line: Vec<char>,
    /// Where the next character goes: 0 = before the first.
    cursor: usize,
    history: Vec<String>,
    /// While browsing the history: which line is shown, and the line that
    /// was being typed before (Down past the newest brings it back).
    browsing: Option<(usize, Vec<char>)>,
}

impl Editor {
    /// Applies a key. Returns the finished line when it was Enter.
    /// `room` says how many characters fit on the line.
    pub fn apply(&mut self, key: Key, room: usize) -> Option<String> {
        match key {
            Key::Character('\n') => return Some(self.finish()),
            Key::Character(BACKSPACE) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.line.remove(self.cursor);
                }
            }
            Key::Character(character) if !character.is_control() => {
                // What doesn't fit on the line anymore is dropped.
                if self.line.len() < room {
                    self.line.insert(self.cursor, character);
                    self.cursor += 1;
                }
            }
            Key::Character(_) => {} // Tab & co.: nothing to do yet
            Key::Edit(Edit::Left) => self.cursor = self.cursor.saturating_sub(1),
            Key::Edit(Edit::Right) => self.cursor = (self.cursor + 1).min(self.line.len()),
            Key::Edit(Edit::Home) => self.cursor = 0,
            Key::Edit(Edit::End) => self.cursor = self.line.len(),
            Key::Edit(Edit::Delete) => {
                if self.cursor < self.line.len() {
                    self.line.remove(self.cursor);
                }
            }
            Key::Edit(Edit::Up) => self.browse_older(),
            Key::Edit(Edit::Down) => self.browse_newer(),
        }
        None
    }

    pub fn text(&self) -> String {
        self.line.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn finish(&mut self) -> String {
        let line = self.text();
        if !line.is_empty() {
            if self.history.len() == HISTORY {
                self.history.remove(0);
            }
            self.history.push(line.clone());
        }
        self.line.clear();
        self.cursor = 0;
        self.browsing = None;
        line
    }

    fn browse_older(&mut self) {
        let index = match &self.browsing {
            None if self.history.is_empty() => return,
            None => {
                let draft = core::mem::take(&mut self.line);
                self.browsing = Some((self.history.len(), draft));
                self.history.len() - 1
            }
            Some((0, _)) => return, // already the oldest
            Some((index, _)) => index - 1,
        };
        self.show_history(index);
    }

    fn browse_newer(&mut self) {
        let Some((index, _)) = &self.browsing else {
            return;
        };
        let index = index + 1;
        if index < self.history.len() {
            self.show_history(index);
        } else if let Some((_, draft)) = self.browsing.take() {
            self.line = draft;
            self.cursor = self.line.len();
        }
    }

    fn show_history(&mut self, index: usize) {
        if let Some((shown, _)) = &mut self.browsing {
            *shown = index;
        }
        self.line = self.history[index].chars().collect();
        self.cursor = self.line.len();
    }
}
