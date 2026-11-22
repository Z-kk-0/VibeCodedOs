//! The terminal: a strip at the bottom of the screen with lines of text and
//! the line being typed (DECISIONS.md, 025, 026, 027). It serves three kinds of
//! clients on one endpoint, told apart by the badge the kernel puts on
//! their handle – which nobody can fake:
//!
//! - the keyboard driver (badge "keyboard"): keys (`userlib::keyboard`).
//!   Editing as in a shell: arrows, Home/End, Delete, Backspace; Up and
//!   Down bring back earlier lines. Enter finishes a line; Ctrl+C drops it
//!   and answers everyone waiting for it (`WAIT_INTERRUPT`) – or, if
//!   nobody waits, the next one who does.
//! - everyone else: lines of output (the log's protocol, so `println!`
//!   works), `READ_LINE` to get the next finished line, `CLEAR`,
//!   `WAIT_INTERRUPT` (`userlib::terminal`). That is the shell and the
//!   programs it starts.
//!
//! A finished line waits until someone reads it (type-ahead); readers wait
//! in turn for lines – a reader that ended while it waited (it was
//! stopped) doesn't take a line with it: the next reader gets it. Every line shown also goes to the log – "typed:
//! <line>" or the output as it is – so the boot test can follow along.
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
use alloc::vec::Vec;
use editor::Editor;
use panel::Panel;
use userlib::keyboard::{CONTROL_C, Key};
use userlib::terminal::{CLEAR, READ_LINE, WAIT_INTERRUPT};
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
    /// Those waiting in `READ_LINE`, first come first served: the duty to
    /// answer, and their message, which holds the page lent for the line.
    readers: VecDeque<(Reply, Received)>,
    /// Those waiting for Ctrl+C.
    interrupt_waiters: Vec<Reply>,
    /// Ctrl+C was pressed while nobody waited for it.
    interrupted: bool,
    /// More than the line being typed changed: draw everything.
    lines_changed: bool,
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
        readers: VecDeque::new(),
        interrupt_waiters: Vec::new(),
        interrupted: false,
        lines_changed: true,
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
            terminal.serve(answer, message);
        }
        terminal.redraw();
    }
}

impl Terminal {
    /// Everyone but the keyboard. What they want is told by what they
    /// attach: a page lent `&` is text (the log's protocol: word 0 is only
    /// its length), a page lent `&mut` is room for a line, nothing is a
    /// command.
    fn serve(&mut self, answer: Reply, message: Received) {
        let words = match (&message.memory, message.words[0]) {
            (Some(ReceivedMemory::Exclusive(_)), READ_LINE) => {
                self.readers.push_back((answer, message));
                self.serve_readers();
                return;
            }
            (Some(ReceivedMemory::Exclusive(_)), _) => [abi::ERROR_BAD_ARGUMENT, 0, 0, 0],
            (None, CLEAR) => {
                if let Some(panel) = &mut self.panel {
                    panel.clear();
                }
                self.lines_changed = true;
                [0; 4]
            }
            (None, WAIT_INTERRUPT) if !core::mem::take(&mut self.interrupted) => {
                self.interrupt_waiters.push(answer);
                return;
            }
            (None, WAIT_INTERRUPT) => [0; 4],
            (None, _) => [abi::ERROR_BAD_ARGUMENT, 0, 0, 0],
            _ => {
                let status = self.write(&message).err().map_or(0, |error| error.0);
                [status, 0, 0, 0]
            }
        };
        let _ = reply(answer, Attach::Nothing, words);
    }

    fn press(&mut self, key: Key) {
        if key == Key::Character(CONTROL_C) {
            let dropped = self.editor.drop_line();
            println!("typed: {dropped}^C");
            if let Some(panel) = &mut self.panel {
                panel.finish_line(&alloc::format!("{dropped}^C"));
            }
            if self.interrupt_waiters.is_empty() {
                self.interrupted = true;
            }
            for waiter in self.interrupt_waiters.drain(..) {
                let _ = reply(waiter, Attach::Nothing, [0; 4]);
            }
            self.lines_changed = true;
            return;
        }
        let room = self
            .panel
            .as_ref()
            .map_or(LONGEST_UNSHOWN_LINE, Panel::characters_per_line);
        if let Some(line) = self.editor.apply(key, room) {
            println!("typed: {line}");
            if let Some(panel) = &mut self.panel {
                panel.finish_line(&line);
            }
            self.lines_changed = true;
            self.typed_ahead.push_back(line);
            self.serve_readers();
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
        self.lines_changed = true;
        Ok(())
    }

    /// Hands unread lines to waiting readers, oldest to oldest.
    fn serve_readers(&mut self) {
        while !self.readers.is_empty() && !self.typed_ahead.is_empty() {
            let (answer, mut message) = self.readers.pop_front().expect("checked");
            let line = self.typed_ahead.pop_front().expect("checked");
            let words = match &mut message.memory {
                Some(ReceivedMemory::Exclusive(page)) => match page.bytes_mut() {
                    Some(page) => {
                        let length = line.len().min(page.len());
                        page[..length].copy_from_slice(&line.as_bytes()[..length]);
                        [0, length as u64, 0, 0]
                    }
                    None => [abi::ERROR_BAD_MODE, 0, 0, 0],
                },
                _ => [abi::ERROR_BAD_MODE, 0, 0, 0],
            };
            if reply(answer, Attach::Nothing, words) == Err(Error(abi::ERROR_PEER_GONE)) {
                // That reader was stopped while it waited: the line is
                // still nobody's.
                self.typed_ahead.push_front(line);
            }
        }
    }

    /// Draws what changed: everything, or – after most keys – only the
    /// line being typed.
    fn redraw(&mut self) {
        let Some(panel) = &mut self.panel else {
            return;
        };
        let (line, cursor) = (self.editor.text(), self.editor.cursor());
        let drawn = if core::mem::take(&mut self.lines_changed) {
            panel.show(&line, cursor)
        } else {
            panel.show_input(&line, cursor)
        };
        if let Err(error) = drawn {
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
