//! Proof from outside the machine that Hematite drew on the screen: after a
//! normal boot test QEMU takes a screenshot (`screendump`, a PPM file) and
//! we check pixels of the picture `programs/paint` draws and of the
//! terminal strip below it (`programs/terminal`). Neither the kernel nor
//! any program can fake this – it is what a person would see.

use crate::Result;
use std::fs;
use std::path::Path;

/// `programs/paint/src/picture.rs`: the background's top colour (seen in
/// the top-left corner) and the red streak through the centre.
const BACKGROUND_TOP: [u8; 3] = [0x15, 0x16, 0x1A];
const STREAK: [u8; 3] = [0x9B, 0x2A, 0x22];
/// `programs/terminal/src/panel.rs`: the strip at the bottom of the screen.
const TERMINAL: [u8; 3] = [0x0E, 0x0F, 0x12];

/// A screenshot: width, height, and 3 bytes (red, green, blue) per pixel.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pixels: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * self.width + x) * 3;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }
}

/// Checks the screenshot QEMU wrote to `path`.
pub fn check(path: &Path) -> Result {
    let bytes = fs::read(path).map_err(|error| format!("no screenshot: {error}"))?;
    let image = parse_ppm(&bytes)?;
    let checks = [
        ("top-left corner", 0, 0, BACKGROUND_TOP),
        ("centre", image.width / 2, image.height / 2, STREAK),
        ("bottom-left corner", 0, image.height - 1, TERMINAL),
    ];
    for (name, x, y, expected) in checks {
        let found = image.pixel(x, y);
        if found != expected {
            return Err(format!(
                "screenshot: the {name} ({x}, {y}) is {found:02X?}, not {expected:02X?} - the picture or the terminal is not on the screen"
            ));
        }
    }
    println!(
        "Screenshot {}x{}: paint's picture and the terminal are on the screen.",
        image.width, image.height
    );
    Ok(())
}

/// Binary PPM ("P6"): a text header – magic, width, height, maximum value –
/// then the pixels. Netpbm format spec; QEMU writes no comments.
pub fn parse_ppm(bytes: &[u8]) -> Result<Image> {
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            at += 1;
        }
        if start == at {
            return Err("screenshot: truncated PPM header".into());
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    at += 1; // exactly one whitespace byte before the pixels
    let number = |index: usize| -> Result<usize> {
        fields[index]
            .parse()
            .map_err(|_| format!("screenshot: bad PPM header field '{}'", fields[index]))
    };
    if fields[0] != "P6" || number(3)? != 255 {
        return Err("screenshot: not an 8-bit binary PPM".into());
    }
    let (width, height) = (number(1)?, number(2)?);
    let pixels = bytes
        .get(at..at + width * height * 3)
        .ok_or("screenshot: PPM too short")?;
    Ok(Image {
        width,
        height,
        pixels: pixels.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_ppm_is_read_pixel_by_pixel() {
        let mut file = b"P6\n2 1\n255\n".to_vec();
        file.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let image = parse_ppm(&file).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixel(1, 0), [4, 5, 6]);
    }

    #[test]
    fn a_short_ppm_is_refused() {
        assert!(parse_ppm(b"P6\n2 2\n255\n\x01\x02").is_err());
        assert!(parse_ppm(b"P5\n1 1\n255\n\x01").is_err());
    }
}
