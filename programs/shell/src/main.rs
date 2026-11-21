//! The shell (DECISIONS.md, 026): reads lines from the terminal and starts
//! programs – but only the ones it holds a handle to.
//!
//! A shell elsewhere searches a `PATH` and may run any file it finds. This
//! one has nothing to search: `init` gave it a `Program` handle for each
//! program it may start, plus a page with their names, and that is all
//! that exists for it. `help` lists exactly that.
//!
//! A started program gets a copy of our terminal handle as its handle 0 –
//! where `println!` goes – so its output appears on the screen, not in the
//! log. Which handles else it gets is the shell's choice per program
//! (`programs.rs`). The shell waits until it has ended and says how: an
//! exit code, the exception the kernel stopped it for, or a contained
//! kernel failure. Whatever happens to it, the shell goes on.
//!
//! Handles: 0 = terminal, 1 = directory, 2 = the names of the programs
//! (a page: one name per line), 3, 4, … = those programs, in that order.

#![no_std]
#![no_main]

extern crate alloc;

mod programs;

use alloc::string::String;
use alloc::vec::Vec;
use programs::Program;
use userlib::terminal::{clear, read_line};
use userlib::{Handle, Memory, exit, println, sleep};

const TERMINAL: Handle = Handle(0);
const NAMES: Handle = Handle(2);
const FIRST_PROGRAM: u64 = 3;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let programs = match programs_given() {
        Some(programs) => programs,
        None => exit(1),
    };
    let Ok(mut page) = Memory::new(1) else {
        exit(2)
    };
    println!("Hematite shell. Type 'help'.");
    loop {
        let line = match read_line(TERMINAL, &mut page) {
            Ok(line) => line,
            Err(error) => {
                println!("cannot read a line: {error:?}");
                sleep(100);
                continue;
            }
        };
        run(&line, &programs);
    }
}

fn run(line: &str, programs: &[Program]) {
    let line = line.trim();
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    match command {
        "" => {}
        "help" => help(programs),
        "echo" => println!("{}", rest.trim()),
        "clear" => {
            if let Err(error) = clear(TERMINAL) {
                println!("cannot clear: {error:?}");
            }
        }
        name => match programs.iter().find(|program| program.name == name) {
            Some(program) => program.start_and_wait(),
            None => println!("{name}: no such command or program. 'help' lists them."),
        },
    }
}

fn help(programs: &[Program]) {
    println!("Commands: help, clear, echo <text>.");
    let names: Vec<&str> = programs
        .iter()
        .map(|program| program.name.as_str())
        .collect();
    println!("Programs I may start: {}.", names.join(", "));
    println!("Nothing else exists for me - init decided.");
}

/// The programs init gave us: names from the page, handles from 3 on.
fn programs_given() -> Option<Vec<Program>> {
    let page = Memory::from_start_handle(NAMES).ok()?;
    let bytes = page.bytes();
    let length = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let names = core::str::from_utf8(&bytes[..length]).ok()?;
    Some(
        (FIRST_PROGRAM..)
            .zip(names.lines().filter(|name| !name.is_empty()))
            .map(|(handle, name)| Program {
                name: String::from(name),
                handle: Handle(handle),
            })
            .collect(),
    )
}
