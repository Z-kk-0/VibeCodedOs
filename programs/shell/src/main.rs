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
//! (`programs.rs`); arguments travel the same way, as a page of text.
//!
//! `name` runs a program in the foreground: the shell waits until it has
//! ended and says how – Ctrl+C stops it. `name &` runs it in the
//! background as a numbered job: `jobs` lists them, `stop <n>` stops one,
//! and before each command the shell reports the jobs that ended.
//! Whatever happens to a program, the shell goes on.
//!
//! Handles: 0 = terminal, 1 = directory, 2 = the names of the programs
//! (a page: one name per line), 3, 4, … = those programs, in that order.

#![no_std]
#![no_main]

extern crate alloc;

mod programs;

use alloc::string::String;
use alloc::vec::Vec;
use programs::{Program, describe};
use userlib::terminal::{clear, read_line, take_interrupt};
use userlib::{Error, Handle, Memory, abi, exit, poll, println, sleep, stop, wait, wait_timeout};

const TERMINAL: Handle = Handle(0);
const NAMES: Handle = Handle(2);
const FIRST_PROGRAM: u64 = 3;
/// How often a foreground program is looked after (Ctrl+C): every 50 ms.
const WATCH_TICKS: u64 = 5;

struct Job {
    number: u32,
    name: String,
    child: Handle,
}

struct Shell {
    programs: Vec<Program>,
    jobs: Vec<Job>,
    next_job: u32,
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Some(programs) = programs_given() else {
        exit(1)
    };
    let Ok(mut page) = Memory::new(1) else {
        exit(2)
    };
    let mut shell = Shell {
        programs,
        jobs: Vec::new(),
        next_job: 1,
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
        shell.report_ended_jobs();
        shell.run(&line);
    }
}

impl Shell {
    fn run(&mut self, line: &str) {
        let line = line.trim();
        let (line, background) = match line.strip_suffix('&') {
            Some(rest) => (rest.trim_end(), true),
            None => (line, false),
        };
        let (command, arguments) = line.split_once(' ').unwrap_or((line, ""));
        let arguments = arguments.trim();
        match command {
            "" => {}
            "help" => self.help(),
            "echo" => println!("{arguments}"),
            "clear" => {
                if let Err(error) = clear(TERMINAL) {
                    println!("cannot clear: {error:?}");
                }
            }
            "jobs" => self.list_jobs(),
            "stop" => self.stop_job(arguments),
            name => self.start(name, arguments, background),
        }
    }

    fn help(&self) {
        println!("Commands: help, clear, echo <text>, jobs, stop <job>.");
        let names: Vec<&str> = self
            .programs
            .iter()
            .map(|program| program.name.as_str())
            .collect();
        println!("Programs I may start: {}.", names.join(", "));
        println!("'name &' runs one in the background; Ctrl+C stops the one in front.");
        println!("Nothing else exists for me - init decided.");
    }

    fn start(&mut self, name: &str, arguments: &str, background: bool) {
        let Some(program) = self.programs.iter().find(|program| program.name == name) else {
            println!("{name}: no such command or program. 'help' lists them.");
            return;
        };
        if !arguments.is_empty() && !program.takes_arguments() {
            println!("{name} takes no arguments - ignored them.");
        }
        let child = match program.start(arguments) {
            Ok(child) => child,
            Err(error) => {
                println!("{name}: cannot start it: {error:?}");
                return;
            }
        };
        if background {
            let number = self.next_job;
            self.next_job += 1;
            println!("[{number}] {name} runs in the background.");
            self.jobs.push(Job {
                number,
                name: String::from(name),
                child,
            });
        } else {
            match watch(child) {
                Ok(ended) => println!("{name} {}", describe(ended)),
                Err(error) => println!("{name}: lost track of it: {error:?}"),
            }
        }
    }

    fn list_jobs(&mut self) {
        self.report_ended_jobs();
        if self.jobs.is_empty() {
            println!("No jobs in the background.");
        }
        for job in &self.jobs {
            println!("[{}] {} is running.", job.number, job.name);
        }
    }

    fn stop_job(&mut self, number: &str) {
        let number = number.trim_start_matches('%');
        let Some(job) = number
            .parse::<u32>()
            .ok()
            .and_then(|number| self.jobs.iter().find(|job| job.number == number))
        else {
            println!("stop: no job '{number}'. 'jobs' lists them.");
            return;
        };
        if let Err(error) = stop(job.child) {
            println!("stop: {error:?}");
            return;
        }
        // It ends at once, or – waiting for a reply – right after it.
        let ended = wait(job.child);
        let (number, name) = (job.number, job.name.clone());
        self.forget(number);
        match ended {
            Ok(ended) => println!("[{number}] {name} {}", describe(ended)),
            Err(error) => println!("[{number}] {name}: lost track of it: {error:?}"),
        }
    }

    /// Says which background jobs ended since the last command.
    fn report_ended_jobs(&mut self) {
        let mut ended = Vec::new();
        for job in &self.jobs {
            match poll(job.child) {
                Ok(Some(how)) => ended.push((job.number, job.name.clone(), describe(how))),
                Ok(None) => {}
                Err(error) => ended.push((
                    job.number,
                    job.name.clone(),
                    alloc::format!("lost: {error:?}"),
                )),
            }
        }
        for (number, name, how) in ended {
            println!("[{number}] {name} {how}");
            self.forget(number);
        }
    }

    /// Gives the job's child handle back to the kernel.
    fn forget(&mut self, number: u32) {
        if let Some(at) = self.jobs.iter().position(|job| job.number == number) {
            let job = self.jobs.remove(at);
            userlib::drop_handle(job.child);
        }
    }
}

/// Waits for a program in the foreground, stopping it on Ctrl+C.
fn watch(child: Handle) -> Result<userlib::Ended, Error> {
    // A Ctrl+C from before it started isn't meant for it.
    let _ = take_interrupt(TERMINAL);
    loop {
        match wait_timeout(child, WATCH_TICKS) {
            Err(Error(abi::ERROR_TIMED_OUT)) => {}
            result => {
                userlib::drop_handle(child);
                return result;
            }
        }
        if take_interrupt(TERMINAL)? {
            stop(child)?;
            let ended = wait(child);
            userlib::drop_handle(child);
            return ended;
        }
    }
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
