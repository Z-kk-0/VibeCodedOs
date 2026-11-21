//! Paints Hematite's first picture: it asks the directory for the display
//! service, renders the picture into memory of its own and lends it to the
//! display `&` – paint never touches the screen itself (DECISIONS.md, 023).
//!
//! The boot test takes a screenshot afterwards and checks two pixels (see
//! `picture.rs`), so "it drew" is proven from outside the machine.
//!
//! Handles: 0 = log, 1 = directory.

#![no_std]
#![no_main]

mod picture;

use userlib::display::{self, Area};
use userlib::{Error, Handle, Memory, abi, directory, exit, println};

const DIRECTORY: Handle = Handle(1);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    match paint() {
        Ok(()) => exit(0),
        Err(error) => {
            println!("painting failed: {error:?}");
            exit(1)
        }
    }
}

fn paint() -> Result<(), Error> {
    let screen = directory::lookup(DIRECTORY, "display")?;
    let (width, height) = display::info(screen)?;
    if width == 0 || height == 0 {
        println!("No screen to paint on.");
        return Ok(());
    }
    let bytes = u64::from(width) * u64::from(height) * 4;
    let mut canvas = Memory::new(bytes.div_ceil(abi::PAGE_SIZE))?;
    picture::render(canvas.bytes_mut(), width, height);
    let whole_screen = Area {
        x: 0,
        y: 0,
        width,
        height,
    };
    display::draw(screen, &canvas, whole_screen)?;
    println!("Painted the {width}x{height} screen - through the display, in borrowed memory.");
    Ok(())
}
