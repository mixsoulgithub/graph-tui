//! `--help` text: its single source is the `cli-help` marker block in `docs/usage.md`,
//! never duplicated in code.
//!
//! Help is documentation output and always English; `--lang zh` only affects the TUI's
//! own strings.

const DOC: &str = include_str!("../docs/usage.md");

const BEGIN: &str = "<!-- cli-help:begin -->";
const END: &str = "<!-- cli-help:end -->";

/// Take the help block out of the document: the first ``` code block between the
/// markers, fences stripped. A missing marker is a build/test error, never a silent
/// fallback.
fn extract(doc: &'static str) -> &'static str {
    let begin = doc
        .find(BEGIN)
        .unwrap_or_else(|| panic!("docs/usage.md: missing {BEGIN}"));
    let rest = &doc[begin + BEGIN.len()..];
    let end = rest
        .find(END)
        .unwrap_or_else(|| panic!("docs/usage.md: missing {END}"));
    let block = &rest[..end];
    let open = block
        .find("```")
        .unwrap_or_else(|| panic!("docs/usage.md: the cli-help block has no code fence"));
    let after = &block[open + 3..];
    let nl = after.find('\n').unwrap_or_else(|| {
        panic!("docs/usage.md: the fenced block does not start with a language line")
    });
    let body = &after[nl + 1..];
    let close = body
        .find("```")
        .unwrap_or_else(|| panic!("docs/usage.md: the cli-help block is not closed"));
    body[..close]
        .trim_end_matches('\n')
        .trim_start_matches('\n')
}

/// Help text (English, from `docs/usage.md`)
pub fn text() -> &'static str {
    extract(DOC)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document carries the marker block holding the full CLI help
    #[test]
    fn doc_has_a_help_block() {
        let t = text();
        assert!(t.len() > 500, "help 太短");
        for want in ["--export", "--format", "--help", "tutor", "headless"] {
            assert!(t.contains(want), "help 没讲 {want}");
        }
        assert!(!t.contains("```"), "help 里还留着代码围栏");
        assert!(!t.contains(BEGIN), "help 里还留着标记");
        // always English
        assert!(!t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    }
}
