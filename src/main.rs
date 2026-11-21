//! graph-tui:终端里的通用图渲染引擎.
//!
//! 本文件只负责命令行、终端进出与主循环;图数据在 model.rs,解析在 dot.rs,
//! 布局在 layout.rs,画法在 canvas.rs + render.rs,交互在 app.rs.

use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use graph_tui::app::{App, EdgeLabelsMode, Flow, Options};
use graph_tui::document::{Document, ViewState};
use graph_tui::dump::Area;
use graph_tui::lang::Lang;
use graph_tui::layout::Layout;
use graph_tui::model::ArrowStyle;

/// 画面的输出格式:真彩 ANSI 或纯文本
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DumpFormat {
    Ansi,
    Text,
}

impl DumpFormat {
    fn parse(s: &str) -> Option<DumpFormat> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ansi" | "color" | "colour" => Some(DumpFormat::Ansi),
            "text" | "plain" => Some(DumpFormat::Text),
            _ => None,
        }
    }
}

struct Args {
    /// 位置参数:一个或多个文件(多个 = 多个 tab;--export/headless 只收一个)
    files: Vec<String>,
    layout: Option<Layout>,
    /// --area:出图矩形,每段都能 auto(不给就是全 auto)
    area: Area,
    /// --format:画面的输出格式(ansi / text);None = 按输出去向自动挑
    format: Option<DumpFormat>,
    node_labels: bool,
    /// --no-node-labels:画面/导出里不画点节点名字
    no_node_labels: bool,
    /// --no-edge-labels:不画边标签
    no_edge_labels: bool,
    /// 光标画法:real(终端真光标)/ drawn(画在 buffer 里)
    cursor: bool,
    /// 撤销步数上限
    history: Option<usize>,
    /// 箭头字形风格
    arrow: Option<ArrowStyle>,
    /// --ascii:纯 ASCII 字形(缺制表符字形的终端用)
    ascii: bool,
    /// 布局间距:同层列距 / 层间行距 / 外边距
    lane_gap: Option<f64>,
    layer_gap: Option<f64>,
    layout_margin: Option<f64>,
    at: Option<String>,
    find: Option<String>,
    lang: Lang,
    /// --lang 显式给过吗(只有显式给了非默认语言才盖过文件里的视图状态)
    lang_set: bool,
    /// --config <path>:显式配置文件(不存在就报错)
    config: Option<String>,
    /// --no-config:完全不读配置文件
    no_config: bool,
    /// --print-config:打印完整默认配置后退出
    print_config: bool,
    /// --init-config [path]:写一份默认配置(已存在要 --force)
    init_config: Option<Option<String>>,
    /// --force:配合 --init-config 覆盖已有文件
    force: bool,
    /// 导出一张图就退出(.svg / .canvas)
    export: Option<String>,
    help: bool,
    version: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            files: Vec::new(),
            layout: None,
            area: Area::auto(),
            format: None,
            node_labels: false,
            no_node_labels: false,
            no_edge_labels: false,
            cursor: true,
            history: None,
            arrow: None,
            ascii: false,
            lane_gap: None,
            layer_gap: None,
            layout_margin: None,
            at: None,
            find: None,
            lang: Lang::default(),
            lang_set: false,
            config: None,
            no_config: false,
            print_config: false,
            init_config: None,
            force: false,
            export: None,
            help: false,
            version: false,
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        let mut next = |what: &str| -> Result<String, String> {
            i += 1;
            argv.get(i)
                .cloned()
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match arg {
            "-h" | "--help" => a.help = true,
            "-v" | "--version" => a.version = true,
            "--node-labels" => a.node_labels = true,
            "--no-node-labels" => a.no_node_labels = true,
            "--no-edge-labels" => a.no_edge_labels = true,
            "--drawn-cursor" => a.cursor = false,
            "--file" => {
                let p = next("--file")?;
                a.files.push(p);
            }
            "--at" => a.at = Some(next("--at")?),
            "--find" => a.find = Some(next("--find")?),
            "--layout" => {
                let v = next("--layout")?;
                a.layout = Some(Layout::parse(&v).ok_or_else(|| format!("unknown layout {v}"))?);
            }
            "--lang" => {
                let v = next("--lang")?;
                a.lang = Lang::parse(&v).ok_or_else(|| format!("unknown language {v}"))?;
                a.lang_set = true;
            }
            "--export" => a.export = Some(next("--export")?),
            "--config" => a.config = Some(next("--config")?),
            "--no-config" => a.no_config = true,
            "--print-config" => a.print_config = true,
            "--force" => a.force = true,
            // 值可省:下一个参数不是 flag 就当路径(先 peek,再让循环正常前进)
            "--init-config" => {
                let val = argv.get(i + 1).filter(|s| !s.starts_with('-')).cloned();
                if val.is_some() {
                    i += 1;
                }
                a.init_config = Some(val);
            }
            "--area" => a.area = parse_area(&next("--area")?)?,
            "--format" => {
                let v = next("--format")?;
                a.format = Some(
                    DumpFormat::parse(&v).ok_or_else(|| format!("bad --format {v} (text|ansi)"))?,
                );
            }
            "--arrow" => {
                let v = next("--arrow")?;
                a.arrow = Some(
                    ArrowStyle::parse(&v)
                        .ok_or_else(|| format!("bad arrow style {v} (ascii|arrows|triangles)"))?,
                );
            }
            "--ascii" => a.ascii = true,
            "--unicode" => a.ascii = false,
            "--lane-gap" | "--layer-gap" | "--layout-margin" => {
                let flag = arg;
                let v = next(flag)?;
                let n: f64 = v
                    .parse()
                    .map_err(|_| format!("bad {flag} {v} (a number of cells)"))?;
                if !n.is_finite() {
                    return Err(format!("bad {flag} {v}"));
                }
                match flag {
                    "--lane-gap" => a.lane_gap = Some(n),
                    "--layer-gap" => a.layer_gap = Some(n),
                    _ => a.layout_margin = Some(n),
                }
            }
            "--history" => {
                let v = next("--history")?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("bad history size {v} (a positive integer)"))?;
                if n == 0 {
                    return Err("bad history size 0".to_string());
                }
                a.history = Some(n);
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}")),
            s => a.files.push(s.to_string()),
        }
        i += 1;
    }
    Ok(a)
}

/// `--area x,y,w,h`:四段每段都能写 auto,单独一个 `auto` 等于四段全 auto
fn parse_area(s: &str) -> Result<Area, String> {
    if s.eq_ignore_ascii_case("auto") {
        return Ok(Area::auto());
    }
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!(
            "--area wants x,y,w,h (each may be `auto`), got {s}"
        ));
    }
    let num = |k: usize, what: &str| -> Result<Option<i64>, String> {
        let v = parts[k];
        if v.eq_ignore_ascii_case("auto") {
            return Ok(None);
        }
        v.parse::<i64>()
            .map(Some)
            .map_err(|_| format!("bad {what} in --area {s}"))
    };
    let (x, y) = (num(0, "x")?, num(1, "y")?);
    let (w, h) = (num(2, "width")?, num(3, "height")?);
    let x = x
        .map(|v| i32::try_from(v).map_err(|_| format!("x out of range in --area {s}")))
        .transpose()?;
    let y = y
        .map(|v| i32::try_from(v).map_err(|_| format!("y out of range in --area {s}")))
        .transpose()?;
    let w = w
        .map(|v| u16::try_from(v).map_err(|_| format!("width out of range in --area {s}")))
        .transpose()?;
    let h = h
        .map(|v| u16::try_from(v).map_err(|_| format!("height out of range in --area {s}")))
        .transpose()?;
    for (we, size) in [("width", w), ("height", h)] {
        if let Some(v) = size {
            if v < 8 && we == "width" || v < 4 && we == "height" {
                return Err(format!("--area {we} too small in {s} (at least 8x4)"));
            }
        }
    }
    Ok(Area { x, y, w, h })
}

/// 读图:文件、stdin("-") 或者没有参数时从 stdin 读.
/// 什么都没给、stdin 又是终端:返回 None,调用方开一份空白缓冲(不落盘).
/// 和 nvim 一样,还不存在的 `.gtui` 是"新文件":空文档 + 这个路径,`:w` 才落盘;
/// `.dot` / `.canvas` 是导入源,缺了照旧报错.`--export` 需要真内容,也照旧报错.
fn source_text(a: &Args) -> Result<Option<(String, Option<String>, bool)>, String> {
    match a.files.first().map(String::as_str) {
        Some(p) if p != "-" => {
            if p.to_ascii_lowercase().ends_with(".gtui")
                && !std::path::Path::new(p).exists()
                && a.export.is_none()
            {
                return Ok(Some((String::new(), Some(p.to_string()), true)));
            }
            let text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?;
            // 空文件(刚 touch 出来 / 上次保存失败留下的 0 字节)当成新文件:
            // `nvim empty.gtui` 也是给一个空 buffer,而不是报"JSON 解析失败"
            if text.trim().is_empty() {
                return Ok(Some((String::new(), Some(p.to_string()), true)));
            }
            Ok(Some((text, Some(p.to_string()), false)))
        }
        Some(_) => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            if text.trim().is_empty() {
                return Err("no input on stdin".to_string());
            }
            Ok(Some((text, None, false)))
        }
        None => {
            if !io::stdin().is_terminal() {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| format!("cannot read stdin: {e}"))?;
                if text.trim().is_empty() {
                    return Err("no input on stdin".to_string());
                }
                return Ok(Some((text, None, false)));
            }
            Ok(None)
        }
    }
}

/// 整幅图导成一张图,然后退出. `.svg` 是自包含的矢量图(浏览器/编辑器都能开),
/// `.canvas` 是 JSON Canvas(Obsidian 的板子).
fn export(app: &mut App, path: &str, labels: bool, area: Area) -> Result<(), String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".gtui") {
        app.save_document(path)?;
        println!("wrote {path}");
        return Ok(());
    }
    if lower.ends_with(".canvas") {
        let (body, warnings) = graph_tui::canvas_io::export(&app.doc);
        write_text(path, &body)?;
        for w in warnings {
            eprintln!("warning: {w}");
        }
        println!("wrote {path}");
        return Ok(());
    }
    if lower.ends_with(".dot") {
        // best-effort 视图:不是源文档,失真处逐条报到 stderr
        let (body, warnings) = graph_tui::dot_write::export(&app.doc)?;
        write_text(path, &body)?;
        for w in warnings {
            eprintln!("warning: {w}");
        }
        println!("wrote {path}");
        return Ok(());
    }
    let body = {
        // 和 stdout 那条路同一套成帧:只画图、1:1、居中、留白.字形跟 App 走(--ascii)
        let view = graph_tui::app::View {
            glyphs: app.view.glyphs,
            ..Default::default()
        };
        let frame = graph_tui::dump::frame(&app.doc, labels, area, &view);
        graph_tui::svg::of_buffer(&frame.buf)
    };
    if lower.ends_with(".png") {
        graph_tui::svg::write_png(&body, path)?;
    } else {
        let tmp = format!("{path}.tmp");
        std::fs::write(&tmp, &body).map_err(|e| format!("cannot write {tmp}: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    println!("wrote {path}");
    Ok(())
}

/// 先写 .tmp 再改名
fn write_text(path: &str, body: &str) -> Result<(), String> {
    graph_tui::paths::ensure_parent(path)?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("cannot write {tmp}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {path}: {e}"))
}

/// `--help` 的文本:来自 docs/usage.md,固定英文(`--lang` 只管 TUI 文案)
fn help_text() -> &'static str {
    graph_tui::help::text()
}

/// 画面的输出格式:显式给了就用它,否则按去向/环境挑
fn choose_format(
    explicit: Option<DumpFormat>,
    stdout_is_tty: bool,
    no_color: bool,
    term_dumb: bool,
) -> DumpFormat {
    if let Some(f) = explicit {
        return f;
    }
    if !stdout_is_tty || no_color || term_dumb {
        DumpFormat::Text
    } else {
        DumpFormat::Ansi
    }
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut a = parse_args(&argv)?;
    // 路径统一展开开头的 `~`(动词里手敲的路径同样走 paths::expand)
    for p in a.files.iter_mut() {
        *p = graph_tui::paths::expand(p);
    }
    if let Some(p) = a.export.as_mut() {
        *p = graph_tui::paths::expand(p);
    }
    if a.help {
        println!("{}", help_text());
        return Ok(());
    }
    if a.version {
        println!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if a.print_config {
        print!("{}", graph_tui::keys::default_text());
        return Ok(());
    }
    if let Some(path) = &a.init_config {
        let path = path
            .clone()
            .unwrap_or_else(graph_tui::keys::default_config_path);
        let path = graph_tui::paths::expand(&path);
        let target = std::path::Path::new(&path);
        if target.exists() && !a.force {
            return Err(format!("{path} already exists (use --force to overwrite)"));
        }
        graph_tui::paths::ensure_parent(&path)?;
        std::fs::write(&path, graph_tui::keys::default_text())
            .map_err(|e| format!("cannot write {path}: {e}"))?;
        println!("{path}");
        return Ok(());
    }
    // 配置:显式 > 环境变量 > XDG 默认;--no-config 全跳过
    let cfg = if a.no_config {
        None
    } else {
        graph_tui::keys::load_config(a.config.as_deref())?
    };
    // 没有文件、stdin 又是终端:开一份空白文档(只在内存里,不落盘;存盘要自己给路径)
    let (doc, mut opts_extra, fresh) = match source_text(&a)? {
        // 新文件:还没有内容,直接开一份空白文档(来源路径已经记好,:w 就写到那儿)
        Some((_, source, true)) => (Document::scratch(), source, true),
        Some((text, source, false)) => {
            // 按扩展名认格式:.gtui 是 native 文档,.canvas 是 JSON Canvas,其余都当 .dot 导入
            let ext = source
                .as_deref()
                .map(|p| p.rsplit('.').next().unwrap_or("").to_ascii_lowercase())
                .unwrap_or_default();
            let source_name = match &ext[..] {
                "gtui" => "graph-tui".to_string(),
                "canvas" => "json-canvas".to_string(),
                _ => "dot".to_string(),
            };
            let doc = match &ext[..] {
                "gtui" => Document::load(&text)?,
                "canvas" => graph_tui::canvas_io::import(&text)?,
                _ => graph_tui::dot::parse_document(&text, ViewState::default(), &source_name)?,
            };
            (doc, source, false)
        }
        None => (Document::scratch(), None, false),
    };
    let mut opts = Options::default();
    prefs_into(&mut opts, cfg.as_ref());
    if let Some(l) = a.layout {
        opts.layout = Some(l);
    }
    if a.node_labels {
        opts.node_labels = true;
    }
    if a.no_edge_labels {
        opts.edge_labels = EdgeLabelsMode::Off;
    }
    // a.cursor = 用终端真光标;drawn 是反过来那个
    opts.drawn = !a.cursor;
    if let Some(v) = a.at.clone() {
        opts.at = Some(v);
    }
    if let Some(v) = a.find.clone() {
        opts.find = Some(v);
    }
    if a.lang_set {
        opts.lang = a.lang;
    }
    if let Some(n) = a.history {
        opts.history = Some(n);
    }
    if a.ascii {
        opts.glyphs = graph_tui::glyphs::Glyphs::ASCII;
        // ASCII 下没有 ↑↓←→ / ▲▼◀▶,箭头一律回落到 ^ v < >
        if a.arrow.is_some_and(|f| f != ArrowStyle::Ascii) {
            eprintln!("warning: --ascii forces ascii arrow glyphs");
            a.arrow = Some(ArrowStyle::Ascii);
        }
    }
    if a.lane_gap.is_some() || a.layer_gap.is_some() || a.layout_margin.is_some() {
        opts.spacing = graph_tui::layout::Spacing::of(a.lane_gap, a.layer_gap, a.layout_margin);
    }
    if let Some(v) = a.arrow {
        opts.arrow = Some(v);
    }
    // 多文件只给交互模式:--export 只认一个输入,别猜
    if a.files.len() > 1 && a.export.is_some() {
        return Err(format!(
            "--export takes one input file, got {}",
            a.files.len()
        ));
    }
    let mut app = App::new(doc, opts)?;
    // 新文件:状态行说一句,免得以为打开失败了
    if fresh {
        if let Some(p) = opts_extra.as_deref() {
            let (zh, en) = (
                format!("新文件 {p}(:w 写入)"),
                format!("new file {p} (:w to write it)"),
            );
            app.view.note = Some(app.view.lang.pick_owned(zh, en));
        }
    }
    app.set_source(opts_extra.take());
    // 其余文件各开一个 tab,最后回到第一个
    if a.files.len() > 1 {
        let files = a.files.clone();
        for p in files.iter().skip(1) {
            app.new_tab(Some(p));
        }
        app.switch_tab(0);
    }

    if let Some(path) = &a.export {
        if path == "-" {
            // `--export -` = 画面到 stdout(和管道里自动出图是同一条路)
            return picture(&app, &a);
        }
        // 导出没有终端光标:退回画在画面里的那种
        app.view.drawn = true;
        export(&mut app, path, !a.no_node_labels, a.area)?;
        return Ok(());
    }

    // stdout 不是终端(管道/文件)时不出交互界面,直接给一帧
    if !io::stdout().is_terminal() {
        return picture(&app, &a);
    }

    // 会话提示只在交互路径上挑:dump / 导出 / headless 保持确定性
    app.seed_tip();
    interactive(app)
}

/// 只画一帧到 stdout:1:1、居中、留白,没有状态行、没有光标.
/// 格式按去向挑(`--format` 显式覆盖),`--area` / `--node-labels` / `--no-node-labels` /
/// `--no-edge-labels` 都生效
fn picture(app: &App, a: &Args) -> Result<(), String> {
    let stdout_tty = io::stdout().is_terminal();
    let view = app.view.clone();
    let frame = graph_tui::dump::frame(&app.doc, !a.no_node_labels, a.area, &view);
    if frame.clipped {
        eprintln!("warning: the picture does not fit the requested size");
    }
    let skipped = frame.hit.labels_skipped.len();
    if skipped > 0 {
        // 名字放不下不会画到图上:stderr 说一声,免得以为是数据少了
        eprintln!(
            "warning: {skipped} label(s) did not fit (use a bigger --area or --no-node-labels)"
        );
    }
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let term_dumb = std::env::var("TERM").is_ok_and(|v| v.eq_ignore_ascii_case("dumb"));
    let format = choose_format(a.format, stdout_tty, no_color, term_dumb);
    let text = match format {
        DumpFormat::Ansi => graph_tui::render::ansi(&frame.buf),
        DumpFormat::Text => {
            let mut t = String::new();
            for line in graph_tui::render::plain_lines(&frame.buf) {
                t.push_str(&line);
                t.push('\n');
            }
            t
        }
    };
    let mut out = io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
    Ok(())
}

/// 配置里的偏好盖到默认值上(命令行开关随后再盖)
fn prefs_into(opts: &mut Options, cfg: Option<&graph_tui::keys::Config>) {
    let Some(cfg) = cfg else { return };
    let p = &cfg.prefs;
    if let Some(v) = p.arrow {
        opts.arrow = Some(v);
    }
    if let Some(n) = p.history {
        opts.history = Some(n);
    }
    if let Some(l) = p.lang {
        opts.lang = l;
    }
    if let Some(d) = p.drawn {
        opts.drawn = d;
    }
    if let Some(m) = p.mouse {
        opts.mouse = m;
    }
    if let Some(m) = p.mouse_edges {
        opts.mouse_edges = m;
    }
    if let Some(g) = p.render {
        opts.glyphs = g;
    }
    if let Some(c) = p.edge_color {
        opts.edge_ink = c;
    }
    if let Some(c) = p.text_color {
        opts.text_ink = c;
    }
    if let Some(o) = p.opener.clone() {
        opts.opener = Some(o);
    }
    opts.links = cfg.links.clone();
    for (k, act) in &cfg.keys {
        opts.keys.bind(k.clone(), *act);
    }
}

/// 进终端跑主循环(交互模式;画面/导出在这之前就返回了)
fn interactive(mut app: App) -> Result<(), String> {
    let mut out = io::stdout();
    enable_raw_mode().map_err(|e| format!("cannot enter raw mode: {e}"))?;
    execute!(out, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| format!("cannot enter the alternate screen: {e}"))?;
    let res = event_loop(&mut app, out);
    let mut out = io::stdout();
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture);
    res
}

/// `graph-tui tutor`:打开内嵌教程.它是一份 scratch 文档,退出不落盘;
/// 重跑命令就是重置(`--lang` 选语言,否则用配置里的,再否则中文)
fn run_tutor(argv: &[String]) -> Result<(), String> {
    let a = parse_args(argv)?;
    if a.help {
        println!("{}", help_text());
        return Ok(());
    }
    if !a.files.is_empty() {
        return Err(
            "tutor opens the built-in tutorial and takes no file (drop the path, or open the file without `tutor`)"
                .into(),
        );
    }
    if a.export.is_some() {
        return Err("tutor is interactive: --export does not apply".into());
    }
    let cfg = if a.no_config {
        None
    } else {
        graph_tui::keys::load_config(a.config.as_deref())?
    };
    let doc = graph_tui::tutor::document()?;
    let mut opts = Options::default();
    prefs_into(&mut opts, cfg.as_ref());
    // 教程内容是英文;`--lang` 仍然只管程序自己的界面文案
    if a.lang_set {
        opts.lang = a.lang;
    }
    opts.drawn = !a.cursor;
    if a.ascii {
        opts.glyphs = graph_tui::glyphs::Glyphs::ASCII;
    }
    // 教程里点节点的名字都要标出来(练习区那个 point 就是给看的)
    opts.node_labels = true;
    let mut app = App::new(doc, opts)?;
    app.set_source(None);
    // 首帧:横向居中到章节列,纵向落在欢迎框(只按光标居中会在宽终端上把框切掉一截)
    let (x0, _, x1, _) = app.doc.bbox();
    app.view.center = Some(((x0 + x1) as f64 / 2.0, app.view.cursor.1 as f64));
    app.view.target = None;
    app.view.note = Some(graph_tui::tutor::hint().to_string());
    app.seed_tip();
    interactive(app)
}

fn event_loop(app: &mut App, out: io::Stdout) -> Result<(), String> {
    let mut term = Terminal::new(CrosstermBackend::new(out))
        .map_err(|e| format!("cannot start the terminal: {e}"))?;
    // 只在画面可能变了的时候重画:鼠标移动(1003 模式)每秒能来上千个事件,
    // 每个都重画一遍会把终端写满,拖着看就是"卡"
    let mut dirty = true;
    loop {
        if dirty {
            app.tick();
            term.draw(|f| {
                let area = f.area();
                app.draw(f.buffer_mut(), area);
                // 真光标:位置由 App 按光标框/输入行算;算不出来就交给 ratatui 藏起来
                if let Some(at) = app.cursor_cell() {
                    f.set_cursor_position(at);
                }
            })
            .map_err(|e| format!("cannot draw: {e}"))?;
            dirty = false;
        }
        // 没有动画在跑就少醒几次
        let wait = if app.busy() { 16 } else { 250 };
        if !event::poll(Duration::from_millis(wait))
            .map_err(|e| format!("cannot read events: {e}"))?
        {
            // 超时:呼吸换了一档、缓动真的挪了才重画,否则继续睡;
            // 顺手看一眼源文件有没有被别的程序改掉(一次 stat)
            if app.check_disk() {
                dirty = true;
            }
            if app.busy() && app.tick() {
                dirty = true;
            }
            continue;
        }
        // 把已经排队的输入一次收干净(拖动/滚轮会连着来一串),只重画一次.
        // 一批最多 64 个:一直在来事件的时候也要保证能画到屏幕上
        let mut batch = 0;
        while batch < 64
            && event::poll(Duration::ZERO).map_err(|e| format!("cannot read events: {e}"))?
        {
            batch += 1;
            let ev = event::read().map_err(|e| format!("cannot read events: {e}"))?;
            match &ev {
                Event::Resize(..) => dirty = true,
                // 鼠标只是挪了位置:没开菜单时什么都不改,不值得重画
                Event::Mouse(me)
                    if me.kind == crossterm::event::MouseEventKind::Moved
                        && app.view.menu.is_none() => {}
                _ => {
                    if app.handle(ev) == Flow::Quit {
                        return Ok(());
                    }
                    dirty = true;
                }
            }
        }
        // Ctrl+G:把输入行交给外部编辑器。终端挂起/恢复都在这里做,App 只给意图
        if app.edit_pending() {
            run_external(app)?;
            // 离开过 alt screen:ratatui 的旧缓冲已经和屏幕对不上了,
            // 换一个新的 Terminal(空缓冲)下一帧就会整屏重画
            term = Terminal::new(CrosstermBackend::new(io::stdout()))
                .map_err(|e| format!("cannot restart the terminal: {e}"))?;
            dirty = true;
        }
        // Enter / gf:打开光标下的链接。同样:App 只给计划,终端与进程都在这里
        if let Some(plan) = app.take_open_request() {
            match run_plan(app, plan) {
                Ok(true) => {
                    // 挂起过:换一个 Terminal,下一帧整屏重画
                    term = Terminal::new(CrosstermBackend::new(io::stdout()))
                        .map_err(|e| format!("cannot restart the terminal: {e}"))?;
                }
                Ok(false) => {}
                Err(e) => app.note_error(e),
            }
            dirty = true;
        }
    }
}

/// 让出终端:光标显示出来、退出 raw 模式与备用屏.失败也不报错(下面恢复时会再试)
fn suspend_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture);
    let _ = out.flush();
}

/// 拿回终端:raw 模式、备用屏、鼠标捕获,并清一屏(旧内容已经不在了)
fn resume_terminal() -> Result<(), String> {
    let mut out = io::stdout();
    enable_raw_mode().map_err(|e| format!("cannot re-enter raw mode: {e}"))?;
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )
    .map_err(|e| format!("cannot restore the screen: {e}"))?;
    Ok(())
}

/// 跑一次外部编辑器:先退出 raw + alt screen,跑完(无论成败)恢复并整屏重画
fn run_external(app: &mut App) -> Result<(), String> {
    suspend_terminal();
    let initial = app.edit_initial().unwrap_or_default();
    let outcome = graph_tui::external::edit(&initial);
    resume_terminal()?;
    app.apply_edit_result(outcome);
    Ok(())
}

/// 执行一个打开计划.
///
/// 返回值:`Ok(true)` 表示终端被挂起过(调用方要重建 Terminal 整屏重画),
/// `Ok(false)` 表示只是 spawn 了一下,画面没离开过备用屏.
fn run_plan(app: &mut App, plan: graph_tui::target::OpenPlan) -> Result<bool, String> {
    use graph_tui::target::OpenPlan;
    match plan {
        // Jump 在 App 里就做完了,走不到这里
        OpenPlan::Jump { .. } => Ok(false),
        OpenPlan::Note(msg) => {
            app.note_error(msg);
            Ok(false)
        }
        OpenPlan::Spawn { program, args } => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            spawn_opener(&program, &refs)?;
            Ok(false)
        }
        OpenPlan::BlockingSpawn {
            program,
            args,
            note,
        } => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_blocking(&program, &refs)?;
            if let Some(note) = note {
                app.note_error(note);
            }
            Ok(true)
        }
        OpenPlan::Opener { raw } => run_opener(app, &raw, graph_tui::link::LinkKind::Local),
        OpenPlan::Editor { path, position } => run_editor(&path, position),
    }
}

/// 自定义 scheme:挂起终端跑完再回来(可能是阅读器 / TUI).
///
/// 命令与参数分开传,不经过 shell;退出码非 0 也算失败
fn run_blocking(program: &str, args: &[&str]) -> Result<(), String> {
    suspend_terminal();
    let status = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    let restore = resume_terminal();
    restore?;
    let status = status.map_err(|e| format!("cannot run {program}: {e}"))?;
    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    Ok(())
}

/// 带位置的本地文本:挂起终端,让编辑器停在那一行/列.
///
/// 命令串可以带参数(`code -w`),所以交给 `sh -c`,不做自己的分词;
/// 位置参数由 `target::editor_args` 拼(纯函数,单测覆盖)
fn run_editor(
    path: &std::path::Path,
    position: Option<graph_tui::target::Position>,
) -> Result<bool, String> {
    let cmd = graph_tui::external::editor_command();
    let (args, column_ok) = match position {
        Some(p) => graph_tui::target::editor_args(&cmd, path, p),
        None => (vec![path.display().to_string()], true),
    };
    suspend_terminal();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} \"$@\""))
        .arg("sh")
        .args(&args)
        .status();
    let restore = resume_terminal();
    restore?;
    status.map_err(|e| format!("cannot run {cmd}: {e}"))?;
    if let (Some(p), false) = (position, column_ok) {
        // 不是 vim 系列:列号没有通用写法,只把行号给了它
        return Err(format!(
            "{}: line {} only, this editor has no column argument",
            cmd, p.a
        ));
    }
    Ok(true)
}

/// 打开一个链接.
///
/// 返回值:`Ok(true)` 表示终端被挂起过(调用方要重建 Terminal 整屏重画),
/// `Ok(false)` 表示只是 spawn 了一下,画面没离开过备用屏.
///
/// - URL:spawn opener 就完事,不挂起(浏览器/GUI 自己 detach).
/// - `.gtui` / `.dot`:开成新标签,不启动任何进程.
/// - 本地其它文件:opener 是 `opener` 时先问 `--which`:
///   `edit` 走挂起(编辑器接管终端),`open` 走 spawn(图片/PDF 不打断画面).
fn run_opener(app: &mut App, raw: &str, kind: graph_tui::link::LinkKind) -> Result<bool, String> {
    use graph_tui::link::{is_graph_doc, resolve_local, LinkKind};
    let opener = app.opener_command();
    match kind {
        LinkKind::Url => {
            spawn_opener(&opener, &[raw])?;
            Ok(false)
        }
        // 自定义 scheme 与 self: 由 target 的 handler 拦下,不会走到这里
        LinkKind::Custom => Err("custom schemes are resolved in the target handler".to_string()),
        LinkKind::SelfDoc => Err("self links are resolved in the app".to_string()),
        LinkKind::Local => {
            let cwd = app.cwd_path();
            let doc_dir = app.doc_dir();
            let path = resolve_local(raw, doc_dir.as_deref(), &cwd)?;
            if !path.exists() {
                return Err(format!("no such file: {}", path.display()));
            }
            if is_graph_doc(&path) {
                app.open_path(&path);
                return Ok(false);
            }
            let abs = path.display().to_string();
            if opener_is_helper(&opener) {
                match opener_which(&opener, &abs)? {
                    Some(true) => {
                        // 编辑器:挂起终端让它接管
                        suspend_terminal();
                        let res = spawn_opener_wait(&opener, &["--", &abs]);
                        let restore = resume_terminal();
                        restore?;
                        res?;
                        Ok(true)
                    }
                    Some(false) | None => {
                        spawn_opener(&opener, &["--", &abs])?;
                        Ok(false)
                    }
                }
            } else {
                spawn_opener(&opener, &[&abs])?;
                Ok(false)
            }
        }
    }
}

/// `opener --which -- <path>`:返回 Some(true) = edit, Some(false) = open, None = 说不清
fn opener_which(opener: &str, path: &str) -> Result<Option<bool>, String> {
    let out = std::process::Command::new(opener)
        .arg("--which")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|e| format!("cannot run {opener}: {e}"))?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    match text.lines().next().map(str::trim) {
        Some("edit") => Ok(Some(true)),
        Some("open") => Ok(Some(false)),
        _ => Ok(None),
    }
}

fn opener_is_helper(opener: &str) -> bool {
    std::path::Path::new(opener)
        .file_name()
        .map(|n| n == "opener")
        .unwrap_or(false)
}

fn spawn_opener(opener: &str, args: &[&str]) -> Result<(), String> {
    std::process::Command::new(opener)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot run {opener}: {e}"))
}

fn spawn_opener_wait(opener: &str, args: &[&str]) -> Result<(), String> {
    std::process::Command::new(opener)
        .args(args)
        .status()
        .map(|_| ())
        .map_err(|e| format!("cannot run {opener}: {e}"))
}

fn main() -> ExitCode {
    // `headless` 子命令:不碰终端,退出码自己定
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(|a| a.as_str()) == Some("headless") {
        let cfg = match graph_tui::headless::parse_args(&argv[1..]) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("graph-tui headless: {e}");
                return ExitCode::from(2);
            }
        };
        let code = graph_tui::headless::run(&cfg);
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    if argv.first().map(|a| a.as_str()) == Some("tutor") {
        return match run_tutor(&argv[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("graph-tui tutor: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("graph-tui: {e}");
            eprintln!("try graph-tui --help");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "graph-tui-cli-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 画面的默认格式:显式赢,其次看去向与环境
    #[test]
    fn dump_默认格式() {
        use DumpFormat::{Ansi, Text};
        // 显式最优先(哪怕在管道里要 ANSI、在终端里要纯文本)
        assert_eq!(choose_format(Some(Ansi), false, true, true), Ansi);
        assert_eq!(choose_format(Some(Text), true, false, false), Text);
        // 管道/文件:纯文本
        assert_eq!(choose_format(None, false, false, false), Text);
        // 终端:NO_COLOR / TERM=dumb 退纯文本,否则真彩
        assert_eq!(choose_format(None, true, true, false), Text);
        assert_eq!(choose_format(None, true, false, true), Text);
        assert_eq!(choose_format(None, true, false, false), Ansi);
    }

    /// 和 nvim 一样:不存在的 .gtui 是新文件,别的缺文件照旧报错,导出/转储要真内容
    #[test]
    fn 缺gtui当新文件() {
        let dir = temp_dir("newfile");
        let gtui = dir.join("fresh.gtui");
        let dot = dir.join("fresh.dot");
        let path = |p: &std::path::Path| p.to_string_lossy().into_owned();

        let a = Args {
            files: vec![path(&gtui)],
            ..Args::default()
        };
        let (_, source, fresh) = source_text(&a).expect("新文件").expect("有来源");
        assert!(fresh, "标记成新文件");
        assert_eq!(source.as_deref(), Some(path(&gtui).as_str()));

        // --export(含 `-`)需要真内容
        for export in [Some("-".to_string()), Some(format!("{}.svg", path(&gtui)))] {
            let a = Args {
                files: vec![path(&gtui)],
                export,
                ..Args::default()
            };
            assert!(source_text(&a).is_err(), "缺文件不能出画面/导出");
        }

        // .dot 是导入源,不能凭空创建
        let a = Args {
            files: vec![path(&dot)],
            ..Args::default()
        };
        assert!(source_text(&a).is_err(), "缺 .dot 仍然报错");

        // 真的存在就正常读
        std::fs::write(&gtui, Document::scratch().save()).expect("写盘");
        let a = Args {
            files: vec![path(&gtui)],
            ..Args::default()
        };
        let (text, _, fresh) = source_text(&a).expect("读得回来").expect("有来源");
        assert!(!fresh);
        assert!(!text.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
