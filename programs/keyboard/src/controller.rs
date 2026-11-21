//! The PC keyboard controller, an Intel 8042 or what imitates one (OSDev
//! wiki "8042 PS/2 Controller"). Port 0x60 is data, 0x64 status (read) and
//! command (write). Firmware leaves it usable, with translation to scancode
//! set 1 on; we only make sure the keyboard port is on and interrupts us.

use userlib::Handle;
use userlib::ports::{self, PortOp};

/// Offsets from the handle's first port, 0x60.
const DATA: u8 = 0;
const STATUS: u8 = 4;
const COMMAND: u8 = 4;

/// Status bits.
const OUTPUT_FULL: u8 = 1 << 0; // a byte waits at DATA for us
const INPUT_FULL: u8 = 1 << 1; // our last write isn't taken yet
const FROM_MOUSE: u8 = 1 << 5; // the waiting byte is the second port's

const READ_CONFIGURATION: u8 = 0x20;
const WRITE_CONFIGURATION: u8 = 0x60;
const ENABLE_FIRST_PORT: u8 = 0xAE;
/// Configuration bits.
const FIRST_PORT_INTERRUPT: u8 = 1 << 0;
const FIRST_PORT_CLOCK_OFF: u8 = 1 << 4;

/// How often to look at the status before giving up on the controller.
const PATIENCE: u32 = 100_000;

pub struct Controller {
    ports: Handle,
}

impl Controller {
    /// Turns on the keyboard port and its interrupt, and drops whatever
    /// the firmware left unread.
    pub fn start(ports: Handle) -> Result<Self, &'static str> {
        let controller = Self { ports };
        // Where no device answers, the bus floats to all ones.
        if controller.status()? == 0xFF {
            return Err("no controller at port 0x60");
        }
        controller.command(READ_CONFIGURATION)?;
        let configuration = controller.read_data()?;
        let configuration = (configuration | FIRST_PORT_INTERRUPT) & !FIRST_PORT_CLOCK_OFF;
        controller.command(WRITE_CONFIGURATION)?;
        controller.write_data(configuration)?;
        controller.command(ENABLE_FIRST_PORT)?;
        while controller.next_byte().is_some() {}
        Ok(controller)
    }

    /// The next byte from the keyboard, if one is waiting. One call per
    /// byte: the kernel reads the status, and the data only if there is any.
    pub fn next_byte(&self) -> Option<u8> {
        loop {
            let [status, byte] = ports::run(
                self.ports,
                [PortOp::read(STATUS), PortOp::read_if(DATA, OUTPUT_FULL)],
            )
            .ok()?;
            let (status, byte) = (status?, byte?);
            if status & FROM_MOUSE == 0 {
                return Some(byte);
            }
            // A byte from the mouse port: no mouse driver yet, dropped.
        }
    }

    fn status(&self) -> Result<u8, &'static str> {
        ports::read(self.ports, STATUS).map_err(|_| "cannot read the status port")
    }

    fn command(&self, command: u8) -> Result<(), &'static str> {
        self.wait_until(|status| status & INPUT_FULL == 0)?;
        ports::write(self.ports, COMMAND, command).map_err(|_| "cannot write a command")
    }

    fn write_data(&self, byte: u8) -> Result<(), &'static str> {
        self.wait_until(|status| status & INPUT_FULL == 0)?;
        ports::write(self.ports, DATA, byte).map_err(|_| "cannot write data")
    }

    fn read_data(&self) -> Result<u8, &'static str> {
        self.wait_until(|status| status & OUTPUT_FULL != 0)?;
        ports::read(self.ports, DATA).map_err(|_| "cannot read data")
    }

    fn wait_until(&self, ready: impl Fn(u8) -> bool) -> Result<(), &'static str> {
        for _ in 0..PATIENCE {
            if ready(self.status()?) {
                return Ok(());
            }
        }
        Err("the controller does not answer")
    }
}
