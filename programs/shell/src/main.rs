//! The shell (DECISIONS.md, 026, 027): reads lines from the terminal and
//! starts programs – but only the ones it holds a handle to.
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
//! `name` runs a program in the foreground: the shell says how it ended,
//! and Ctrl+C stops it. `name &` runs it in the background as a numbered
//! job: `jobs` lists them, `stop <n>` stops one, and the moment one ends
//! the shell says so. Whatever happens to a program, the shell goes on.
//!
//! The shell never waits for one thing: it waits in its mailbox for
//! everything at once (DECISIONS.md, 027). It has asked – with `send` –
//! for the next line, for the next Ctrl+C and for the end of every program
//! it started; whichever comes first, the answer's tag says what it is.
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
use userlib::terminal::{clear, line_in, request_line, watch_interrupt};
use userlib::{Answer, Ended, Handle, Incoming, Mailbox, Memory, Pending, exit, println};
use userlib::{sleep, stop, watch};

const TERMINAL: Handle = Handle(0);
const NAMES: Handle = Handle(2);
const FIRST_PROGRAM: u64 = 3;

/// What an answer in our mailbox is about.
const LINE: u64 = 1;
const INTERRUPT: u64 = 2;
/// The program in the foreground has ended.
const FOREGROUND: u64 = 3;
/// Background job `n` has ended: `JOB + n`.
const JOB: u64 = 1 << 32;

struct Job {
    number: u64,
    name: String,
    child: Handle,
}

struct Shell {
    programs: Vec<Program>,
    mailbox: Mailbox,
    /// The line we asked the terminal for; it holds the page lent for it.
    line: Option<Pending>,
    /// A page for the next line, while no request is out.
    page: Option<Memory>,
    foreground: Option<Job>,
    jobs: Vec<Job>,
    next_job: u64,
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    let Some(programs) = programs_given() else {
        exit(1)
    };
    let (Ok(page), Ok(mailbox)) = (Memory::new(1), Mailbox::new()) else {
        exit(2)
    };
    let mut shell = Shell {
        programs,
        mailbox,
        line: None,
        page: Some(page),
        foreground: None,
        jobs: Vec::new(),
        next_job: 1,
    };
    println!("Hematite shell. Type 'help'.");
    if let Err(error) = watch_interrupt(TERMINAL, &shell.mailbox, INTERRUPT) {
        println!("Ctrl+C won't work: {error:?}");
    }
    loop {
        shell.ask_for_line();
        match shell.mailbox.receive() {
            Ok(Incoming::Answer(answer)) => shell.answered(answer),
            // Nobody else holds a sending end of our mailbox.
            Ok(Incoming::Call(_)) => {}
            Err(error) => {
                println!("cannot receive: {error:?}");
                sleep(100);
            }
        }
    }
}

impl Shell {
    /// Asks the terminal for the next line – unless a program is in the
    /// foreground (lines typed meanwhile wait at the terminal for it or us)
    /// or we asked already.
    fn ask_for_line(&mut self) {
        if self.foreground.is_some() || self.line.is_some() {
            return;
        }
        let Some(page) = self.page.take() else {
            return;
        };
        match request_line(TERMINAL, page, &self.mailbox, LINE) {
            Ok(pending) => self.line = Some(pending),
            Err(refused) => {
                println!("cannot read a line: {:?}", refused.error);
                self.page = refused.memory;
                sleep(100);
            }
        }
    }

    fn answered(&mut self, answer: Answer) {
        match answer.tag {
            LINE => {
                let page = self.line.take().and_then(Pending::finish);
                let line = match (&answer.result, &page) {
                    (Ok(reply), Some(page)) => line_in(reply, page),
                    (Err(error), _) => Err(*error),
                    (Ok(_), None) => unreachable!("the page went with the request"),
                };
                self.page = page;
                match line {
                    Ok(line) => self.run(&line),
                    Err(error) => {
                        println!("cannot read a line: {error:?}");
                        sleep(100);
                    }
                }
            }
            INTERRUPT => {
                // A Ctrl+C at the prompt is for nobody.
                if let Some(job) = &self.foreground
                    && let Err(error) = stop(job.child)
                {
                    println!("cannot stop {}: {error:?}", job.name);
                }
                if let Err(error) = watch_interrupt(TERMINAL, &self.mailbox, INTERRUPT) {
                    println!("Ctrl+C won't work anymore: {error:?}");
                }
            }
            FOREGROUND => {
                if let Some(job) = self.foreground.take() {
                    println!("{} {}", job.name, how(&answer));
                    userlib::drop_handle(job.child);
                }
            }
            tag if tag > JOB => {
                if let Some(at) = self.jobs.iter().position(|job| JOB + job.number == tag) {
                    let job = self.jobs.remove(at);
                    println!("[{}] {} {}", job.number, job.name, how(&answer));
                    userlib::drop_handle(job.child);
                }
            }
            _ => {}
        }
    }

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
        let (number, tag) = if background {
            let number = self.next_job;
            self.next_job += 1;
            (number, JOB + number)
        } else {
            (0, FOREGROUND)
        };
        if let Err(error) = watch(child, &self.mailbox, tag) {
            println!("{name}: cannot watch it ({error:?}) - stopping it.");
            let _ = stop(child);
            userlib::drop_handle(child);
            return;
        }
        let job = Job {
            number,
            name: String::from(name),
            child,
        };
        if background {
            println!("[{number}] {name} runs in the background.");
            self.jobs.push(job);
        } else {
            self.foreground = Some(job);
        }
    }

    fn list_jobs(&self) {
        if self.jobs.is_empty() {
            println!("No jobs in the background.");
        }
        for job in &self.jobs {
            println!("[{}] {} is running.", job.number, job.name);
        }
    }

    /// Stops a job; its end is reported when it comes – which is at once.
    fn stop_job(&mut self, number: &str) {
        let number = number.trim_start_matches('%');
        let Some(job) = number
            .parse::<u64>()
            .ok()
            .and_then(|number| self.jobs.iter().find(|job| job.number == number))
        else {
            println!("stop: no job '{number}'. 'jobs' lists them.");
            return;
        };
        if let Err(error) = stop(job.child) {
            println!("stop: {error:?}");
        }
    }
}

/// How a program ended, from the answer to watching it.
fn how(answer: &Answer) -> String {
    match &answer.result {
        Ok(reply) => describe(Ended::from_words(reply.words)),
        Err(error) => alloc::format!("- lost track of it: {error:?}"),
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
