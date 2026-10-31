//! 路径小工具:把开头的 `~` 展开成家目录.
//!
//! 动词里的路径是手敲的,shell 不会替我们展开(`:w ~/a.gtui`),所以在入口统一做一次.
//! 只认 `~` 与 `~/...`,不做 `~user`(那要查 passwd,不值得).

/// 展开开头的 `~`;不是家目录写法就原样返回(去掉首尾空白)
pub fn expand(path: &str) -> String {
    expand_with(path, std::env::var("HOME").ok().as_deref())
}

/// 同 `expand`,但家目录从参数来(方便测试)
pub fn expand_with(path: &str, home: Option<&str>) -> String {
    let p = path.trim();
    if p.is_empty() {
        return String::new();
    }
    let Some(rest) = p.strip_prefix('~') else {
        return p.to_string();
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return p.to_string();
    }
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return p.to_string();
    };
    if rest.is_empty() {
        home.to_string()
    } else {
        format!("{home}{rest}")
    }
}

/// 写文件前把父目录建出来(文件就在当前目录时没什么可建的).
/// 返回值是错误说明,调用方自己加前缀
pub fn ensure_parent(path: &str) -> Result<(), String> {
    let Some(dir) = std::path::Path::new(path).parent() else {
        return Ok(());
    };
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

/// 先写 .tmp 再 rename:中途失败不会把原文件写坏
pub fn write_atomic(path: &str, body: &str) -> Result<(), String> {
    ensure_parent(path)?;
    let tmp = format!("{path}.tmp");
    if let Err(e) = std::fs::write(&tmp, body.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {tmp}: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {path}: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 建父目录() {
        let dir = std::env::temp_dir().join(format!("gtui-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let deep = dir.join("a").join("b").join("x.gtui");
        let path = deep.to_str().unwrap();
        assert_eq!(ensure_parent(path), Ok(()));
        assert!(deep.parent().unwrap().is_dir());
        // 已经在当前目录:不用建,也不报错
        assert_eq!(ensure_parent("x.gtui"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 只展开家目录写法() {
        assert_eq!(
            expand_with("~/a/b.gtui", Some("/home/m")),
            "/home/m/a/b.gtui"
        );
        assert_eq!(expand_with("~", Some("/home/m")), "/home/m");
        assert_eq!(expand_with("  ~/x  ", Some("/home/m")), "/home/m/x");
        // 这些原样返回
        assert_eq!(expand_with("~user/x", Some("/home/m")), "~user/x");
        assert_eq!(expand_with("a/~/b", Some("/home/m")), "a/~/b");
        assert_eq!(expand_with("/abs/x", Some("/home/m")), "/abs/x");
        assert_eq!(expand_with("~/x", None), "~/x");
        assert_eq!(expand_with("", Some("/home/m")), "");
    }
}
