//! The picture: a hematite crystal – steel grey, rhombohedral – above its
//! streak. Rubbed on unglazed porcelain, the grey mineral leaves a *red*
//! line (that's its name: Greek "haima", blood). Below it, the name.
//!
//! The boot test (`xtask/src/screenshot.rs`) checks two pixels of this:
//! the top-left one is `BACKGROUND_TOP`, the one in the centre of the
//! screen is `STREAK` – keep them so.

use userlib::font;

pub const BACKGROUND_TOP: u32 = 0x15_16_1A;
const BACKGROUND_BOTTOM: u32 = 0x2B_2D_34;
pub const STREAK: u32 = 0x9B_2A_22;
const TEXT: u32 = 0xC9_CC_D3;
/// The crystal's four faces, lit from the top left.
const FACES: [u32; 4] = [0xA7_AB_B4, 0x7C_80_8A, 0x5A_5E_67, 0x40_43_4B];

/// Draws into `canvas`: `width` × `height` pixels, `0x00RRGGBB` each.
pub fn render(canvas: &mut [u8], width: u32, height: u32) {
    let mut canvas = Canvas {
        bytes: canvas,
        width,
        height,
    };
    let (center_x, center_y) = (width / 2, height / 2);
    for y in 0..height {
        let color = blend(BACKGROUND_TOP, BACKGROUND_BOTTOM, y, height.max(2) - 1);
        canvas.fill(0, y, width, 1, color);
    }
    crystal(&mut canvas, center_x, center_y - height / 5, height / 7);

    // The streak runs through the very centre of the screen.
    let thickness = (height / 60).max(4);
    let length = width / 5;
    canvas.fill(
        center_x - length,
        center_y - thickness / 2,
        2 * length,
        thickness,
        STREAK,
    );

    let scale = (height / 120).max(2);
    let text_width = font::width("HEMATITE", scale);
    let mut x = center_x.saturating_sub(text_width / 2);
    let y = center_y + height / 10;
    for letter in "HEMATITE".chars() {
        font::draw(letter, |dx, dy| {
            canvas.fill(x + dx * scale, y + dy * scale, scale, scale, TEXT)
        });
        x += font::advance(scale);
    }
}

/// A rhombus standing on its tip, split into four faces by its diagonals.
fn crystal(canvas: &mut Canvas, center_x: u32, center_y: u32, half_height: u32) {
    let half_width = half_height * 3 / 4;
    let (half_width, half_height) = (i64::from(half_width), i64::from(half_height));
    for dy in -half_height..=half_height {
        for dx in -half_width..=half_width {
            // Inside the rhombus: |dx|/w + |dy|/h <= 1, without dividing.
            if dx.abs() * half_height + dy.abs() * half_width > half_width * half_height {
                continue;
            }
            let face = match (dx < 0, dy < 0) {
                (true, true) => FACES[0],
                (false, true) => FACES[1],
                (true, false) => FACES[2],
                (false, false) => FACES[3],
            };
            let x = i64::from(center_x) + dx;
            let y = i64::from(center_y) + dy;
            if x >= 0 && y >= 0 {
                canvas.fill(x as u32, y as u32, 1, 1, face);
            }
        }
    }
}

/// `from` at `step` 0, `to` at `step == steps`, in between per channel.
fn blend(from: u32, to: u32, step: u32, steps: u32) -> u32 {
    let channel = |shift: u32| {
        let (a, b) = ((from >> shift) & 0xFF, (to >> shift) & 0xFF);
        let value = (a * (steps - step) + b * step) / steps;
        value << shift
    };
    channel(16) | channel(8) | channel(0)
}

struct Canvas<'a> {
    bytes: &'a mut [u8],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
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
