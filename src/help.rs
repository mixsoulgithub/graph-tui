//! `--help` 的文字:唯一来源是 `docs/usage.md` 里的 `cli-help` 标记块,
//! 不在代码里重复一份.
//!
//! help 是文档输出,固定英文;`--lang zh` 只影响 TUI 自己的文案.

const DOC: &str = include_str!("../docs/usage.md");

const BEGIN: &str = "<!-- cli-help:begin -->";
const END: &str = "<!-- cli-help:end -->";

/// 从文档里取 help 块:标记之间的第一个 ``` 代码块,去掉围栏.
/// 标记缺失是构建/测试错误,不做静默回退.
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

/// help 文本(英文,来自 `docs/usage.md`)
pub fn text() -> &'static str {
    extract(DOC)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文档里带标记块,里面是最完整的 CLI help
    #[test]
    fn 文档里有_help_块() {
        let t = text();
        assert!(t.len() > 500, "help 太短");
        for want in ["--export", "--format", "--help", "tutor", "headless"] {
            assert!(t.contains(want), "help 没讲 {want}");
        }
        assert!(!t.contains("```"), "help 里还留着代码围栏");
        assert!(!t.contains(BEGIN), "help 里还留着标记");
        // 固定英文
        assert!(!t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    }
}
