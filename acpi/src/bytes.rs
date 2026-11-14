//! Little-endian fields at byte offsets – ACPI tables are packed structs
//! with no alignment guarantees, so we never cast them to Rust types.

pub fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut field = [0; 4];
    field.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(field)
}

pub fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut field = [0; 8];
    field.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(field)
}

/// Every ACPI structure is built so that all its bytes add up to 0 (mod 256).
pub fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}
