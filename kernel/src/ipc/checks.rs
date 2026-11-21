//! Boot-time proof that user space can run itself (DECISIONS.md, 020, 021).
//!
//! The kernel starts exactly one program, `init`, and gives it the console,
//! one `Program` handle per built-in program and the devices it found: the
//! screen as a `Device` handle (DECISIONS.md, 023), the keyboard as its
//! interrupt line and its two I/O ports (025). Everything
//! else – which services run, who may talk to whom, who drives the screen,
//! the checks that the ownership rules hold – is `init`'s business (see
//! `programs/init`). The kernel only looks at the result: init exits with 0
//! only if every check passed.

use super::object::{Capability, Device, Memory, Ports, ProgramCapability};
use crate::programs;
use crate::task::{Exit, core_count, sleep, spawn_program, task_count, wait};
use boot_info::{Framebuffer, PixelFormat};
use paging::Caching;

/// In the order `init` expects them, after the console (handle 0).
const PROGRAMS: [(&str, &[u8]); 14] = [
    ("log", programs::LOG),
    ("dir", programs::DIRECTORY),
    ("shout", programs::SHOUT),
    ("vandal", programs::VANDAL),
    ("hello", programs::HELLO),
    ("owner", programs::OWNER),
    ("giver", programs::GIVER),
    ("intruder", programs::INTRUDER),
    ("display", programs::DISPLAY),
    ("paint", programs::PAINT),
    ("keyboard", programs::KEYBOARD),
    ("terminal", programs::TERMINAL),
    ("shell", programs::SHELL),
    ("count", programs::COUNT),
];
/// Services that keep running once init is done.
const SERVICES: usize = 7; // log, dir, shout, display, keyboard, terminal, shell

/// The PC keyboard controller (i8042): its interrupt and its data (0x60)
/// and status/command (0x64) ports.
pub const KEYBOARD_IRQ: u8 = 1;
const KEYBOARD_PORTS: Ports = Ports {
    first: 0x60,
    mask: 1 << 0 | 1 << 4,
};

pub fn check(screen: Framebuffer) {
    let handles = core::iter::once(Capability::Console)
        .chain(
            PROGRAMS
                .iter()
                .map(|&(name, image)| Capability::Program(ProgramCapability { name, image })),
        )
        .chain([
            screen_device(screen),
            Capability::Ports(KEYBOARD_PORTS),
            Capability::Interrupt(KEYBOARD_IRQ),
        ]);
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

/// The screen as a device: its memory, write-combining (DECISIONS.md, 023),
/// and what a driver needs to know to draw on it.
fn screen_device(screen: Framebuffer) -> Capability {
    let memory = screen
        .exists()
        .then(|| Memory::device(screen.address, screen.pages(), Caching::WriteCombining));
    let format = match screen.format {
        PixelFormat::RedGreenBlue => abi::PIXEL_RED_GREEN_BLUE,
        PixelFormat::BlueGreenRed => abi::PIXEL_BLUE_GREEN_RED,
    };
    let info = [screen.width, screen.height, screen.stride].map(u64::from);
    Capability::Device(Device {
        memory,
        info: [info[0], info[1], info[2], format],
    })
}
