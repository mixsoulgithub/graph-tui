//! `--help` 的文字:唯一来源是 `docs/usage.md` / `docs/usage.zh.md` 里的
//! `cli-help` 标记块,不再在代码里重复一份.
//!
//! 语言按环境挑(`--lang` 由调用方先处理):LC_ALL > LC_MESSAGES > LANGUAGE > LANG,
//! 以 `zh` 开头就用中文,其余英文.

use crate::lang::Lang;

const EN: &str = include_str!("../docs/usage.md");
const ZH: &str = include_str!("../docs/usage.zh.md");

const BEGIN: &str = "<!-- cli-help:begin -->";
const END: &str = "<!-- cli-help:end -->";

/// 从文档里取 help 块:标记之间的第一个 ``` 代码块,去掉围栏.
/// 标记缺失是构建/测试错误,不做静默回退.
fn extract(doc: &'static str, which: &str) -> &'static str {
    let begin = doc
        .find(BEGIN)
        .unwrap_or_else(|| panic!("{which}: missing {BEGIN}"));
    let rest = &doc[begin + BEGIN.len()..];
    let end = rest
        .find(END)
        .unwrap_or_else(|| panic!("{which}: missing {END}"));
    let block = &rest[..end];
    let open = block
        .find("```")
        .unwrap_or_else(|| panic!("{which}: the cli-help block has no code fence"));
    let after = &block[open + 3..];
    let nl = after
        .find('\n')
        .unwrap_or_else(|| panic!("{which}: fenced block does not start with a language line"));
    let body = &after[nl + 1..];
    let close = body
        .find("```")
        .unwrap_or_else(|| panic!("{which}: the cli-help block is not closed"));
    body[..close]
        .trim_end_matches('\n')
        .trim_start_matches('\n')
}

/// help 文本(按语言).两份文档都必须带标记块
pub fn text(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => extract(ZH, "docs/usage.zh.md"),
        Lang::En => extract(EN, "docs/usage.md"),
    }
}

/// 从几个环境变量的值里挑语言(空值跳过;`zh*` 中文,其余英文)
pub fn lang_of(vars: [Option<&str>; 4]) -> Lang {
    for v in vars.into_iter().flatten() {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let lower = v.to_ascii_lowercase();
        return if lower.starts_with("zh") {
            Lang::Zh
        } else {
            Lang::En
        };
    }
    Lang::En
}

/// 按 LC_ALL / LC_MESSAGES / LANGUAGE / LANG 挑 help 的语言
pub fn lang_from_env() -> Lang {
    let get = |k: &str| std::env::var(k).ok();
    lang_of([
        get("LC_ALL").as_deref(),
        get("LC_MESSAGES").as_deref(),
        get("LANGUAGE").as_deref(),
        get("LANG").as_deref(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两份文档都带标记块,里面是最完整的 CLI help
    #[test]
    fn 两份文档都有_help_块() {
        for (lang, name) in [(Lang::En, "en"), (Lang::Zh, "zh")] {
            let t = text(lang);
            assert!(t.len() > 500, "{name} 的 help 太短");
            assert!(t.contains("--dump"), "{name} 没讲 --dump");
            assert!(t.contains("--format"), "{name} 没讲 --format");
            assert!(t.contains("--help"), "{name} 没讲 --help");
            assert!(t.contains("tutor"), "{name} 没讲 tutor");
            assert!(t.contains("headless"), "{name} 没讲 headless");
            assert!(!t.contains("```"), "{name} 里还留着代码围栏");
            assert!(!t.contains(BEGIN), "{name} 里还留着标记");
        }
    }

    /// 中文那份确实是中文(至少含汉字),英文那份不含
    #[test]
    fn 中文_help_是中文() {
        let zh = text(Lang::Zh);
        assert!(zh.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        assert_eq!(text(Lang::En), text(Lang::En));
        assert_ne!(text(Lang::En), zh);
    }

    /// 语言优先级:空值跳过,`zh*` 中文,其余英文
    #[test]
    fn 语言按环境挑() {
        assert_eq!(lang_of([Some("zh_CN.UTF-8"), None, None, None]), Lang::Zh);
        assert_eq!(lang_of([Some(""), Some("zh-Hans"), None, None]), Lang::Zh);
        assert_eq!(lang_of([None, None, None, Some("en_US.UTF-8")]), Lang::En);
        assert_eq!(lang_of([Some("C"), Some("zh_CN"), None, None]), Lang::En);
        assert_eq!(lang_of([None, None, None, None]), Lang::En);
        // 大小写不敏感
        assert_eq!(lang_of([Some("ZH_cn"), None, None, None]), Lang::Zh);
    }
}
