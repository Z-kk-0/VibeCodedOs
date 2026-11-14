//! Boot-time proof that user mode, capabilities and ownership IPC work.
//!
//! Starts the two services (`log`, `shout`) and five programs, each with
//! exactly the handles it needs (handle numbers in each program's docs):
//!
//! - `hello` prints through `log` – the only way to print.
//! - `owner` lends `shout` a page `&mut` (comes back upper-cased), then
//!   lends `vandal` the page `&` – vandal tries to write, is stopped, and
//!   owner gets its page back unchanged.
//! - `giver` moves a page to `shout`, then reaches for it: must be stopped.
//! - `intruder` forges a handle, then reads kernel memory: must be stopped.

use super::object::{Capability, Endpoint};
use crate::programs;
use crate::task::{Exit, core_count, sleep, spawn_program, task_count, wait};

pub fn check() {
    let log = Endpoint::new();
    let shout = Endpoint::new();
    let vandal = Endpoint::new();
    let start = |name, file, handles: alloc::vec::Vec<Capability>| {
        spawn_program(name, file, handles).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    // Services: they run forever, waiting in `receive`.
    start(
        "log",
        programs::LOG,
        alloc::vec![Capability::receiver(&log), Capability::Console],
    );
    start(
        "shout",
        programs::SHOUT,
        alloc::vec![
            Capability::sender(&log, "shout"),
            Capability::receiver(&shout)
        ],
    );
    let vandal_task = start(
        "vandal",
        programs::VANDAL,
        alloc::vec![
            Capability::sender(&log, "vandal"),
            Capability::receiver(&vandal)
        ],
    );
    let hello = start(
        "hello",
        programs::HELLO,
        alloc::vec![Capability::sender(&log, "hello")],
    );
    let owner = start(
        "owner",
        programs::OWNER,
        alloc::vec![
            Capability::sender(&log, "owner"),
            Capability::sender(&shout, "owner"),
            Capability::sender(&vandal, "owner"),
        ],
    );
    let giver = start(
        "giver",
        programs::GIVER,
        alloc::vec![
            Capability::sender(&log, "giver"),
            Capability::sender(&shout, "giver"),
        ],
    );
    let intruder = start(
        "intruder",
        programs::INTRUDER,
        alloc::vec![Capability::sender(&log, "intruder")],
    );

    assert_eq!(wait(hello), Exit::Code(0), "hello did not exit cleanly");
    let owner_exit = wait(owner);
    assert_eq!(owner_exit, Exit::Code(0), "an ownership check failed");
    for (name, task) in [
        ("vandal", vandal_task),
        ("giver", giver),
        ("intruder", intruder),
    ] {
        let exit = wait(task);
        assert_eq!(
            exit,
            Exit::Killed("Page Fault"),
            "{name} was not stopped by the page tables: {exit:?}"
        );
    }
    sleep(2); // let the scheduler free the finished programs

    // Left: boot, an idle task per core, and the two services.
    assert_eq!(
        task_count(),
        1 + core_count() + 2,
        "finished programs were not freed"
    );
    println!("User mode works: hello printed through the log service and exited with 0.");
    println!(
        "Ownership IPC works: &mut lent and returned, & lent and kept safe from a vandal, \
         a moved page gone from its giver, a forged handle refused."
    );
}
