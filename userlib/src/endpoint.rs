//! Making endpoints and sending ends, and revoking them (DECISIONS.md, 021).

use crate::ipc::{Error, Handle, check};
use crate::system_call::{Registers, system_call};

/// A new endpoint; returns its receiving end.
pub fn endpoint_new() -> Result<Handle, Error> {
    handle_result(Registers {
        rax: abi::ENDPOINT_NEW,
        ..Registers::default()
    })
}

/// A new sending end of the endpoint whose receiving end we hold, badged so
/// that we know who it was given to.
pub fn mint(receiving_end: Handle, badge: u64) -> Result<Handle, Error> {
    handle_result(Registers {
        rax: abi::MINT,
        rdi: receiving_end.0,
        rsi: badge,
        ..Registers::default()
    })
}

/// Every sending end with `badge` – and every copy of one – stops working.
pub fn revoke(receiving_end: Handle, badge: u64) -> Result<(), Error> {
    let result = system_call(Registers {
        rax: abi::REVOKE,
        rdi: receiving_end.0,
        rsi: badge,
        ..Registers::default()
    });
    check(result.rax)
}

fn handle_result(registers: Registers) -> Result<Handle, Error> {
    let result = system_call(registers);
    check(result.rax)?;
    Ok(Handle(result.rsi))
}
