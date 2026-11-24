//! graph-tui: a general-purpose graph rendering engine for the terminal.
//!
//! This file only handles the command line, terminal setup and the main loop; graph
//! data lives in model.rs, parsing in dot.rs, layout in layout.rs, drawing in
//! canvas.rs + render.rs and interaction in app.rs.

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

/// Output format of a picture: truecolour ANSI or plain text.
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
    /// Positional arguments: one or more files (several = several tabs; --export
    /// takes one).
    files: Vec<String>,
    layout: Option<Layout>,
    /// --area: picture rectangle; every component may be auto (omitted = all auto).
    area: Area,
    /// --format: output format of the picture (ansi / text); None = pick by
    /// destination.
    format: Option<DumpFormat>,
    node_labels: bool,
    /// --no-node-labels: do not draw point node names in pictures or exports.
    no_node_labels: bool,
    /// --no-edge-labels: do not draw edge labels.
    no_edge_labels: bool,
    /// Cursor mode: real (the terminal cursor) / drawn (painted into the buffer).
    cursor: bool,
    /// Undo depth limit.
    history: Option<usize>,
    /// Arrow glyph style.
    /// --ascii: pure ASCII glyphs (for terminals without box-drawing glyphs).
    ascii: bool,
    /// Layout spacings: lane gap / layer gap / margin.
    lane_gap: Option<f64>,
    layer_gap: Option<f64>,
    layout_margin: Option<f64>,
    at: Option<String>,
    find: Option<String>,
    lang: Lang,
    /// Was --lang given explicitly? Only an explicit non-default language overrides
    /// the view state stored in the file.
    lang_set: bool,
    /// --config <path>: explicit config file (an absent file is an error).
    config: Option<String>,
    /// --no-config: never read a config file.
    no_config: bool,
    /// --print-config: print the full default config and exit.
    print_config: bool,
    /// --init-config [path]: write a default config (needs --force when it exists).
    init_config: Option<Option<String>>,
    /// --force: let --init-config overwrite an existing file.
    force: bool,
    /// Export one picture and exit (.svg / .canvas).
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
            // Optional value: a next argument that is not a flag is the path (peek
            // first, then let the loop advance normally).
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
            "--ascii" => a.ascii = true,
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

/// `--area x,y,w,h`: each of the four components may be auto; a bare `auto` means
/// all four are auto.
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

/// Reads the graph: from a file, from stdin ("-"), or from stdin when there is no
/// argument.
/// Nothing given and stdin is a terminal: returns None and the caller opens an empty
/// buffer (never written to disk).
/// As in nvim, a `.gtui` that does not exist yet is a "new file": an empty document
/// plus that path, written only on `:w`; `.dot` / `.canvas` are import sources and a
/// missing one is still an error. `--export` needs real content and errors as well.
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
            // An empty file (just touched, or a 0-byte leftover from a failed save) is
            // a new file: `nvim empty.gtui` also gives an empty buffer instead of a
            // "JSON parse failed" error.
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

/// Exports the whole graph as one picture and exits. `.svg` is a self-contained
/// vector image (browsers and editors can open it), `.canvas` is JSON Canvas
/// (Obsidian's board format).
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
        // Best-effort view: not the source document; every loss is reported to stderr.
        let (body, warnings) = graph_tui::dot_write::export(&app.doc)?;
        write_text(path, &body)?;
        for w in warnings {
            eprintln!("warning: {w}");
        }
        println!("wrote {path}");
        return Ok(());
    }
    let body = {
        // Same framing as the stdout path: picture only, 1:1, centred, padded. Glyphs
        // come from the App (--ascii).
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

/// Writes `.tmp` first, then renames.
fn write_text(path: &str, body: &str) -> Result<(), String> {
    graph_tui::paths::ensure_parent(path)?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("cannot write {tmp}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {path}: {e}"))
}

/// `--help` text: comes from docs/usage.md, always English (`--lang` only affects
/// TUI strings).
fn help_text() -> &'static str {
    graph_tui::help::text()
}

/// Output format of a picture: an explicit choice wins, otherwise pick by destination
/// and environment.
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
    // Expand a leading `~` on every path (paths typed in commands go through
    // paths::expand too).
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
    // Config: explicit > environment variable > XDG default; --no-config skips it all.
    let cfg = if a.no_config {
        None
    } else {
        graph_tui::keys::load_config(a.config.as_deref())?
    };
    // No file and stdin is a terminal: open an empty document (memory only, never
    // saved; saving needs a path from the user).
    let (doc, mut opts_extra, fresh) = match source_text(&a)? {
        // New file: no content yet, so open an empty document (the source path is
        // already known; `:w` writes there).
        Some((_, source, true)) => (Document::scratch(), source, true),
        Some((text, source, false)) => {
            // Format by extension: `.gtui` is a native document, `.canvas` is JSON
            // Canvas, everything else is imported as `.dot`.
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
    // `a.cursor` = use the real terminal cursor; `drawn` is the opposite.
    opts.drawn = !a.cursor;
    if let Some(v) = a.at.clone() {
        opts.at = Some(v);
    }
    if let Some(v) = a.find.clone() {
        opts.find = Some(v);
    }
    if a.lang_set {
        opts.lang = Some(a.lang);
    }
    if let Some(n) = a.history {
        opts.history = Some(n);
    }
    if a.ascii {
        opts.glyphs = graph_tui::glyphs::Glyphs::ASCII;
    }
    if a.lane_gap.is_some() || a.layer_gap.is_some() || a.layout_margin.is_some() {
        opts.spacing = graph_tui::layout::Spacing::of(a.lane_gap, a.layer_gap, a.layout_margin);
    }
    // Multiple files are interactive-only: --export takes exactly one input.
    if a.files.len() > 1 && a.export.is_some() {
        return Err(format!(
            "--export takes one input file, got {}",
            a.files.len()
        ));
    }
    let mut app = App::new(doc, opts)?;
    // New file: say so on the status line so it does not look like a failed open.
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
    // The remaining files each get a tab; finish on the first one.
    if a.files.len() > 1 {
        let files = a.files.clone();
        for p in files.iter().skip(1) {
            app.new_tab(Some(p));
        }
        app.switch_tab(0);
    }

    if let Some(path) = &a.export {
        if path == "-" {
            // `--export -` sends the picture to stdout (the same path as the automatic
            // dump when piping).
            return picture(&app, &a);
        }
        // Exports carry the picture only: no terminal cursor and no drawn cursor.
        export(&mut app, path, !a.no_node_labels, a.area)?;
        return Ok(());
    }

    // When stdout is not a terminal (pipe or file), skip the UI and dump one frame.
    if !io::stdout().is_terminal() {
        return picture(&app, &a);
    }

    // Session tips are picked on the interactive path only: dump and export stay
    // deterministic.
    app.seed_tip();
    interactive(app)
}

/// Draws one frame to stdout: 1:1, centred, padded, no status line and no cursor.
/// The format follows the destination (`--format` overrides it); `--area`,
/// `--node-labels`, `--no-node-labels` and `--no-edge-labels` all apply.
fn picture(app: &App, a: &Args) -> Result<(), String> {
    let stdout_tty = io::stdout().is_terminal();
    let view = app.view.clone();
    let frame = graph_tui::dump::frame(&app.doc, !a.no_node_labels, a.area, &view);
    if frame.clipped {
        eprintln!("warning: the picture does not fit the requested size");
    }
    let skipped = frame.hit.labels_skipped.len();
    if skipped > 0 {
        // Names that do not fit are dropped from the picture: warn on stderr so it
        // does not look like missing data.
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

/// Overlays config preferences on the defaults (command line flags override later).
fn prefs_into(opts: &mut Options, cfg: Option<&graph_tui::keys::Config>) {
    let Some(cfg) = cfg else { return };
    let p = &cfg.prefs;
    if let Some(n) = p.history {
        opts.history = Some(n);
    }
    if let Some(l) = p.lang {
        opts.lang = Some(l);
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

/// Runs the main loop in the terminal (interactive mode; dump and export return
/// earlier).
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

/// `graph-tui tutor`: opens the built-in tutorial. It is a scratch document and is
/// never saved on exit; rerunning the command resets it. The UI text follows
/// `--lang`, else `[prefs] lang` from the config, else the locale (English).
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
    // The tutorial content is English; `--lang` still only affects the program's own
    // UI strings.
    if a.lang_set {
        opts.lang = Some(a.lang);
    }
    opts.drawn = !a.cursor;
    if a.ascii {
        opts.glyphs = graph_tui::glyphs::Glyphs::ASCII;
    }
    // Point node names must be visible in the tutorial (the point in the exercise
    // area is there to be seen).
    opts.node_labels = true;
    let mut app = App::new(doc, opts)?;
    app.set_source(None);
    // First frame: centre horizontally on the section column and vertically on the
    // welcome box (centring on the cursor alone clips the box on wide terminals).
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
    // Redraw only when the picture may have changed: mouse motion (mode 1003) can
    // deliver thousands of events per second and redrawing each one floods the
    // terminal, which looks like a stall while dragging.
    let mut dirty = true;
    loop {
        if dirty {
            app.tick();
            term.draw(|f| {
                let area = f.area();
                app.draw(f.buffer_mut(), area);
                // Real cursor: the App computes the position from the cursor box or the
                // prompt line; when it cannot, ratatui hides the cursor.
                if let Some(at) = app.cursor_cell() {
                    f.set_cursor_position(at);
                }
            })
            .map_err(|e| format!("cannot draw: {e}"))?;
            dirty = false;
        }
        // Wake up less often when no animation is running.
        let wait = if app.busy() { 16 } else { 250 };
        if !event::poll(Duration::from_millis(wait))
            .map_err(|e| format!("cannot read events: {e}"))?
        {
            // Timeout: redraw only when the pulse changed phase or easing actually
            // moved; otherwise keep sleeping.
            // Also check once (a single stat) whether another program changed the
            // source file.
            if app.check_disk() {
                dirty = true;
            }
            if app.busy() && app.tick() {
                dirty = true;
            }
            continue;
        }
        // Drain the queued input in one go (drags and wheel events arrive in bursts)
        // and redraw once.
        // At most 64 per batch: drawing must still reach the screen while events keep
        // coming.
        let mut batch = 0;
        while batch < 64
            && event::poll(Duration::ZERO).map_err(|e| format!("cannot read events: {e}"))?
        {
            batch += 1;
            let ev = event::read().map_err(|e| format!("cannot read events: {e}"))?;
            match &ev {
                Event::Resize(..) => dirty = true,
                // The mouse only moved: with no menu open nothing changes, so no redraw.
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
        // Ctrl+G: hand the prompt line to the external editor. Terminal suspend and
        // resume happen here; the App only states the intent.
        if app.edit_pending() {
            run_external(app)?;
            // The alternate screen was left: ratatui's old buffer no longer matches
            // the screen, so a fresh Terminal (empty buffer) repaints everything on the
            // next frame.
            term = Terminal::new(CrosstermBackend::new(io::stdout()))
                .map_err(|e| format!("cannot restart the terminal: {e}"))?;
            dirty = true;
        }
        // Enter / gf: open the link under the cursor. Again the App only returns a
        // plan; the terminal and the processes are handled here.
        if let Some(plan) = app.take_open_request() {
            match run_plan(app, plan) {
                Ok(true) => {
                    // It suspended: use a fresh Terminal so the next frame repaints
                    // everything.
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

/// Hands the terminal back: show the cursor, leave raw mode and the alternate screen.
/// Failures are ignored (resume retries below).
fn suspend_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture);
    let _ = out.flush();
}

/// Takes the terminal back: raw mode, alternate screen, mouse capture and a cleared
/// screen (the old contents are gone).
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

/// Runs the external editor once: leave raw mode and the alternate screen, then
/// restore and repaint the whole screen whether it succeeded or not.
fn run_external(app: &mut App) -> Result<(), String> {
    suspend_terminal();
    let initial = app.edit_initial().unwrap_or_default();
    let outcome = graph_tui::external::edit(&initial);
    resume_terminal()?;
    app.apply_edit_result(outcome);
    Ok(())
}

/// Executes an open plan.
///
/// Returns `Ok(true)` when the terminal was suspended (the caller must rebuild the
/// Terminal and repaint everything), `Ok(false)` when it only spawned a process and
/// the picture never left the alternate screen.
fn run_plan(app: &mut App, plan: graph_tui::target::OpenPlan) -> Result<bool, String> {
    use graph_tui::target::OpenPlan;
    match plan {
        // Jump is done inside the App and never reaches this point.
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

/// Custom scheme: suspend the terminal, run the command and come back (it may be a
/// reader or a TUI).
///
/// The command and its arguments are passed separately, never through a shell; a
/// non-zero exit status is a failure.
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

/// A local text file with a position: suspend the terminal and let the editor stop
/// at that line and column.
///
/// The command string may carry arguments (`code -w`), so it goes through `sh -c`
/// instead of a home-grown split; the position arguments are built by
/// `target::editor_args` (a pure function covered by unit tests).
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
        // Not a vim: there is no portable column argument, so it got the line only.
        return Err(format!(
            "{}: line {} only, this editor has no column argument",
            cmd, p.a
        ));
    }
    Ok(true)
}

/// Opens a link.
///
/// Returns `Ok(true)` when the terminal was suspended (the caller must rebuild the
/// Terminal and repaint everything), `Ok(false)` when it only spawned a process and
/// the picture never left the alternate screen.
///
/// - URL: spawn the opener and stop there, no suspend (browsers and GUIs detach
///   themselves).
/// - `.gtui` / `.dot`: open in a new tab, no process at all.
/// - Any other local file: when the opener is `opener`, ask `--which` first:
///   `edit` suspends (the editor takes over the terminal), `open` spawns (images and
///   PDFs do not interrupt the picture).
fn run_opener(app: &mut App, raw: &str, kind: graph_tui::link::LinkKind) -> Result<bool, String> {
    use graph_tui::link::{is_graph_doc, resolve_local, LinkKind};
    let opener = app.opener_command();
    match kind {
        LinkKind::Url => {
            spawn_opener(&opener, &[raw])?;
            Ok(false)
        }
        // Custom schemes and self links are caught by the target handler and never
        // reach this point.
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
                        // Editor: suspend the terminal and let it take over.
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

/// Split a configured command into program + arguments: `[prefs] opener`, `$OPENER` and the
/// `[links]` values share the rule (first word is the program, no shell).
fn split_command(cmd: &str) -> (&str, Vec<&str>) {
    let mut words = cmd.split_whitespace();
    (words.next().unwrap_or(cmd), words.collect())
}

/// `opener --which -- <path>`: Some(true) = edit, Some(false) = open, None = unclear.
fn opener_which(opener: &str, path: &str) -> Result<Option<bool>, String> {
    let (program, extra) = split_command(opener);
    let out = std::process::Command::new(program)
        .args(extra)
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
    let (program, _) = split_command(opener);
    std::path::Path::new(program)
        .file_name()
        .map(|n| n == "opener")
        .unwrap_or(false)
}

fn spawn_opener(opener: &str, args: &[&str]) -> Result<(), String> {
    let (program, extra) = split_command(opener);
    std::process::Command::new(program)
        .args(extra)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot run {opener}: {e}"))
}

fn spawn_opener_wait(opener: &str, args: &[&str]) -> Result<(), String> {
    let (program, extra) = split_command(opener);
    std::process::Command::new(program)
        .args(extra)
        .args(args)
        .status()
        .map(|_| ())
        .map_err(|e| format!("cannot run {opener}: {e}"))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
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

    /// The opener command takes options like a `[links]` value: first word is the program
    #[test]
    fn opener_command_splits_into_program_and_args() {
        assert_eq!(split_command("firefox"), ("firefox", vec![]));
        assert_eq!(
            split_command("  code -w --new-window  "),
            ("code", vec!["-w", "--new-window"])
        );
        assert!(opener_is_helper("/usr/local/bin/opener"));
        assert!(opener_is_helper("opener --quiet"));
        assert!(!opener_is_helper("/usr/bin/xdg-open"));
    }

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

    /// Default picture format: an explicit choice wins, then destination and
    /// environment.
    #[test]
    fn dump_default_format() {
        use DumpFormat::{Ansi, Text};
        // Explicit wins (ANSI in a pipe, plain text in a terminal).
        assert_eq!(choose_format(Some(Ansi), false, true, true), Ansi);
        assert_eq!(choose_format(Some(Text), true, false, false), Text);
        // Pipe or file: plain text.
        assert_eq!(choose_format(None, false, false, false), Text);
        // Terminal: NO_COLOR or TERM=dumb gives plain text, otherwise truecolour.
        assert_eq!(choose_format(None, true, true, false), Text);
        assert_eq!(choose_format(None, true, false, true), Text);
        assert_eq!(choose_format(None, true, false, false), Ansi);
    }

    /// As in nvim: a missing .gtui is a new file, any other missing file is an error,
    /// and export or dump needs real content.
    #[test]
    fn missing_gtui_is_a_new_file() {
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

        // --export (including `-`) needs real content.
        for export in [Some("-".to_string()), Some(format!("{}.svg", path(&gtui)))] {
            let a = Args {
                files: vec![path(&gtui)],
                export,
                ..Args::default()
            };
            assert!(source_text(&a).is_err(), "缺文件不能出画面/导出");
        }

        // .dot is an import source and cannot be conjured up.
        let a = Args {
            files: vec![path(&dot)],
            ..Args::default()
        };
        assert!(source_text(&a).is_err(), "缺 .dot 仍然报错");

        // An existing file reads normally.
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
