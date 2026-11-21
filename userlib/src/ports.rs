//! I/O ports (DECISIONS.md, 025): a `Ports` handle covers a few of them;
//! the kernel does each `in`/`out` for us, up to four per call. `offset`
//! counts from the handle's first port.

use crate::ipc::{Attach, Error, Handle, call};
pub use abi::PortOp;

/// Runs up to four operations in one call (see `abi::PORT_READ`). For
/// each: `Some(byte)` for a read that ran, `Some(0)` for a write, `None`
/// for a read skipped by its condition.
pub fn run<const N: usize>(ports: Handle, ops: [PortOp; N]) -> Result<[Option<u8>; N], Error> {
    const { assert!(N >= 1 && N <= 4, "one call runs 1 to 4 port operations") };
    let mut words = [0; 4];
    for (word, op) in words.iter_mut().zip(ops) {
        *word = op.to_word();
    }
    let reply = call(ports, Attach::Nothing, words)?;
    Ok(core::array::from_fn(|index| {
        let word = reply.words[index];
        (word & abi::PORT_DONE != 0).then_some(word as u8)
    }))
}

pub fn read(ports: Handle, offset: u8) -> Result<u8, Error> {
    let [byte] = run(ports, [PortOp::read(offset)])?;
    Ok(byte.unwrap_or(0))
}

pub fn write(ports: Handle, offset: u8, value: u8) -> Result<(), Error> {
    run(ports, [PortOp::write(offset, value)])?;
    Ok(())
}
