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

/// 提示里用的绝对路径:`~` 展开、相对路径拼当前目录,顺手消掉 `.` 与 `..`.
/// 纯粹按字符串算,不碰文件系统(提示每敲一个字都要重算,不能有 IO)
/// 把 `$HOME` 开头的路径缩成 `~`(状态行显示用;不做别的展开)
pub fn tildify(path: &str) -> String {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    let Some(home) = home else {
        return path.to_string();
    };
    let home = home.to_string_lossy().into_owned();
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(&home) {
        if rest.starts_with('/') {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

pub fn absolute_display(path: &str) -> String {
    let expanded = expand(path);
    if expanded.is_empty() {
        return String::new();
    }
    let joined = if expanded.starts_with('/') {
        expanded
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(&expanded).to_string_lossy().into_owned(),
            Err(_) => expanded,
        }
    };
    normalize(&joined)
}

/// 消掉 `.` 与 `..`(只对绝对路径有意义,相对路径先拼过 cwd 了)
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                // 已经到根就不再往上
                out.pop();
            }
            p => out.push(p),
        }
    }
    format!("/{}", out.join("/"))
}

/// 按前缀扫目录,给出能填进命令行的候选:目录结尾带 `/`(否则进不去子目录),
/// 目录排在文件前面,同类按字典序,最多 50 个.
/// `exts` 非空时文件只留这些扩展名(不带点,大小写不敏感).
/// 候选保留用户写法的前缀(`~/x` 补出来还是 `~/...`),隐藏文件要自己打点才列.
pub fn complete_path(arg: &str, exts: &[&str]) -> Vec<String> {
    let (head, base) = match arg.rfind('/') {
        Some(i) => (&arg[..=i], &arg[i + 1..]),
        None => ("", arg),
    };
    let scan = if head.is_empty() {
        ".".to_string()
    } else {
        expand(head)
    };
    let Ok(rd) = std::fs::read_dir(&scan) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(base) {
            continue;
        }
        if name.starts_with('.') && !base.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(name);
            continue;
        }
        if !exts.is_empty() {
            let ext = std::path::Path::new(&name)
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                continue;
            }
        }
        files.push(name);
    }
    dirs.sort();
    files.sort();
    dirs.into_iter()
        .map(|n| format!("{head}{n}/"))
        .chain(files.into_iter().map(|n| format!("{head}{n}")))
        .take(50)
        .collect()
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
    fn 提示用的绝对路径() {
        // 绝对路径:顺手消掉 . 与 ..
        assert_eq!(absolute_display("/x/./y/../z"), "/x/z");
        assert_eq!(absolute_display("/a/../../b"), "/b");
        assert_eq!(absolute_display("/x/"), "/x");
        // 相对路径拼当前目录
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_display("out/a.gtui"),
            cwd.join("out/a.gtui").to_string_lossy()
        );
        assert_eq!(absolute_display(""), "");
    }

    #[test]
    fn 按前缀补路径() {
        let dir = std::env::temp_dir().join(format!("gtui-comp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.gtui"), "{}").unwrap();
        std::fs::write(dir.join("ab.dot"), "{}").unwrap();
        std::fs::write(dir.join("c.txt"), "").unwrap();
        std::fs::write(dir.join(".hidden.gtui"), "{}").unwrap();
        let d = dir.to_str().unwrap();
        let pre = format!("{d}/");
        // 目录在前、带结尾 `/`;扩展名过滤掉 c.txt;隐藏文件要自己打点
        assert_eq!(
            complete_path(&pre, &["gtui", "dot"]),
            vec![
                format!("{d}/sub/"),
                format!("{d}/a.gtui"),
                format!("{d}/ab.dot")
            ]
        );
        // 按前缀过滤,整段(含目录)一起回填
        assert_eq!(
            complete_path(&format!("{pre}ab"), &[]),
            vec![format!("{d}/ab.dot")]
        );
        // 不过滤时 .txt 也在
        assert!(complete_path(&pre, &[]).contains(&format!("{d}/c.txt")));
        // 打了点才看隐藏文件
        assert_eq!(
            complete_path(&format!("{pre}."), &["gtui"]),
            vec![format!("{d}/.hidden.gtui")]
        );
        // 目录不存在:空
        assert!(complete_path(&format!("{d}/nope/x"), &[]).is_empty());
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
