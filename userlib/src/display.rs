//! Talking to the display service (`programs/display`), which owns the
//! screen and draws on it for everyone else (DECISIONS.md, 023).
//!
//! Message word 0 is the request:
//! - `INFO`: reply words 1 and 2 = width and height in pixels (0 × 0: no screen).
//! - `DRAW`: lend the pixels `&` (shared – the display only reads them);
//!   word 1 = x | y << 32, word 2 = width | height << 32. The pixels are
//!   rows of `width` `u32`s, `0x00RRGGBB`, without padding. Whatever lies
//!   outside the screen is cut off.
//!
//! Reply word 0 = 0 or an error code.

use crate::ipc::{Attach, Error, Handle, call, check};
use crate::memory::Memory;

pub const INFO: u64 = 1;
pub const DRAW: u64 = 2;

/// A rectangle of pixels, `x`/`y` its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Area {
    /// The two message words that carry it.
    pub fn to_words(self) -> [u64; 2] {
        [pack(self.x, self.y), pack(self.width, self.height)]
    }

    pub fn from_words([position, size]: [u64; 2]) -> Self {
        let ((x, y), (width, height)) = (unpack(position), unpack(size));
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// The screen's size: (width, height).
pub fn info(display: Handle) -> Result<(u32, u32), Error> {
    let reply = call(display, Attach::Nothing, [INFO, 0, 0, 0])?;
    check(reply.words[0])?;
    Ok((reply.words[1] as u32, reply.words[2] as u32))
}

/// Puts `pixels` on the screen at `area`.
pub fn draw(display: Handle, pixels: &Memory, area: Area) -> Result<(), Error> {
    let [position, size] = area.to_words();
    let reply = call(display, Attach::Lend(pixels), [DRAW, position, size, 0])?;
    check(reply.words[0])
}

fn pack(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

fn unpack(word: u64) -> (u32, u32) {
    (word as u32, (word >> 32) as u32)
}
