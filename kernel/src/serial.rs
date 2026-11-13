//! COM1 serial port (16550 UART), polled.
//!
//! Our first output once the firmware is gone: trivial to drive, and QEMU
//! forwards it to the host, so boots can be checked headless.
//! Register layout: OSDev wiki "Serial Ports", PC16550D datasheet.

use crate::port;
use core::fmt;

/// COM1's base port is fixed by the PC platform.
const COM1: u16 = 0x3F8;

// Register offsets from the base port.
/// Send/receive byte; divisor low byte while DLAB is set.
const DATA: u16 = 0;
/// Interrupt enable; divisor high byte while DLAB is set.
const INTERRUPT_ENABLE: u16 = 1;
const FIFO_CONTROL: u16 = 2;
const LINE_CONTROL: u16 = 3;
const MODEM_CONTROL: u16 = 4;
const LINE_STATUS: u16 = 5;

/// "Divisor latch access bit": remaps DATA/INTERRUPT_ENABLE to the baud divisor.
const DLAB: u8 = 0x80;
const EIGHT_BITS_NO_PARITY_ONE_STOP: u8 = 0x03;
const ENABLE_AND_CLEAR_FIFOS: u8 = 0xC7;
const DATA_TERMINAL_READY_AND_REQUEST_TO_SEND: u8 = 0x03;
const TRANSMITTER_EMPTY: u8 = 0x20;
/// Baud rate = 115200 / divisor.
const BAUD_DIVISOR: u16 = 1;

pub struct Serial {
    base: u16,
}

impl Serial {
    /// Sets up COM1. Safe to call repeatedly (e.g. again from the panic handler).
    pub fn com1() -> Self {
        let serial = Self { base: COM1 };
        serial.init();
        serial
    }

    fn init(&self) {
        self.write_register(INTERRUPT_ENABLE, 0); // we poll instead
        self.write_register(LINE_CONTROL, DLAB);
        let [low, high] = BAUD_DIVISOR.to_le_bytes();
        self.write_register(DATA, low);
        self.write_register(INTERRUPT_ENABLE, high);
        self.write_register(LINE_CONTROL, EIGHT_BITS_NO_PARITY_ONE_STOP); // also clears DLAB
        self.write_register(FIFO_CONTROL, ENABLE_AND_CLEAR_FIFOS);
        self.write_register(MODEM_CONTROL, DATA_TERMINAL_READY_AND_REQUEST_TO_SEND);
    }

    pub fn write_byte(&self, byte: u8) {
        while self.read_register(LINE_STATUS) & TRANSMITTER_EMPTY == 0 {
            core::hint::spin_loop();
        }
        self.write_register(DATA, byte);
    }

    fn write_register(&self, offset: u16, value: u8) {
        // SAFETY: `base` is a 16550 UART; its registers only affect the UART.
        unsafe { port::write_u8(self.base + offset, value) }
    }

    fn read_register(&self, offset: u16) -> u8 {
        // SAFETY: as above; we only read status, never the receive FIFO.
        unsafe { port::read_u8(self.base + offset) }
    }
}

impl fmt::Write for Serial {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}
