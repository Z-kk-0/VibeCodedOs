//! The terminal: a strip at the bottom of the screen that shows what you
//! type (DECISIONS.md, 025). Keys come from the keyboard driver as calls
//! (`userlib::keyboard`); the picture goes to the display like paint's –
//! the terminal touches neither the keyboard nor the screen itself.
//!
//! Editing as in a shell: arrows, Home/End, Delete, Backspace; Up and Down
//! bring back earlier lines. Enter finishes a line: it moves up, and the
//! log gets it as "typed: <line>" (the boot test types lines and looks for
//! them there).
//!
//! Handles: 0 = log, 1 = directory, 2 = where keys arrive (receiving end).

#![no_std]
#![no_main]

extern crate alloc;

mod editor;
mod panel;

use editor::Editor;
use panel::Panel;
use userlib::keyboard::Key;
use userlib::{Attach, Error, Handle, directory, display, exit, println, receive, reply};

const DIRECTORY: Handle = Handle(1);
const KEYS: Handle = Handle(2);
/// Without a screen, lines are only logged: no limit but this.
const LONGEST_UNSHOWN_LINE: usize = 1000;

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
    let mut editor = Editor::default();
    loop {
        let Ok(mut message) = receive(KEYS) else {
            continue;
        };
        if let Some(answer) = message.reply.take() {
            // Answer at once: the keyboard driver goes back to its device
            // while we draw.
            let _ = reply(answer, Attach::Nothing, [0; 4]);
        }
        let Some(key) = Key::from_words(message.words) else {
            continue;
        };
        let room = panel
            .as_ref()
            .map_or(LONGEST_UNSHOWN_LINE, Panel::characters_per_line);
        if let Some(line) = editor.apply(key, room) {
            println!("typed: {line}");
            if let Some(panel) = &mut panel {
                panel.finish_line(&line);
            }
        }
        if let Some(panel) = &mut panel
            && let Err(error) = panel.show(&editor.text(), editor.cursor())
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
    panel.show("", 0)?;
    Ok(Some(panel))
}
