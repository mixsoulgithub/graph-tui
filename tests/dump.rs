//! Frame of the picture (`--export -`) / exports: auto size, padding, centring, picture only.

use std::process::Command;

use graph_tui::app::View;
use graph_tui::document::{Box, DocEdge, Document, Group, Layer, Text, ViewState};
use graph_tui::dump;
use graph_tui::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};
use ratatui::style::Color;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_graph-tui")
}

fn temp(tag: &str, ext: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!(
            "graph-tui-dump-{tag}-{}-{nanos}.{ext}",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

/// Two boxes and one edge, handy for measuring sizes
fn doc() -> Document {
    let group = Group {
        id: "g0".into(),
        label: "线".into(),
        color: (0x4c, 0x8e, 0xf8),
    };
    let layer = Layer {
        name: String::new(),
        order: 0,
    };
    let b = |id: &str, at: (i32, i32)| Box {
        id: id.into(),
        at,
        size: (6, 3),
        style: BoxStyle::Single,
        color: Some((0x4c, 0x8e, 0xf8)),
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    };
    let t = |id: &str, box_id: &str, at: (i32, i32), text: &str| Text {
        id: id.into(),
        text: text.into(),
        at,
        box_id: box_id.into(),
        color: None,
        bg: None,
        align: Align::Left,
        layer: String::new(),
    };
    Document::from_parts(
        "test",
        vec![group],
        vec![layer],
        vec![b("a", (0, 0)), b("b", (0, 8))],
        vec![t("t1", "a", (1, 1), "A"), t("t2", "b", (1, 9), "B")],
        vec![DocEdge {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
            dir: Dir::Forward,
            group: "g0".into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            corner: Corner::Rounded,
            layer: String::new(),
            from_side: None,
            to_side: None,
            head: None,
        }],
        ViewState::default(),
    )
    .expect("样例文档")
}

fn write_doc(tag: &str) -> String {
    let path = temp(tag, "gtui");
    std::fs::write(&path, doc().save()).expect("写样例");
    path
}

fn run(args: &[&str]) -> (String, String) {
    let out = Command::new(bin()).args(args).output().expect("run");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// ANSI output of inline styles: bold/dim/italic/underline/strikethrough each yield an SGR
#[test]
fn inline_styles_emit_sgr() {
    use graph_tui::model::BoxStyle;
    let mut d = doc();
    d.texts[0].text = "**b** <dim>d</dim> *i* <u>u</u> ~~s~~".into();
    // Make the text wide enough not to be clipped by the box
    d.boxes[0].size = (40, 3);
    d.boxes[0].style = BoxStyle::Rounded;
    let path = temp("ansi-inline", "gtui");
    std::fs::write(&path, d.save()).expect("写样例");
    let out = Command::new(bin())
        .args(["--export", "-", "--format", "ansi", path.as_str()])
        .output()
        .expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // Each modifier owns one cell, so its SGR ends there: `;1m` `;2m` ...
    for code in [";1m", ";2m", ";3m", ";4m", ";9m"] {
        assert!(text.contains(code), "ANSI 里少了 {code}");
    }
}

/// A 0-byte file is a new file: `nvim empty.gtui` also starts from an empty buffer, so no
/// JSON error
#[test]
fn empty_file_is_new_document() {
    let path = temp("empty", "gtui");
    std::fs::write(&path, "").expect("建空文件");
    let out = Command::new(bin())
        .args(["--export", "-", "--format", "text", path.as_str()])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "空文件不该报错:{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.trim().is_empty(), "该给一份空白文档的图,而不是空输出");
}

/// 1/2/3/5: auto size = content + padding, blank on all four sides, no status line
#[test]
fn auto_size_is_content_plus_padding() {
    let d = doc();
    // Auto size is the picture plus padding on every side
    let frame = dump::frame(&d, true, dump::Area::auto(), &View::default());
    let (w, h) = (frame.buf.area.width, frame.buf.area.height);
    // Content: two 6x3 boxes 8 rows apart plus the edge; width 6, height 11
    assert_eq!(
        w as u32,
        6 + 2 * u32::from(dump::PAD_X),
        "宽 = 内容 + 左右留白"
    );
    assert_eq!(
        h as u32,
        11 + 2 * u32::from(dump::PAD_Y),
        "高 = 内容 + 上下留白"
    );
    // The outermost ring is blank
    for x in 0..w {
        for y in [0, h - 1] {
            assert_eq!(frame.buf[(x, y)].symbol(), " ", "({x},{y}) 该是留白");
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            assert_eq!(frame.buf[(x, y)].symbol(), " ", "({x},{y}) 该是留白");
        }
    }
    // The full box frame is inside the content (map through the probe frame's box rect)
    let r = frame.rects[0];
    let at = |dx: i32, dy: i32| {
        let p = frame.map((r.0 + dx, r.1 + dy));
        frame.buf[(p.0 as u16, p.1 as u16)].symbol().to_string()
    };
    assert_eq!(at(0, 0), "┌", "左上角");
    assert_eq!(at(5, 2), "┘");
    assert_eq!(at(0, 2), "└");
    // The text output has no status line or legend
    let lines = graph_tui::render::plain_lines(&frame.buf);
    assert!(!lines.iter().any(|l| l.contains("VIEW")), "不该有状态行");
}

/// 4/13: no cursor highlight, no selection reverse video
#[test]
fn no_cursor_or_selection() {
    let d = doc();
    let view = View {
        cursor: (1, 1),
        drawn: true,
        ..Default::default()
    };
    let frame = dump::frame(&d, true, dump::Area::auto(), &view);
    // The cursor cell is the box text cell: dump draws the glyph as usual, unmarked
    let r = frame.rects[0];
    let p = frame.map((r.0 + 1, r.1 + 1));
    assert_eq!(frame.buf[(p.0 as u16, p.1 as u16)].symbol(), "A");
    for c in frame.buf.content() {
        assert!(
            !c.modifier.contains(ratatui::style::Modifier::REVERSED),
            "不该有反显"
        );
    }
}

/// Point node: dump draws no cursor glyph (solid circle), only the normal node glyph + label
#[test]
fn no_cursor_glyph() {
    let mut d = doc();
    d.add_box("点", (20, 0), BoxStyle::Point).expect("加点");
    d.rebuild();
    let view = View {
        cursor: (20, 0),
        drawn: true,
        ..Default::default()
    };
    let frame = dump::frame(&d, true, dump::Area::auto(), &view);
    let text = graph_tui::render::plain_lines(&frame.buf).join("\n");
    assert!(!text.contains('●'), "dump 里不该有实心光标字形:{text}");
    assert!(!text.contains('◉'), "dump 里不该有实心枢纽字形");
}

/// 6/17: crop-copy keeps colours without re-rendering (two frames match cell by cell)
#[test]
fn crop_copy_keeps_colors_and_is_stable() {
    let d = doc();
    let f1 = dump::frame(&d, true, dump::Area::auto(), &View::default());
    let f2 = dump::frame(&d, true, dump::Area::auto(), &View::default());
    assert_eq!(f1.buf, f2.buf, "同一份文档两次出帧必须逐格相同");
    // The box frame colour is carried over intact
    let p = f1.map((f1.rects[0].0, f1.rects[0].1));
    assert_eq!(
        f1.buf[(p.0 as u16, p.1 as u16)].fg,
        Color::Rgb(0x4c, 0x8e, 0xf8),
        "边框色不该在搬运时丢"
    );
}

/// 7: 1:1: content cell pitch matches the TUI at zoom=1 (a 6-wide box is 6 columns)
#[test]
fn one_to_one_no_scaling() {
    let d = doc();
    let frame = dump::frame(&d, true, dump::Area::auto(), &View::default());
    let top = frame.map((frame.rects[0].0, frame.rects[0].1));
    let row: String = (0..6)
        .map(|k| {
            frame.buf[((top.0 + k) as u16, top.1 as u16)]
                .symbol()
                .to_string()
        })
        .collect();
    assert_eq!(row, "┌────┐", "框的宽在 1:1 下就是 6 格");
}

/// 8/9/10: explicit size, auto equivalence, plain text without ANSI
#[test]
fn cli_size_and_format() {
    let path = write_doc("cli");
    // No size: auto
    let (auto_text, err) = run(&["--export", "-", "--format", "text", &path]);
    assert!(err.is_empty(), "提示只走 stderr:{err}");
    let auto_lines: Vec<&str> = auto_text.lines().collect();
    assert_eq!(auto_lines.len(), 13, "11 行内容 + 上下各 1");
    assert_eq!(auto_lines[0].trim(), "", "第一行是留白");
    // auto equals omitting
    let (auto2, _) = run(&["--export", "-", "--format", "text", "--area", "auto", &path]);
    assert_eq!(auto_text, auto2, "--area auto 等价于不给");
    // An explicit size is not auto-adjusted
    let (small, _) = run(&[
        "--export",
        "-",
        "--format",
        "text",
        "--area",
        "auto,auto,20,6",
        &path,
    ]);
    let small_lines: Vec<&str> = small.lines().collect();
    assert_eq!(small_lines.len(), 6);
    assert!(small_lines.iter().all(|l| l.chars().count() <= 20));
    // ansi has colour, text does not (in a pipe no --format means text; see the next test)
    let (ansi, _) = run(&["--export", "-", "--format", "ansi", &path]);
    assert!(ansi.contains("\u{1b}[38;2;"), "ansi 该有颜色");
    assert!(!auto_text.contains('\u{1b}'), "text 不该有转义");
    let _ = std::fs::remove_file(&path);
}

/// `--export -` sends the picture to stdout, byte-identical to the piped auto render, and
/// writes no file
#[test]
fn export_dash_is_frame() {
    let path = write_doc("dash");
    let dash = std::path::Path::new(&path)
        .parent()
        .expect("目录")
        .join("-");
    let out = Command::new(bin())
        .args(["--export", "-", &path])
        .output()
        .expect("run");
    assert!(out.status.success(), "{:?}", out.stderr);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains('┌') || text.contains('╭'), "该有图:{text:?}");
    assert!(!dash.exists(), "`-` 不该写成文件");
    // An explicit ansi still wins
    let ansi = Command::new(bin())
        .args(["--export", "-", "--format", "ansi", &path])
        .output()
        .expect("run");
    let ansi = String::from_utf8_lossy(&ansi.stdout);
    assert!(ansi.contains("\u{1b}[38;2;"), "ansi 该有颜色");
    let _ = std::fs::remove_file(&path);
}

/// `--area x,y,w,h`: every part may be auto. The origin pins the top-left, the size fixes
/// output dimensions
#[test]
fn area_origin_and_size() {
    let path = write_doc("area");
    let frame = |av: &str| -> Vec<String> {
        let (text, err) = run(&["--export", "-", "--format", "text", "--area", av, &path]);
        assert!(err.is_empty(), "{av}: {err}");
        text.lines().map(str::to_string).collect()
    };
    // All auto = omitted: content size
    let auto = frame("auto");
    assert_eq!(auto, frame("auto,auto,auto,auto"));
    // Size given, origin auto: centred, exact size
    let sized = frame("auto,auto,60,20");
    assert_eq!(sized.len(), 20, "高度按给定值");
    assert!(
        sized.iter().all(|l| l.chars().count() <= 60),
        "宽度按给定值"
    );
    // Origin (0,0) + size: the top-left is the canvas origin, unlike centring
    let anchored = frame("0,0,60,20");
    assert_eq!(anchored.len(), 20);
    assert_ne!(anchored, sized, "定原点就不该再居中");
    // Origin shift: the whole picture moves left/up (content size unchanged)
    let shifted = frame("5,5,auto,auto");
    assert_eq!(shifted.len(), auto.len(), "尺寸按内容");
    assert_ne!(shifted, auto, "原点变了内容位置就变");
    // Part of the size auto: only the height axis follows content
    let tall = frame("auto,auto,auto,40");
    assert_eq!(tall.len(), 40, "高度给定");
    let wide = frame("auto,auto,40,auto");
    assert_eq!(wide.len(), auto.len(), "高度按内容");
    assert!(wide.iter().all(|l| l.chars().count() <= 40));
    let _ = std::fs::remove_file(&path);
}

/// A malformed `--area` must say so clearly
#[test]
fn area_argument_validation() {
    let path = write_doc("areabad");
    for (av, want) in [
        ("1,2,3", "x,y,w,h"),
        ("auto,auto,4,3", "too small"),
        ("a,b,c,d", "bad x"),
        ("1,2,3,4,5", "x,y,w,h"),
    ] {
        let (text, err) = run(&["--export", "-", "--area", av, &path]);
        assert!(text.is_empty(), "{av} 不该出图");
        assert!(err.contains(want), "{av} -> {err:?}");
    }
    // --size is gone
    let (_, err) = run(&["--export", "-", "--size", "40x12", &path]);
    assert!(err.contains("unknown option --size"), "{err}");
    let _ = std::fs::remove_file(&path);
}

/// `--dump` was removed: report it clearly instead of silently treating it as another flag
#[test]
fn dump_option_removed() {
    let path = write_doc("nodump");
    let out = Command::new(bin())
        .args(["--dump", &path])
        .output()
        .expect("run");
    assert!(!out.status.success(), "--dump 该被拒绝");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown option --dump"), "{err}");
    let _ = std::fs::remove_file(&path);
}

/// With stdout not a terminal no flag is needed: emit one plain-text frame with no escapes
#[test]
fn piped_without_flags_renders() {
    let path = write_doc("autodump");
    let out = Command::new(bin()).arg(&path).output().expect("run");
    assert!(out.status.success(), "{:?}", out.stderr);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.is_empty(), "该有图");
    assert!(!text.contains('\u{1b}'), "管道里不该有转义:{text:?}");
    assert!(
        text.contains('┌') || text.contains('╭'),
        "框线要留着:{text:?}"
    );
    // Matches an explicit --export -
    let explicit = Command::new(bin())
        .args(["--export", "-", &path])
        .output()
        .expect("run");
    assert_eq!(text, String::from_utf8_lossy(&explicit.stdout));
    let _ = std::fs::remove_file(&path);
}

/// `--help` text comes from a marked block in docs/usage.md; neither locale nor `--lang`
/// changes it (always English)
#[test]
fn help_from_docs_and_always_english() {
    let plain = Command::new(bin()).arg("--help").output().expect("run");
    let plain = String::from_utf8_lossy(&plain.stdout).into_owned();
    assert_eq!(plain.trim_end(), graph_tui::help::text());
    assert!(plain.contains("--export") && plain.contains("--format"));
    // A Chinese locale and --lang zh do not affect help
    let zh = Command::new(bin())
        .args(["--help", "--lang", "zh"])
        .env("LC_ALL", "zh_CN.UTF-8")
        .output()
        .expect("run");
    assert_eq!(plain, String::from_utf8_lossy(&zh.stdout));
    assert!(
        !plain
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
        "help 该是纯英文"
    );
}

/// `.canvas` is picked by extension and converted: text shows, `toEnd: none` draws no arrow
#[test]
fn canvas_file_opens_by_extension() {
    let path = temp("canvas-open", "canvas");
    std::fs::write(
        &path,
        r#"{"nodes":[
            {"id":"a","type":"text","text":"hello canvas","x":0,"y":0,"width":200,"height":60},
            {"id":"b","type":"text","text":"second","x":400,"y":0,"width":200,"height":60}
        ],"edges":[{"id":"e","fromNode":"a","toNode":"b","toEnd":"none"}]}"#,
    )
    .expect("write canvas");
    let out = Command::new(bin())
        .args(["--export", "-", "--format", "text", &path])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("hello canvas"), "{text}");
    assert!(text.contains("second"), "{text}");
    // `toEnd: none` keeps the edge unarrowed: no `<` or `>` heads anywhere
    assert!(!text.contains('>') && !text.contains('<'), "{text}");
    let _ = std::fs::remove_file(&path);
}

/// 11: SVG uses the same auto sizing
#[test]
fn svg_size_matches() {
    let path = write_doc("svg");
    let frame = dump::frame(&doc(), true, dump::Area::auto(), &View::default());
    let (w, h) = (frame.buf.area.width, frame.buf.area.height);
    let svg = temp("svg", "svg");
    let (_, err) = run(&["--export", &svg, &path]);
    assert!(err.is_empty(), "{err}");
    let body = std::fs::read_to_string(&svg).expect("读 svg");
    let head = body.lines().next().unwrap_or("");
    assert!(
        head.contains(&format!("width=\"{}\"", w * 9)),
        "svg 宽该是 {}: {head}",
        w * 9
    );
    assert!(
        head.contains(&format!("height=\"{}\"", h * 18)),
        "svg 高该是 {}: {head}",
        h * 18
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&svg);
}

/// 14: content beyond the probe range (a distant via) expands the probe once and is not
/// clipped
#[test]
fn probe_near_edge_expands_once() {
    let mut d = doc();
    // Fling the edge a little past the box (a few cells beyond the first probe, so one
    // expansion fits it)
    d.edges[0].via = vec![(20, 4)];
    d.rebuild();
    let f = dump::frame(&d, true, dump::Area::auto(), &View::default());
    // Content width must fit the flung-out line
    assert!(
        f.buf.area.width >= 20,
        "via 该被算进内容,实际宽 {}",
        f.buf.area.width
    );
    // The rightmost column (padding) is still blank, so nothing was cut off against the edge
    let w = f.buf.area.width;
    for y in 0..f.buf.area.height {
        assert_eq!(f.buf[(w - 1, y)].symbol(), " ", "右边该是留白");
    }
}

/// 13: empty document / single box / very wide label / very long edge do not panic
#[test]
fn extreme_documents_do_not_panic() {
    // Single box
    let mut d = Document::scratch();
    let f = dump::frame(&d, true, dump::Area::auto(), &View::default());
    assert!(f.buf.area.width >= 1 && f.buf.area.height >= 1);
    // Very wide label
    let i = d
        .add_box(&"很长的标签".repeat(10), (0, 0), BoxStyle::Single)
        .expect("加框");
    d.rebuild();
    assert!(i < d.boxes.len());
    let f = dump::frame(&d, true, dump::Area::auto(), &View::default());
    assert!(f.buf.area.width >= 4, "宽标签要有足够的缓冲");
    // Very long edge: the boxes are far apart
    let mut d2 = doc();
    d2.move_box(1, 0, 60);
    d2.rebuild();
    let f = dump::frame(&d2, true, dump::Area::auto(), &View::default());
    assert!(f.buf.area.height >= 60, "长边要被算进内容");
}

/// 15: stdout carries only the picture; notes go to stderr
#[test]
fn stdout_has_frame_only() {
    let path = write_doc("stdout");
    let out = Command::new(bin())
        .args(["--export", "-", &path])
        .output()
        .expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("VIEW"), "不该有状态行");
    assert!(!text.contains("wrote"), "不该有额外提示");
    // Exactly one trailing newline
    assert!(text.ends_with('\n'));
    assert!(!text.ends_with("\n\n"));
    let _ = std::fs::remove_file(&path);
}

/// `--ascii`: frames, connectors and point nodes all become ASCII, topology unchanged
#[test]
fn ascii_glyphs() {
    let path = temp("ascii", "dot");
    std::fs::write(
        &path,
        "digraph g { a -> b; b -> c; c -> a; a [shape=box]; }\n",
    )
    .unwrap();
    let run = |ascii: bool| -> String {
        let mut cmd = Command::new(bin());
        cmd.args(["--export", "-", "--format", "text", "--lang", "en"])
            .arg(&path);
        if ascii {
            cmd.arg("--ascii");
        }
        let out = cmd.output().expect("run");
        assert!(out.status.success(), "{:?}", out);
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let uni = run(false);
    let plain = run(true);
    // The Unicode version must have box glyphs; the ASCII version none at all
    assert!(
        uni.contains('─') || uni.contains('│'),
        "unicode 版该有制表符"
    );
    for ch in plain.chars() {
        assert!(ch.is_ascii(), "ASCII 版出现了 {ch:?}");
    }
    // Same structure: the two point nodes sit at the same places (○ -> o, ◎ -> @)
    let at = |s: &str, needle: char| -> Vec<(usize, usize)> {
        s.lines()
            .enumerate()
            .flat_map(|(y, l)| {
                l.char_indices()
                    .filter(|(_, c)| *c == needle)
                    .map(move |(x, _)| (x, y))
            })
            .collect()
    };
    let mut u_nodes: Vec<_> = at(&uni, '○').into_iter().chain(at(&uni, '◎')).collect();
    let mut a_nodes: Vec<_> = at(&plain, 'o').into_iter().chain(at(&plain, '@')).collect();
    u_nodes.sort_unstable();
    a_nodes.sort_unstable();
    assert_eq!(u_nodes, a_nodes, "拓扑位置该一致");
    let _ = std::fs::remove_file(&path);
}
