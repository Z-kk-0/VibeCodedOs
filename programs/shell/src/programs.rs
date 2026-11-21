//! Starting a program and saying how it ended.

use alloc::string::String;
use userlib::{Ended, Error, Handle, Memory, Pass, abi, println, spawn, wait};

const TERMINAL: Handle = Handle(0);
const DIRECTORY: Handle = Handle(1);
/// What `hello` finds in the page it gets at its start.
const GREETING: &str = "a page from the shell";

pub struct Program {
    pub name: String,
    pub handle: Handle,
}

impl Program {
    pub fn start_and_wait(&self) {
        let ended = self.start().and_then(wait);
        match ended {
            Ok(Ended::Exited(0)) => println!("{} ended: exit code 0.", self.name),
            Ok(Ended::Exited(code)) => println!("{} ended: exit code {code}.", self.name),
            Ok(Ended::Killed(exception)) => println!(
                "{} was stopped by the kernel: {} ({exception}).",
                self.name,
                exception_name(exception)
            ),
            Ok(Ended::KernelFailed(cause)) => println!(
                "{}: the kernel failed while working for it ({}); only it ended.",
                self.name,
                if cause == abi::KERNEL_PANIC {
                    "a panic"
                } else {
                    exception_name(cause)
                }
            ),
            Err(error) => println!("{}: cannot start it: {error:?}", self.name),
        }
    }

    /// Starts it with the handles this program needs: always our terminal
    /// as its handle 0, then what it works with.
    fn start(&self) -> Result<Handle, Error> {
        let output = Pass::Copy(TERMINAL);
        match self.name.as_str() {
            "hello" => {
                let mut greeting = Memory::new(1)?;
                greeting.bytes_mut()[..GREETING.len()].copy_from_slice(GREETING.as_bytes());
                spawn(self.handle, [output, Pass::Memory(greeting)])
            }
            "paint" | "giver" => spawn(self.handle, [output, Pass::Copy(DIRECTORY)]),
            _ => spawn(self.handle, [output]),
        }
    }
}

/// The CPU exceptions a program is usually stopped for (Intel SDM Vol. 3A,
/// Table 6-1).
fn exception_name(vector: u64) -> &'static str {
    match vector {
        0 => "divide error",
        3 => "breakpoint",
        6 => "invalid opcode",
        13 => "general protection fault",
        14 => "page fault",
        _ => "CPU exception",
    }
}
