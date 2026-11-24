//! `--help` text: its single source is the `cli-help` marker block in `docs/usage.md`.
//! The block is Markdown (so the same text reads well on GitHub) and is rendered for the
//! terminal here: tables become aligned columns, heading and inline markers are dropped.
//! Keys are not repeated in the document; they come from the table the `?` overlay uses.
//!
//! Help is documentation output and always English; `--lang zh` only affects the TUI's
//! own strings.

use std::sync::OnceLock;

use crate::lang::Lang;

const DOC: &str = include_str!("../docs/usage.md");

const BEGIN: &str = "<!-- cli-help:begin -->";
const END: &str = "<!-- cli-help:end -->";

/// Columns to wrap the rendered help at.
const WIDTH: usize = 100;

/// The Markdown between the markers. A missing marker is a build/test error, never a
/// silent fallback.
fn section(doc: &'static str) -> &'static str {
    let begin = doc
        .find(BEGIN)
        .unwrap_or_else(|| panic!("docs/usage.md: missing {BEGIN}"));
    let rest = &doc[begin + BEGIN.len()..];
    let end = rest
        .find(END)
        .unwrap_or_else(|| panic!("docs/usage.md: missing {END}"));
    rest[..end].trim()
}

/// Help text (English): the document section, then the key table from the code
/// (`render::help_keys`, the same rows the `?` overlay shows).
pub fn text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut md = String::from(section(DOC));
        md.push_str("\n\n### Keys\n\n| Key | What it does |\n| --- | --- |\n");
        for (key, what) in crate::render::help_keys(Lang::En) {
            md.push_str(&format!(
                "| `{}` | {} |\n",
                key.replace('|', "\\|"),
                what.replace('|', "\\|")
            ));
        }
        md.push_str(
            "\nPress `?` inside the app for the same list (any key closes it);\n\
             `docs/keybindings.md` has the long version.\n",
        );
        crate::md::to_terminal(&md, WIDTH)
    })
    .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document carries the marker block and nothing else defines the help
    #[test]
    fn doc_has_a_help_block() {
        let t = text();
        assert!(t.len() > 500, "help 太短");
        for want in ["--export", "--format", "--help", "tutor"] {
            assert!(t.contains(want), "help 没讲 {want}");
        }
        assert!(!t.contains("```"), "help 里还留着代码围栏");
        assert!(
            !t.contains(BEGIN) && !t.contains("<!--"),
            "help 里还留着标记"
        );
        // Markdown syntax must be gone: no table separators, no heading hashes
        assert!(
            !t.contains("| ---") && !t.contains("--- |"),
            "help 里还留着表格分隔行"
        );
        assert!(
            !t.lines().any(|l| l.starts_with('#')),
            "help 里还留着标题井号"
        );
        // always English
        assert!(!t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    }

    /// Keys come from the overlay table, so `--help` and `?` cannot drift apart
    #[test]
    fn keys_come_from_the_overlay_table() {
        let t = text();
        for (key, _) in crate::render::help_keys(Lang::En).iter().take(3) {
            assert!(t.contains(key), "help 少了按键 {key}");
        }
        assert!(t.contains("key reference"), "help 没讲 `?`");
    }

    /// Every rendered line fits the width, except verbatim code (the synopsis)
    #[test]
    fn help_fits_the_width() {
        for line in text().lines() {
            if line.contains("usage: graph-tui") {
                continue;
            }
            assert!(
                unicode_width::UnicodeWidthStr::width(line) <= WIDTH,
                "这一行太宽: {line}"
            );
        }
    }
}
