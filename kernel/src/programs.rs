//! User programs built into the kernel image, until there is a file system.
//! `cargo xtask build` compiles them first and points `HEMATITE_PROGRAMS`
//! at the folder with the results (default: .cargo/config.toml).

macro_rules! program {
    ($name:ident, $file:literal) => {
        pub static $name: &[u8] = include_bytes!(concat!(env!("HEMATITE_PROGRAMS"), "/", $file));
    };
}

program!(INIT, "init");
program!(LOG, "log");
program!(DIRECTORY, "directory");
program!(SHOUT, "shout");
program!(VANDAL, "vandal");
program!(HELLO, "hello");
program!(OWNER, "owner");
program!(GIVER, "giver");
program!(INTRUDER, "intruder");
