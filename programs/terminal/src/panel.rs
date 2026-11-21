//! The strip of lines at the bottom of the screen: the last lines – typed
//! ones dimmed, after a dimmed prompt; output bright – and below them the
//! one being typed, after the prompt.
//!
//! The boot test checks the bottom-left pixel of the screen for
//! `BACKGROUND` – keep it so.

use alloc::collections::VecDeque;
use alloc::string::String;
use userlib::display::{self, Area};
use userlib::{Error, Handle, Memory, abi, font};

pub const BACKGROUND: u32 = 0x0E_0F_12;
const TEXT: u32 = 0xC9_CC_D3;
const OLD_TEXT: u32 = 0x7C_80_8A;
/// Hematite's streak red, as in paint's picture.
const PROMPT: u32 = 0x9B_2A_22;
const OLD_PROMPT: u32 = 0x5A_25_21;

/// Lines shown above the one being typed.
const HISTORY: usize = 10;
const PADDING: u32 = 8;
const PROMPT_TEXT: &str = "> ";

pub struct Panel {
    display: Handle,
    pixels: Memory,
    /// Just the line being typed: what most key presses change.
    input: Memory,
    width: u32,
    height: u32,
    /// Where the panel starts on the screen.
    top: u32,
    scale: u32,
    lines: VecDeque<(String, Kind)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Typed,
    Output,
}

impl Panel {
    pub fn new(display: Handle, screen_width: u32, screen_height: u32) -> Result<Self, Error> {
        let scale = (screen_height / 300).max(2);
        let rows = HISTORY as u32 + 1;
        let height = (rows * row_height(scale) + 2 * PADDING).min(screen_height);
        let bytes = u64::from(screen_width) * u64::from(height) * 4;
        Ok(Self {
            display,
            pixels: Memory::new(bytes.div_ceil(abi::PAGE_SIZE))?,
            input: Memory::new(
                (u64::from(screen_width) * u64::from(row_height(scale)) * 4)
                    .div_ceil(abi::PAGE_SIZE),
            )?,
            width: screen_width,
            height,
            top: screen_height - height,
            scale,
            lines: VecDeque::new(),
        })
    }

    /// How many characters fit behind the prompt (and the cursor after them).
    pub fn characters_per_line(&self) -> usize {
        (self.columns() as usize).saturating_sub(PROMPT_TEXT.len() + 1)
    }

    pub fn finish_line(&mut self, line: &str) {
        self.push(String::from(line), Kind::Typed);
    }

    /// Shows `text` (one line, no newline) as output, wrapped at the edge.
    pub fn add_output(&mut self, text: &str) {
        let columns = self.columns().max(1) as usize;
        let mut rest = text;
        loop {
            let cut = rest
                .char_indices()
                .nth(columns)
                .map_or(rest.len(), |(at, _)| at);
            self.push(String::from(&rest[..cut]), Kind::Output);
            rest = &rest[cut..];
            if rest.is_empty() {
                break;
            }
        }
    }

    /// Empties the strip (the shell's `clear`).
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Draws everything with `line` as the one being typed and the cursor
    /// under its character number `cursor`, and lends it to the display.
    pub fn show(&mut self, line: &str, cursor: usize) -> Result<(), Error> {
        let (width, height, scale) = (self.width, self.height, self.scale);
        let mut canvas = Canvas {
            bytes: self.pixels.bytes_mut(),
            width,
            height,
        };
        canvas.fill(0, 0, width, height, BACKGROUND);
        let mut y = PADDING;
        for (old, kind) in &self.lines {
            match kind {
                Kind::Typed => {
                    let x = canvas.text(PADDING, y, PROMPT_TEXT, scale, OLD_PROMPT);
                    canvas.text(x, y, old, scale, OLD_TEXT);
                }
                Kind::Output => {
                    canvas.text(PADDING, y, old, scale, TEXT);
                }
            }
            y += row_height(scale);
        }
        input_row(&mut canvas, y, line, cursor, scale);
        let area = Area {
            x: 0,
            y: self.top,
            width,
            height,
        };
        display::draw(self.display, &self.pixels, area)
    }

    /// Draws only the line being typed – after a key that changed nothing
    /// else, a twelfth of the pixels of `show`.
    pub fn show_input(&mut self, line: &str, cursor: usize) -> Result<(), Error> {
        let (width, scale) = (self.width, self.scale);
        let height = row_height(scale);
        let mut canvas = Canvas {
            bytes: self.input.bytes_mut(),
            width,
            height,
        };
        canvas.fill(0, 0, width, height, BACKGROUND);
        input_row(&mut canvas, 0, line, cursor, scale);
        let area = Area {
            x: 0,
            y: self.top + PADDING + self.lines.len() as u32 * height,
            width,
            height,
        };
        display::draw(self.display, &self.input, area)
    }

    /// Letters that fit across the strip.
    fn columns(&self) -> u32 {
        self.width.saturating_sub(2 * PADDING) / font::advance(self.scale)
    }

    fn push(&mut self, line: String, kind: Kind) {
        if self.lines.len() == HISTORY {
            self.lines.pop_front();
        }
        self.lines.push_back((line, kind));
    }
}

/// The prompt, the line being typed, and the cursor – a bar just below
/// the letter it stands before.
fn input_row(canvas: &mut Canvas, y: u32, line: &str, cursor: usize, scale: u32) {
    let x = canvas.text(PADDING, y, PROMPT_TEXT, scale, PROMPT);
    canvas.text(x, y, line, scale, TEXT);
    let cursor_x = x + cursor as u32 * font::advance(scale);
    let cursor_y = y + (font::HEIGHT + 1) * scale;
    canvas.fill(
        cursor_x,
        cursor_y,
        font::advance(scale) - scale,
        scale,
        TEXT,
    );
}

fn row_height(scale: u32) -> u32 {
    (font::HEIGHT + 3) * scale
}

struct Canvas<'a> {
    bytes: &'a mut [u8],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
    /// Draws `text` with its top-left corner at (`x`, `y`); returns where
    /// the next letter would start.
    fn text(&mut self, mut x: u32, y: u32, text: &str, scale: u32, color: u32) -> u32 {
        for letter in text.chars() {
            font::draw(letter, |dx, dy| {
                self.fill(x + dx * scale, y + dy * scale, scale, scale, color)
            });
            x += font::advance(scale);
        }
        x
    }

    /// Fills a rectangle; whatever lies outside the canvas is left out.
    fn fill(&mut self, x: u32, y: u32, width: u32, height: u32, color: u32) {
        let end_x = x.saturating_add(width).min(self.width);
        let end_y = y.saturating_add(height).min(self.height);
        for row in y..end_y {
            for column in x..end_x {
                let at = (row as usize * self.width as usize + column as usize) * 4;
                self.bytes[at..at + 4].copy_from_slice(&color.to_le_bytes());
            }
        }
    }
}
