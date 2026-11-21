//! Just enough letters for one word: 5 × 7 pixel glyphs, one byte per row,
//! the lowest 5 bits from left (bit 4) to right (bit 0).

const WIDTH: u32 = 5;
/// One empty column between letters.
const GAP: u32 = 1;

fn glyph(letter: char) -> [u8; 7] {
    match letter {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        _ => [0; 7], // unknown letters stay blank
    }
}

/// Calls `pixel(column, row)` for every lit pixel of `letter`.
pub fn draw(letter: char, mut pixel: impl FnMut(u32, u32)) {
    for (row, bits) in (0..).zip(glyph(letter)) {
        for column in 0..WIDTH {
            if bits & (1 << (WIDTH - 1 - column)) != 0 {
                pixel(column, row);
            }
        }
    }
}

/// How far the next letter starts, in pixels.
pub fn advance(scale: u32) -> u32 {
    (WIDTH + GAP) * scale
}

/// Pixels `text` takes up, without the gap after its last letter.
pub fn width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32 * advance(scale)).saturating_sub(GAP * scale)
}
