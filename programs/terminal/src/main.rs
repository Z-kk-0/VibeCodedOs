//! The terminal: a strip at the bottom of the screen with lines of text and
//! the line being typed (DECISIONS.md, 025, 026). It serves three kinds of
//! clients on one endpoint, told apart by the badge the kernel puts on
//! their handle – which nobody can fake:
//!
//! - the keyboard driver (badge "keyboard"): keys (`userlib::keyboard`).
//!   Editing as in a shell: arrows, Home/End, Delete, Backspace; Up and
//!   Down bring back earlier lines. Enter finishes a line.
//! - everyone else: lines of output (the log's protocol, so `println!`
//!   works), `READ_LINE` to get the next finished line, `CLEAR`
//!   (`userlib::terminal`). That is the shell and the programs it starts.
//!
//! A finished line waits until someone reads it (type-ahead). Every line
//! shown also goes to the log – "typed: <line>" or the output as it is –
//! so the boot test can follow along.
//!
//! The terminal touches neither the keyboard nor the screen itself.
//!
//! Handles: 0 = log, 1 = directory, 2 = its endpoint (receiving end).

#![no_std]
#![no_main]

extern crate alloc;

mod editor;
mod panel;

use alloc::collections::VecDeque;
use alloc::string::String;
use editor::Editor;
use panel::Panel;
use userlib::keyboard::Key;
use userlib::terminal::{CLEAR, READ_LINE};
use userlib::{Attach, Error, Handle, Received, ReceivedMemory, Reply, abi};
use userlib::{directory, display, exit, println, receive, reply};

const DIRECTORY: Handle = Handle(1);
const REQUESTS: Handle = Handle(2);
/// Without a screen, lines are only logged: no limit but this.
const LONGEST_UNSHOWN_LINE: usize = 1000;

struct Terminal {
    panel: Option<Panel>,
    editor: Editor,
    /// Finished lines nobody has read yet.
    typed_ahead: VecDeque<String>,
    /// Someone waiting in `READ_LINE`: the duty to answer, and their
    /// message, which holds the page they lent us for the line.
    reader: Option<(Reply, Received)>,
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let panel = match open_panel() {
        Ok(panel) => panel,
        Err(error) => {
            println!("cannot draw: {error:?}");
            exit(1)
        }
    };
    let mut terminal = Terminal {
        panel,
        editor: Editor::default(),
        typed_ahead: VecDeque::new(),
        reader: None,
    };
    terminal.redraw();
    println!("Ready: type, and press Enter.");
    let keyboard = abi::badge_from_name("keyboard");
    loop {
        let Ok(mut message) = receive(REQUESTS) else {
            continue;
        };
        let Some(answer) = message.reply.take() else {
            continue;
        };
        if message.badge == keyboard {
            // Answer at once: the keyboard driver goes back to its device
            // while we draw.
            let _ = reply(answer, Attach::Nothing, [0; 4]);
            if let Some(key) = Key::from_words(message.words) {
                terminal.press(key);
            }
        } else {
            // What the others want is told by what they attach: a page
            // lent `&` is text (the log's protocol: word 0 is only its
            // length), a page lent `&mut` is room for a line, nothing is a
            // command.
            match (&message.memory, message.words[0]) {
                (Some(ReceivedMemory::Exclusive(_)), READ_LINE) if terminal.reader.is_none() => {
                    terminal.reader = Some((answer, message));
                    terminal.serve_reader();
                }
                (Some(ReceivedMemory::Exclusive(_)), READ_LINE) => {
                    let _ = reply(answer, Attach::Nothing, [abi::ERROR_BUSY, 0, 0, 0]);
                }
                (Some(ReceivedMemory::Exclusive(_)), _) | (None, _)
                    if message.words[0] != CLEAR =>
                {
                    let _ = reply(answer, Attach::Nothing, [abi::ERROR_BAD_ARGUMENT, 0, 0, 0]);
                }
                (Some(ReceivedMemory::Exclusive(_)), _) => {
                    let _ = reply(answer, Attach::Nothing, [abi::ERROR_BAD_ARGUMENT, 0, 0, 0]);
                }
                (None, _) => {
                    let _ = reply(answer, Attach::Nothing, [0; 4]);
                    if let Some(panel) = &mut terminal.panel {
                        panel.clear();
                    }
                }
                _ => {
                    let status = terminal.write(&message).err().map_or(0, |error| error.0);
                    let _ = reply(answer, Attach::Nothing, [status, 0, 0, 0]);
                }
            }
        }
        terminal.redraw();
    }
}

impl Terminal {
    fn press(&mut self, key: Key) {
        let room = self
            .panel
            .as_ref()
            .map_or(LONGEST_UNSHOWN_LINE, Panel::characters_per_line);
        if let Some(line) = self.editor.apply(key, room) {
            println!("typed: {line}");
            if let Some(panel) = &mut self.panel {
                panel.finish_line(&line);
            }
            self.typed_ahead.push_back(line);
            self.serve_reader();
        }
    }

    /// Lines of output, lent to us like to the log.
    fn write(&mut self, message: &Received) -> Result<(), Error> {
        let text = match &message.memory {
            Some(ReceivedMemory::Shared(text) | ReceivedMemory::Exclusive(text)) => text.bytes(),
            Some(ReceivedMemory::Owned(text)) => text.bytes(),
            None => return Err(Error(abi::ERROR_WRONG_KIND)),
        };
        let text = text
            .get(..message.words[0] as usize)
            .ok_or(Error(abi::ERROR_TOO_LONG))?;
        let text = core::str::from_utf8(text).map_err(|_| Error(abi::ERROR_NOT_UTF8))?;
        for line in text.lines() {
            println!("{line}");
            if let Some(panel) = &mut self.panel {
                panel.add_output(line);
            }
        }
        Ok(())
    }

    /// Hands the oldest unread line to the waiting reader, if both exist.
    fn serve_reader(&mut self) {
        if self.reader.is_none() || self.typed_ahead.is_empty() {
            return;
        }
        let (answer, mut message) = self.reader.take().expect("checked");
        let line = self.typed_ahead.pop_front().expect("checked");
        let page = match &mut message.memory {
            Some(ReceivedMemory::Exclusive(page)) => page.bytes_mut(),
            Some(ReceivedMemory::Owned(page)) => Some(page.bytes_mut()),
            _ => None,
        };
        let words = match page {
            Some(page) => {
                let length = line.len().min(page.len());
                page[..length].copy_from_slice(&line.as_bytes()[..length]);
                [0, length as u64, 0, 0]
            }
            // Lent `&`: we can't write the line into it.
            None => [abi::ERROR_BAD_MODE, 0, 0, 0],
        };
        let _ = reply(answer, Attach::Nothing, words);
    }

    fn redraw(&mut self) {
        if let Some(panel) = &mut self.panel
            && let Err(error) = panel.show(&self.editor.text(), self.editor.cursor())
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
    Ok(Some(Panel::new(screen, width, height)?))
}
