//! User programs built into the kernel image, until there is a file system.
//! `cargo xtask build` compiles them first and points `HEMATITE_PROGRAMS`
//! at the folder with the results (default: .cargo/config.toml).

pub static HELLO: &[u8] = include_bytes!(concat!(env!("HEMATITE_PROGRAMS"), "/hello"));
pub static INTRUDER: &[u8] = include_bytes!(concat!(env!("HEMATITE_PROGRAMS"), "/intruder"));
