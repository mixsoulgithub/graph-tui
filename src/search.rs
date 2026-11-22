//! Alias normalisation: the string handling search needs (pinyin, initials, case).
//! The actual matching lives in document::search (it looks at text and groups alike).

/// Normalise an alias query: keep alphanumerics only, lowercase, treat `ü` and v as u
pub fn latin(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            // `ü` and v both map to u ("nv" and "nü" are one entry): this must come first,
            // before the letter branch, or v is kept as an ordinary letter instead
            if c == 'ü' || c == 'ǖ' || c == 'ǘ' || c == 'ǚ' || c == 'ǜ' || c == 'v' || c == 'V'
            {
                Some('u')
            } else if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_alphanumeric() {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else {
                None
            }
        })
        .collect()
}

/// Initials of a pinyin string: "wu lin guang chang" -> "wlgc"
pub fn initials(pinyin: &str) -> String {
    pinyin
        .split_whitespace()
        .filter_map(|s| s.chars().next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_keeps_alphanumeric_lowercase() {
        assert_eq!(latin("Wu Lin 广场"), "wulin广场");
        assert_eq!(latin("a-b_c!"), "abc");
    }

    #[test]
    fn v_and_umlaut_map_to_u() {
        assert_eq!(latin("lü"), "lu");
        assert_eq!(latin("nv"), "nu");
    }

    #[test]
    fn initials_split_on_whitespace() {
        assert_eq!(initials("wu lin guang chang"), "wlgc");
        assert_eq!(initials(""), "");
    }
}
