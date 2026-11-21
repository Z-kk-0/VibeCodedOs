//! Starting a program, and saying how it ended.

use alloc::string::String;
use userlib::{Ended, Error, Handle, Memory, Pass, abi, spawn};

const TERMINAL: Handle = Handle(0);
const DIRECTORY: Handle = Handle(1);
/// What `hello` finds in the page it gets at its start.
const GREETING: &str = "a page from the shell";

pub struct Program {
    pub name: String,
    pub handle: Handle,
}

impl Program {
    /// Whether it takes arguments (a page with the rest of the line).
    pub fn takes_arguments(&self) -> bool {
        self.name == "count"
    }

    /// Starts it with the handles this program needs: always our terminal
    /// as its handle 0, then what it works with.
    pub fn start(&self, arguments: &str) -> Result<Handle, Error> {
        let output = Pass::Copy(TERMINAL);
        match self.name.as_str() {
            "hello" => spawn(self.handle, [output, Pass::Memory(page(GREETING)?)]),
            "count" => spawn(self.handle, [output, Pass::Memory(page(arguments)?)]),
            "paint" | "giver" => spawn(self.handle, [output, Pass::Copy(DIRECTORY)]),
            _ => spawn(self.handle, [output]),
        }
    }
}

/// A page with `text` in it (zeros after it).
fn page(text: &str) -> Result<Memory, Error> {
    let mut page = Memory::new(1)?;
    let length = text.len().min(page.len() - 1);
    page.bytes_mut()[..length].copy_from_slice(&text.as_bytes()[..length]);
    Ok(page)
}

/// How a program ended, as a sentence after its name.
pub fn describe(ended: Ended) -> String {
    match ended {
        Ended::Exited(code) => alloc::format!("ended: exit code {code}."),
        Ended::Killed(exception) => alloc::format!(
            "was stopped by the kernel: {} ({exception}).",
            exception_name(exception)
        ),
        Ended::KernelFailed(cause) => alloc::format!(
            "- the kernel failed while working for it ({}); only it ended.",
            if cause == abi::KERNEL_PANIC {
                "a panic"
            } else {
                exception_name(cause)
            }
        ),
        Ended::Stopped => String::from("was stopped."),
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
