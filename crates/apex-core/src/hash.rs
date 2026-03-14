/// FNV-1a hash — stable file_id from repo-relative path strings.
pub fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        assert_eq!(fnv1a_hash("src/index.js"), fnv1a_hash("src/index.js"));
    }

    #[test]
    fn empty_returns_offset_basis() {
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn different_strings_differ() {
        assert_ne!(fnv1a_hash("a.js"), fnv1a_hash("b.js"));
    }
}
