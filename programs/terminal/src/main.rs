//! The terminal: a strip at the bottom of the screen that shows what you
//! type (DECISIONS.md, 025). Keys come from the keyboard driver as calls
//! (`userlib::keyboard`); the picture goes to the display like paint's –
//! the terminal touches neither the keyboard nor the screen itself.
//!
//! Enter finishes a line: it moves up, and the log gets it as
//! "typed: <line>" (the boot test types a line and looks for it there).
//!
//! Handles: 0 = log, 1 = directory, 2 = where keys arrive (receiving end).

#![no_std]
#![no_main]

extern crate alloc;

mod panel;

use alloc::string::String;
use panel::Panel;
use userlib::keyboard::{BACKSPACE, KEY};
use userlib::{Attach, Error, Handle, directory, display, exit, println, receive, reply};

const DIRECTORY: Handle = Handle(1);
const KEYS: Handle = Handle(2);

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let mut panel = match open_panel() {
        Ok(panel) => panel,
        Err(error) => {
            println!("cannot draw: {error:?}");
            exit(1)
        }
    };
    println!("Ready: type, and press Enter.");
    let mut line = String::new();
    loop {
        let Ok(mut message) = receive(KEYS) else {
            continue;
        };
        if let Some(answer) = message.reply.take() {
            // Answer at once: the keyboard driver goes back to its device
            // while we draw.
            let _ = reply(answer, Attach::Nothing, [0; 4]);
        }
        if message.words[0] != KEY {
            continue;
        }
        let Some(key) = char::from_u32(message.words[1] as u32) else {
            continue;
        };
        match key {
            '\n' => {
                println!("typed: {line}");
                if let Some(panel) = &mut panel {
                    panel.finish_line(&line);
                }
                line.clear();
            }
            BACKSPACE => {
                line.pop();
            }
            // What doesn't fit on the line anymore is dropped.
            key if !key.is_control()
                && panel
                    .as_ref()
                    .is_none_or(|panel| panel.fits(line.len() + 1)) =>
            {
                line.push(key);
            }
            _ => {}
        }
        if let Some(panel) = &mut panel
            && let Err(error) = panel.show(&line)
        {
            println!("cannot draw: {error:?}");
        }
    }
}

/// The panel at the bottom of the screen; `None` without a screen.
fn open_panel() -> Result<Option<Panel>, Error> {
    let screen = directory::lookup(DIRECTORY, "display")?;
    let (width, height) = display::info(screen)?;
    if width == 0 || height == 0 {
        println!("No screen - I only log what you type.");
        return Ok(None);
    }
    let mut panel = Panel::new(screen, width, height)?;
    panel.show("")?;
    Ok(Some(panel))
}
