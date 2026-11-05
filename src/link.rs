//! 文字里的链接:`[显示文字](目标)` 的目标识别与本地路径解析.
//!
//! 这里只做纯逻辑(不碰终端、不起进程),打开动作在 `main.rs` 里:
//! URL 交给 opener 就完事,本地文本文件要把终端挂起让编辑器接管.

use std::path::{Path, PathBuf};

/// 链接目标的种类
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// `http` / `https` / `mailto`:交给 opener,不碰本地文件系统
    Url,
    /// `file://`、`~/`、`./`、`../`、`/`:先解析成本地路径
    Local,
    /// `omp://`:认得出来,但 v1 不打开(见 `target::OmpHandler`)
    Omp,
    /// 保留字 `self`:当前文档(可以带 `:x,y`)
    SelfDoc,
}

/// 目标是不是链接.只有这几种前缀算,别的 `[x](y)` 保持普通文字
pub fn classify(raw: &str) -> Option<LinkKind> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if starts_ci(t, "http://") || starts_ci(t, "https://") || starts_ci(t, "mailto:") {
        return Some(LinkKind::Url);
    }
    if starts_ci(t, "file://") {
        return Some(LinkKind::Local);
    }
    if starts_ci(t, "omp://") {
        return Some(LinkKind::Omp);
    }
    // `self` 是保留字,不是文件;位置后缀 `self:10,10` 也算链接
    if t.eq_ignore_ascii_case("self") || starts_ci(t, "self:") {
        return Some(LinkKind::SelfDoc);
    }
    // 别的 scheme(`javascript:`、`data:`、`vscode://` ...)一律不认
    if has_scheme(t) {
        return None;
    }
    if t.starts_with("~/") || t.starts_with("./") || t.starts_with("../") || t.starts_with('/') {
        return Some(LinkKind::Local);
    }
    None
}

fn starts_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p)
}

/// `scheme:` 开头(允许 `+` `-` `.`):用来挡掉不认识的协议
fn has_scheme(s: &str) -> bool {
    let Some((head, _)) = s.split_once(':') else {
        return false;
    };
    let mut chars = head.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// 把本地目标解析成路径.
///
/// `doc_dir` 是当前文档所在目录(`./`、`../` 相对它),空白文档没有路径时退回 `cwd`.
pub fn resolve_local(raw: &str, doc_dir: Option<&Path>, cwd: &Path) -> Result<PathBuf, String> {
    let t = raw.trim();
    if starts_ci(t, "file://") {
        let rest = &t["file://".len()..];
        // host 只接受空或 localhost;别的 host 不是本机文件
        let path = if let Some(after) = rest.strip_prefix("localhost") {
            if !after.starts_with('/') {
                return Err("file:// link has no absolute path".to_string());
            }
            after
        } else if rest.starts_with('/') {
            rest
        } else if rest.is_empty() {
            return Err("file:// link has no path".to_string());
        } else {
            let host = rest.split('/').next().unwrap_or(rest);
            return Err(format!("file://{host} is not a local file"));
        };
        return Ok(normalize(Path::new(&percent_decode(path))));
    }
    if let Some(rest) = t.strip_prefix("~/") {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
        return Ok(normalize(&PathBuf::from(home).join(rest)));
    }
    if t.starts_with('/') {
        return Ok(normalize(Path::new(t)));
    }
    if t.starts_with("./") || t.starts_with("../") {
        let base = doc_dir.unwrap_or(cwd);
        return Ok(normalize(&base.join(t)));
    }
    Err(format!("{t} is not a local path"))
}

/// 这个路径能不能在 graph-tui 里直接开成新标签
pub fn is_graph_doc(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("gtui") | Some("dot")
    )
}

/// `%20` -> 空格:file:// 里的路径是百分号编码的
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 词法上的规范化:去掉 `.`、吃掉 `..`,不碰文件系统(路径不存在也能算)
fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只认白名单里的目标() {
        for ok in [
            "https://example.com",
            "http://a.b/c",
            "MAILTO:foo@example.com",
            "file:///tmp/a.md",
            "~/notes.md",
            "./a.md",
            "../a.md",
            "/tmp/a.md",
        ] {
            assert!(classify(ok).is_some(), "{ok} 该识别");
        }
        for no in [
            "notes.md",
            "javascript:alert(1)",
            "data:text/plain,x",
            "vscode://x",
        ] {
            assert!(classify(no).is_none(), "{no} 不该识别");
        }
        assert_eq!(classify("https://x"), Some(LinkKind::Url));
        assert_eq!(classify("~/x"), Some(LinkKind::Local));
    }

    #[test]
    fn 本地目标解析() {
        let cwd = Path::new("/work");
        let doc_dir = Path::new("/work/docs");
        assert_eq!(
            resolve_local("file:///tmp/a%20b.md", None, cwd).unwrap(),
            PathBuf::from("/tmp/a b.md")
        );
        assert_eq!(
            resolve_local("./a.md", Some(doc_dir), cwd).unwrap(),
            PathBuf::from("/work/docs/a.md")
        );
        assert_eq!(
            resolve_local("../a.md", Some(doc_dir), cwd).unwrap(),
            PathBuf::from("/work/a.md")
        );
        assert_eq!(
            resolve_local("./a.md", None, cwd).unwrap(),
            PathBuf::from("/work/a.md"),
            "没有文档路径时按 cwd"
        );
        assert!(resolve_local("file://otherhost/a.md", None, cwd).is_err());
        assert!(resolve_local("file://localhost/tmp/x", None, cwd).is_ok());
        // `~/` 按真实 HOME 展开(测试不去改环境变量,免得和别的测试打架)
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                resolve_local("~/n.md", None, cwd).unwrap(),
                PathBuf::from(home).join("n.md")
            );
        }
    }

    #[test]
    fn 只有gtui和dot算图档() {
        assert!(is_graph_doc(Path::new("/a/b.gtui")));
        assert!(is_graph_doc(Path::new("x.dot")));
        assert!(!is_graph_doc(Path::new("x.md")));
        assert!(!is_graph_doc(Path::new("x")));
    }
}
