//! `--dump` / 导出的成帧:自动尺寸、留白、居中、只画图.

use std::io::Cursor;
use std::process::Command;

use graph_tui::app::View;
use graph_tui::document::{Box, DocEdge, Document, Group, Layer, Text, ViewState};
use graph_tui::dump;
use graph_tui::headless::{self, Config};
use graph_tui::json::Value;
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

/// 两框一条边,适合量尺寸
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

/// 1/2/3/5:自动尺寸 = 内容 + 留白,四边是空白,没有状态行
#[test]
fn 自动尺寸等于内容加留白() {
    let d = doc();
    // 自动尺寸就是图本身 + 四周留白
    let frame = dump::frame(&d, true, None, &View::default());
    let (w, h) = (frame.buf.area.width, frame.buf.area.height);
    // 内容:两框 6x3、相距 8 行,加边;宽 6 高 11
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
    // 最外一圈是空白
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
    // 框的完整边框在内容里(用探测帧里的框矩形换算)
    let r = frame.rects[0];
    let at = |dx: i32, dy: i32| {
        let p = frame.map((r.0 + dx, r.1 + dy));
        frame.buf[(p.0 as u16, p.1 as u16)].symbol().to_string()
    };
    assert_eq!(at(0, 0), "┌", "左上角");
    assert_eq!(at(5, 2), "┘");
    assert_eq!(at(0, 2), "└");
    // 文本输出里没有状态行/图例
    let lines = graph_tui::render::plain_lines(&frame.buf);
    assert!(!lines.iter().any(|l| l.contains("VIEW")), "不该有状态行");
}

/// 4/13:没有光标高亮、没有选区反显
#[test]
fn 不带光标与选区() {
    let d = doc();
    let view = View {
        cursor: (1, 1),
        drawn: true,
        ..Default::default()
    };
    let frame = dump::frame(&d, true, None, &view);
    // 光标格就是框里的文字格:dump 照旧画字,不该被光标标记改掉
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

/// 点节点:dump 不画光标字形(实心圆),只画普通节点字形 + 标签
#[test]
fn 不画光标字形() {
    let mut d = doc();
    d.add_box("点", (20, 0), BoxStyle::Point).expect("加点");
    d.rebuild();
    let view = View {
        cursor: (20, 0),
        drawn: true,
        ..Default::default()
    };
    let frame = dump::frame(&d, true, None, &view);
    let text = graph_tui::render::plain_lines(&frame.buf).join("\n");
    assert!(!text.contains('●'), "dump 里不该有实心光标字形:{text}");
    assert!(!text.contains('◉'), "dump 里不该有实心枢纽字形");
}

/// 6/17:裁剪复制保留颜色,不重新渲染(两次出帧逐格相同)
#[test]
fn 裁剪复制保留颜色且稳定() {
    let d = doc();
    let f1 = dump::frame(&d, true, None, &View::default());
    let f2 = dump::frame(&d, true, None, &View::default());
    assert_eq!(f1.buf, f2.buf, "同一份文档两次出帧必须逐格相同");
    // 框的边框色被完整搬过来了
    let p = f1.map((f1.rects[0].0, f1.rects[0].1));
    assert_eq!(
        f1.buf[(p.0 as u16, p.1 as u16)].fg,
        Color::Rgb(0x4c, 0x8e, 0xf8),
        "边框色不该在搬运时丢"
    );
}

/// 7:1:1 —— 内容格距和 TUI zoom=1 一致(框宽 6 就是 6 列)
#[test]
fn 一比一不缩放() {
    let d = doc();
    let frame = dump::frame(&d, true, None, &View::default());
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

/// 8/9/10:显式尺寸、auto 等价、纯文本没有 ANSI
#[test]
fn 命令行尺寸与格式() {
    let path = write_doc("cli");
    // 不给尺寸:自动
    let (auto_text, err) = run(&["--dump", "--format", "text", &path]);
    assert!(err.is_empty(), "提示只走 stderr:{err}");
    let auto_lines: Vec<&str> = auto_text.lines().collect();
    assert_eq!(auto_lines.len(), 13, "11 行内容 + 上下各 1");
    assert_eq!(auto_lines[0].trim(), "", "第一行是留白");
    // auto 等于不给
    let (auto2, _) = run(&["--dump", "--format", "text", "--size", "auto", &path]);
    assert_eq!(auto_text, auto2, "--size auto 等价于不给");
    // 显式尺寸不自动改
    let (small, _) = run(&["--dump", "--format", "text", "--size", "20x6", &path]);
    let small_lines: Vec<&str> = small.lines().collect();
    assert_eq!(small_lines.len(), 6);
    assert!(small_lines.iter().all(|l| l.chars().count() <= 20));
    // ansi 有颜色,text 没有
    let (ansi, _) = run(&["--dump", &path]);
    assert!(ansi.contains("\u{1b}[38;2;"), "ansi 该有颜色");
    assert!(!auto_text.contains('\u{1b}'), "text 不该有转义");
    let _ = std::fs::remove_file(&path);
}

/// 11:SVG 用同一套自动尺寸
#[test]
fn svg尺寸一致() {
    let path = write_doc("svg");
    let frame = dump::frame(&doc(), true, None, &View::default());
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

/// 16:headless 的 size=auto 和 --dump 尺寸一致
#[test]
fn headless_auto与dump一致() {
    let path = write_doc("headless");
    let cfg = Config {
        file: Some(path.clone()),
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = headless::run_with(
        &cfg,
        &mut Cursor::new("{\"id\":1,\"op\":\"render\",\"format\":\"text\",\"size\":\"auto\"}"),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let resp = graph_tui::json::parse(String::from_utf8_lossy(&out).trim()).unwrap();
    let size = resp
        .get("result")
        .and_then(|r| r.get("size"))
        .and_then(Value::as_arr)
        .expect("size");
    let (w, h) = (size[0].as_i32().unwrap(), size[1].as_i32().unwrap());
    let (dump_text, _) = run(&["--dump", "--format", "text", &path]);
    let lines: Vec<&str> = dump_text.lines().collect();
    assert_eq!(lines.len() as i32, h, "高度该一致");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(lines[0]) as i32,
        w,
        "宽度该一致"
    );
    let _ = std::fs::remove_file(&path);
}

/// 14:内容超出探测范围(很远的 via)时扩一次探测,最终不被裁掉
#[test]
fn 探测贴边会扩一次() {
    let mut d = doc();
    // 把边甩到框右边一点点(比第一次探测的范围多出几格,扩一次就装得下)
    d.edges[0].via = vec![(20, 4)];
    d.rebuild();
    let f = dump::frame(&d, true, None, &View::default());
    // 内容宽要装得下那根甩出去的线
    assert!(
        f.buf.area.width >= 20,
        "via 该被算进内容,实际宽 {}",
        f.buf.area.width
    );
    // 最右一列(留白)仍然是空的,说明没被切断后还贴着边
    let w = f.buf.area.width;
    for y in 0..f.buf.area.height {
        assert_eq!(f.buf[(w - 1, y)].symbol(), " ", "右边该是留白");
    }
}

/// 13:空文档 / 单框 / 超宽标签 / 超长边都不崩
#[test]
fn 极端文档不崩() {
    // 单框
    let mut d = Document::scratch();
    let f = dump::frame(&d, true, None, &View::default());
    assert!(f.buf.area.width >= 1 && f.buf.area.height >= 1);
    // 超宽标签
    let i = d
        .add_box(&"很长的标签".repeat(10), (0, 0), BoxStyle::Single)
        .expect("加框");
    d.rebuild();
    assert!(i < d.boxes.len());
    let f = dump::frame(&d, true, None, &View::default());
    assert!(f.buf.area.width >= 4, "宽标签要有足够的缓冲");
    // 超长边:两框拉得很远
    let mut d2 = doc();
    d2.move_box(1, 0, 60);
    d2.rebuild();
    let f = dump::frame(&d2, true, None, &View::default());
    assert!(f.buf.area.height >= 60, "长边要被算进内容");
}

/// 15:stdout 只有画面;提示写 stderr
#[test]
fn 输出只有画面() {
    let path = write_doc("stdout");
    let out = Command::new(bin())
        .args(["--dump", &path])
        .output()
        .expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("VIEW"), "不该有状态行");
    assert!(!text.contains("wrote"), "不该有额外提示");
    // 末尾正好一个换行
    assert!(text.ends_with('\n'));
    assert!(!text.ends_with("\n\n"));
    let _ = std::fs::remove_file(&path);
}

/// `--ascii`:框线/连接点/点节点全部换成 ASCII,拓扑位置不变
#[test]
fn ascii字形() {
    let path = temp("ascii", "dot");
    std::fs::write(
        &path,
        "digraph g { a -> b; b -> c; c -> a; a [shape=box]; }\n",
    )
    .unwrap();
    let run = |ascii: bool| -> String {
        let mut cmd = Command::new(bin());
        cmd.args(["--dump", "--format", "text", "--lang", "en"])
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
    // Unicode 版必须有制表符,ASCII 版一个都不能有
    assert!(
        uni.contains('─') || uni.contains('│'),
        "unicode 版该有制表符"
    );
    for ch in plain.chars() {
        assert!(ch.is_ascii(), "ASCII 版出现了 {ch:?}");
    }
    // 结构一致:两个点节点的位置相同(○ -> o,◎ -> @)
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

/// headless `render ascii:true` 与命令行开关一致
#[test]
fn headless_ascii() {
    let path = temp("headless-ascii", "gtui");
    let mut d = doc();
    d.boxes[0].style = BoxStyle::Double;
    let body = d.save();
    std::fs::write(&path, body).unwrap();
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let cfg = Config {
        file: Some(path.clone()),
        ..Config::default()
    };
    let req = "{\"id\":1,\"op\":\"render\",\"format\":\"text\",\"size\":[30,10],\"ascii\":true}\n";
    let code = headless::run_with(&cfg, &mut Cursor::new(req), &mut out, &mut err);
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains('+'), "双线框在 ASCII 下该退化成 +:\n{text}");
    for ch in text.chars() {
        assert!(ch.is_ascii(), "出现了非 ASCII 字符 {ch:?}");
    }
    let _ = std::fs::remove_file(&path);
}
