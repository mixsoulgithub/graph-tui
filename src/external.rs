//! External editor bridge (`Ctrl+G`): write the current input text to a temp file,
//! hand it to `$VISUAL` / `$EDITOR`, and read the file back once the editor exits.
//!
//! No "wait for the user to type :wq" convention: editors have too many exit paths
//! (`:wq` / `:x` / `ZZ` / closing a GUI window).
//! Only reading the file after exit and comparing it with the initial content is
//! reliable. `q!` leaves the file untouched, so nothing gets injected.
//!
//! Pure functions: nothing here touches the terminal (`main` handles suspend/resume),
//! so this can run under `cargo test` with a fake editor.

use std::io::Write;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

/// What the editor run concluded about the input buffer
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Content unchanged (`:q!`, plain quit, GUI editor that did not block): cancel
    Unchanged,
    /// New content (trailing CRs and exactly one trailing newline already stripped)
    Changed(String),
    /// The editor exited with a non-zero code
    Failed(i32),
    /// The editor removed the temp file
    NoFile,
    /// Not valid UTF-8
    BadEncoding,
    /// Contains `\r` or NUL: rejected for both single-line and multi-line input
    BadText,
    /// Platform unsupported (Windows has no `sh`, so launching an editor is off)
    Unsupported,
}

/// Which editor to use: `$VISUAL` -> `$EDITOR` -> `vi`.
///
/// An empty string counts as unset; `true` / `false` also count as unset -- some tools
/// use them as placeholders so the child process does not hang in an editor, and
/// using one as the real editor would mean nothing ever happens.
pub fn editor_command() -> String {
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if !v.is_empty() && !matches!(v, "true" | "false" | ":") {
                return v.to_string();
            }
        }
    }
    "vi".to_string()
}

/// Temp file: only clean once removed
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Create a fresh 0600 temp file holding `initial` plus one newline (editors like
/// files to end with a newline).
///
/// The sequence number comes from a process-wide monotonic atomic: a name is never
/// reused after Drop removed the file (under parallel unit tests, reuse would race
/// with another call that just created the same name).
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

/// Read back: CRLF -> LF, strip exactly one trailing newline; any remaining `\r` /
/// NUL makes bad text
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

/// Run the editor once. `cmd` may carry arguments (`code -w`): the whole string goes
/// to `sh -c`, with no word splitting of our own.
/// Non-Unix platforms have no `sh`, so return `Unsupported` instead of a confusing
/// failure.
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
        // The editor must block: hand it the path and wait for it to exit
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

/// Run `$VISUAL` / `$EDITOR` once
pub fn edit(initial: &str) -> Outcome {
    edit_with(&editor_command(), initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These cases use shell snippets directly as the "editor": `edit_with` wraps them as
    // `<snippet> "$1"`. Writing no script file means only sh runs, so parallel tests do
    // not hit `Text file busy`.

    #[test]
    fn unchanged_exit_is_not_a_change() {
        assert_eq!(edit_with("true; #", "原文"), Outcome::Unchanged);
    }

    #[test]
    fn appended_lines_are_read_back() {
        assert_eq!(
            edit_with(r#"printf 'a\nb\n' >"#, "旧"),
            Outcome::Changed("a\nb".to_string()),
            "尾换行只去一个"
        );
    }

    #[test]
    fn strips_one_trailing_newline_keeps_inner_blanks() {
        assert_eq!(
            edit_with(r#"printf 'x\n\n\ny\n' >"#, "旧"),
            Outcome::Changed("x\n\n\ny".to_string())
        );
    }

    #[test]
    fn crlf_becomes_lf() {
        assert_eq!(
            edit_with(r#"printf 'a\r\nb\r\n' >"#, "旧"),
            Outcome::Changed("a\nb".to_string())
        );
    }

    #[test]
    fn nonzero_exit_is_failure() {
        assert_eq!(edit_with("exit 3; #", "原"), Outcome::Failed(3));
    }

    #[test]
    fn deleted_file_is_cancel() {
        assert_eq!(edit_with("rm -f", "原"), Outcome::NoFile);
    }

    #[test]
    fn rejects_bad_text_and_bad_encoding() {
        assert_eq!(edit_with(r#"printf 'a\rb' >"#, "原"), Outcome::BadText);
        assert_eq!(
            edit_with(r#"printf '\377\376' >"#, "原"),
            Outcome::BadEncoding
        );
    }

    #[test]
    fn missing_editor_is_failure() {
        assert_eq!(edit_with("/nonexistent/editor", "原"), Outcome::Failed(127));
    }

    #[test]
    fn editor_may_take_arguments() {
        // An `$EDITOR` with arguments (`code -w`) must still work: the whole string goes to
        // sh -c, with no word splitting of our own
        assert!(matches!(
            edit_with("true -x; #", "原"),
            Outcome::Unchanged | Outcome::Failed(_)
        ));
    }
}
