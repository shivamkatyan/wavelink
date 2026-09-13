//! CRC32 helper (per-frame integrity on the lossless path).

/// Compute CRC-32 (IEEE 802.3, reflected polynomial 0xEDB88320) over `data`.
/// Pure table-less bitwise implementation: deterministic, no allocation, tiny.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_values() {
        // Standard CRC-32 check vectors.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(&[0u8; 32]), 0x190A_55AD);
    }
}
