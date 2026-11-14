//! Boot-time proof that user space can run itself (DECISIONS.md, 020, 021).
//!
//! The kernel starts exactly one program, `init`, and gives it the console
//! and one `Program` handle per built-in program. Everything else – which
//! services run, who may talk to whom, the checks that the ownership rules
//! hold – is `init`'s business (see `programs/init`). The kernel only looks
//! at the result: init exits with 0 only if every check passed.

use super::object::{Capability, ProgramCapability};
use crate::programs;
use crate::task::{Exit, core_count, sleep, spawn_program, task_count, wait};

/// In the order `init` expects them, after the console (handle 0).
const PROGRAMS: [(&str, &[u8]); 8] = [
    ("log", programs::LOG),
    ("dir", programs::DIRECTORY),
    ("shout", programs::SHOUT),
    ("vandal", programs::VANDAL),
    ("hello", programs::HELLO),
    ("owner", programs::OWNER),
    ("giver", programs::GIVER),
    ("intruder", programs::INTRUDER),
];
/// Services that keep running once init is done.
const SERVICES: usize = 3; // log, dir, shout

pub fn check() {
    let handles = core::iter::once(Capability::Console).chain(
        PROGRAMS
            .iter()
            .map(|&(name, image)| Capability::Program(ProgramCapability { name, image })),
    );
    let init = spawn_program("init", programs::INIT, handles)
        .unwrap_or_else(|error| panic!("init: {error}"));
    let exit = wait(init);
    assert_eq!(exit, Exit::Code(0), "init reports a failed check");
    sleep(2); // let the scheduler free the finished programs

    // Left: boot, an idle task per core, and the services.
    assert_eq!(
        task_count(),
        1 + core_count() + SERVICES,
        "finished programs were not freed"
    );
    println!("User space runs itself: init started every program and all its checks passed.");
}
