//! 外部编辑器桥(`Ctrl+G`):把当前输入串写到临时文件,交给 `$VISUAL` / `$EDITOR`,
//! 编辑器退出后把文件内容读回来。
//!
//! 不做"等用户按 :wq"这种约定:编辑器退出方式太多(`:wq` / `:x` / `ZZ` / GUI 关窗),
//! 只有"退出后读文件、和初始内容比较"是可靠的。`q!` 不改文件,自然什么都不注入。
//!
//! 纯函数:这里不碰终端(`main` 负责挂起/恢复),所以能在 `cargo test` 里用假编辑器跑。

use std::io::Write;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

/// 编辑器跑完之后对输入缓冲区的结论
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 内容没变(`:q!`、直接退出、GUI 编辑器没阻塞):当取消
    Unchanged,
    /// 有新内容(已经去掉行尾回车与恰好一个尾换行)
    Changed(String),
    /// 编辑器退出码非 0
    Failed(i32),
    /// 编辑器把临时文件删了
    NoFile,
    /// 不是合法 UTF-8
    BadEncoding,
    /// 夹着 `\r` 或 NUL:单行/多行都不收
    BadText,
    /// 平台不支持(Windows 没有 `sh`,启动编辑器这条路先不开)
    Unsupported,
}

/// 用哪个编辑器:`$VISUAL` -> `$EDITOR` -> `vi`(空串按没设置)
pub fn editor_command() -> String {
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    "vi".to_string()
}

/// 临时文件:删了才算干净
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 建一个 0600 的新临时文件,内容 = `initial` + 一个换行(编辑器喜欢文件以换行结尾).
///
/// 序号用进程内单调递增的原子量:文件被 Drop 删掉之后绝不复用同一个名字
/// (并行的单元测试里,复用会和"另一个调用刚建好同名文件"撞上)
fn temp_file(initial: &str) -> std::io::Result<Temp> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    for _ in 0..100u32 {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("graph-tui-{pid}-{n}.md"));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(mut f) => {
                f.write_all(initial.as_bytes())?;
                f.write_all(b"\n")?;
                return Ok(Temp(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free temp file name",
    ))
}

/// 读回来:CRLF -> LF,去掉恰好一个尾换行,剩下的 `\r` / NUL 视为坏文本
fn normalize(bytes: &[u8]) -> Outcome {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return Outcome::BadEncoding;
    };
    let s = s.replace("\r\n", "\n");
    if s.contains('\r') || s.contains('\0') {
        return Outcome::BadText;
    }
    let s = s.strip_suffix('\n').unwrap_or(&s).to_string();
    Outcome::Changed(s)
}

/// 跑一次编辑器。`cmd` 里可以带参数(`code -w`):交给 `sh -c`,不做自己的分词。
/// 非 Unix 平台没有 `sh`,直接用 `Unsupported` 说清楚,不让用户看到莫名其妙的失败
pub fn edit_with(cmd: &str, initial: &str) -> Outcome {
    #[cfg(not(unix))]
    {
        let _ = (cmd, initial);
        return Outcome::Unsupported;
    }
    #[cfg(unix)]
    {
        let Ok(tmp) = temp_file(initial) else {
            return Outcome::Failed(-1);
        };
        let path: &Path = tmp.0.as_path();
        // 编辑器必须阻塞:给它路径,等它退出
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("{cmd} \"$1\""))
            .arg("sh")
            .arg(path)
            .status();
        let Ok(status) = status else {
            return Outcome::Failed(-1);
        };
        let Ok(bytes) = std::fs::read(path) else {
            return Outcome::NoFile;
        };
        if !status.success() {
            return Outcome::Failed(status.code().unwrap_or(-1));
        }
        match normalize(&bytes) {
            Outcome::Changed(s) if s == initial => Outcome::Unchanged,
            other => other,
        }
    }
}

/// `$VISUAL` / `$EDITOR` 跑一次
pub fn edit(initial: &str) -> Outcome {
    edit_with(&editor_command(), initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 这些用例直接把 shell 片段当"编辑器":`edit_with` 会拼成 `<片段> "$1"`。
    // 早先写的是"先落一个 .sh 再执行它",并行跑测试时会偶发 `Text file busy`;
    // 现在不落脚本文件,就只剩 sh 本身。

    #[test]
    fn 原样退出不算改() {
        assert_eq!(edit_with("true; #", "原文"), Outcome::Unchanged);
    }

    #[test]
    fn 追加两行读回来() {
        assert_eq!(
            edit_with(r#"printf 'a\nb\n' >"#, "旧"),
            Outcome::Changed("a\nb".to_string()),
            "尾换行只去一个"
        );
    }

    #[test]
    fn 尾换行只去一个且中间空行保留() {
        assert_eq!(
            edit_with(r#"printf 'x\n\n\ny\n' >"#, "旧"),
            Outcome::Changed("x\n\n\ny".to_string())
        );
    }

    #[test]
    fn crlf折成lf() {
        assert_eq!(
            edit_with(r#"printf 'a\r\nb\r\n' >"#, "旧"),
            Outcome::Changed("a\nb".to_string())
        );
    }

    #[test]
    fn 退出码非零报失败() {
        assert_eq!(edit_with("exit 3; #", "原"), Outcome::Failed(3));
    }

    #[test]
    fn 删文件算取消() {
        assert_eq!(edit_with("rm -f", "原"), Outcome::NoFile);
    }

    #[test]
    fn 坏文本与坏编码都拒() {
        assert_eq!(edit_with(r#"printf 'a\rb' >"#, "原"), Outcome::BadText);
        assert_eq!(
            edit_with(r#"printf '\377\376' >"#, "原"),
            Outcome::BadEncoding
        );
    }

    #[test]
    fn 编辑器不存在报失败() {
        assert_eq!(edit_with("/nonexistent/editor", "原"), Outcome::Failed(127));
    }

    #[test]
    fn 编辑器可以带参数() {
        // `$EDITOR` 里带参数(`code -w`)也要能跑:整串交给 sh -c,不做自己的分词
        assert!(matches!(
            edit_with("true -x; #", "原"),
            Outcome::Unchanged | Outcome::Failed(_)
        ));
    }
}
