//! Bounds-checked little-endian reads and writes on byte slices.
//!
//! The embedded ELF has no alignment guarantees, so we never cast pointers
//! into it – we copy bytes out and assemble the numbers.

use super::Error;

fn array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], Error> {
    offset
        .checked_add(N)
        .and_then(|end| bytes.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or("ELF: read past the end of the data")
}

pub fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    array(bytes, offset).map(u16::from_le_bytes)
}

pub fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    array(bytes, offset).map(u32::from_le_bytes)
}

pub fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    array(bytes, offset).map(u64::from_le_bytes)
}

pub fn write_u64(bytes: &mut [u8], offset: usize, value: u64) -> Result<(), Error> {
    offset
        .checked_add(8)
        .and_then(|end| bytes.get_mut(offset..end))
        .ok_or("ELF: write past the end of the image")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
