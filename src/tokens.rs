// Calibrated for code and JSON: symbols and short identifiers make it denser than prose (~4.0).
// Kept identical to repo-pack so both tools report comparable numbers
const CHARS_PER_TOKEN: f64 = 3.8;

pub const CONTEXT_WINDOWS: [(&str, usize); 3] =
    [("128k", 128_000), ("200k", 200_000), ("1M", 1_000_000)];

pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 / CHARS_PER_TOKEN).ceil() as usize
}

pub fn percentage_of(tokens: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    tokens as f64 * 100.0 / total as f64
}

#[cfg(test)]
mod tests {
    use super::{estimate_tokens, percentage_of};

    #[test]
    fn estimates_tokens_rounding_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 2);
        assert_eq!(estimate_tokens(&"a".repeat(38)), 10);
    }

    #[test]
    fn counts_unicode_chars_not_bytes() {
        assert_eq!(estimate_tokens("ñandú"), estimate_tokens("nandu"));
    }

    #[test]
    fn computes_percentages_safely() {
        assert_eq!(percentage_of(50_000, 200_000), 25.0);
        assert_eq!(percentage_of(10, 0), 0.0);
    }
}
