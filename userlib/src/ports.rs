//! I/O ports (DECISIONS.md, 025): a `Ports` handle covers a few of them;
//! the kernel does each `in`/`out` for us. `offset` counts from the
//! handle's first port.

use crate::ipc::{Attach, Error, Handle, call};

pub fn read(ports: Handle, offset: u16) -> Result<u8, Error> {
    let reply = call(
        ports,
        Attach::Nothing,
        [abi::PORT_READ, u64::from(offset), 0, 0],
    )?;
    Ok(reply.words[0] as u8)
}

pub fn write(ports: Handle, offset: u16, value: u8) -> Result<(), Error> {
    call(
        ports,
        Attach::Nothing,
        [abi::PORT_WRITE, u64::from(offset), u64::from(value), 0],
    )?;
    Ok(())
}
