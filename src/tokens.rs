// Tokens: Deterministic rough token-count heuristic (~1 token per 4 bytes) for display budgeting — a "map, not exact" estimate, NOT a real BPE tokenizer and with no vocabulary or dependencies. NOT concerned with model-accurate counts. | I/O: (byte length) -> estimated token count

/// Estimate the token count of a file from its byte length with a deterministic
/// heuristic: roughly one token per four bytes (`ceil(len_bytes / 4)`). This is a
/// display approximation within the tool's "map, not exact" contract — not a real
/// BPE tokenizer, so it needs no vocabulary and adds no dependencies.
///
/// Takes a byte length (not the file contents) so callers can source it from
/// `std::fs::metadata().len()` without reading the file. Saturates rather than
/// wraps for pathologically huge files.
pub fn estimate(len_bytes: u64) -> u32 {
    len_bytes.div_ceil(4).min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate(0), 0);
    }

    #[test]
    fn rounds_up_partial_chunks() {
        assert_eq!(estimate(1), 1); // 1 byte -> ceil(1/4)
        assert_eq!(estimate(4), 1); // 4 bytes -> exactly 1
        assert_eq!(estimate(5), 2); // 5 bytes -> ceil(5/4)
    }

    #[test]
    fn monotonic_in_length() {
        let mut prev = 0;
        for len in 0..256u64 {
            let next = estimate(len);
            assert!(next >= prev, "estimate must not decrease as bytes grow");
            prev = next;
        }
    }

    #[test]
    fn saturates_instead_of_wrapping() {
        assert_eq!(estimate(u64::MAX), u32::MAX);
    }
}
