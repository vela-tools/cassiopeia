//! Human-readable byte-count formatting.

/// Formats a byte count with binary (IEC) units and one decimal place. Integer arithmetic
/// throughout, so no lossy float cast is needed; the `u128` widening keeps the tenths multiplication
/// exact.
#[must_use]
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut unit = 0;
    let mut divisor: u64 = 1;
    while unit + 1 < UNITS.len() && bytes / divisor >= 1024 {
        divisor *= 1024;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let tenths = u128::from(bytes) * 10 / u128::from(divisor);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use crate::byte_size::human_size;

    #[test]
    fn human_size_uses_binary_units_with_one_decimal() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(1229), "1.2 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(133_744_204), "127.5 MiB");
    }
}
