//! Path helpers: expand a leading `~` to the home directory.
//!
//! Paths in commands are typed by hand and the shell does not expand them
//! (`:w ~/a.gtui`), so expansion happens once at the entry point.
//! Only `~` and `~/...` are recognised; `~user` would need a passwd lookup and is
//! not worth it.

/// Expands a leading `~`; anything that is not a home path is returned as is
/// (trimmed).
pub fn expand(path: &str) -> String {
    expand_with(path, std::env::var("HOME").ok().as_deref())
}

/// Same as `expand`, but the home directory comes from the argument (for tests).
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

/// Shrinks a path starting with `$HOME` to `~` (for the status line; no other
/// expansion).
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

/// Absolute path for the prompt: expands `~`, joins relative paths to the current
/// directory and drops `.` and `..`. Pure string work, no filesystem access (the
/// prompt recomputes on every keystroke, so no IO allowed).
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

/// Drops `.` and `..` (only meaningful for absolute paths; relative paths are
/// joined to the cwd first).
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                // At the root: do not go further up.
                out.pop();
            }
            p => out.push(p),
        }
    }
    format!("/{}", out.join("/"))
}

/// Scans a directory by prefix and returns candidates for the command line:
/// directories end with `/` (otherwise they cannot be descended into) and sort
/// before files; within each group entries are in lexicographic order, capped at
/// 50.
/// When `exts` is non-empty only files with those extensions are kept (no dot,
/// case-insensitive).
/// Candidates keep the prefix as the user typed it (`~/x` completes to `~/...`);
/// hidden files are listed only once the user types the dot.
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

/// Creates the parent directory before writing (nothing to create when the file
/// lives in the current directory).
/// Returns an error message; the caller adds its own prefix.
pub fn ensure_parent(path: &str) -> Result<(), String> {
    let Some(dir) = std::path::Path::new(path).parent() else {
        return Ok(());
    };
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

/// Writes `.tmp` first and renames: a failure midway never corrupts the original
/// file.
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
    fn creates_parent_directory() {
        let dir = std::env::temp_dir().join(format!("gtui-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let deep = dir.join("a").join("b").join("x.gtui");
        let path = deep.to_str().unwrap();
        assert_eq!(ensure_parent(path), Ok(()));
        assert!(deep.parent().unwrap().is_dir());
        // Already in the current directory: nothing to create, no error.
        assert_eq!(ensure_parent("x.gtui"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absolute_display_for_prompt() {
        // Absolute path: drops `.` and `..`.
        assert_eq!(absolute_display("/x/./y/../z"), "/x/z");
        assert_eq!(absolute_display("/a/../../b"), "/b");
        assert_eq!(absolute_display("/x/"), "/x");
        // Relative paths are joined to the current directory.
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_display("out/a.gtui"),
            cwd.join("out/a.gtui").to_string_lossy()
        );
        assert_eq!(absolute_display(""), "");
    }

    #[test]
    fn completes_paths_by_prefix() {
        let dir = std::env::temp_dir().join(format!("gtui-comp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.gtui"), "{}").unwrap();
        std::fs::write(dir.join("ab.dot"), "{}").unwrap();
        std::fs::write(dir.join("c.txt"), "").unwrap();
        std::fs::write(dir.join(".hidden.gtui"), "{}").unwrap();
        let d = dir.to_str().unwrap();
        let pre = format!("{d}/");
        // Directories first with a trailing `/`; the extension filter drops c.txt;
        // hidden files need an explicit dot.
        assert_eq!(
            complete_path(&pre, &["gtui", "dot"]),
            vec![
                format!("{d}/sub/"),
                format!("{d}/a.gtui"),
                format!("{d}/ab.dot")
            ]
        );
        // Prefix filtering refills the whole head, directory part included.
        assert_eq!(
            complete_path(&format!("{pre}ab"), &[]),
            vec![format!("{d}/ab.dot")]
        );
        // Without a filter, .txt is included too.
        assert!(complete_path(&pre, &[]).contains(&format!("{d}/c.txt")));
        // Hidden files are listed only after typing a dot.
        assert_eq!(
            complete_path(&format!("{pre}."), &["gtui"]),
            vec![format!("{d}/.hidden.gtui")]
        );
        // Missing directory: empty result.
        assert!(complete_path(&format!("{d}/nope/x"), &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_home_paths_only() {
        assert_eq!(
            expand_with("~/a/b.gtui", Some("/home/m")),
            "/home/m/a/b.gtui"
        );
        assert_eq!(expand_with("~", Some("/home/m")), "/home/m");
        assert_eq!(expand_with("  ~/x  ", Some("/home/m")), "/home/m/x");
        // These are returned unchanged.
        assert_eq!(expand_with("~user/x", Some("/home/m")), "~user/x");
        assert_eq!(expand_with("a/~/b", Some("/home/m")), "a/~/b");
        assert_eq!(expand_with("/abs/x", Some("/home/m")), "/abs/x");
        assert_eq!(expand_with("~/x", None), "~/x");
        assert_eq!(expand_with("", Some("/home/m")), "");
    }
}
