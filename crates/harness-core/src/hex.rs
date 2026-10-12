//! Lowercase hex of bytes: what a SHA-256 digest is written as (manifests, checksums, ids).

/// `bytes` as lowercase hex, two digits per byte.
pub fn lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn lower_pads_each_byte_to_two_digits() {
        assert_eq!(super::lower(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
        assert_eq!(super::lower(&[]), "");
    }
}
