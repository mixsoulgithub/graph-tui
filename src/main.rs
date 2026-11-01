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

use graph_tui::app::{App, Flow, LegendMode, NotesMode, Options};
use graph_tui::document::{Document, ViewState};
use graph_tui::lang::Lang;
use graph_tui::layout::Layout;
use graph_tui::model::ArrowStyle;

const USAGE: &str = "\
graph-tui - a universal graph renderer for the terminal
(box-drawing edges, one true colour per group, orthogonal routing, interactive pan/zoom)

usage: graph-tui [options] [file.gtui | file.dot | file.canvas]

With no file: if stdin is a pipe, the document is read from it; on a terminal you
get an empty scratch document (one box, never written to disk -- save it yourself
with `:w <path.gtui>`). `.gtui` is the native, editable document; `.dot` and
`.canvas` are imports (the first save writes a `.gtui` next to them).

options:
  --file <path>       read a graph .dot file (same as giving it as an argument; '-' = stdin)
  --layout <name>     geo | relax | force | layered. Default: geo when every node has a
                      position, otherwise layered for acyclic graphs and force otherwise
  --dump              draw the picture to stdout and exit: 1:1, centred, padded,
                      no status line and no cursor (the legend follows the document).
                      Size fits the content unless --size is given
  --export <path>     write the document out and exit: .gtui (native save), .svg
                      (self-contained picture), .png (rasterised by rsvg-convert),
                      .canvas (JSON Canvas) or .dot (best-effort view, lossy)
  --size <WxH|auto>   canvas size for --dump / --export (default auto: content + padding);
                      a smaller size clips instead of scaling
  --labels            start with every node name drawn (same as pressing b)
  --format <ansi|text>  output for --dump: true-colour ANSI (default) or plain text
  --no-labels         do not draw point-node names in --dump / --export
  --legend            force the legend panel on (it follows view.legend by default)
  --no-legend         force it off (the document decides unless one of these is given)
  --no-notes          do not draw edge notes
  --history <N>       undo steps to keep (default 200)
  --arrow <style>     arrow glyphs: ascii (^ v < >) | arrows (up/down/left/right) |
                      triangles (solid triangles). Default ascii
  --drawn-cursor      mark the cursor inside the picture instead of using the terminal cursor
                      (TUI only; --dump and --export never draw a cursor)
  --at <name>         start with the cursor on that node
  --find <text>       run a search at startup (name / alias / group number)
  --lang <zh|en>      language of the app's own text (default: zh). The help and errors
                      on this page stay English either way
  --config <path>     read this config file (default: $GRAPH_TUI_CONFIG or
                      $XDG_CONFIG_HOME/graph-tui/config.ini). A missing explicit file is an error
  --no-config         ignore every config file (built-in defaults)
  --print-config      print the full default config (prefs + keys) and exit
  --init-config [p]   write the default config (to [p] or the default path); needs --force to
                      overwrite
  --force             allow --init-config to overwrite an existing file
  -v, --version       print the version and exit
  -h, --help          show this help and exit

headless (no TTY, JSON in / JSON out):
  graph-tui headless --file a.gtui --op '{\"op\":\"info\"}'
  graph-tui headless --file a.gtui < requests.ndjson > responses.ndjson
  graph-tui headless --file a.gtui --batch ops.ndjson --save-on-exit [--partial]
      --file <path>      open .gtui / .dot / .canvas first
      --op <json>        run one request and exit
      --batch <path>     run one request per line (atomic unless --partial)
      --save-on-exit     write the document back to --file at the end
      --limit <n>        max requests per session (default 100000)

input (.dot subset):
  digraph deps {                          graph or digraph (digraph: edges get an arrowhead)
    node [box=single];                    default attributes for nodes / edges
    \"a\" [pos=\"0,0\", terms=\"alpha|ay\", box=\"rounded\", color=\"#4c8ef8\"];
    \"a\" -> \"b\" [group=\"deps\", color=\"#4c8ef8\", style=\"dashed\", via=\"10,4 20,4\"];
  }
  a .canvas file (JSON Canvas / Obsidian) is read by extension instead of .dot
  node: pos (x right / y up) | terms (search aliases) | box=point|none|single|rounded|double|heavy
        | color (border and text)          edge: group | color | style=solid|dashed|dotted
        | via (canvas points the line is forced through)
  labels take **bold** and /italic/ (same as markdown; the markers take no width)

layouts:
  geo      use the positions from the input as they are
  relax    spring-relax those positions so spacing evens out and runs snap to - | / \\\\
  force    force-directed, for graphs with no positions at all
  layered  layered (Sugiyama), for directed dependency graphs and flowcharts

keys:
  h j k l / arrows   move to the nearest node on screen in that direction
  H K / J L          step one node along the group (its own order, not geography)
  + - / 0            zoom in / out / back to 1:1 (the canvas is infinite: pan has no bounds)
  space z            centre the view on the cursor node
  f / F              fit the focused group / fit the whole graph
  b                  toggle node labels
  /                  search names or aliases; a bare number focuses that group
  n / N              next / previous search hit
  V                  rectangle select (same as v / Ctrl-q)
  r                  cycle the layout
  Ctrl-d / Ctrl-u    jump half a screen down / up
  Ctrl-o / Ctrl-i    back / forward through the nodes you jumped to
  ?                  key reference (any key closes it)
  v                  rectangle select from the cursor node (move the cursor to grow it, esc to drop)
  Ctrl-h j k l       nudge the selection (or the cursor node) by one screen cell
  L                  cycle the layer under the cursor (other layers are dimmed)
  e                  two-phase link: sources, then move to a target and press Enter
  E                  cycle the edge selection through the edges at the cursor node
  s / c              style menu / colour menu (Enter applies, esc cancels)
  :                  command line: box <style> | nodecolor <#rrggbb|auto> | style <solid|dashed
                     |dotted> | edgecolor <#rrggbb|auto> | via <x,y x,y|off> | text <rest of line>
                     | layer <name|all> | tolayer <name> | export <path.svg|png|canvas>
                     | export-canvas <path> | save [path] | layout <name> | side <x>
                     | cycle <x> | lang <zh|en>
  mouse              click = jump; drag on a node = move it, drag elsewhere = pan;
                     wheel = zoom, Ctrl+wheel = pan
  :q / :wq           quit / save and quit (q and esc do not quit)
";

/// `--dump` 的输出格式:真彩 ANSI 或纯文本
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
    file: Option<String>,
    layout: Option<Layout>,
    dump: bool,
    /// --size:不给(None)就按内容自动定尺寸;`--size auto` 也是 None
    size: Option<(u16, u16)>,
    /// --format:dump 的输出格式(ansi / text)
    format: DumpFormat,
    labels: bool,
    /// --no-labels:dump/导出里不画点节点名字
    no_labels: bool,
    /// --legend / --no-legend 显式给的开关
    legend: Option<LegendMode>,
    /// --no-notes:不画边注释
    no_notes: bool,
    /// 光标画法:real(终端真光标)/ drawn(画在 buffer 里)
    cursor: bool,
    /// 撤销步数上限
    history: Option<usize>,
    /// 箭头字形风格
    arrow: Option<ArrowStyle>,
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
            file: None,
            layout: None,
            dump: false,
            size: None,
            format: DumpFormat::Ansi,
            labels: false,
            no_labels: false,
            legend: None,
            no_notes: false,
            cursor: true,
            history: None,
            arrow: None,
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

/// `--legend` 与 `--no-legend` 互斥
fn set_legend(a: &mut Args, m: LegendMode) -> Result<(), String> {
    if let Some(prev) = a.legend {
        if prev != m {
            return Err("--legend and --no-legend cannot be used together".to_string());
        }
    }
    a.legend = Some(m);
    Ok(())
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
            "--dump" => a.dump = true,
            "--labels" => a.labels = true,
            "--no-labels" => a.no_labels = true,
            "--legend" => set_legend(&mut a, LegendMode::On)?,
            "--no-legend" => set_legend(&mut a, LegendMode::Off)?,
            "--no-notes" => a.no_notes = true,
            "--drawn-cursor" => a.cursor = false,
            "--file" => a.file = Some(next("--file")?),
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
            "--size" => {
                let v = next("--size")?;
                a.size = if v.eq_ignore_ascii_case("auto") {
                    None
                } else {
                    Some(parse_size(&v)?)
                };
            }
            "--format" => {
                let v = next("--format")?;
                a.format =
                    DumpFormat::parse(&v).ok_or_else(|| format!("bad --format {v} (text|ansi)"))?;
            }
            "--arrow" => {
                let v = next("--arrow")?;
                a.arrow = Some(
                    ArrowStyle::parse(&v)
                        .ok_or_else(|| format!("bad arrow style {v} (ascii|arrows|triangles)"))?,
                );
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
            s => {
                if a.file.is_some() {
                    return Err(format!(
                        "only one input file is allowed (already got one, then {s})"
                    ));
                }
                a.file = Some(s.to_string());
            }
        }
        i += 1;
    }
    Ok(a)
}

fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("--size wants WxH, got {s}"))?;
    let w: u16 = w.trim().parse().map_err(|_| format!("bad width in {s}"))?;
    let h: u16 = h.trim().parse().map_err(|_| format!("bad height in {s}"))?;
    if w < 8 || h < 4 {
        return Err("canvas too small (at least 8x4)".to_string());
    }
    Ok((w, h))
}

/// 读图:文件、stdin("-") 或者没有参数时从 stdin 读.
/// 什么都没给、stdin 又是终端:返回 None,调用方开一份空白缓冲(不落盘).
fn source_text(a: &Args) -> Result<Option<(String, Option<String>)>, String> {
    match a.file.as_deref() {
        Some(p) if p != "-" => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?;
            Ok(Some((text, Some(p.to_string()))))
        }
        Some(_) => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            if text.trim().is_empty() {
                return Err("no input on stdin".to_string());
            }
            Ok(Some((text, None)))
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
                return Ok(Some((text, None)));
            }
            Ok(None)
        }
    }
}

/// 整幅图导成一张图,然后退出. `.svg` 是自包含的矢量图(浏览器/编辑器都能开),
/// `.canvas` 是 JSON Canvas(Obsidian 的板子).
fn export(app: &mut App, path: &str, labels: bool, size: Option<(u16, u16)>) -> Result<(), String> {
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
        // 和 --dump 同一套成帧:只画图、1:1、居中、留白
        let view = graph_tui::app::View::default();
        let frame = graph_tui::dump::frame(&app.doc, labels, size, &view);
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

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut a = parse_args(&argv)?;
    // 路径统一展开开头的 `~`(动词里手敲的路径同样走 paths::expand)
    for p in [&mut a.file, &mut a.export].into_iter().flatten() {
        *p = graph_tui::paths::expand(p);
    }
    if a.help {
        print!("{USAGE}");
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
    let (doc, mut opts_extra) = match source_text(&a)? {
        Some((text, source)) => {
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
            (doc, source)
        }
        None => (Document::scratch(), None),
    };
    let mut opts = Options::default();
    if let Some(cfg) = &cfg {
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
        for (k, act) in &cfg.keys {
            opts.keys.bind(k.clone(), *act);
        }
    }
    if let Some(l) = a.layout {
        opts.layout = Some(l);
    }
    if a.labels {
        opts.labels = true;
    }
    if let Some(m) = a.legend {
        opts.legend = m;
    }
    if a.no_notes {
        opts.notes = NotesMode::Off;
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
    if let Some(v) = a.arrow {
        opts.arrow = Some(v);
    }
    let mut app = App::new(doc, opts)?;
    app.source = opts_extra.take();

    if let Some(path) = &a.export {
        // 导出没有终端光标:退回画在画面里的那种
        app.view.drawn = true;
        export(&mut app, path, !a.no_labels, a.size)?;
        return Ok(());
    }

    if a.dump {
        // 只画图:1:1、居中、留白,没有状态行/光标.图例跟着文档与命令行开关
        let view = app.view.clone();
        let frame = graph_tui::dump::frame(&app.doc, !a.no_labels, a.size, &view);
        if frame.clipped {
            eprintln!("warning: the picture does not fit the requested size");
        }
        let text = match a.format {
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
        return Ok(());
    }

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
            // 超时:呼吸换了一档、缓动真的挪了才重画,否则继续睡
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
    }
}

/// 跑一次外部编辑器:先退出 raw + alt screen,跑完(无论成败)恢复并整屏重画
fn run_external(app: &mut App) -> Result<(), String> {
    let mut out = io::stdout();
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture);
    let initial = app.edit_initial().unwrap_or_default();
    let outcome = graph_tui::external::edit(&initial);
    let mut out = io::stdout();
    enable_raw_mode().map_err(|e| format!("cannot re-enter raw mode: {e}"))?;
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )
    .map_err(|e| format!("cannot restore the screen: {e}"))?;
    app.apply_edit_result(outcome);
    Ok(())
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
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("graph-tui: {e}");
            eprintln!("try graph-tui --help");
            ExitCode::FAILURE
        }
    }
}
