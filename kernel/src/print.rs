//! `print!` / `println!` for the kernel, writing to the serial port.

macro_rules! print {
    ($($argument:tt)*) => {{
        use core::fmt::Write as _;
        // Serial output cannot fail, so the `fmt::Result` is ignored.
        let _ = write!($crate::serial::Serial::com1(), $($argument)*);
    }};
}

macro_rules! println {
    () => { print!("\n") };
    ($($argument:tt)*) => {{
        print!($($argument)*);
        print!("\n");
    }};
}
