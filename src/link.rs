//! Links in text: target recognition and local path resolution for
//! `[display text](target)`.
//!
//! Pure logic only (no terminal, no processes); the open action lives in `main.rs`:
//! a URL just goes to the opener, while a local text file needs the terminal suspended so
//! the editor can take over.

use std::path::{Path, PathBuf};

/// Kind of link target
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// `http` / `https` / `mailto`: handed to the opener, never touching the filesystem
    Url,
    /// `file://`, `~/`, `./`, `../`, `/`: resolved to a local path first
    Local,
    /// Any other `scheme://`: a generic custom scheme whose open command comes from the
    /// `[links]` config
    Custom,
    /// Reserved word `self`: the current document (optionally with `:x,y`)
    SelfDoc,
}

/// Is the target a link? Only these prefixes count; any other `[x](y)` stays plain text
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
    // any other valid `scheme://`: a generic custom scheme (the core knows no specific one)
    if scheme_of(t).is_some_and(|s| s.len() < t.len() && t[s.len()..].starts_with("://")) {
        return Some(LinkKind::Custom);
    }
    // `self` is a reserved word, not a file; a position suffix such as `self:10,10` is a link
    if t.eq_ignore_ascii_case("self") || starts_ci(t, "self:") {
        return Some(LinkKind::SelfDoc);
    }
    // schemes without `//` (`javascript:`, `data:`, ...) are never links
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

/// The `scheme` part (`[A-Za-z][A-Za-z0-9+.-]*`): `Some("foo")` = `foo:...`
pub fn scheme_of(s: &str) -> Option<&str> {
    let end = s.find(':')?;
    let head = &s[..end];
    if head.is_empty() {
        return None;
    }
    let mut chars = head.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some(head)
}

/// Starts with `scheme:` (allowing `+` `-` `.`): schemes without `//` (`javascript:`,
/// `data:`) do not classify
fn has_scheme(s: &str) -> bool {
    scheme_of(s).is_some()
}

/// Resolve a local target into a path.
///
/// `doc_dir` is the current document's directory (`./` and `../` are relative to it);
/// a scratch document with no path falls back to `cwd`.
pub fn resolve_local(raw: &str, doc_dir: Option<&Path>, cwd: &Path) -> Result<PathBuf, String> {
    let t = raw.trim();
    if starts_ci(t, "file://") {
        let rest = &t["file://".len()..];
        // the host must be empty or localhost; any other host is not a local file
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

/// Whether this path opens directly as a new tab in graph-tui
pub fn is_graph_doc(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("gtui") | Some("dot")
    )
}

/// `%20` -> space: paths inside file:// are percent-encoded
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

/// Lexical normalisation: drop `.`, consume `..`, never touching the filesystem (it works
/// for paths that do not exist)
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
    fn only_whitelisted_targets_classify() {
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
        for no in ["notes.md", "javascript:alert(1)", "data:text/plain,x"] {
            assert!(classify(no).is_none(), "{no} 不该识别");
        }
        // any other `scheme://` is a generic custom scheme; `[links]` decides on allowed ones
        assert_eq!(classify("vscode://x"), Some(LinkKind::Custom));
        assert_eq!(classify("FOO://x"), Some(LinkKind::Custom));
        assert_eq!(classify("a+b-c.d://x"), Some(LinkKind::Custom));
        assert_eq!(scheme_of("FOO://x"), Some("FOO"));
        assert_eq!(classify("https://x"), Some(LinkKind::Url));
        assert_eq!(classify("~/x"), Some(LinkKind::Local));
    }

    #[test]
    fn local_target_resolution() {
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
        // `~/` expands against the real HOME (the test avoids changing env vars, which
        // would disturb other tests)
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                resolve_local("~/n.md", None, cwd).unwrap(),
                PathBuf::from(home).join("n.md")
            );
        }
    }

    #[test]
    fn only_gtui_and_dot_are_graph_docs() {
        assert!(is_graph_doc(Path::new("/a/b.gtui")));
        assert!(is_graph_doc(Path::new("x.dot")));
        assert!(!is_graph_doc(Path::new("x.md")));
        assert!(!is_graph_doc(Path::new("x")));
    }
}
