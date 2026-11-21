//! The display service – Hematite's first driver, and it runs in user
//! space (DECISIONS.md, 023). It holds the screen's memory, and nobody else
//! does: everyone else lends it their pixels (`&`, so it can only read
//! them) and it copies them onto the screen, in the screen's pixel format.
//!
//! Registers itself in the directory as "display". Protocol: `userlib::display`.
//!
//! Handles: 0 = log, 1 = directory, 2 = the screen (a `Device` handle).

#![no_std]
#![no_main]

mod screen;

use screen::Screen;
use userlib::display::{Area, DRAW, INFO};
use userlib::{Attach, Handle, ReceivedMemory, abi, directory, endpoint_new, exit, println};
use userlib::{receive, reply};

const DIRECTORY: Handle = Handle(1);
const SCREEN: Handle = Handle(2);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let mut screen = match Screen::open(SCREEN) {
        Ok(screen) => screen,
        Err(error) => {
            println!("cannot open the screen: {error:?}");
            exit(1)
        }
    };
    match &screen {
        Some(screen) => println!(
            "Screen opened: {}x{} pixels, write-combining. Only I can draw on it.",
            screen.width(),
            screen.height()
        ),
        None => println!("No screen on this machine - I will draw nowhere."),
    }
    let Ok(requests) = endpoint_new() else {
        exit(2)
    };
    if directory::register(DIRECTORY, "display", requests).is_err() {
        exit(3);
    }
    loop {
        let Ok(mut request) = receive(requests) else {
            continue;
        };
        let Some(answer) = request.reply.take() else {
            continue;
        };
        let words = match request.words[0] {
            INFO => {
                let (width, height) = screen.as_ref().map_or((0, 0), |screen| screen.size());
                [0, u64::from(width), u64::from(height), 0]
            }
            DRAW => {
                let area = Area::from_words([request.words[1], request.words[2]]);
                let pixels = match &request.memory {
                    Some(ReceivedMemory::Shared(pixels) | ReceivedMemory::Exclusive(pixels)) => {
                        Some(pixels.bytes())
                    }
                    Some(ReceivedMemory::Owned(pixels)) => Some(pixels.bytes()),
                    None => None,
                };
                let status = match (pixels, &mut screen) {
                    (None, _) => abi::ERROR_WRONG_KIND,
                    (Some(_), None) => 0, // all of it lies outside a 0 x 0 screen
                    (Some(pixels), Some(screen)) => screen.draw(pixels, area),
                };
                [status, 0, 0, 0]
            }
            _ => [abi::ERROR_BAD_ARGUMENT, 0, 0, 0],
        };
        drop(request); // borrowed pixels go back with the reply, owned ones are freed
        let _ = reply(answer, Attach::Nothing, words);
    }
}
