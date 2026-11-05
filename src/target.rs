//! `gf` / Enter 的目标处理:把链接目标字符串解析成 `LinkTarget`,再交给一组按固定
//! 顺序排好的 handler 变成 `OpenPlan`.
//!
//! 分层是为了以后能加插件:渲染和命中只存原始目标字符串(`FrameHit.links`),打开时
//! 才解析,所以 Markdown 语法、位置语法、opener、编辑器都不在同一个函数里.
//! 这里不碰终端,也不改文档:`main` 执行 `Editor` / `Opener` / `Spawn`,`App` 自己
//! 执行 `Jump`(它要切 tab、动光标).

use std::path::{Path, PathBuf};

/// 目标里的精确位置.`a,b` 的含义由目标种类决定(见 `Plan` 的注释)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub a: u32,
    pub b: u32,
}

/// 解析出来的目标
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// `http(s)://`、`mailto:`:交给 opener,不碰文件系统
    Url(String),
    /// 本地文件;`position` 里的 `a,b` 是 `line,column`(1-based)
    Local {
        path: String,
        position: Option<Position>,
    },
    /// 保留字 `self`:就是当前文档
    SelfDoc { position: Option<Position> },
    /// 任意 `scheme://`:打开命令由 `[links]` 配置决定,核心不认识具体协议
    Custom {
        /// 小写的 scheme 名(配置里用的 key)
        scheme: String,
        /// 原始 target(作为命令的最后一个参数)
        raw: String,
        position: Option<Position>,
    },
    /// 不是链接(不该走到这儿,兜底用)
    Unknown(String),
}

/// 打开一个目标要做的事
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenPlan {
    /// 图内跳转.`path` 为 None = 当前文档;`cell` 为 None = 只开/切过去,不动光标.
    /// 同一个文件已经开着时按路径切过去(App 用 `tab_of_path` 算,handler 不查 tab)
    Jump {
        path: Option<PathBuf>,
        cell: Option<(i32, i32)>,
    },
    /// 带位置的本地文本:挂起终端,用 `$VISUAL` / `$EDITOR` 定位打开
    Editor {
        path: PathBuf,
        position: Option<Position>,
    },
    /// 20 的老路:本地文件交给 opener,由它 `--which` 决定 edit / open
    Opener { raw: String },
    /// 直接起一个后台程序(URL 交给 opener 就是这样)
    Spawn { program: String, args: Vec<String> },
    /// 挂起终端跑完再回来(自定义 scheme:可能是阅读器/TUI)
    BlockingSpawn {
        program: String,
        args: Vec<String>,
        /// 跑完在状态行补一句(比如"位置不支持")
        note: Option<String>,
    },
    /// 只在状态行说一句
    Note(String),
}

/// handler 需要的环境(全是只读快照,handler 不做文件系统之外的事情)
pub struct OpenContext<'a> {
    /// 当前文档所在目录(`./`、`../` 相对它)
    pub doc_dir: Option<&'a Path>,
    /// 进程工作目录(没有文档路径时的基准)
    pub cwd: &'a Path,
    /// 打开链接用的命令(20 的 `opener_command()`)
    pub opener: &'a str,
    /// `[links]` 里的自定义 scheme:小写 scheme -> 命令
    pub links: &'a [(String, String)],
}

/// 一个目标形态的处理器.以后加插件就是往表里插一个
pub trait TargetHandler {
    fn name(&self) -> &'static str;
    /// 认领这个目标;`None` = 不归我管,交给下一个
    fn plan(&self, ctx: &OpenContext, target: &LinkTarget) -> Option<OpenPlan>;
}

/// 固定顺序:`self` -> URL -> 自定义 scheme -> 本地 -> unknown.
/// 顺序错了 `self:10,10` 会被当成本地文件,所以有测试盯着
pub fn handlers() -> Vec<Box<dyn TargetHandler>> {
    vec![
        Box::new(SelfHandler),
        Box::new(UrlHandler),
        // 自定义 scheme 要在本地路径前面,否则 `foo://x` 会被当路径
        Box::new(CustomHandler),
        Box::new(LocalHandler),
    ]
}

/// 把目标变成计划.没人认领就提示,不 panic
pub fn plan(ctx: &OpenContext, target: &LinkTarget) -> OpenPlan {
    for h in handlers() {
        if let Some(p) = h.plan(ctx, target) {
            return p;
        }
    }
    OpenPlan::Note(format!("cannot open {}", describe(target)))
}

/// 从原始字符串直接做计划(渲染层存的就是这个字符串)
pub fn plan_raw(ctx: &OpenContext, raw: &str) -> OpenPlan {
    plan(ctx, &parse(raw))
}

/// 目标的简短描述,给状态行用
pub fn describe(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Url(u) => u.clone(),
        LinkTarget::Local { path, .. } => path.clone(),
        LinkTarget::SelfDoc { .. } => "self".to_string(),
        LinkTarget::Custom { raw, .. } => raw.clone(),
        LinkTarget::Unknown(s) => s.clone(),
    }
}

/// 解析目标.位置后缀只在本地文件和 `self` 上有意义;URL 里出现 `:10,10` 保持原样
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
            // `file:///a.md:3,4`:位置在路径之后,切掉再交给 resolve_local
            let (path, position) = split_position(t);
            LinkTarget::Local { path, position }
        }
        // `notes.md:3,4` 这种没前缀的不当链接(和 20 一致)
        None => LinkTarget::Unknown(t.to_string()),
    }
}

/// `^(.*):([0-9]+),([0-9]+)$` -> (前半, 位置).解析失败就把整串当路径
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
    // `1,2,3` 这种:tail 里还有逗号,按普通文字
    if b.contains(',') {
        return (s.to_string(), None);
    }
    let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else {
        return (s.to_string(), None);
    };
    (head.to_string(), Some(Position { a, b }))
}

/// 保留字 `self`:当前文档,不查文件系统
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
            // self 的坐标是图内 `x,y`,0-based
            cell: position.map(|p| (p.a as i32, p.b as i32)),
        })
    }
}

/// `http(s)` / `mailto`:交给 opener 异步起,不动终端
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

/// 任意 `scheme://`:命令只从 `[links]` 来;没配置就只说一句,不猜也不调 opener
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
        Some(OpenPlan::BlockingSpawn {
            program: cmd.clone(),
            // target 是独立参数,永远不进 shell 字符串
            args: vec![raw.clone()],
            note: position.map(|p| {
                format!(
                    "{scheme}: position {},{} is not supported, opened the target only",
                    p.a, p.b
                )
            }),
        })
    }
}

/// 本地文件:图档跳进去,文本带位置进编辑器,其余交给 opener
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
            // `.dot` 是导入格式,没有稳定的图坐标:位置忽略,只开 tab(和 20 一致)
            let gtui = resolved.extension().is_some_and(|e| e == "gtui");
            return Some(OpenPlan::Jump {
                path: Some(resolved),
                // 图档的位置是 `x,y`,0-based
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

/// 编辑器参数:带位置的纯函数,方便单测
///
/// nvim / vim / vi 用 `+call cursor(line, col)` 一次把行列都放好;
/// 别的编辑器只给行号(它们的列语法各不相同),调用方负责说明列没生效
pub fn editor_args(program: &str, path: &Path, position: Position) -> (Vec<String>, bool) {
    // 命令串可能带参数(`code -w`):只看第一个词
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
    fn 位置后缀解析() {
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
        // URL 里的冒号逗号不是位置
        assert_eq!(
            parse("https://host/a:10,10"),
            LinkTarget::Url("https://host/a:10,10".into())
        );
        assert_eq!(
            parse("mailto:a@b:10,10"),
            LinkTarget::Url("mailto:a@b:10,10".into())
        );
        // 自定义 scheme 记下位置,但 v1 不应用
        assert_eq!(
            parse("custom://x:10,10"),
            LinkTarget::Custom {
                scheme: "custom".into(),
                raw: "custom://x:10,10".into(),
                position: Some(Position { a: 10, b: 10 })
            }
        );
        // 不像位置的尾巴原样留在路径里
        for raw in ["~/a:10", "~/a:10,", "~/a:,10", "~/a:10,10,10"] {
            let t = parse(raw);
            assert!(
                matches!(&t, LinkTarget::Local { path, position: None } if path == raw),
                "{raw} 不该被拆: {t:?}"
            );
        }
        // 负数 / 溢出:整串当路径
        for raw in ["~/a:-1,2", "~/a:99999999999999,1"] {
            let t = parse(raw);
            assert!(
                matches!(&t, LinkTarget::Local { path, position: None } if path == raw),
                "{raw} 不该被拆: {t:?}"
            );
        }
    }

    #[test]
    fn 非链接落到unknown() {
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
    fn self排在本地文件前面() {
        // `self` 不是文件,即使工作目录里真有个叫 self 的文件也不该被当路径
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
    fn url走spawn() {
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

    /// 任意 `scheme://` 都成链接,但只有配置过命令才执行
    #[test]
    fn 自定义scheme只认配置() {
        let dir = Path::new("/w");
        // 没配置:只说没有 handler,不调 opener、不起进程
        let p = plan(&ctx(dir, dir, "opener"), &parse("foo://x"));
        assert_eq!(p, OpenPlan::Note("no handler for scheme foo".to_string()));
        // 配置过:命令 + target 作为独立参数
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
        // scheme 大小写不敏感,查表用规范化后的小写
        assert_eq!(
            plan(&c, &parse("FOO://x/y")),
            OpenPlan::BlockingSpawn {
                program: "/bin/echo".into(),
                args: vec!["FOO://x/y".into()],
                note: None
            }
        );
        // target 是独立参数:带 shell 元字符也不会被拼接
        let p = plan(&c, &parse("foo://x; rm -rf /"));
        match p {
            OpenPlan::BlockingSpawn { args, .. } => {
                assert_eq!(args, vec!["foo://x; rm -rf /".to_string()]);
            }
            other => panic!("该是 BlockingSpawn:{other:?}"),
        }
        // 带位置:v1 照样打开,但说一句位置没生效
        let p = plan(&c, &parse("foo://x:3,4"));
        assert!(
            matches!(p, OpenPlan::BlockingSpawn { note: Some(ref n), .. } if n.contains("not supported")),
            "{p:?}"
        );
    }

    #[test]
    fn 自定义scheme解析() {
        assert_eq!(
            parse("foo://x"),
            LinkTarget::Custom {
                scheme: "foo".into(),
                raw: "foo://x".into(),
                position: None
            }
        );
        // 大写规范化成小写,raw 保留原样
        assert_eq!(
            parse("FOO://x"),
            LinkTarget::Custom {
                scheme: "foo".into(),
                raw: "FOO://x".into(),
                position: None
            }
        );
        // 不带 `//` 的 scheme 仍然不是链接
        assert!(matches!(
            parse("javascript:alert(1)"),
            LinkTarget::Unknown(_)
        ));
        assert!(matches!(parse("data:text/plain,x"), LinkTarget::Unknown(_)));
    }

    #[test]
    fn 本地文本没有位置仍走opener() {
        let dir = std::env::temp_dir().join("graph-tui-target-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("plain.md");
        std::fs::write(&f, "x\n").unwrap();
        let raw = f.display().to_string();
        let p = plan(&ctx(&dir, &dir, "opener"), &parse(&raw));
        assert_eq!(p, OpenPlan::Opener { raw: raw.clone() });
        // 带位置就是编辑器
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
    fn 图档带位置是jump() {
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
        // 位置越界只是数字,不拦(画布是无限大)
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
        // 不存在的文件只提示
        let p = plan(&ctx(&dir, &dir, "opener"), &parse("./nope.gtui:1,2"));
        assert!(matches!(p, OpenPlan::Note(m) if m.contains("no such file")));
        // `.dot` 只开 tab,不吃图坐标
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
        // `.gtui` 不带位置:只开/切 tab,不动光标
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
    fn 编辑器参数() {
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
