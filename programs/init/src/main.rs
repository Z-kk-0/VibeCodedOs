//! The first program – the only one the kernel starts (DECISIONS.md, 021).
//!
//! It decides what runs and who may talk to whom: it makes the endpoints,
//! mints a badged sending end per program (so the log knows who's
//! talking), starts every program with exactly the handles it needs, and
//! finally checks how the test programs ended. It exits 0 only if all of
//! them ended as they must – the kernel's boot check looks at nothing else.
//!
//! `shout` starts last on purpose: `owner` and `giver` look it up in the
//! directory before it exists, and the directory makes them wait.
//!
//! The screen goes to `display` and nobody else (DECISIONS.md, 023): init
//! moves the device handle away without ever opening it. `paint` draws
//! through `display`.
//!
//! The keyboard (DECISIONS.md, 025) works the same way: its interrupt and
//! ports go to `keyboard` alone, which calls `terminal` with every key –
//! through the terminal's endpoint. The terminal starts only after paint is
//! done, so its strip lands on top of the picture.
//!
//! The shell (DECISIONS.md, 026) reads its commands from the terminal, and
//! may start exactly the programs init gives it a copy of: `SHELL_PROGRAMS`.
//!
//! Handles: 0 = console, 1–14 = the programs log, dir, shout, vandal,
//! hello, owner, giver, intruder, display, paint, keyboard, terminal,
//! shell, count; 15 = the screen (device); 16, 17 = the keyboard's ports
//! and interrupt.

#![no_std]
#![no_main]

use userlib::set_log;
use userlib::{Ended, Error, Handle, Memory, Pass, abi, endpoint_new, exit, mint, println};
use userlib::{spawn, wait};

const CONSOLE: Handle = Handle(0);
const LOG: Handle = Handle(1);
const DIRECTORY: Handle = Handle(2);
const SHOUT: Handle = Handle(3);
const VANDAL: Handle = Handle(4);
const HELLO: Handle = Handle(5);
const OWNER: Handle = Handle(6);
const GIVER: Handle = Handle(7);
const INTRUDER: Handle = Handle(8);
const DISPLAY: Handle = Handle(9);
const PAINT: Handle = Handle(10);
const KEYBOARD: Handle = Handle(11);
const TERMINAL: Handle = Handle(12);
const SHELL: Handle = Handle(13);
const COUNT: Handle = Handle(14);
const SCREEN: Handle = Handle(15);
const KEYBOARD_PORTS: Handle = Handle(16);
const KEYBOARD_INTERRUPT: Handle = Handle(17);
/// What the shell may start – nothing else exists for it.
const SHELL_PROGRAMS: [(&str, Handle); 5] = [
    ("hello", HELLO),
    ("count", COUNT),
    ("intruder", INTRUDER),
    ("giver", GIVER),
    ("paint", PAINT),
];
/// The CPU exception the attackers must be stopped by.
const PAGE_FAULT: u64 = 14;
/// Handed to hello at its start, in a page of its own.
const GREETING: &str = "a page from init, handed over at your start";

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    // Endpoints first; every sending end is minted before its receiving
    // end moves away to its service.
    let log = must(endpoint_new(), 1);
    let log_for = |name| must(mint(log, abi::badge_from_name(name)), 2);
    let directory = must(endpoint_new(), 3);
    let directory_for = |name| must(mint(directory, abi::badge_from_name(name)), 4);
    let vandal = must(endpoint_new(), 5);
    let vandal_for_owner = must(mint(vandal, abi::badge_from_name("owner")), 6);
    let terminal = must(endpoint_new(), 18);
    let terminal_for = |name| must(mint(terminal, abi::badge_from_name(name)), 19);
    let terminal_for_keyboard = terminal_for("keyboard");
    let terminal_for_shell = terminal_for("shell");

    let my_log = log_for("init");
    let [
        log_dir,
        log_shout,
        log_vandal,
        log_hello,
        log_owner,
        log_giver,
        log_intruder,
        log_display,
        log_paint,
        log_keyboard,
        log_terminal,
    ] = [
        "dir", "shout", "vandal", "hello", "owner", "giver", "intruder", "display", "paint",
        "keyboard", "terminal",
    ]
    .map(log_for);
    let [
        directory_shout,
        directory_owner,
        directory_giver,
        directory_display,
        directory_paint,
        directory_terminal,
        directory_shell,
    ] = [
        "shout", "owner", "giver", "display", "paint", "terminal", "shell",
    ]
    .map(directory_for);

    must(spawn(LOG, [Pass::Move(log), Pass::Move(CONSOLE)]), 7);
    set_log(my_log);
    println!("Started the log; now the others, each with only the handles it needs.");
    must(
        spawn(DIRECTORY, [Pass::Move(log_dir), Pass::Move(directory)]),
        8,
    );
    let vandal = must(
        spawn(VANDAL, [Pass::Move(log_vandal), Pass::Move(vandal)]),
        9,
    );
    let mut greeting = must(Memory::new(1), 15);
    greeting.bytes_mut()[..GREETING.len()].copy_from_slice(GREETING.as_bytes());
    let hello = must(
        spawn(HELLO, [Pass::Move(log_hello), Pass::Memory(greeting)]),
        10,
    );
    let owner = must(
        spawn(
            OWNER,
            [
                Pass::Move(log_owner),
                Pass::Move(directory_owner),
                Pass::Move(vandal_for_owner),
            ],
        ),
        11,
    );
    let giver = must(
        spawn(GIVER, [Pass::Move(log_giver), Pass::Move(directory_giver)]),
        12,
    );
    must(
        spawn(SHOUT, [Pass::Move(log_shout), Pass::Move(directory_shout)]),
        13,
    );
    let intruder = must(spawn(INTRUDER, [Pass::Move(log_intruder)]), 14);
    must(
        spawn(
            DISPLAY,
            [
                Pass::Move(log_display),
                Pass::Move(directory_display),
                Pass::Move(SCREEN),
            ],
        ),
        16,
    );
    let paint = must(
        spawn(PAINT, [Pass::Move(log_paint), Pass::Move(directory_paint)]),
        17,
    );

    let expected = [
        ("hello", hello, Ended::Exited(0)),
        ("owner", owner, Ended::Exited(0)),
        ("vandal", vandal, Ended::Killed(PAGE_FAULT)),
        ("giver", giver, Ended::Killed(PAGE_FAULT)),
        ("intruder", intruder, Ended::Killed(PAGE_FAULT)),
        ("paint", paint, Ended::Exited(0)),
    ];
    let programs = expected.len();
    let mut failures = 0;
    for (name, child, must_end) in expected {
        let ended = wait(child);
        if ended != Ok(must_end) {
            println!("{name} ended as {ended:?}, but should have: {must_end:?}");
            failures += 1;
        }
    }
    // The picture is on the screen: now the terminal, keyboard and shell.
    must(
        spawn(
            TERMINAL,
            [
                Pass::Move(log_terminal),
                Pass::Move(directory_terminal),
                Pass::Move(terminal),
            ],
        ),
        20,
    );
    must(
        spawn(
            KEYBOARD,
            [
                Pass::Move(log_keyboard),
                Pass::Move(KEYBOARD_PORTS),
                Pass::Move(KEYBOARD_INTERRUPT),
                Pass::Move(terminal_for_keyboard),
            ],
        ),
        21,
    );
    let mut names = must(Memory::new(1), 22);
    let mut at = 0;
    for (name, _) in SHELL_PROGRAMS {
        names.bytes_mut()[at..at + name.len()].copy_from_slice(name.as_bytes());
        names.bytes_mut()[at + name.len()] = b'\n';
        at += name.len() + 1;
    }
    let [hello, count, intruder, giver, paint] =
        SHELL_PROGRAMS.map(|(_, program)| Pass::Copy(program));
    must(
        spawn(
            SHELL,
            [
                Pass::Move(terminal_for_shell),
                Pass::Move(directory_shell),
                Pass::Memory(names),
                hello,
                count,
                intruder,
                giver,
                paint,
            ],
        ),
        23,
    );
    if failures == 0 {
        println!("All {programs} programs ended as they should. The services keep running.");
    }
    exit(failures)
}

/// The value, or exit with `code`: init can't go on without it.
fn must<T>(result: Result<T, Error>, code: u64) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            println!("setup step {code} failed: {error:?}");
            exit(100 + code)
        }
    }
}
