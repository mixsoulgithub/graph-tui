//! `gf` / Enter target handling: parse a link target string into `LinkTarget`, then hand it to a
//! fixed set of ordered handlers that produce an `OpenPlan`.
//!
//! The layering keeps plugins possible: rendering and hits store only the raw target string
//! (`FrameHit.links`) and parsing happens on open, so Markdown syntax, position syntax, the
//! opener and the editor never share a function.
//! Nothing here touches the terminal or mutates the document: `main` executes `Editor` /
//! `Opener` / `Spawn`, while `App` executes `Jump` itself (it switches tabs and moves the
//! cursor).

use std::path::{Path, PathBuf};

/// Exact position inside a target. The meaning of `a,b` depends on the target kind (see the
/// `Plan` comment)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub a: u32,
    pub b: u32,
}

/// A parsed target
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// `http(s)://`, `mailto:`: handed to the opener, no filesystem access
    Url(String),
    /// Local file; `a,b` in `position` is `line,column` (1-based)
    Local {
        path: String,
        position: Option<Position>,
    },
    /// Reserved word `self`: the current document
    SelfDoc { position: Option<Position> },
    /// Any `scheme://`: the open command comes from `[links]` config; the core does not know the
    /// protocol
    Custom {
        /// Lowercase scheme name (the config key)
        scheme: String,
        /// Raw target (passed as the command's last argument)
        raw: String,
        position: Option<Position>,
    },
    /// Not a link (should not happen; fallback only)
    Unknown(String),
}

/// What opening a target does
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenPlan {
    /// In-graph jump. `path` None = current document; `cell` None = just open/switch, leave the
    /// cursor alone. When the same file is already open, switch to it by path (App uses
    /// `tab_of_path`; the handler does not look at tabs)
    Jump {
        path: Option<PathBuf>,
        cell: Option<(i32, i32)>,
    },
    /// Local text with a position: suspend the terminal and open it at the position with
    /// `$VISUAL` / `$EDITOR`
    Editor {
        path: PathBuf,
        position: Option<Position>,
    },
    /// The old 2.0 path: a local file goes to the opener, whose `--which` decides edit / open
    Opener { raw: String },
    /// Start a background program (this is how a URL reaches the opener)
    Spawn { program: String, args: Vec<String> },
    /// Suspend the terminal, run it, then come back (custom schemes: possibly a reader/TUI)
    BlockingSpawn {
        program: String,
        args: Vec<String>,
        /// Append a line to the status line afterwards (e.g. "position not supported")
        note: Option<String>,
    },
    /// Say one line on the status line only
    Note(String),
}

/// Environment a handler needs (all read-only snapshots; a handler does nothing beyond the
/// filesystem)
pub struct OpenContext<'a> {
    /// Directory of the current document (`./` and `../` resolve against it)
    pub doc_dir: Option<&'a Path>,
    /// Process working directory (the base when there is no document path)
    pub cwd: &'a Path,
    /// Command used to open links (2.0's `opener_command()`)
    pub opener: &'a str,
    /// Custom schemes from `[links]`: lowercase scheme -> command
    pub links: &'a [(String, String)],
}

/// Handler for one target shape. Adding a plugin means inserting one entry into the table
pub trait TargetHandler {
    fn name(&self) -> &'static str;
    /// Claim this target; `None` = not mine, pass it to the next one
    fn plan(&self, ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan>;
}

/// Fixed order: `self` -> URL -> custom scheme -> local -> unknown.
/// A wrong order would treat `self:10,10` as a local file, so a test guards it
pub fn handlers() -> Vec<Box<dyn TargetHandler>> {
    vec![
        Box::new(SelfHandler),
        Box::new(UrlHandler),
        // Custom schemes must come before local paths, otherwise `foo://x` is treated as a path
        Box::new(CustomHandler),
        Box::new(LocalHandler),
    ]
}

/// Turn a target into a plan. An unclaimed target yields a note, never a panic
pub fn plan(ctx: &OpenContext, target: &LinkTarget) -> OpenPlan {
    for h in handlers() {
        if let Some(p) = h.plan(ctx, target) {
            return p;
        }
    }
    OpenPlan::Note(format!("cannot open {}", describe(target)))
}

/// Build a plan straight from the raw string (that is what the render layer stores)
pub fn plan_raw(ctx: &OpenContext, raw: &str) -> OpenPlan {
    plan(ctx, &parse(raw))
}

/// Short description of a target, for the status line
pub fn describe(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Url(u) => u.clone(),
        LinkTarget::Local { path, .. } => path.clone(),
        LinkTarget::SelfDoc { .. } => "self".to_string(),
        LinkTarget::Custom { raw, .. } => raw.clone(),
        LinkTarget::Unknown(s) => s.clone(),
    }
}

/// Parse a target. The position suffix is meaningful only for local files and `self`; a `:10,10`
/// inside a URL stays as-is
pub fn parse(raw: &str) -> LinkTarget {
    let t = raw.trim();
    if t.is_empty() {
        return LinkTarget::Unknown(t.to_string());
    }
    let lower = t.to_ascii_lowercase();
    if lower == "self" || lower.starts_with("self:") {
        let (_, position) = split_position(t);
        return LinkTarget::SelfDoc { position };
    }
    match crate::link::classify(t) {
        Some(crate::link::LinkKind::Url) => LinkTarget::Url(t.to_string()),
        Some(crate::link::LinkKind::SelfDoc) => {
            let (_, position) = split_position(t);
            LinkTarget::SelfDoc { position }
        }
        Some(crate::link::LinkKind::Custom) => {
            let scheme = crate::link::scheme_of(t)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            let (_, position) = split_position(t);
            LinkTarget::Custom {
                scheme,
                raw: t.to_string(),
                position,
            }
        }
        Some(crate::link::LinkKind::Local) => {
            // `file:///a.md:3,4`: the position follows the path, so strip it before
            // resolve_local
            let (path, position) = split_position(t);
            LinkTarget::Local { path, position }
        }
        // A bare `notes.md:3,4` is not a link (same as 2.0)
        None => LinkTarget::Unknown(t.to_string()),
    }
}

/// `^(.*):([0-9]+),([0-9]+)$` -> (head, position). A parse failure keeps the whole string as the
/// path
fn split_position(s: &str) -> (String, Option<Position>) {
    let Some((head, tail)) = s.rsplit_once(':') else {
        return (s.to_string(), None);
    };
    if head.is_empty() {
        return (s.to_string(), None);
    }
    let Some((a, b)) = tail.split_once(',') else {
        return (s.to_string(), None);
    };
    if a.is_empty()
        || b.is_empty()
        || !a.bytes().all(|c| c.is_ascii_digit())
        || !b.bytes().all(|c| c.is_ascii_digit())
    {
        return (s.to_string(), None);
    }
    // `1,2,3` and the like: another comma in the tail, so treat it as plain text
    if b.contains(',') {
        return (s.to_string(), None);
    }
    let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else {
        return (s.to_string(), None);
    };
    (head.to_string(), Some(Position { a, b }))
}

/// Reserved word `self`: the current document, no filesystem access
struct SelfHandler;

impl TargetHandler for SelfHandler {
    fn name(&self) -> &'static str {
        "self"
    }

    fn plan(&self, _ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan> {
        let LinkTarget::SelfDoc { position } = target else {
            return None;
        };
        Some(OpenPlan::Jump {
            path: None,
            // self coordinates are in-graph `x,y`, 0-based
            cell: position.map(|p| (p.a as i32, p.b as i32)),
        })
    }
}

/// `http(s)` / `mailto`: started asynchronously by the opener, terminal untouched
struct UrlHandler;

impl TargetHandler for UrlHandler {
    fn name(&self) -> &'static str {
        "url"
    }

    fn plan(&self, ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan> {
        let LinkTarget::Url(raw) = target else {
            return None;
        };
        Some(OpenPlan::Spawn {
            program: ctx.opener.to_string(),
            args: vec![raw.clone()],
        })
    }
}

/// Any `scheme://`: the command comes only from `[links]`; without config, say so -- never guess
/// and never fall back to the opener
struct CustomHandler;

impl TargetHandler for CustomHandler {
    fn name(&self) -> &'static str {
        "custom"
    }

    fn plan(&self, ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan> {
        let LinkTarget::Custom {
            scheme,
            raw,
            position,
        } = target
        else {
            return None;
        };
        let Some((_, cmd)) = ctx.links.iter().find(|(k, _)| k == scheme) else {
            return Some(OpenPlan::Note(format!("no handler for scheme {scheme}")));
        };
        // The value may carry options (`my-reader --wait`): the first word is the program.
        // The config parser rejects an empty command, so `program` is never empty here.
        let mut words = cmd.split_whitespace();
        let program = words.next().unwrap_or(cmd).to_string();
        let mut args: Vec<String> = words.map(str::to_string).collect();
        // The target is a separate argument and never enters a shell string
        args.push(raw.clone());
        Some(OpenPlan::BlockingSpawn {
            program,
            args,
            note: position.map(|p| {
                format!(
                    "{scheme}: position {},{} is not supported, opened the target only",
                    p.a, p.b
                )
            }),
        })
    }
}

/// Local file: a graph document jumps, text with a position goes to the editor, anything else
/// goes to the opener
struct LocalHandler;

impl TargetHandler for LocalHandler {
    fn name(&self) -> &'static str {
        "local"
    }

    fn plan(&self, ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan> {
        let LinkTarget::Local { path, position } = target else {
            return None;
        };
        let resolved = match crate::link::resolve_local(path, ctx.doc_dir, ctx.cwd) {
            Ok(p) => p,
            Err(e) => return Some(OpenPlan::Note(e)),
        };
        let exists = resolved.exists();
        if !exists {
            return Some(OpenPlan::Note(format!(
                "no such file: {}",
                resolved.display()
            )));
        }
        if crate::link::is_graph_doc(&resolved) {
            // `.dot` is an import format without stable graph coordinates: ignore the position
            // and just open the tab (same as 2.0)
            let gtui = resolved.extension().is_some_and(|e| e == "gtui");
            return Some(OpenPlan::Jump {
                path: Some(resolved),
                // Graph document positions are `x,y`, 0-based
                cell: position.filter(|_| gtui).map(|p| (p.a as i32, p.b as i32)),
            });
        }
        if let Some(pos) = position.as_ref() {
            return Some(OpenPlan::Editor {
                path: resolved,
                position: Some(*pos),
            });
        }
        Some(OpenPlan::Opener {
            raw: resolved.display().to_string(),
        })
    }
}

/// Editor arguments: a pure function over the position, easy to unit test
///
/// nvim / vim / vi use `+call cursor(line, col)` to set row and column in one go;
/// other editors only get the line number (their column syntax differs), and the caller
/// explains that the column does not apply
pub fn editor_args(program: &str, path: &Path, position: Position) -> (Vec<String>, bool) {
    // The command string may carry arguments (`code -w`): only the first word matters
    let first = program.split_whitespace().next().unwrap_or(program);
    let name = Path::new(first)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(first)
        .to_ascii_lowercase();
    let vim_like = matches!(
        name.as_str(),
        "nvim" | "vim" | "vi" | "view" | "gvim" | "nvim-qt"
    );
    let mut args = Vec::new();
    if vim_like {
        args.push(format!(
            "+call cursor({},{})",
            position.a.max(1),
            position.b.max(1)
        ));
        args.push(path.display().to_string());
        (args, true)
    } else {
        args.push(format!("+{}", position.a.max(1)));
        args.push(path.display().to_string());
        (args, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(dir: &'a Path, cwd: &'a Path, opener: &'a str) -> OpenContext<'a> {
        ctx_links(dir, cwd, opener, &[])
    }

    fn ctx_links<'a>(
        dir: &'a Path,
        cwd: &'a Path,
        opener: &'a str,
        links: &'a [(String, String)],
    ) -> OpenContext<'a> {
        OpenContext {
            doc_dir: Some(dir),
            cwd,
            opener,
            links,
        }
    }

    #[test]
    fn position_suffix_parsing() {
        assert_eq!(
            parse("./a.md:10,10"),
            LinkTarget::Local {
                path: "./a.md".into(),
                position: Some(Position { a: 10, b: 10 })
            }
        );
        assert_eq!(
            parse("self:10,10"),
            LinkTarget::SelfDoc {
                position: Some(Position { a: 10, b: 10 })
            }
        );
        assert_eq!(parse("self"), LinkTarget::SelfDoc { position: None });
        assert_eq!(
            parse("./a.gtui:10,10"),
            LinkTarget::Local {
                path: "./a.gtui".into(),
                position: Some(Position { a: 10, b: 10 })
            }
        );
        // A colon and commas in a URL are not a position
        assert_eq!(
            parse("https://host/a:10,10"),
            LinkTarget::Url("https://host/a:10,10".into())
        );
        assert_eq!(
            parse("mailto:a@b:10,10"),
            LinkTarget::Url("mailto:a@b:10,10".into())
        );
        // A custom scheme records the position, but v1 does not apply it
        assert_eq!(
            parse("custom://x:10,10"),
            LinkTarget::Custom {
                scheme: "custom".into(),
                raw: "custom://x:10,10".into(),
                position: Some(Position { a: 10, b: 10 })
            }
        );
        // Tails that do not look like a position stay in the path verbatim
        for raw in ["~/a:10", "~/a:10,", "~/a:,10", "~/a:10,10,10"] {
            let t = parse(raw);
            assert!(
                matches!(&t, LinkTarget::Local { path, position: None } if path == raw),
                "{raw} 不该被拆: {t:?}"
            );
        }
        // Negative numbers / overflow: the whole string becomes the path
        for raw in ["~/a:-1,2", "~/a:99999999999999,1"] {
            let t = parse(raw);
            assert!(
                matches!(&t, LinkTarget::Local { path, position: None } if path == raw),
                "{raw} 不该被拆: {t:?}"
            );
        }
    }

    #[test]
    fn non_link_falls_back_to_unknown() {
        assert_eq!(
            parse("notes.md:3,4"),
            LinkTarget::Unknown("notes.md:3,4".into())
        );
        assert_eq!(
            parse("javascript:alert(1)"),
            LinkTarget::Unknown("javascript:alert(1)".into())
        );
        assert!(matches!(
            plan(
                &ctx(Path::new("/w"), Path::new("/w"), "opener"),
                &parse("notes.md")
            ),
            OpenPlan::Note(_)
        ));
    }

    #[test]
    fn self_wins_over_local_file() {
        // `self` is not a file: even if a file named self exists in the working directory it
        // must not be treated as a path
        let p = plan(
            &ctx(Path::new("/w"), Path::new("/w"), "opener"),
            &parse("self:3,4"),
        );
        assert_eq!(
            p,
            OpenPlan::Jump {
                path: None,
                cell: Some((3, 4))
            }
        );
    }

    #[test]
    fn url_goes_to_spawn() {
        let p = plan(
            &ctx(Path::new("/w"), Path::new("/w"), "my-opener"),
            &parse("https://example.com/a:1,2"),
        );
        assert_eq!(
            p,
            OpenPlan::Spawn {
                program: "my-opener".into(),
                args: vec!["https://example.com/a:1,2".into()]
            }
        );
    }

    /// Any `scheme://` becomes a link, but it only runs when a command is configured
    #[test]
    fn custom_scheme_needs_config() {
        let dir = Path::new("/w");
        // Not configured: only report that there is no handler; do not call the opener or start
        // a process
        let p = plan(&ctx(dir, dir, "opener"), &parse("foo://x"));
        assert_eq!(p, OpenPlan::Note("no handler for scheme foo".to_string()));
        // Configured: command + target as a separate argument
        let links = vec![("foo".to_string(), "/bin/echo".to_string())];
        let c = ctx_links(dir, dir, "opener", &links);
        assert_eq!(
            plan(&c, &parse("foo://x/y")),
            OpenPlan::BlockingSpawn {
                program: "/bin/echo".into(),
                args: vec!["foo://x/y".into()],
                note: None
            }
        );
        // Scheme matching is case-insensitive; lookup uses the normalised lowercase form
        assert_eq!(
            plan(&c, &parse("FOO://x/y")),
            OpenPlan::BlockingSpawn {
                program: "/bin/echo".into(),
                args: vec!["FOO://x/y".into()],
                note: None
            }
        );
        // target is a separate argument: shell metacharacters are never spliced in
        let p = plan(&c, &parse("foo://x; rm -rf /"));
        match p {
            OpenPlan::BlockingSpawn { args, .. } => {
                assert_eq!(args, vec!["foo://x; rm -rf /".to_string()]);
            }
            other => panic!("该是 BlockingSpawn:{other:?}"),
        }
        // With a position: v1 opens anyway but says the position was not applied
        let p = plan(&c, &parse("foo://x:3,4"));
        assert!(
            matches!(p, OpenPlan::BlockingSpawn { note: Some(ref n), .. } if n.contains("not supported")),
            "{p:?}"
        );
    }

    /// The `[links]` value may carry options; the target still goes last
    #[test]
    fn custom_scheme_command_takes_arguments() {
        let dir = Path::new("/w");
        let links = vec![(
            "custom".to_string(),
            "my-reader --wait --flag=x".to_string(),
        )];
        let c = ctx_links(dir, dir, "opener", &links);
        assert_eq!(
            plan(&c, &parse("custom://a/b")),
            OpenPlan::BlockingSpawn {
                program: "my-reader".into(),
                args: vec!["--wait".into(), "--flag=x".into(), "custom://a/b".into()],
                note: None
            }
        );
        // Extra blanks around the command are ignored
        let links = vec![("custom".to_string(), "  /bin/echo   ".to_string())];
        let c = ctx_links(dir, dir, "opener", &links);
        assert_eq!(
            plan(&c, &parse("custom://x")),
            OpenPlan::BlockingSpawn {
                program: "/bin/echo".into(),
                args: vec!["custom://x".into()],
                note: None
            }
        );
    }

    #[test]
    fn custom_scheme_parsing() {
        assert_eq!(
            parse("foo://x"),
            LinkTarget::Custom {
                scheme: "foo".into(),
                raw: "foo://x".into(),
                position: None
            }
        );
        // Uppercase is normalised to lowercase, raw is kept as-is
        assert_eq!(
            parse("FOO://x"),
            LinkTarget::Custom {
                scheme: "foo".into(),
                raw: "FOO://x".into(),
                position: None
            }
        );
        // A scheme without `//` is still not a link
        assert!(matches!(
            parse("javascript:alert(1)"),
            LinkTarget::Unknown(_)
        ));
        assert!(matches!(parse("data:text/plain,x"), LinkTarget::Unknown(_)));
    }

    #[test]
    fn local_text_without_position_uses_opener() {
        let dir = std::env::temp_dir().join("graph-tui-target-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("plain.md");
        std::fs::write(&f, "x\n").unwrap();
        let raw = f.display().to_string();
        let p = plan(&ctx(&dir, &dir, "opener"), &parse(&raw));
        assert_eq!(p, OpenPlan::Opener { raw: raw.clone() });
        // With a position it goes to the editor
        let p = plan(&ctx(&dir, &dir, "opener"), &parse(&format!("{raw}:7,3")));
        assert_eq!(
            p,
            OpenPlan::Editor {
                path: f.clone(),
                position: Some(Position { a: 7, b: 3 })
            }
        );
    }

    #[test]
    fn graph_doc_with_position_jumps() {
        let dir = std::env::temp_dir().join("graph-tui-target-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("g.gtui");
        std::fs::write(&f, "{}").unwrap();
        let p = plan(
            &ctx(&dir, &dir, "opener"),
            &parse(&format!("{}:5,6", f.display())),
        );
        assert_eq!(
            p,
            OpenPlan::Jump {
                path: Some(f.clone()),
                cell: Some((5, 6))
            }
        );
        // An out-of-range position is just a number and is not blocked (the canvas is
        // unbounded)
        let p = plan(
            &ctx(&dir, &dir, "opener"),
            &parse(&format!("{}:99999,99999", f.display())),
        );
        assert!(matches!(
            p,
            OpenPlan::Jump {
                cell: Some((99999, 99999)),
                ..
            }
        ));
        // A missing file only produces a note
        let p = plan(&ctx(&dir, &dir, "opener"), &parse("./nope.gtui:1,2"));
        assert!(matches!(p, OpenPlan::Note(m) if m.contains("no such file")));
        // `.dot` opens the tab only and ignores graph coordinates
        let dot = dir.join("d.dot");
        std::fs::write(&dot, "digraph{}").unwrap();
        let p = plan(
            &ctx(&dir, &dir, "opener"),
            &parse(&format!("{}:3,4", dot.display())),
        );
        assert_eq!(
            p,
            OpenPlan::Jump {
                path: Some(dot),
                cell: None
            }
        );
        // `.gtui` without a position: open/switch the tab only, leave the cursor alone
        let p = plan(&ctx(&dir, &dir, "opener"), &parse(&f.display().to_string()));
        assert_eq!(
            p,
            OpenPlan::Jump {
                path: Some(f),
                cell: None
            }
        );
    }

    #[test]
    fn editor_args_table() {
        let p = Path::new("/w/a.rs");
        assert_eq!(
            editor_args("nvim", p, Position { a: 10, b: 4 }),
            (
                vec!["+call cursor(10,4)".to_string(), "/w/a.rs".to_string()],
                true
            )
        );
        assert_eq!(
            editor_args("/usr/bin/VIM", p, Position { a: 3, b: 0 }),
            (
                vec!["+call cursor(3,1)".to_string(), "/w/a.rs".to_string()],
                true
            ),
            "列 0 抬到 1(光标语义)"
        );
        assert_eq!(
            editor_args("emacs", p, Position { a: 10, b: 4 }),
            (vec!["+10".to_string(), "/w/a.rs".to_string()], false),
            "别的编辑器只给行号"
        );
        assert_eq!(
            editor_args("code -w", p, Position { a: 10, b: 4 }),
            (vec!["+10".to_string(), "/w/a.rs".to_string()], false),
            "命令串带参数时按第一个词判断"
        );
    }
}
