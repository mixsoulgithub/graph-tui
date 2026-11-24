use super::*;
use crate::document::{DocEdge, Group, Layer, Text, ViewState};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

/// Render one frame (used only to read the bottom two rows).
fn buf_status(d: &Document, view: &View) -> Buffer {
    let area = Rect::new(0, 0, 200, 50);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, d, view);
    buf
}

/// The status row: second from the bottom with two rows, last row on a short terminal.
fn status_line(buf: &Buffer) -> String {
    let h = buf.area.height;
    let y = if status_rows(buf.area) > 1 {
        h - 2
    } else {
        h - 1
    };
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
}

/// Truncate a long hint from the left, keeping the file name; draw nothing when it cannot fit.
#[test]
fn hint_truncated_from_left() {
    assert_eq!(fit_tail("abc", 5).as_deref(), Some("abc"));
    assert_eq!(fit_tail("abc", 3).as_deref(), Some("abc"));
    // Truncated: keep the tail and prefix it with "...".
    assert_eq!(
        fit_tail("/very/long/path.gtui", 10).as_deref(),
        Some("...th.gtui")
    );
    // Width is measured in display columns (a CJK glyph takes two).
    assert_eq!(fit_tail("中文文件名", 7).as_deref(), Some("...件名"));
    // Not even "..." fits: draw nothing.
    assert_eq!(fit_tail("abcdef", 3), None);
    assert_eq!(fit_tail("abcdef", 0), None);
}

/// Put the cursor on the text origin of box i (rendering tests).
fn set_at(d: &Document, view: &mut View, i: usize) {
    view.pick = Pick::Box(i);
    view.cursor = d
        .text(i)
        .map(|t| t.at)
        .unwrap_or((d.boxes[i].at.0 + 1, d.boxes[i].at.1 + 1));
}

fn grp(id: &str, label: &str, color: (u8, u8, u8)) -> Group {
    Group {
        id: id.into(),
        label: label.into(),
        color,
    }
}

fn lyr(name: &str, order: i32) -> Layer {
    Layer {
        name: name.into(),
        order,
    }
}

fn bx(id: &str, at: (i32, i32), size: (u16, u16), style: BoxStyle) -> Box {
    Box {
        id: id.into(),
        at,
        size,
        style,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    }
}

fn txt(id: &str, s: &str, at: (i32, i32), box_id: &str) -> Text {
    Text {
        id: id.into(),
        text: s.into(),
        at,
        box_id: box_id.into(),
        color: None,
        bg: None,
        align: Align::Left,
        layer: String::new(),
    }
}

fn edg(id: &str, from: &str, to: &str, dir: Dir, group: &str) -> DocEdge {
    DocEdge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        dir,
        group: group.into(),
        style: EdgeStyle::Solid,
        color: None,
        bg: None,
        via: Vec::new(),
        corner: Corner::Rounded,
        layer: String::new(),
        from_side: None,
        to_side: None,
        head: None,
    }
}

fn doc_of(
    groups: Vec<Group>,
    layers: Vec<Layer>,
    boxes: Vec<Box>,
    texts: Vec<Text>,
    edges: Vec<DocEdge>,
) -> Document {
    let mut layers = layers;
    if !layers.iter().any(|l| l.name.is_empty()) {
        layers.insert(0, lyr("", 0));
    }
    Document::from_parts(
        "graph-tui test",
        groups,
        layers,
        boxes,
        texts,
        edges,
        ViewState::default(),
    )
    .expect("样例文档")
}

/// One boxed node plus one point node, joined by a directed edge with a bend.
fn sample() -> Document {
    let mut e = edg("e1", "b1", "b2", Dir::Forward, "g0");
    e.via = vec![(4, 6)];
    doc_of(
        vec![grp("g0", "主线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("b1", (2, 2), (9, 3), BoxStyle::Rounded),
            bx("b2", (2, 8), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "中央", (3, 3), "b1"),
            txt("t2", "点", (2, 9), "b2"),
        ],
        vec![e],
    )
}

/// Three point nodes in an L: 甲 and 乙 share a line, 丙 sits below.
fn points() -> Document {
    doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("pa", (0, 0), (1, 1), BoxStyle::Point),
            bx("pb", (10, 0), (1, 1), BoxStyle::Point),
            bx("pc", (20, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "甲", (0, 1), "pa"),
            txt("tb", "乙", (10, 1), "pb"),
            txt("tc", "丙", (20, 11), "pc"),
        ],
        vec![edg("e1", "pa", "pb", Dir::Forward, "g0")],
    )
}

/// Read the picture as text: skip the continuation cell of a wide glyph.
fn screen(buf: &Buffer) -> Vec<String> {
    let a = buf.area;
    (a.y..a.y + a.height)
        .map(|y| {
            let mut line = String::new();
            let mut x = a.x;
            while x < a.x + a.width {
                let sym = buf[(x, y)].symbol();
                line.push_str(sym);
                x += width(sym).max(1) as u16;
            }
            line
        })
        .collect()
}

fn last_line(buf: &Buffer) -> String {
    screen(buf).pop().expect("有最后一行")
}

/// The box is drawn at `at`: its top-left cell holds the corner glyph, not a centre-derived cell.
#[test]
fn box_drawn_at_top_left_cell() {
    let d = sample();
    let area = Rect::new(0, 0, 80, 30);
    let mut view = View::default();
    set_at(&d, &mut view, 1); // Cursor on the point node, so the box itself stays clear.
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    let at = |dx: i32, dy: i32| buf[((x + dx) as u16, (y + dy) as u16)].symbol().to_string();
    assert_eq!(at(0, 0), "╭", "左上角就是 at");
    assert_eq!(at(8, 0), "╮");
    assert_eq!(at(0, 2), "╰");
    assert_eq!(at(8, 2), "╯");
    // Text is drawn at its own `at` (box top-left + (1, 1)).
    let (tx, ty) = p.proj.cell((3.0, 3.0));
    assert_eq!((tx, ty), (x + 1, y + 1));
    assert_eq!(
        buf[(tx as u16, ty as u16)].symbol(),
        "中",
        "文字该在它自己的 at 上"
    );
    // Deriving the origin from the centre would put the box at (6, 3).
    let (cx, cy) = p.proj.cell((6.0, 3.0));
    assert_ne!((cx, cy), (x, y));
}

/// A point node takes one cell and draws a circle; one shared by two groups draws a bullseye.
#[test]
fn point_node_occupies_one_cell() {
    let d = doc_of(
        vec![grp("g0", "一", (255, 0, 0)), grp("g1", "二", (0, 255, 0))],
        vec![lyr("", 0)],
        vec![
            bx("p1", (5, 5), (1, 1), BoxStyle::Point),
            bx("p2", (15, 5), (1, 1), BoxStyle::Point),
            bx("p3", (5, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "a", (5, 6), "p1"),
            txt("t2", "b", (15, 6), "p2"),
            txt("t3", "c", (5, 11), "p3"),
        ],
        vec![
            edg("e1", "p1", "p2", Dir::Forward, "g0"),
            edg("e2", "p3", "p2", Dir::Forward, "g1"),
        ],
    );
    assert!(d.hub(1), "p2 经过两个分组");
    let area = Rect::new(0, 0, 60, 24);
    let mut view = View::default();
    set_at(&d, &mut view, 2);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((5.0, 5.0));
    assert_eq!(buf[(x as u16, y as u16)].symbol(), "○");
    let (hx, hy) = p.proj.cell((15.0, 5.0));
    assert_eq!(buf[(hx as u16, hy as u16)].symbol(), "◎", "枢纽");
    // Only one cell: the cells to either side are not node glyphs.
    for dx in [-1i32, 1] {
        let sym = buf[((x + dx) as u16, y as u16)].symbol();
        assert!(!matches!(sym, "○" | "◎" | "●" | "◉"), "点节点只占一格");
    }
}

/// `cursor = drawn`: the point node under the cursor becomes a solid glyph (the terminal
/// cursor is not used at all in that mode)
#[test]
fn drawn_cursor_solidifies_the_node_under_it() {
    let d = points();
    let area = Rect::new(0, 0, 40, 12);
    let mut view = View::default();
    view.cursor = (0, 0);
    view.drawn = false; // 交互模式默认用终端光标,画面里不画
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((0.0, 0.0));
    assert_eq!(buf[(x as u16, y as u16)].symbol(), "○", "终端光标模式不画");
    view.drawn = true;
    let mut drawn = Buffer::empty(area);
    render(&mut drawn, area, &d, &view);
    assert_eq!(drawn[(x as u16, y as u16)].symbol(), "●", "画进画面后实心");
}

/// `cursor = drawn` on a text-only box: there is no glyph or border to paint, so the text
/// under the cursor brightens
#[test]
fn drawn_cursor_brightens_a_text_box() {
    let d = doc_of(
        vec![grp("g0", "x", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (2, 1), BoxStyle::None)],
        vec![txt("ta", "hi", (0, 0), "a")],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 12);
    let mut view = View::default();
    view.cursor = (0, 0);
    view.drawn = false;
    let mut plain = Buffer::empty(area);
    let p = render(&mut plain, area, &d, &view);
    let (x, y) = p.proj.cell((0.0, 0.0));
    let before = plain[(x as u16, y as u16)].clone();
    assert_eq!(before.symbol(), "h", "文字该在光标格");
    view.drawn = true;
    let mut drawn = Buffer::empty(area);
    render(&mut drawn, area, &d, &view);
    let after = drawn[(x as u16, y as u16)].clone();
    assert_eq!(after.symbol(), "h", "字形不变,只是反显");
    assert!(
        after.modifier.contains(ratatui::style::Modifier::REVERSED),
        "该反显:{after:?}"
    );
    assert!(
        after.modifier.contains(ratatui::style::Modifier::BOLD),
        "该加粗:{after:?}"
    );
    assert!(!before.modifier.contains(ratatui::style::Modifier::REVERSED));
}

/// Directed edge: the head follows `dir`; a boxed target gets it on the cell just outside.
/// Per-edge head glyph: one picture can mix triangles with plain arrows.
#[test]
fn per_edge_head_glyph() {
    let area = Rect::new(0, 0, 60, 20);
    let mut tri = edg("e1", "a", "b", Dir::Forward, "g0");
    tri.head = Some(crate::model::ArrowStyle::Triangles);
    let mut arr = edg("e2", "c", "d", Dir::Forward, "g0");
    arr.head = Some(crate::model::ArrowStyle::Arrows);
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (1, 1), BoxStyle::Point),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
            bx("c", (0, 5), (1, 1), BoxStyle::Point),
            bx("d", (10, 5), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "a", (0, 2), "a"),
            txt("tb", "b", (10, 2), "b"),
            txt("tc", "c", (0, 6), "c"),
            txt("td", "d", (10, 6), "d"),
        ],
        vec![tri, arr],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &doc, &View::default());
    let (tx, ty) = p.proj.cell((10.0, 1.0));
    let (ax, ay) = p.proj.cell((10.0, 5.0));
    assert_eq!(buf[((tx - 1) as u16, ty as u16)].symbol(), "▶");
    assert_eq!(buf[((ax - 1) as u16, ay as u16)].symbol(), "→");
    // Edges without an explicit head use the global setting.
    let plain = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (1, 1), BoxStyle::Point),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 2), "a"), txt("tb", "b", (10, 2), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &plain, &View::default());
    let (ex, ey) = p.proj.cell((10.0, 1.0));
    assert_eq!(buf[((ex - 1) as u16, ey as u16)].symbol(), ">");
    // ASCII is a hard compatibility mode: even `head = triangles` draws `^ v < >`
    let mut ascii_tri = plain.clone();
    ascii_tri.edges[0].head = Some(crate::model::ArrowStyle::Triangles);
    let mut view = View {
        glyphs: crate::glyphs::Glyphs::ASCII,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &ascii_tri, &view);
    let (ex, ey) = p.proj.cell((10.0, 1.0));
    assert_eq!(
        buf[((ex - 1) as u16, ey as u16)].symbol(),
        ">",
        "ASCII 压过单边样式"
    );
    // The same edge in Unicode keeps its triangle head
    view.glyphs = crate::glyphs::Glyphs::UNICODE;
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &ascii_tri, &view);
    assert_eq!(
        buf[((ex - 1) as u16, ey as u16)].symbol(),
        "▶",
        "Unicode 按单边 head"
    );
}

#[test]
fn arrow_lands_on_border_by_dir() {
    let area = Rect::new(0, 0, 60, 20);
    // Point-node target: the head sits one cell before the end.
    let plain_target = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (1, 1), BoxStyle::Point),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 2), "a"), txt("tb", "b", (10, 2), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &plain_target, &View::default());
    let (ex, ey) = p.proj.cell((10.0, 1.0));
    assert_eq!(buf[((ex - 1) as u16, ey as u16)].symbol(), ">");
    assert_eq!(buf[(ex as u16, ey as u16)].symbol(), "○", "终点本身是节点");
    // Boxed target: the head is embedded in the left border cell.
    let boxed_target = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (1, 1), BoxStyle::Point),
            bx("b", (10, 0), (4, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (0, 2), "a"), txt("tb", "bb", (11, 1), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &boxed_target, &View::default());
    let (lx, ty) = p.proj.cell((10.0, 0.0));
    let (_, my) = p.proj.cell((11.0, 1.0));
    assert_eq!(buf[(lx as u16, ty as u16)].symbol(), "┌", "框的左上角");
    assert_eq!(
        buf[(lx as u16, my as u16)].symbol(),
        ">",
        "箭头该嵌在左边框上"
    );
    assert_eq!(
        buf[((lx - 1) as u16, my as u16)].symbol(),
        "─",
        "框外不再画箭头"
    );
    // Undirected edges draw no head.
    let plain_edge = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (1, 1), BoxStyle::Point),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 2), "a"), txt("tb", "b", (10, 2), "b")],
        vec![edg("e1", "a", "b", Dir::None, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &plain_edge, &View::default());
    let (bx, by) = p.proj.cell((10.0, 1.0));
    assert_eq!(buf[((bx - 1) as u16, by as u16)].symbol(), "─");
}

/// Connect mode: steady source, breathing target, count in status line, no T/head in preview.
#[test]
fn connect_mode_three_markers() {
    let area = Rect::new(0, 0, 80, 24);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 3), (6, 3), BoxStyle::Single),
            bx("b", (2, 12), (6, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (3, 4), "a"), txt("tb", "b", (3, 13), "b")],
        vec![],
    );
    let mut view = View {
        lang: Lang::Zh,
        ..View::default()
    };
    view.pick = Pick::Box(1);
    view.link = Some(crate::app::LinkState {
        sources: vec![0],
        targets: vec![1],
        confirming: false,
    });
    view.pulse = 0.0;
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        (sx as u16, sy as u16)
    };
    // Source box: its top-left corner uses the steady highlight colour.
    let src = &buf[at(2, 3)];
    assert_eq!(src.symbol(), "┌");
    assert_eq!(src.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
    // Target box: the same cell uses the pulse colour, different from the source.
    let dst = &buf[at(2, 12)];
    assert_eq!(dst.symbol(), "┌");
    assert_ne!(dst.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
    // Preview line: dim blue-grey, and the target border keeps no T/head overlay.
    let mut preview = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if buf[(x, y)].fg == Color::Rgb(PREVIEW.0, PREVIEW.1, PREVIEW.2) {
                preview += 1;
            }
        }
    }
    assert!(preview > 0, "该画出预览线");
    // The status line reports the count (second from the bottom with two rows).
    // A wide glyph spans two cells, the second blank: strip whitespace before comparing.
    let row: String = status_line(&buf)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(row.contains("连边:源1->目标1"), "状态栏该有源和目标:{row}");
    assert!(row.contains("将建1条"), "状态栏该报数量:{row}");
}

/// The preview draws at most 64 lines; beyond that only the count is shown.
#[test]
fn connect_preview_caps_at_64() {
    let area = Rect::new(0, 0, 120, 40);
    let n: i32 = 18;
    let boxes: Vec<_> = (0..n)
        .map(|i| {
            bx(
                &format!("b{i}"),
                ((i % 9) * 12, (i / 9) * 8),
                (6, 3),
                BoxStyle::Single,
            )
        })
        .collect();
    let texts: Vec<_> = (0..n)
        .map(|i| {
            txt(
                &format!("t{i}"),
                "x",
                ((i % 9) * 12 + 1, (i / 9) * 8 + 1),
                &format!("b{i}"),
            )
        })
        .collect();
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        boxes,
        texts,
        vec![],
    );
    let mut view = View::default();
    view.link = Some(crate::app::LinkState {
        sources: (0..9).collect(),
        targets: (9..18).collect(),
        confirming: false,
    });
    let p = panels(&d, area, &view);
    let mut buf = Buffer::empty(area);
    let drawn = draw_link_preview(&mut buf, &p, &d, &view);
    assert_eq!(drawn, 64, "9x9 只画 64 条预览");
}

/// none: no border, text only; edges still stop outside the region, the head just outside.
#[test]
fn none_node_draws_text_only() {
    let area = Rect::new(0, 0, 40, 14);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (4, 3), BoxStyle::Single),
            bx("t", (10, 3), (4, 1), BoxStyle::None),
        ],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "文字", (10, 3), "t")],
        vec![edg("e1", "a", "t", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    // Text sits at the region origin, no border (a wide glyph's second cell is blank).
    assert_eq!(at(10, 3), "文");
    assert_eq!(at(12, 3), "字");
    // The edge comes in from the left: its head lands just outside the region.
    assert_eq!(at(9, 3), ">", "箭头该在区域外");
    // The region itself must show no border glyphs.
    for x in 10..14 {
        let ch = at(x, 3);
        assert!(
            !"┌┐└┘├┤┬┴┼─│╭╮╯╰".contains(&ch),
            "({x},3) 出现了框线 {ch:?}"
        );
    }
}

/// Text fill and edge fill are independent and never overwrite each other.
#[test]
fn text_and_edge_fills_independent() {
    let area = Rect::new(0, 0, 40, 14);
    let mut a = bx("a", (0, 3), (4, 3), BoxStyle::Single);
    a.fill = Some((0x11, 0x11, 0x11));
    let t = bx("t", (10, 3), (4, 1), BoxStyle::None);
    let mut tb = txt("tb", "文字", (10, 3), "t");
    tb.bg = Some((0x33, 0x44, 0x55));
    let mut e = edg("e1", "a", "t", Dir::Forward, "g0");
    e.bg = Some((0x55, 0x22, 0x11));
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![a, t],
        vec![txt("ta", "a", (1, 4), "a"), tb],
        vec![e],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].clone()
    };
    // Text: its own fill.
    assert_eq!(at(10, 3).symbol(), "文");
    assert_eq!(at(10, 3).bg, Color::Rgb(0x33, 0x44, 0x55), "文字底色");
    // Cells the edge crosses: the edge fill.
    let (ex, ey) = (9, 3);
    assert_eq!(at(ex, ey).bg, Color::Rgb(0x55, 0x22, 0x11), "边底色");
    // Inside the box: the fill is untouched, no bleed from the edge fill.
    assert_eq!(at(1, 4).bg, Color::Rgb(0x11, 0x11, 0x11), "框的填充");
}

/// Port routing: odd width aligns top/bottom, zero bends, T and head in the same column.
/// Compute the cell of one endpoint in the PortPlan (test helper).
fn plan_cell(d: &Document, box_i: usize, arrow: bool, e: usize) -> Option<(i32, i32)> {
    let area = Rect::new(0, 0, 60, 20);
    let view = View::default();
    let p = panels(d, area, &view);
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(d, i, &p.proj)).collect();
    plan_ports(d, &p.proj, &rects, "")
        .get(box_i, arrow, e)
        .map(|(_, c)| c)
}

/// Changing the box style keeps the ports: anchors come from the text, the box only swaps glyphs.
#[test]
fn box_style_does_not_move_ports() {
    let make = |style: BoxStyle| {
        doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 2), (6, 3), style),
                bx("b", (2, 9), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "ab", (3, 3), "a"), txt("tb", "b", (2, 10), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        )
    };
    let one = make(BoxStyle::Single);
    let two = make(BoxStyle::Rounded);
    assert_eq!(
        plan_cell(&one, 0, false, 0),
        plan_cell(&two, 0, false, 0),
        "换框型端口不该动"
    );
}

/// Borderless text node: with enough room each edge takes a virtual cell, head just outside.
#[test]
fn text_node_ports_one_cell_each() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (11, 0), (1, 1), BoxStyle::Point),
            bx("b", (14, 0), (1, 1), BoxStyle::Point),
            bx("c", (17, 0), (1, 1), BoxStyle::Point),
            bx("t", (10, 6), (8, 1), BoxStyle::None),
        ],
        vec![
            txt("ta", "a", (11, 1), "a"),
            txt("tb", "b", (14, 1), "b"),
            txt("tc", "c", (17, 1), "c"),
            txt("tt", "ABCDEFGH", (10, 6), "t"),
        ],
        vec![
            edg("e1", "a", "t", Dir::Forward, "g0"),
            edg("e2", "b", "t", Dir::Forward, "g0"),
            edg("e3", "c", "t", Dir::Forward, "g0"),
        ],
    );
    let cells: Vec<(u8, (i32, i32))> = (0..3)
        .map(|e| {
            let area = Rect::new(0, 0, 60, 20);
            let view = View::default();
            let p = panels(&d, area, &view);
            let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
                .map(|i| rect_of(&d, i, &p.proj))
                .collect();
            plan_ports(&d, &p.proj, &rects, "")
                .get(3, true, e)
                .expect("有端口")
        })
        .collect();
    assert!(
        cells.iter().all(|&(s, _)| s == UP),
        "箭头该落在文字上方:{cells:?}"
    );
    let mut set: Vec<(i32, i32)> = cells.iter().map(|&(_, c)| c).collect();
    set.sort_unstable();
    set.dedup();
    assert_eq!(set.len(), 3, "三条边该各占一格:{cells:?}");
}

/// Three edges entering one side of a wide box: one cell each.
fn three_arrow_doc(extra: bool) -> Document {
    let mut edges = vec![
        edg("e1", "a", "t", Dir::Forward, "g0"),
        edg("e2", "b", "t", Dir::Forward, "g0"),
        edg("e3", "c", "t", Dir::Forward, "g0"),
    ];
    if extra {
        edges.push(edg("e4", "d", "t", Dir::Forward, "g0"));
    }
    doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 0), (1, 1), BoxStyle::Point),
            bx("b", (13, 0), (1, 1), BoxStyle::Point),
            bx("c", (16, 0), (1, 1), BoxStyle::Point),
            bx("d", (19, 0), (1, 1), BoxStyle::Point),
            bx("t", (8, 6), (12, 3), BoxStyle::Single),
        ],
        vec![
            txt("ta", "a", (10, 1), "a"),
            txt("tb", "b", (13, 1), "b"),
            txt("tc", "c", (16, 1), "c"),
            txt("td", "d", (19, 1), "d"),
            txt("tt", "target", (9, 7), "t"),
        ],
        edges,
    )
}

/// Adding an edge leaves existing ports alone: assignment just walks the array order.
#[test]
fn adding_edge_keeps_ports() {
    let two = three_arrow_doc(false);
    let three = three_arrow_doc(true);
    let before: Vec<_> = (0..3).map(|e| plan_cell(&two, 4, true, e)).collect();
    let after: Vec<_> = (0..3).map(|e| plan_cell(&three, 4, true, e)).collect();
    assert_eq!(
        before, after,
        "加边把已有端口挤走了:{before:?} -> {after:?}"
    );
}

/// Removing the last edge leaves every remaining port where it was.
#[test]
fn removing_edge_keeps_ports() {
    let two = three_arrow_doc(false);
    let three = three_arrow_doc(true);
    // Dropping e4 returns to the three-edge document.
    let before: Vec<_> = (0..3).map(|e| plan_cell(&three, 4, true, e)).collect();
    let after: Vec<_> = (0..3).map(|e| plan_cell(&two, 4, true, e)).collect();
    assert_eq!(
        before, after,
        "删边把别的端口挤走了:{before:?} -> {after:?}"
    );
}

/// Two runs over one document give a cell-for-cell identical port table.
#[test]
fn port_assignment_is_deterministic() {
    let d = three_arrow_doc(true);
    let area = Rect::new(0, 0, 60, 24);
    let view = View::default();
    let p = panels(&d, area, &view);
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let a = plan_ports(&d, &p.proj, &rects, "");
    let b = plan_ports(&d, &p.proj, &rects, "");
    assert!(a == b, "两次分配结果不同");
}

/// When a side is full: heads move to another side, lines tee into the nearest one.
#[test]
fn full_side_moves_arrow_and_merges_line() {
    // Target box is 4 wide: the top side has 2 inner cells but must take 3 edges.
    let make = |dir: Dir| {
        doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (10, 0), (1, 1), BoxStyle::Point),
                bx("b", (13, 0), (1, 1), BoxStyle::Point),
                bx("c", (16, 0), (1, 1), BoxStyle::Point),
                bx("t", (10, 6), (4, 3), BoxStyle::Single),
            ],
            vec![
                txt("ta", "a", (10, 1), "a"),
                txt("tb", "b", (13, 1), "b"),
                txt("tc", "c", (16, 1), "c"),
                txt("tt", "xy", (11, 7), "t"),
            ],
            vec![
                edg("e1", "a", "t", dir, "g0"),
                edg("e2", "b", "t", dir, "g0"),
                edg("e3", "c", "t", dir, "g0"),
            ],
        )
    };
    let area = Rect::new(0, 0, 60, 24);
    let view = View::default();
    let plan_of = |d: &Document| {
        let p = panels(d, area, &view);
        let rects: Vec<(i32, i32, i32, i32)> =
            (0..d.boxes.len()).map(|i| rect_of(d, i, &p.proj)).collect();
        plan_ports(d, &p.proj, &rects, "")
    };
    // Three heads: after the top side's two cells, the third moves elsewhere.
    let arrows = plan_of(&make(Dir::Forward));
    let sides: Vec<u8> = (0..3)
        .map(|e| arrows.get(3, true, e).expect("有端口").0)
        .collect();
    assert!(sides.iter().any(|&s| s != UP), "一侧挤满该换侧:{sides:?}");
    // Three plain links: weak direction, so one moves off when over capacity.
    let lines = plan_of(&make(Dir::None));
    let got: Vec<(u8, (i32, i32))> = (0..3)
        .map(|e| lines.get(3, false, e).expect("有端口"))
        .collect();
    let stay = got.iter().filter(|(s, _)| *s == UP).count();
    assert_eq!(stay, 2, "上边两个格,应该留两条、挪走一条:{got:?}");
}

/// Three edges on one side: on a wide enough box each gets its own cell.
#[test]
fn three_edges_one_cell_each() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 0), (1, 1), BoxStyle::Point),
            bx("b", (14, 0), (1, 1), BoxStyle::Point),
            bx("c", (18, 0), (1, 1), BoxStyle::Point),
            bx("t", (10, 6), (8, 3), BoxStyle::Single),
        ],
        vec![
            txt("ta", "a", (10, 1), "a"),
            txt("tb", "b", (14, 1), "b"),
            txt("tc", "c", (18, 1), "c"),
            txt("tt", "target", (11, 7), "t"),
        ],
        vec![
            edg("e1", "a", "t", Dir::Forward, "g0"),
            edg("e2", "b", "t", Dir::Forward, "g0"),
            edg("e3", "c", "t", Dir::Forward, "g0"),
        ],
    );
    let cells: Vec<_> = (0..3)
        .map(|e| plan_cell(&d, 3, true, e).expect("有端口"))
        .collect();
    assert_ne!(cells[0], cells[1], "三条边该各占一格:{cells:?}");
    assert_ne!(cells[1], cells[2], "三条边该各占一格:{cells:?}");
    assert_ne!(cells[0], cells[2], "三条边该各占一格:{cells:?}");
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let r = rect_of(&d, 3, &p.proj);
    // Three heads, one cell each: three head glyphs along the top row.
    let arrows = (r.0..r.0 + r.2)
        .filter(|&x| buf[(x as u16, r.1 as u16)].symbol() == "v")
        .count();
    assert_eq!(arrows, 3, "三个箭头该各占一格");
}

/// One in (head) and one out (plain link) on one side: separate cells, T keeps the head.
#[test]
fn arrow_and_plain_link_one_cell_each() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("p", (12, 0), (1, 1), BoxStyle::Point),
            bx("q", (12, 12), (1, 1), BoxStyle::Point),
            bx("t", (10, 6), (8, 3), BoxStyle::Single),
        ],
        vec![
            txt("tp", "p", (12, 1), "p"),
            txt("tq", "q", (12, 13), "q"),
            txt("tt", "target", (11, 7), "t"),
        ],
        vec![
            edg("e1", "p", "t", Dir::Forward, "g0"),
            edg("e2", "t", "q", Dir::Forward, "g0"),
        ],
    );
    let up_arrow = plan_cell(&d, 2, true, 0).expect("进线端口");
    let down_line = plan_cell(&d, 2, false, 1).expect("出线端口");
    assert_ne!(up_arrow, down_line, "上下两侧各用各的口");
    // One port per side group: another incoming line shares the existing one.
    let both = plan_cell(&d, 2, true, 0).unwrap();
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let _ = p;
    assert_eq!(
        buf[(both.0 as u16, both.1 as u16)].symbol(),
        "v",
        "进线那格是箭头"
    );
}

/// Explicit ports still apply: two edges pinned to one side each get a cell.
#[test]
fn explicit_ports_follow_two_rules() {
    let mut e1 = edg("e1", "a", "t", Dir::Forward, "g0");
    e1.from_side = Some(crate::model::Side::Right);
    e1.to_side = Some(crate::model::Side::Up);
    let mut e2 = edg("e2", "b", "t", Dir::Forward, "g0");
    e2.from_side = Some(crate::model::Side::Left);
    e2.to_side = Some(crate::model::Side::Up);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (20, 4), (1, 1), BoxStyle::Point),
            bx("b", (0, 4), (1, 1), BoxStyle::Point),
            bx("t", (10, 8), (8, 3), BoxStyle::Single),
        ],
        vec![
            txt("ta", "a", (20, 5), "a"),
            txt("tb", "b", (0, 5), "b"),
            txt("tt", "target", (11, 9), "t"),
        ],
        vec![e1, e2],
    );
    let up0 = plan_cell(&d, 2, true, 0).expect("显式端口");
    let up1 = plan_cell(&d, 2, true, 1).expect("显式端口");
    assert_ne!(up0, up1, "两条边各一格:{up0:?} {up1:?}");
    let r = rect_of(
        &d,
        2,
        &panels(&d, Rect::new(0, 0, 60, 20), &View::default()).proj,
    );
    assert!(up0.1 == r.1 && up1.1 == r.1, "都该落在上边");
}

/// The port sits on the text centre: even a 5-wide single-glyph box ports under the glyph.
#[test]
fn odd_width_port_aligns_to_content() {
    let area = Rect::new(0, 0, 40, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (5, 3), BoxStyle::Single),
            bx("b", (0, 9), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (1, 10), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    // Text at (1,4): the nearest bottom cell is (1,5), not the box centre column.
    assert_eq!(at(1, 5), "┬", "起点 T");
    assert_eq!(at(1, 9), "v", "终点箭头");
    for y in 6..9 {
        assert_eq!(at(1, y), "│", "第 {y} 行该是直线");
    }
    assert_eq!(at(2, 5), "─", "底边其余是边框");
}

/// Port routing: even width is off by one row, two bends, jog hugging the target end.
/// Facing but content columns differ: both ends align on one column, one straight run between.
#[test]
fn facing_boxes_align_ports() {
    let area = Rect::new(0, 0, 40, 24);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (6, 3), BoxStyle::Single),
            bx("b", (0, 12), (6, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "ab", (1, 4), "a"), txt("tb", "b", (2, 13), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    // Exit: the T on the upper box's bottom border; entry: the head on the lower box's top.
    let port = (1..5).find(|&x| at(x, 5) == "┬").expect("出口有个 T");
    let arrow = (1..5).find(|&x| at(x, 12) == "v").expect("目标那边有箭头");
    assert_eq!(port, arrow, "两端该对齐到同一列");
    for y in 6..12 {
        assert_eq!(at(port, y), "│", "第 {y} 行该是直的");
    }
    assert_eq!(at(port + 1, 9), " ", "旁边不该有线");
}

/// Both ends port on their content centre: differing columns end in a one-cell final jog.
#[test]
fn horizontal_port_offset() {
    let area = Rect::new(0, 0, 40, 14);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 2), (6, 3), BoxStyle::Single),
            bx("b", (20, 5), (6, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (3, 3), "a"), txt("tb", "b", (21, 6), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    // a's right side uses top+h/2 = 3; b's left uses top+(h-1)/2 = 6.
    assert_eq!(at(7, 3), "├", "起点 T");
    assert_eq!(at(20, 6), ">", "箭头嵌在左边框");
    assert_eq!(at(18, 3), "╮", "竖直 jog 的起点");
    assert_eq!(at(18, 6), "╰", "竖直 jog 的终点");
    for x in 8..18 {
        assert_eq!(at(x, 3), "─", "第 {x} 列走在源行");
    }
}

/// Port routing: swap left/right; the chosen side flips with it.
#[test]
fn swapped_horizontal_sides() {
    let area = Rect::new(0, 0, 40, 14);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (20, 5), (6, 3), BoxStyle::Single),
            bx("b", (2, 2), (6, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (21, 6), "a"), txt("tb", "b", (3, 3), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    assert_eq!(at(20, 6), "┤", "a 改用左边");
    assert_eq!(at(7, 3), "<", "b 的右边框嵌入箭头");
}

/// Port routing: swap up/down; the chosen side flips to top/bottom.
/// Up/down swapped: both ends land in their own text column.
#[test]
fn swapped_vertical_sides() {
    let area = Rect::new(0, 0, 40, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 10), (5, 3), BoxStyle::Single),
            bx("b", (0, 3), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 11), "a"), txt("tb", "b", (1, 4), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    assert_eq!(at(1, 10), "┴", "a 改用上边");
    assert_eq!(at(1, 5), "^", "b 的底边嵌入朝上的箭头");
    for y in 6..10 {
        assert_eq!(at(1, y), "│", "第 {y} 行是直线");
    }
}

/// Manual via point: leave along the source normal, run across to the via column, drop in.
#[test]
fn via_point_port_route() {
    let area = Rect::new(0, 0, 40, 26);
    let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
    e.via = vec![(11, 8)];
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (5, 3), BoxStyle::Single),
            bx("b", (10, 18), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (11, 19), "b")],
        vec![e],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let at = |x: i32, y: i32| {
        let (sx, sy) = p.proj.cell((x as f64, y as f64));
        buf[(sx as u16, sy as u16)].symbol().to_string()
    };
    // The source port is under the box: go straight down to the via row.
    let port = (1..4).find(|&x| at(x, 5) == "┬").expect("出口有个 T");
    for y in 6..8 {
        assert_eq!(at(port, y), "│", "第 {y} 行沿源法线");
    }
    assert_eq!(at(port, 8), "╰", "转到 via 那一行");
    for x in (port + 1)..11 {
        assert_eq!(at(x, 8), "─", "第 {x} 列横着走");
    }
    assert_eq!(at(11, 8), "╮", "到 via 后转向下");
    assert_eq!(at(11, 17), "│", "末段竖着进目标");
    assert_eq!(at(11, 18), "v", "箭头");
}

/// Directed edge: the T sits on the source border, the head embeds in the target border.
#[test]
fn border_tee_and_embedded_arrow() {
    let area = Rect::new(0, 0, 60, 20);
    for (dir, want_a, want_b) in [
        (Dir::None, '├', '┤'),
        (Dir::Forward, '├', '>'),
        (Dir::Back, '<', '┤'),
        (Dir::Both, '<', '>'),
    ] {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (5, 3), BoxStyle::Single),
                bx("b", (12, 3), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
            vec![edg("e1", "a", "b", dir, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((4.0, 4.0));
        let (bx_, by_) = p.proj.cell((12.0, 4.0));
        assert_eq!(
            buf[(ax as u16, ay as u16)].symbol(),
            want_a.to_string(),
            "{dir:?} 的起点"
        );
        assert_eq!(
            buf[(bx_ as u16, by_ as u16)].symbol(),
            want_b.to_string(),
            "{dir:?} 的终点"
        );
    }
}

/// Thick boxes use the thick T glyph.
#[test]
fn thick_box_tee_uses_thick_glyph() {
    let area = Rect::new(0, 0, 60, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (5, 3), BoxStyle::Heavy),
            bx("b", (12, 3), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ax, ay) = p.proj.cell((4.0, 4.0));
    assert_eq!(buf[(ax as u16, ay as u16)].symbol(), "┣");
}

/// Colour: the tee takes the border colour, the embedded head the edge colour; both keep the fill.
#[test]
fn tee_and_arrow_color_and_fill() {
    let area = Rect::new(0, 0, 60, 20);
    let mut a = bx("a", (0, 3), (5, 3), BoxStyle::Single);
    a.color = Some((10, 20, 30));
    a.fill = Some((5, 6, 7));
    let mut b = bx("b", (12, 3), (5, 3), BoxStyle::Single);
    b.fill = Some((5, 6, 7));
    let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
    e.color = Some((200, 30, 40));
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![a, b],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
        vec![e],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ax, ay) = p.proj.cell((4.0, 4.0));
    let (bx_, by_) = p.proj.cell((12.0, 4.0));
    let ta = &buf[(ax as u16, ay as u16)];
    let ar = &buf[(bx_ as u16, by_ as u16)];
    assert_eq!(ta.symbol(), "├");
    assert_eq!(
        ta.fg,
        Color::Rgb(200, 30, 40),
        "T 用这条边的颜色,和箭头一致"
    );
    assert_eq!(ta.bg, Color::Rgb(5, 6, 7), "T 保留填充底色");
    assert_eq!(ar.symbol(), ">");
    assert_eq!(ar.fg, Color::Rgb(200, 30, 40), "箭头用边的颜色");
    assert_eq!(ar.bg, Color::Rgb(5, 6, 7), "箭头保留填充底色");
}

/// No room for a second head on a side: skip it, no error and no drawing elsewhere.
#[test]
fn no_free_cell_no_arrow() {
    let area = Rect::new(0, 0, 60, 20);
    // Box height is 3, so each side has one cell: the third head has nowhere to go.
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("p", (0, 4), (1, 1), BoxStyle::Point),
            bx("q", (0, 6), (1, 1), BoxStyle::Point),
            bx("r", (0, 8), (1, 1), BoxStyle::Point),
            bx("b", (12, 5), (6, 3), BoxStyle::Single),
        ],
        vec![
            txt("tp", "p", (0, 5), "p"),
            txt("tq", "q", (0, 7), "q"),
            txt("tr", "r", (0, 9), "r"),
            txt("tb", "b", (13, 6), "b"),
        ],
        vec![
            edg("e1", "p", "b", Dir::Forward, "g0"),
            edg("e2", "q", "b", Dir::Forward, "g0"),
            edg("e3", "r", "b", Dir::Forward, "g0"),
        ],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    // Only the middle row can hold a head on the left: two are skipped, no overflow.
    let mut n = 0;
    for y in 5..8 {
        let (sx, sy) = p.proj.cell((12.0, y as f64));
        if buf[(sx as u16, sy as u16)].symbol() == ">" {
            n += 1;
        }
    }
    assert!(n <= 1, "左边只有一格,画了 {n} 个箭头");
}

/// A box pushed off the picture: off-screen tees are skipped, the visible side is normal.
#[test]
fn box_off_screen_is_safe() {
    let area = Rect::new(0, 0, 24, 10);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 2), (5, 3), BoxStyle::Single),
            bx("b", (30, 2), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 3), "a"), txt("tb", "b", (31, 3), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    // Centre the view on a: b falls outside, so its head cells must not be drawn.
    let view = View {
        center: Some((2.0, 3.5)),
        ..Default::default()
    };
    render(&mut buf, area, &d, &view);
    let t = (0..area.height).any(|y| (0..area.width).any(|x| buf[(x, y)].symbol() == "├"));
    assert!(t, "可见那侧照常画 T");
}

/// Single-glyph box: candidates come from border capacity (4 inner cells on a 6-wide top),
/// so two heads get a cell each instead of collapsing into one port.
#[test]
fn single_char_box_two_arrows() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (12, 0), (1, 1), BoxStyle::Point),
            bx("c", (16, 0), (1, 1), BoxStyle::Point),
            bx("b", (12, 4), (6, 3), BoxStyle::Single),
        ],
        vec![
            txt("ta", "a", (12, 1), "a"),
            txt("tc", "c", (16, 1), "c"),
            txt("tb", "b", (13, 5), "b"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "c", "b", Dir::Forward, "g0"),
        ],
    );
    let one = plan_cell(&d, 2, true, 0).expect("有端口");
    let two = plan_cell(&d, 2, true, 1).expect("有端口");
    assert_ne!(one, two, "两个箭头不该挤在同一格:{one:?} {two:?}");
    let area = Rect::new(0, 0, 60, 20);
    let view = View::default();
    let p = panels(&d, area, &view);
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let sides: Vec<u8> = (0..2).map(|e| plan.get(2, true, e).unwrap().0).collect();
    assert_eq!(sides[0], sides[1], "上边够宽,两个箭头都在上边:{sides:?}");
}
/// Several heads on one side: a taken cell defers to the nearest free one.
#[test]
fn many_arrows_same_side_one_cell_each() {
    let area = Rect::new(0, 0, 60, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (12, 0), (1, 1), BoxStyle::Point),
            bx("c", (16, 0), (1, 1), BoxStyle::Point),
            bx("b", (12, 4), (6, 3), BoxStyle::Single),
        ],
        vec![
            txt("ta", "a", (12, 1), "a"),
            txt("tc", "c", (16, 1), "c"),
            txt("tb", "bb", (13, 5), "b"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "c", "b", Dir::Forward, "g0"),
        ],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    // b's top spans 13..16; nearest to the content centre (14,5) are 14 then 13.
    let mut arrows = Vec::new();
    for x in 13..17 {
        let (sx, sy) = p.proj.cell((x as f64, 4.0));
        if buf[(sx as u16, sy as u16)].symbol() == "v" {
            arrows.push(x);
        }
    }
    assert_eq!(arrows.len(), 2, "两个箭头该各占一格: {arrows:?}");
    // Each head aligns with its target column: 12 is a corner, so it backs off to 13.
    assert_eq!(arrows, vec![13, 16], "箭头该对齐各自的目标列:{arrows:?}");
}

/// Via point: the head lands on the side matching the direction of the last segment.
#[test]
fn via_point_decides_entry_side() {
    let area = Rect::new(0, 0, 60, 20);
    let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
    e.via = vec![(6, 6), (6, 8)];
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 2), (1, 1), BoxStyle::Point),
            bx("b", (10, 8), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (2, 3), "a"), txt("tb", "b", (11, 9), "b")],
        vec![e],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    // The last segment runs horizontally from (6,8) into b's left side.
    let (lx, my) = p.proj.cell((10.0, 9.0));
    assert_eq!(buf[(lx as u16, my as u16)].symbol(), ">", "从左边进来");
}

/// Double-line boxes use the double T glyph.
#[test]
fn double_box_tee_uses_double_glyph() {
    let area = Rect::new(0, 0, 60, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 3), (5, 3), BoxStyle::Double),
            bx("b", (12, 3), (5, 3), BoxStyle::Double),
        ],
        vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ax, ay) = p.proj.cell((4.0, 4.0));
    assert_eq!(buf[(ax as u16, ay as u16)].symbol(), "╠", "双线框右边是 ╠");
}

/// Cell choice follows width/height parity: a 4-wide top picks the right-middle column.
/// Tee cells align to the content centre: a 4-wide box with a 2-glyph label ports at (11,6).
#[test]
fn tee_cell_centres_on_content() {
    let area = Rect::new(0, 0, 60, 20);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (11, 0), (1, 1), BoxStyle::Point),
            bx("b", (10, 6), (4, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (11, 1), "a"), txt("tb", "bb", (11, 7), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (x, y) = p.proj.cell((11.0, 6.0));
    assert_eq!(buf[(x as u16, y as u16)].symbol(), "v");
    let (x2, y2) = p.proj.cell((12.0, 6.0));
    assert_ne!(buf[(x2 as u16, y2 as u16)].symbol(), "v", "不是正中列");
}

/// Four direction states: 0/1/1/2 heads, at the from end, at the to end.
#[test]
fn four_dir_states_arrow_counts() {
    let area = Rect::new(0, 0, 60, 20);
    for (dir, want) in [
        (Dir::None, (false, false)),
        (Dir::Forward, (false, true)),
        (Dir::Back, (true, false)),
        (Dir::Both, (true, true)),
    ] {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (1, 1), BoxStyle::Point),
                bx("b", (12, 5), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (2, 6), "a"), txt("tb", "b", (12, 6), "b")],
            vec![edg("e1", "a", "b", dir, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((2.0, 5.0));
        let (bx, by) = p.proj.cell((12.0, 5.0));
        assert_eq!(
            buf[((ax + 1) as u16, ay as u16)].symbol() == "<",
            want.0,
            "{} 的 from 端箭头",
            dir.name()
        );
        assert_eq!(
            buf[((bx - 1) as u16, by as u16)].symbol() == ">",
            want.1,
            "{} 的 to 端箭头",
            dir.name()
        );
    }
}

/// Box fill: every cell of the rectangle gets a background, none outside; point nodes skip it.
#[test]
fn fill_covers_box_interior() {
    let area = Rect::new(0, 0, 80, 30);
    let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
    b1.fill = Some((1, 2, 3));
    let mut p1 = bx("p1", (20, 8), (1, 1), BoxStyle::Point);
    p1.fill = Some((1, 2, 3));
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![b1, p1],
        vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
        Vec::new(),
    );
    let mut view = View::default();
    set_at(&d, &mut view, 1);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    let fill = Color::Rgb(1, 2, 3);
    for dy in 0..3 {
        for dx in 0..8 {
            let cell = &buf[((x + dx) as u16, (y + dy) as u16)];
            assert_eq!(cell.bg, fill, "框内 ({dx}, {dy}) 该有填充");
        }
    }
    assert_ne!(buf[((x - 1) as u16, y as u16)].bg, fill, "框左外边不填");
    assert_ne!(buf[(x as u16, (y - 1) as u16)].bg, fill, "框上外边不填");
    assert_ne!(buf[(x as u16, (y + 3) as u16)].bg, fill, "框下外边不填");
    // The text cell keeps its background and the glyph is drawn on top.
    let cell = &buf[((x + 1) as u16, (y + 1) as u16)];
    assert_eq!(cell.symbol(), "a");
    assert_eq!(cell.bg, fill);
    // A point node has no box, so no fill.
    let (px, py) = p.proj.cell((20.0, 8.0));
    assert_ne!(buf[(px as u16, py as u16)].bg, fill, "点节点忽略 fill");
}

/// The cursor box only changes the foreground (pulse colour) and keeps the fill.
#[test]
fn cursor_box_keeps_fill() {
    let area = Rect::new(0, 0, 80, 30);
    let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
    b1.fill = Some((1, 2, 3));
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![b1],
        vec![txt("t1", "ab", (3, 3), "b1")],
        Vec::new(),
    );
    let mut view = View::default();
    set_at(&d, &mut view, 0);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    for dy in 0..3 {
        for dx in 0..8 {
            assert_eq!(
                buf[((x + dx) as u16, (y + dy) as u16)].bg,
                Color::Rgb(1, 2, 3),
                "光标框 ({dx}, {dy}) 也要有填充"
            );
        }
    }
}

/// A wide glyph's continuation cell also keeps the fill: text changes foreground only.
#[test]
fn wide_glyph_continuation_keeps_fill() {
    let area = Rect::new(0, 0, 80, 30);
    let mut b1 = bx("b1", (2, 2), (10, 3), BoxStyle::Rounded);
    b1.fill = Some((1, 2, 3));
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![b1],
        vec![txt("t1", "中文", (3, 3), "b1")],
        Vec::new(),
    );
    let mut view = View::default();
    set_at(&d, &mut view, 0);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    // Interior spans 8 cells: 中 and 文 take two columns each; continuation cells keep the fill.
    for dx in 1..9 {
        assert_eq!(
            buf[((x + dx) as u16, (y + 1) as u16)].bg,
            Color::Rgb(1, 2, 3),
            "内区第 {dx} 格该有填充"
        );
    }
}

/// Selection pulses red/white with no background: boxes flash the border, point nodes the glyph.
#[test]
fn selection_pulses_red_white() {
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("b1", (2, 2), (8, 3), BoxStyle::Rounded),
            bx("p1", (20, 8), (1, 1), BoxStyle::Point),
        ],
        vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
        Vec::new(),
    );
    let area = Rect::new(0, 0, 80, 30);
    let shot = |pick: Pick, cursor: (i32, i32), pulse: f64| {
        let mut view = View::default();
        view.pick = pick;
        view.cursor = cursor;
        view.pulse = pulse;
        view.node_labels = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let (px, py) = p.proj.cell((20.0, 9.0));
        (
            buf[(x as u16, y as u16)].clone(),
            buf[(px as u16, py as u16)].clone(),
        )
    };
    // Selected box: white at pulse=0, red at 1; the background never changes.
    let (white, _) = shot(Pick::Box(0), (3, 3), 0.0);
    let (red, _) = shot(Pick::Box(0), (3, 3), 1.0);
    assert_eq!(
        white.fg,
        Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2),
        "呼吸到白"
    );
    assert_eq!(
        red.fg,
        Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
        "呼吸到红"
    );
    assert_eq!(white.bg, red.bg, "不铺底色");

    // Selected point node: its label is what flashes.
    let (_, label_w) = shot(Pick::Box(1), (20, 8), 0.0);
    let (_, label_r) = shot(Pick::Box(1), (20, 8), 1.0);
    assert_eq!(
        label_w.fg,
        Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2),
        "名字呼吸到白"
    );
    assert_eq!(
        label_r.fg,
        Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
        "名字呼吸到红"
    );
}

/// Self loop: loops around the node, its cells go into the hit table and are clickable.
#[test]
fn self_loop_drawn_and_hittable() {
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 5), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (3, 6), "a"), txt("tb", "b", (30, 5), "b")],
        vec![
            edg("e1", "a", "a", Dir::Forward, "g0"),
            edg("e2", "b", "b", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let view = View::default();
    let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    let screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
    for e in 0..2 {
        let cells = hit.edge_cells.get(&e).cloned().unwrap_or_default();
        assert!(!cells.is_empty(), "自环 e{e} 该画出格子");
        // At least two turns, so not a straight line.
        let xs: std::collections::HashSet<i32> = cells.iter().map(|c| c.0).collect();
        let ys: std::collections::HashSet<i32> = cells.iter().map(|c| c.1).collect();
        assert!(xs.len() > 1 && ys.len() > 1, "自环 e{e} 该是个环:{cells:?}");
        let c = cells[0];
        assert!(
            matches!(pick_at(&d, &hit, (0, 0), c, &view), Pick::Edge(k) if k == e),
            "自环 e{e} 的格子该点得到"
        );
    }
    assert!(screen((2, 5)) != (0, 0));
}

/// Multi-line text: each line lands on a consecutive row inside the box, align applies per line.
#[test]
fn multiline_text_drawn_per_row() {
    let mut d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (8, 3), BoxStyle::Rounded)],
        vec![txt("ta", "x", (1, 1), "a")],
        vec![],
    );
    d.set_text(0, "一\n二二\n三三三");
    assert_eq!(d.boxes[0].size.1, 5, "三行 + 上下边框");
    d.texts[0].align = Align::Center;
    let area = Rect::new(0, 0, 30, 12);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (x, y) = p.proj.cell((0.0, 0.0));
    let text = screen(&buf).join("\n");
    assert!(text.contains("一"), "{text}");
    // All three lines fit inside the box, one row apart.
    let rows: Vec<String> = (0..5)
        .map(|k| screen(&buf)[(y + 1 + k) as usize].clone())
        .collect();
    assert!(rows[0].contains('一'), "{rows:?}");
    assert!(rows[1].contains("二二"), "{rows:?}");
    assert!(rows[2].contains("三三三"), "{rows:?}");
    let _ = x;
}

/// An edge label is a plain none box: drawn at its own coordinates, gone entirely when toggled off.
#[test]
fn edge_label_follows_toggle() {
    let mut d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 5), (8, 3), BoxStyle::Rounded),
        ],
        vec![txt("ta", "甲", (3, 6), "a"), txt("tb", "乙", (31, 6), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let b = d.new_edge_label(0).expect("该能建注释框");
    d.set_text(b, "收到 ACK");
    d.place_box(b, (18, 9));
    let area = Rect::new(0, 0, 60, 20);
    let mut view = View::default();
    let mut buf = Buffer::empty(area);
    let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    let (x, y) = p.proj.cell((18.0, 9.0));
    let text = screen(&buf).join("\n");
    assert!(text.contains("收到 ACK"), "{text}");
    // An edge label is hittable like any box.
    assert!(
        matches!(pick_at(&d, &hit, (18, 9), (x, y), &view), Pick::Box(k) if k == b),
        "注释框该点得到"
    );
    let (_, y0, _, _) = d.bbox_shown(true);
    let (_, _, _, y1) = d.bbox_shown(false);
    assert!(
        y0 != y1 || d.bbox() != d.bbox_shown(false),
        "关掉后包围盒变小"
    );

    // Toggled off: not drawn and not hittable.
    view.edge_labels = false;
    let mut buf = Buffer::empty(area);
    let (p2, hit2) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    let (x2, y2) = p2.proj.cell((18.0, 9.0));
    assert!(
        !screen(&buf).join("\n").contains("收到 ACK"),
        "关掉就不该画"
    );
    assert_eq!(
        pick_at(&d, &hit2, (18, 9), (x2, y2), &view),
        Pick::None,
        "关掉的注释框不该点得到"
    );
}

/// E mode: labels of lit edges light up too, even when labels are switched off.
#[test]
fn neighbour_highlight_includes_edge_labels() {
    let mut d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 5), (8, 3), BoxStyle::Rounded),
            bx("c", (2, 20), (8, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "甲", (3, 6), "a"),
            txt("tb", "乙", (31, 6), "b"),
            txt("tc", "丙", (3, 21), "c"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "a", "c", Dir::Forward, "g0"),
        ],
    );
    let b = d.new_edge_label(0).expect("该能建注释框");
    d.set_text(b, "收到 ACK");
    let area = Rect::new(0, 0, 60, 30);
    let mut view = View::default();
    view.edge_labels = false;
    view.ego = Some(0);
    let mut buf = Buffer::empty(area);
    let (p, _) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    let text = screen(&buf).join("\n");
    assert!(text.contains("收到 ACK"), "E 模式下临时露出来:{text}");
    let (x, y) = p
        .proj
        .cell((d.boxes[b].at.0 as f64, d.boxes[b].at.1 as f64));
    assert!(x >= 0 && y >= 0);
}

/// Hit priority: box > edge > empty.
#[test]
fn hit_priority_box_edge_empty() {
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 5), (6, 3), BoxStyle::Rounded),
            bx("b", (20, 5), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (3, 6), "a"), txt("tb", "", (20, 6), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let view = View::default();
    let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    // Box: canvas cell and screen cell both resolve to it.
    let screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
    assert!(matches!(
        pick_at(&d, &hit, (4, 6), screen((4, 6)), &view),
        Pick::Box(0)
    ));
    assert!(
        matches!(
            pick_at(&d, &hit, (2, 5), screen((2, 5)), &view),
            Pick::Box(0)
        ),
        "边框也算"
    );
    // Edge: the hit table stores screen cells, so pass a canvas cell outside any box.
    let s = screen((11, 6));
    let got = pick_at(&d, &hit, (11, 6), s, &view);
    assert!(matches!(got, Pick::Edge(_)), "中间那格命中边:{got:?}");
    let s2 = screen((11, 14));
    assert_eq!(pick_at(&d, &hit, (11, 14), s2, &view), Pick::None);
}

/// Text links: the hit table covers every shown cell (two per wide glyph); targets add no width.
#[test]
fn link_hit_table_covers_text() {
    let base = sample();
    let area = Rect::new(0, 0, 80, 24);
    let v = View::default();
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &base, &v, &FrameHit::default());
    assert!(hit.links.is_empty(), "样例没有链接");

    // Switch to linked text: one full-width glyph plus one ASCII glyph.
    let mut d = base.clone();
    let ti = d.text_of(0).expect("有文字");
    d.texts[ti].text = "[中x](https://e.com)".to_string();
    let at = d.texts[ti].at;
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &d, &v, &FrameHit::default());
    assert_eq!(
        hit.links.get(&at).map(String::as_str),
        Some("https://e.com"),
        "第一格该命中"
    );
    assert_eq!(
        hit.links.get(&(at.0 + 1, at.1)).map(String::as_str),
        Some("https://e.com"),
        "全角字的第二格也算"
    );
    assert_eq!(
        hit.links.get(&(at.0 + 2, at.1)).map(String::as_str),
        Some("https://e.com"),
        "后面那个 x 也在链接里"
    );
    assert!(
        !hit.links.contains_key(&(at.0 + 3, at.1)),
        "链接以外的字不算"
    );
}

/// Per-frame hit table: stroke cells resolve to their edge, blank cells resolve to none.
#[test]
fn hit_table_records_edge_cells() {
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("a", (2, 5), (1, 1), BoxStyle::Point),
            bx("b", (20, 5), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "", (2, 6), "a"), txt("tb", "", (20, 6), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let view = View::default();
    let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    // The cell between the two points belongs to this edge.
    let (x, y) = p.proj.cell((11.0, 5.0));
    assert_eq!(hit.edge_at((x, y)), Some(0), "中间那格该属于 e1");
    // Nothing in the blank area.
    let (bx, by) = p.proj.cell((11.0, 12.0));
    assert_eq!(hit.edge_at((bx, by)), None);
    // The visible-cell list holds this edge and more than one cell.
    assert!(hit.edge_cells.get(&0).map(|v| v.len() > 5).unwrap_or(false));
    // It is also among the candidates.
    assert!(hit.candidates_at((x, y)).contains(&0));
}

/// A box dimmed by the focus filter skips its fill and keeps the terminal background.
#[test]
fn dimmed_box_skips_fill() {
    let area = Rect::new(0, 0, 80, 30);
    let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
    b1.fill = Some((1, 2, 3));
    let d = doc_of(
        vec![grp("g0", "一", (255, 0, 0)), grp("g1", "二", (0, 255, 0))],
        vec![lyr("", 0)],
        vec![b1, bx("p1", (20, 8), (1, 1), BoxStyle::Point)],
        vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
        Vec::new(),
    );
    let mut view = View::default();
    view.focus = vec![1];
    set_at(&d, &mut view, 1); // Keep the cursor off the dimmed box so highlight cannot mask it.
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    for dy in 0..3 {
        for dx in 0..8 {
            let bg = buf[((x + dx) as u16, (y + dy) as u16)].bg;
            assert_ne!(bg, Color::Rgb(1, 2, 3), "压暗时不该有填充");
            assert_eq!(bg, Color::Reset, "压暗时保持终端底色");
        }
    }
}

/// Border colour and text colour are independent.
#[test]
fn border_and_text_colors_separate() {
    let area = Rect::new(0, 0, 80, 30);
    let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
    b1.color = Some((10, 20, 30));
    b1.fill = Some((5, 6, 7));
    let mut t1 = txt("t1", "ab", (3, 3), "b1");
    t1.color = Some((40, 50, 60));
    let mut t2 = txt("t2", "p", (20, 9), "p1");
    t2.color = Some((70, 80, 90));
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![b1, bx("p1", (20, 8), (1, 1), BoxStyle::Point)],
        vec![t1, t2],
        Vec::new(),
    );
    let mut view = View::default();
    // Park the cursor on blank space: this tests colour independence, not the selection pulse.
    view.cursor = (-5, -5);
    view.pick = Pick::None;
    view.node_labels = true; // Point labels are optional; force them on here.
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    let border = &buf[(x as u16, y as u16)];
    assert_eq!(border.fg, Color::Rgb(10, 20, 30), "边框用自己的颜色");
    assert_eq!(border.bg, Color::Rgb(5, 6, 7), "边框格同样铺填充");
    let text = &buf[((x + 1) as u16, (y + 1) as u16)];
    assert_eq!(text.fg, Color::Rgb(40, 50, 60), "文字用自己的颜色");
    assert_eq!(text.bg, Color::Rgb(5, 6, 7), "文字格同样铺填充");
    let (px, py) = p.proj.cell((20.0, 9.0));
    assert_eq!(
        buf[(px as u16, py as u16)].fg,
        Color::Rgb(70, 80, 90),
        "点节点文字用自己的颜色,不取框色"
    );
}

/// Horizontal align: left starts one cell in, center centres, right leaves one cell.
#[test]
fn text_offset_by_align() {
    let area = Rect::new(0, 0, 80, 30);
    // Box is 12 wide, 10 usable, and "abc" is 3 wide.
    for (align, off) in [(Align::Left, 0), (Align::Center, 3), (Align::Right, 7)] {
        let mut t = txt("t1", "abc", (3, 3), "b1");
        t.align = align;
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![bx("b1", (2, 2), (12, 3), BoxStyle::Rounded)],
            vec![t],
            Vec::new(),
        );
        let mut view = View::default();
        set_at(&d, &mut view, 0);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let tx = x + 1 + off;
        assert_eq!(
            buf[(tx as u16, (y + 1) as u16)].symbol(),
            "a",
            "{} 时文字起点",
            align.name()
        );
        if off > 0 {
            assert_ne!(buf[((tx - 1) as u16, (y + 1) as u16)].symbol(), "a");
        }
    }
}

/// Corner style: sharp draws corners with square glyphs, rounded with round ones.
#[test]
fn corner_glyphs_follow_style() {
    let area = Rect::new(0, 0, 60, 20);
    for (corner, want) in [(Corner::Sharp, "┐"), (Corner::Rounded, "╮")] {
        let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
        e.corner = corner;
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 2), (1, 1), BoxStyle::Point),
                bx("b", (20, 10), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (2, 3), "a"), txt("tb", "b", (20, 11), "b")],
            vec![e],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (cx, cy) = p.proj.cell((20.0, 2.0));
        assert_eq!(
            buf[(cx as u16, cy as u16)].symbol(),
            want,
            "{} 的拐角",
            corner.name()
        );
    }
}

/// Dashed: straight runs follow the dash pattern, corners stay solid.
#[test]
fn dashed_line_gaps_runs_keeps_corners() {
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (20, 0), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 1), "b")],
        vec![{
            let mut e = edg("e1", "a", "b", Dir::None, "g0");
            e.style = EdgeStyle::Dashed;
            e
        }],
    );
    let area = Rect::new(0, 0, 60, 12);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (x0, y) = p.proj.cell((0.0, 0.0));
    let (x1, _) = p.proj.cell((20.0, 0.0));
    let mut blank = false;
    let mut drawn = false;
    for x in x0 + 1..x1 {
        match buf[(x as u16, y as u16)].symbol() {
            " " => blank = true,
            "─" => drawn = true,
            _ => {}
        }
    }
    assert!(drawn, "虚线里也该有实笔");
    assert!(blank, "虚线里该有断开的空格");
    // A dashed bend still needs a glyph at the corner.
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (20, 10), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 11), "b")],
        vec![{
            let mut e = edg("e1", "a", "b", Dir::None, "g0");
            e.style = EdgeStyle::Dashed;
            e
        }],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &View::default());
    let text = screen(&buf).join("\n");
    assert!(
        text.chars().any(|c| matches!(c, '╭' | '╮' | '╰' | '╯')),
        "虚线拐角该有字形:\n{text}"
    );
}

/// Layer view: nodes and edges outside the current layer are dimmed, the current one is normal.
#[test]
fn other_layers_dimmed() {
    let mut a = bx("a", (0, 0), (1, 1), BoxStyle::Point);
    a.layer = "底".into();
    let mut b = bx("b", (10, 0), (1, 1), BoxStyle::Point);
    b.layer = "顶".into();
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0), lyr("底", 1), lyr("顶", 2)],
        vec![a, b],
        vec![txt("ta", "下", (0, 1), "a"), txt("tb", "上", (10, 1), "b")],
        vec![edg("e1", "a", "b", Dir::None, "g0")],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut view = View::default();
    set_at(&d, &mut view, 1);
    view.layer = Some("顶".to_string());
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (ax, ay) = p.proj.cell((0.0, 0.0));
    assert_eq!(
        buf[(ax as u16, ay as u16)].fg,
        DIM_COLOR,
        "底层的节点该压暗"
    );
    let (cx, cy) = p.proj.cell((10.0, 0.0));
    assert_ne!(
        buf[(cx as u16, cy as u16)].fg,
        DIM_COLOR,
        "当前层的节点不该压暗"
    );
    let (mx, my) = p.proj.cell((5.0, 0.0));
    assert_eq!(
        buf[(mx as u16, my as u16)].fg,
        DIM_COLOR,
        "别层的边也该压暗"
    );
}

/// Block selection draws a continuous grey border: background only, no glyphs covered, no gaps.
#[test]
fn block_selection_draws_continuous_border() {
    let d = doc_of(
        vec![grp("g0", "线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![bx("a", (2, 2), (8, 3), BoxStyle::Rounded)],
        vec![txt("ta", "a", (3, 3), "a")],
        Vec::new(),
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut view = View::default();
    set_at(&d, &mut view, 0);
    view.sel = Some(crate::app::RectSel {
        anchor: (2, 2),
        end: view.cursor,
        confirmed: false,
    });
    view.cursor = (12, 7);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x0, y0) = p.proj.cell((2.0, 2.0));
    let (x1, y1) = p.proj.cell((12.0, 7.0));
    let gray = |x: i32, y: i32| buf[(x as u16, y as u16)].bg == SEL_GRAY;
    assert!(gray(x0, y0), "左上角");
    assert!(gray(x1, y0), "右上角");
    assert!(gray(x0, y1), "左下角");
    assert!(gray(x1, y1), "右下角");
    // The box glyphs survive: the top-left cell already holds the corner glyph.
    assert_eq!(buf[(x0 as u16, y0 as u16)].symbol(), "╭");
    // All four sides are continuous: every cell between the corners is grey.
    for y in y0..=y1 {
        assert!(gray(x0, y) && gray(x1, y), "竖边在 y={y} 处断了");
    }
    for x in x0..=x1 {
        assert!(gray(x, y0) && gray(x, y1), "横边在 x={x} 处断了");
    }
    // Nothing inside the box.
    assert!(buf[(x0 as u16 + 1, y0 as u16 + 1)].bg != SEL_GRAY);
}

/// Rect selection: collect boxes by cell rectangle, selected boxes pulse (no reverse video).
#[test]
fn rect_selection_collects_boxes_by_cell() {
    let d = points();
    let area = Rect::new(0, 0, 60, 24);
    let mut view = View::default();
    let c0 = d.center(0);
    view.sel = Some(crate::app::RectSel {
        anchor: (c0.0 as i32, c0.1 as i32),
        end: view.cursor,
        confirmed: false,
    });
    set_at(&d, &mut view, 1);
    let p = panels(&d, area, &view);
    assert_eq!(
        selected(&d, &p, &view),
        vec![0, 1],
        "矩形里只有锚点与光标,下面那个不在里头"
    );
    let mut buf = Buffer::empty(area);
    view.pulse = 1.0; // Pin to red for a stable assertion.
    render(&mut buf, area, &d, &view);
    for i in [0usize, 1] {
        let (x, y) = p
            .proj
            .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
        assert_eq!(
            buf[(x as u16, y as u16)].fg,
            Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
            "选中的 box {i} 该闪红"
        );
        assert!(
            !buf[(x as u16, y as u16)]
                .modifier
                .contains(Modifier::REVERSED),
            "选区不再用反显"
        );
    }
    let (x, y) = p.proj.cell((20.0, 10.0));
    assert_ne!(
        buf[(x as u16, y as u16)].fg,
        Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
        "矩形外的框不该闪"
    );
}

/// Map-only entry: no legend, no status line, the zoom argument applies.
#[test]
fn map_only_no_legend_or_status() {
    let d = doc_of(
        vec![grp("g0", "主线", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (10, 0), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (10, 1), "b")],
        vec![edg("e1", "a", "b", Dir::None, "g0")],
    );
    let wide = Rect::new(0, 0, 200, 50);
    let mut view = View::default();
    view.note = Some("提示".to_string());
    let mut buf = Buffer::empty(wide);
    let map = render_map(&mut buf, wide, &d, &view, 1.0);
    assert_eq!(
        (map.x, map.y, map.width, map.height),
        (wide.x, wide.y, wide.width, wide.height),
        "没图例没状态行时地图区就是整块"
    );
    let text = screen(&buf).join("\n");
    for gone in ["分组", "有向边", "box=", "提示", "VIEW"] {
        assert!(!text.contains(gone), "导出地图不该有 {gone:?}:\n{text}");
    }
    // Double the zoom and the on-screen distance between the nodes doubles.
    let gap = |zoom: f64| {
        let mut buf = Buffer::empty(wide);
        render_map(&mut buf, wide, &d, &view, zoom);
        let mut pts = Vec::new();
        for y in 0..wide.height {
            for x in 0..wide.width {
                if matches!(buf[(x, y)].symbol(), "○" | "●") {
                    pts.push(x as i32);
                }
            }
        }
        pts.sort_unstable();
        *pts.last().unwrap() - *pts.first().unwrap()
    };
    assert_eq!(gap(1.0), 10, "zoom 1 时按单元格一格一格");
    assert_eq!(gap(2.0), 20, "zoom 2 时距离翻倍");
}

/// Explicit ports: left/right/up/down each have their own exit cell; auto keeps its cell.
#[test]
fn forced_port_lands_on_side() {
    use crate::model::Side;
    let d = sample();
    let p = panels(&d, Rect::new(0, 0, 80, 30), &View::default());
    let r = rect_of(&d, 0, &p.proj);
    let rs: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let auto = end_mark(&d, 0, 1, &rs, None).expect("带框节点有端口");
    // auto: stacked boxes, so the edge leaves from the bottom.
    assert_eq!(auto.side, DOWN, "auto 该按中心差选");
    assert_eq!(auto.cell, Canvas::side_base(r, DOWN));
    // Here the target is lower-left: left/down face it, right/up point away.
    // (Away-facing pins fall back to auto; see `pinned_side_facing_away_uses_auto`.)
    for (side, bit) in [(Side::Left, LEFT), (Side::Down, DOWN)] {
        let m = end_mark(&d, 0, 1, &rs, Some(side)).expect("端口");
        assert_eq!(m.side, bit, "{} 端口", side.name());
        assert_eq!(m.cell, Canvas::side_base(r, bit));
    }
    for side in [Side::Right, Side::Up] {
        let m = end_mark(&d, 0, 1, &rs, Some(side)).expect("端口");
        assert_eq!(m.side, DOWN, "{} 背对目标,按自动走", side.name());
    }
    // Point nodes have no border: a forced port changes nothing (the centre cell wins).
    let d2 = points();
    let p2 = panels(&d2, Rect::new(0, 0, 80, 30), &View::default());
    let rs2: Vec<(i32, i32, i32, i32)> = (0..d2.boxes.len())
        .map(|i| rect_of(&d2, i, &p2.proj))
        .collect();
    assert!(end_mark(&d2, 0, 1, &rs2, Some(Side::Left)).is_none());
}

/// Two edges crossing: the first drawn owns the crossing cell; an E-lit edge paints over it.
fn cross_doc() -> Document {
    doc_of(
        vec![grp("g0", "全部", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![
            bx("h1", (0, 6), (1, 1), BoxStyle::Point),
            bx("h2", (16, 6), (1, 1), BoxStyle::Point),
            bx("v1", (8, 2), (1, 1), BoxStyle::Point),
            bx("v2", (8, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "h1", (0, 6), "h1"),
            txt("t2", "h2", (16, 6), "h2"),
            txt("t3", "v1", (8, 2), "v1"),
            txt("t4", "v2", (8, 10), "v2"),
        ],
        // The vertical edge is drawn first and owns the crossing cell.
        vec![
            edg("ev", "v1", "v2", Dir::None, "g0"),
            edg("eh", "h1", "h2", Dir::Forward, "g0"),
        ],
    )
}

/// An E-lit edge paints over other edges, so the crossing never shows a one-cell gap.
#[test]
fn ego_mode_raises_edge_to_top() {
    let d = cross_doc();
    let area = Rect::new(0, 0, 40, 20);
    let draw = |view: &View| {
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, view);
        let s = (
            p.proj.cell((8.0, 6.0)).0 as u16,
            p.proj.cell((8.0, 6.0)).1 as u16,
        );
        (buf, s)
    };
    let mut view = View::default();
    let (buf, s) = draw(&view);
    assert_eq!(buf[s].symbol(), "─", "平时先画的那条占着交叉格");
    // Cursor on the vertical edge's head in E mode: it lights and the crossing turns vertical.
    view.pick = Pick::Box(2);
    view.cursor = (8, 2);
    view.ego = Some(2);
    let (buf, s) = draw(&view);
    assert_eq!(buf[s].symbol(), "│", "E 亮起来的边该压上来");
}

/// E overlay: a highlighted edge is repainted with its glyph and colour even where text covered it.
#[test]
fn ego_overlay_redraws_edge_over_text() {
    let mut cv = Canvas::new(
        (0, 0, 20, 8),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 19, 7);
    cv.pen_edge = 0;
    cv.edge((0, 4), (12, 4), 0, (200, 30, 40), None, (false, false));
    let ego = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from([0usize]),
        edge_labels: HashSet::new(),
    };
    // Cover a cell with text, as if text had been drawn later.
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
    for y in 0..8u16 {
        for x in 0..20u16 {
            buf[(x, y)].set_symbol("文");
        }
    }
    overlay_ego_edges(
        &mut buf,
        &cv,
        &doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![],
            vec![],
            vec![],
        ),
        &ego,
        None,
        0.0,
    );
    let cell = &buf[(6, 4)];
    assert_eq!(cell.symbol(), "─", "高亮边该盖住文字");
    assert_eq!(cell.fg, Color::Rgb(200, 30, 40), "颜色跟这条边");
    assert_eq!(buf[(6, 0)].symbol(), "文", "没被这条边占的格子不动");
}

/// E overlay: the selected edge keeps its pulse colour instead of the plain edge colour.
#[test]
fn ego_overlay_keeps_selected_pulse() {
    let mut hub = bx("hub", (10, 2), (5, 9), BoxStyle::Single);
    hub.color = Some((10, 20, 30));
    let d = doc_of(
        vec![grp("g0", "红", (255, 0, 0))],
        vec![lyr("", 0)],
        vec![hub, bx("n", (26, 2), (2, 1), BoxStyle::None)],
        vec![
            txt("th", "hub", (11, 6), "hub"),
            txt("tn", "邻", (26, 2), "n"),
        ],
        vec![edg("e1", "n", "hub", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 40, 16);
    let mut view = View::default();
    view.ego = Some(1);
    view.pick = Pick::Edge(0);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let want = pulse_color(BRIGHT, view.pulse);
    let mut found = false;
    for y in 0..16u16 {
        for x in 0..40u16 {
            let c = &buf[(x, y)];
            if matches!(c.symbol(), "─" | "│" | "╭" | "╮" | "╰" | "╯" | ">" | "<") && c.fg == want
            {
                found = true;
            }
        }
    }
    assert!(found, "选中的边该有呼吸色");
}

/// E isolate: a neighbour box keeps only ports of connected edges; other ports are not drawn.
#[test]
fn ego_draws_only_connected_ports() {
    let mut hub = bx("hub", (10, 2), (5, 9), BoxStyle::Single);
    hub.color = Some((10, 20, 30));
    let d = doc_of(
        vec![grp("g0", "红", (255, 0, 0)), grp("g1", "蓝", (0, 0, 255))],
        vec![lyr("", 0)],
        vec![
            hub,
            bx("n", (26, 2), (2, 1), BoxStyle::None),
            bx("m", (26, 8), (2, 1), BoxStyle::None),
        ],
        vec![
            txt("th", "hub", (11, 6), "hub"),
            txt("tn", "邻", (26, 2), "n"),
            txt("tm", "远", (26, 8), "m"),
        ],
        vec![
            edg("e1", "n", "hub", Dir::Forward, "g1"),
            edg("e2", "m", "hub", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 40, 16);
    let mut view = View::default();
    view.ego = Some(1); // n is a neighbour of hub.
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (hl, ht, hw, hh) = rect_of(&d, 0, &p.proj);
    let mut lit = 0;
    let mut other = 0;
    for y in (ht + 1)..(ht + hh - 1) {
        for x in [hl, hl + hw - 1] {
            let cell = &buf[(x as u16, y as u16)];
            if cell.symbol() == ">" || cell.symbol() == "<" {
                if cell.fg == Color::Rgb(0, 0, 255) {
                    lit += 1;
                } else {
                    other += 1;
                }
            }
        }
    }
    assert_eq!(lit, 1, "只有相连的那条边该画出来");
    assert_eq!(other, 0, "隐藏边的端口不该留在边框上");
}

/// systemd: with E on a node only connected ports remain; other ports vanish rather than dim.
#[test]
fn ego_neighbour_boxes_keep_connected_ports() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let Some(n) = (0..d.boxes.len()).find(|&i| {
        d.text(i)
            .map(|t| t.text.starts_with("systemd-logind"))
            .unwrap_or(false)
    }) else {
        return;
    };
    let area = Rect::new(0, 0, 140, 44);
    let dim = dim_style().fg.unwrap_or(Color::Reset);
    let count = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let (mut bright, mut dimmed) = (0, 0);
        for y in 0..44u16 {
            for x in 0..140u16 {
                let c = &buf[(x, y)];
                if matches!(c.symbol(), "v" | "^" | "<" | ">") && c.fg == dim {
                    dimmed += 1;
                } else if matches!(c.symbol(), "v" | "^" | "<" | ">") {
                    bright += 1;
                }
            }
        }
        (bright, dimmed)
    };
    let (b0, d0) = count(None);
    let (b1, d1) = count(Some(n));
    assert!(d0 <= 1, "不开 E 时不该有压暗的箭头:{d0}");
    assert!(b1 >= 1, "相连边的端口要留着");
    assert!(b1 < b0, "E 之后画出来的箭头应该变少:{b0} -> {b1}");
    assert!(d1 <= 1, "隐藏边的端口不该以压暗的样子留下:{d1}");
}

/// A head lands on a cell another edge crosses: the head still wins on screen.
#[test]
fn arrow_wins_over_crossing_line() {
    let d = doc_of(
        vec![grp("g0", "全部", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![
            bx("h1", (0, 6), (1, 1), BoxStyle::Point),
            bx("h2", (10, 6), (1, 1), BoxStyle::Point),
            bx("v1", (9, 2), (1, 1), BoxStyle::Point),
            bx("v2", (9, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "h1", (0, 6), "h1"),
            txt("t2", "h2", (10, 6), "h2"),
            txt("t3", "v1", (9, 2), "v1"),
            txt("t4", "v2", (9, 10), "v2"),
        ],
        // The vertical line is drawn first; the horizontal head lands on it.
        vec![
            edg("ev", "v1", "v2", Dir::None, "g0"),
            edg("eh", "h1", "h2", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 30, 20);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let c = p.proj.cell((9.0, 6.0));
    assert_eq!(
        buf[(c.0 as u16, c.1 as u16)].symbol(),
        ">",
        "箭头该盖在交叉的竖线上"
    );
}

/// Switching a boxed node to none matches a native none node: text stays, edges re-route.
#[test]
fn none_matches_native_text_node() {
    let d = doc_of(
        vec![grp("g0", "全部", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![
            bx("hub", (20, 10), (8, 3), BoxStyle::Rounded),
            bx("n", (20, 2), (4, 1), BoxStyle::None),
            bx("s", (20, 18), (4, 1), BoxStyle::None),
            bx("w", (2, 10), (6, 1), BoxStyle::None),
            bx("e", (34, 10), (6, 1), BoxStyle::None),
        ],
        vec![
            txt("th", "hub", (21, 11), "hub"),
            txt("tn", "up", (20, 2), "n"),
            txt("ts", "dn", (20, 18), "s"),
            txt("tw", "left", (2, 10), "w"),
            txt("te", "right", (34, 10), "e"),
        ],
        vec![
            edg("e1", "n", "hub", Dir::Forward, "g0"),
            edg("e2", "hub", "s", Dir::Forward, "g0"),
            edg("e3", "w", "hub", Dir::Forward, "g0"),
            edg("e4", "hub", "e", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 60, 30);
    let shot = |d: &Document| {
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, d, &View::default());
        plain_lines(&buf).join("\n")
    };
    let text_at = d.text(0).map(|t| t.at).unwrap();
    let mut converted = d.clone();
    converted.set_style(0, BoxStyle::None);
    assert_eq!(converted.text(0).map(|t| t.at), Some(text_at), "文字不动");
    assert_eq!(converted.boxes[0].at, text_at, "框贴到文字上");

    // Build a native none document with the same position, size and text.
    let mut native = d.clone();
    native.boxes[0].style = BoxStyle::None;
    native.boxes[0].size = (3, 1);
    native.boxes[0].at = text_at;
    native.texts[0].at = text_at;
    native.rebuild();
    assert_eq!(shot(&converted), shot(&native), "该和原生文字节点画得一样");
}

/// No selection rectangle while a box is dragged; it returns once the drag ends.
#[test]
fn drag_hides_selection_rect() {
    let d = sample();
    let p = panels(&d, Rect::new(0, 0, 60, 20), &View::default());
    let mut view = View::default();
    view.sel = Some(crate::app::RectSel {
        anchor: (0, 0),
        end: (4, 4),
        confirmed: true,
    });
    assert!(sel_rect(&p, &view).is_some(), "平时该有选区矩形");
    view.drag_hide_sel = true;
    assert!(sel_rect(&p, &view).is_none(), "拖动时该藏起来");
    view.drag_hide_sel = false;
    assert!(sel_rect(&p, &view).is_some(), "松手后回来");
}

/// Region: the border encloses members and the title shows; collapsed leaves only the title bar.
#[test]
fn region_border_and_collapse() {
    use crate::document::Region;
    let mut d = crate::document::Document::scratch();
    d.add_box("b", (30, 0), BoxStyle::Single).unwrap();
    d.regions.push(Region {
        id: "r1".into(),
        label: "grp".into(),
        color: None,
        style: BoxStyle::Rounded,
        layer: String::new(),
        padding: 1,
        members: vec!["b1".into(), "b2".into()],
        collapsed: false,
    });
    d.rebuild();
    let area = Rect::new(0, 0, 70, 24);
    let shot = |doc: &Document| {
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, doc, &View::default());
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    };
    let open = shot(&d);
    assert!(open.contains("grp"), "标题该画出来");
    assert!(open.contains("╭ grp "), "标题骑在左上角那条边上:\n{open}");
    // Collapsed: member glyphs vanish, the region stays.
    let mut folded = d.clone();
    folded.regions[0].collapsed = true;
    folded.rebuild();
    let text = shot(&folded);
    assert!(text.contains("grp"), "折叠后标题还在");
    assert!(!text.contains("b2"), "折叠后成员不画");
    assert_ne!(open, text);
}

/// Low-zoom LOD: text goes first, further out the status line reports LOD.
#[test]
fn low_zoom_drops_text() {
    let d = sample();
    let area = Rect::new(0, 0, 120, 40);
    let text_of = |zoom: f64| {
        let mut view = View::default();
        view.zoom = zoom;
        view.node_labels = true;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let mut out = String::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(&line);
            out.push('\n');
        }
        out
    };
    // A wide glyph's second cell is blank, so count the glyph itself, not substrings.
    let count = |text: &str, ch: char| text.matches(ch).count();
    assert!(count(&text_of(1.0), '中') > 0, "1:1 该有文字");
    // 0.4: text is dropped entirely, but boxes and edges remain.
    let mid = text_of(0.4);
    assert_eq!(count(&mid, '中'), 0, "0.4 不该画文字");
    assert!(mid.contains("─") || mid.contains("│"), "0.4 还该有框线");
    // 0.2: the status line reports LOD.
    let small = text_of(0.2);
    assert_eq!(count(&small, '中'), 0, "0.2 不该画文字");
    assert!(small.contains("LOD"), "0.2 状态行该标 LOD");
}

/// Unplaced point labels are recorded: present in a small area, absent in a large, order stable.
#[test]
fn dropped_labels_are_recorded() {
    let d = points();
    let shot = |w: u16, h: u16| {
        let area = Rect::new(0, 0, w, h);
        let mut view = View::default();
        view.zoom = zoom_for_fit(&d, w, h, view.edge_labels);
        let mut buf = Buffer::empty(area);
        let (_, hit) = render_picture_hit(&mut buf, area, &d, &view);
        hit.labels_skipped
    };
    // Large area: all three points are labelled.
    assert!(shot(120, 30).is_empty(), "大窗口不该丢标签");
    // Small area: they do not fit, but must be reported and be identical across renders.
    let a = shot(4, 3);
    let b = shot(4, 3);
    assert!(!a.is_empty(), "小窗口该报出没放下的标签");
    assert_eq!(a, b, "同尺寸两次渲染要一致");
    for s in &a {
        assert!(!s.text.is_empty() && !s.box_id.is_empty());
    }
}

/// When a selected edge crosses another's head cell, it shows its own stroke there.
#[test]
fn selected_edge_overrides_foreign_arrow() {
    let mut e2 = edg("e2", "b", "q", Dir::Forward, "g0");
    e2.via = vec![(12, 6)]; // From b it goes right first, straight onto another head cell.
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("p", (20, 6), (1, 1), BoxStyle::Point),
            bx("b", (10, 6), (1, 1), BoxStyle::Point),
            bx("q", (12, 12), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("tp", "p", (21, 6), "p"),
            txt("tb", "b", (11, 7), "b"),
            txt("tq", "q", (12, 13), "q"),
        ],
        vec![edg("e1", "p", "b", Dir::Forward, "g0"), e2],
    );
    let area = Rect::new(0, 0, 60, 24);
    let shot = |pick: Pick| {
        let mut v = View::default();
        v.node_labels = false;
        v.pick = pick;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &v);
        let (x, y) = p.proj.cell((11.0, 6.0));
        buf[(x as u16, y as u16)].symbol().to_string()
    };
    assert_eq!(shot(Pick::None), "<", "没选中时那是 e1 的箭头");
    assert_eq!(shot(Pick::Edge(1)), "─", "选中 e2 时该显示自己的笔画");
}

/// Text clipped by the picture edge still draws its visible part: off-screen columns are
/// dropped, not wrapped back in.
#[test]
fn clipped_text_keeps_visible_part() {
    let long: String = (0..60).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (60, 1), BoxStyle::None)],
        vec![txt("ta", &long, (0, 0), "a")],
        Vec::new(),
    );
    let area = Rect::new(0, 0, 30, 10);
    // Shift the view right by 40: the first 40 columns fall outside, 20 stay visible.
    let mut view = View::default();
    view.zoom = 1.0;
    view.center = Some((40.0, 0.0));
    let mut buf = Buffer::empty(area);
    let (p, _) = render_hit(&mut buf, area, &doc, &view, &FrameHit::default());
    let y = p.proj.cell((0.0, 0.0)).1 as u16;
    let line: String = (p.map.x..p.map.x + p.map.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect();
    let letters = line.chars().filter(|c| c.is_ascii_lowercase()).count();
    assert!(letters >= 10, "左边裁掉后右边仍该有字:{line:?}");
    assert!(
        !line.trim_start().starts_with('a') && !line.trim_start().starts_with("abc"),
        "开头那几列在画面外,不该被硬塞回来:{line:?}"
    );
}

/// Multi-line text: rows above the picture are clipped, rows below are still drawn.
#[test]
fn clipped_multiline_keeps_visible_rows() {
    let text = "line0\nline1\nline2\nline3\nline4\nline5";
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (5, 6), BoxStyle::None)],
        vec![txt("ta", text, (0, 0), "a")],
        Vec::new(),
    );
    let area = Rect::new(0, 0, 30, 8);
    let mut view = View::default();
    view.zoom = 1.0;
    view.center = Some((2.0, 4.0));
    let mut buf = Buffer::empty(area);
    let (p, _) = render_hit(&mut buf, area, &doc, &view, &FrameHit::default());
    let mut seen = String::new();
    for y in p.map.y..p.map.y + p.map.height {
        for x in p.map.x..p.map.x + p.map.width {
            seen.push_str(buf[(x, y)].symbol());
        }
    }
    assert!(seen.contains("line5"), "最后一行该在画面里:{seen:?}");
    assert!(seen.contains("line2"), "中间几行也该在:{seen:?}");
}

/// With no target box yet, the preview still draws a dashed line to the cursor.
#[test]
fn preview_without_target_draws_line() {
    let area = Rect::new(0, 0, 60, 16);
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 1), (4, 3), BoxStyle::Single),
            bx("b", (20, 8), (4, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 2), "a"), txt("tb", "b", (21, 9), "b")],
        Vec::new(),
    );
    let mut view = View::default();
    view.zoom = 1.0;
    view.link = Some(crate::app::LinkState {
        sources: vec![0],
        targets: Vec::new(),
        confirming: false,
    });
    view.cursor = (30, 12);
    let mut buf = Buffer::empty(area);
    let (p, _) = render_hit(&mut buf, area, &doc, &view, &FrameHit::default());
    let mut buf2 = Buffer::empty(area);
    let n = draw_link_preview(&mut buf2, &p, &doc, &view);
    if std::env::var("DBG").is_ok() {
        for y in 0..area.height {
            let row: String = (0..area.width)
                .map(|x| buf2[(x, y)].symbol().to_string())
                .collect();
            eprintln!("{y:2}|{row}|");
        }
    }
    assert_eq!(n, 1, "源到光标该有一条预览线");
    // The line is dashed: at least one stroke cell along the way.
    let strokes = (p.map.y..p.map.y + p.map.height)
        .flat_map(|y| (p.map.x..p.map.x + p.map.width).map(move |x| (x, y)))
        .filter(|(x, y)| matches!(buf2[(*x, *y)].symbol(), "─" | "│" | "╭" | "╮" | "╰" | "╯"))
        .count();
    assert!(strokes > 0, "该画出虚线");
    // No target box, so no head.
    assert!(
        !(p.map.y..p.map.y + p.map.height)
            .flat_map(|y| (p.map.x..p.map.x + p.map.width).map(move |x| (x, y)))
            .any(|(x, y)| matches!(buf2[(x, y)].symbol(), ">" | "<" | "^" | "v")),
        "自由预览不画箭头"
    );
}

/// Same-group edges must not run collinear for long: the later one would vanish.
#[test]
fn same_group_edges_avoid_collinear_run() {
    // a is left, both targets right: straight runs would share a row, so they must offset.
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 10), (4, 3), BoxStyle::Rounded),
            bx("b", (30, 4), (4, 3), BoxStyle::Rounded),
            bx("c", (30, 16), (4, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "a", (1, 11), "a"),
            txt("tb", "b", (31, 5), "b"),
            txt("tc", "c", (31, 17), "c"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "a", "c", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 80, 30);
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &doc, &View::default(), &FrameHit::default());
    let cells = |e: usize| -> std::collections::HashSet<(i32, i32)> {
        hit.edge_cells
            .get(&e)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default()
    };
    let (a, b) = (cells(0), cells(1));
    let shared = a.intersection(&b).count();
    assert_eq!(
        shared, 0,
        "两条边共线了 {shared} 格(画面上有一段是看不见的)"
    );
}

/// A cache hit must render cell-for-cell identical to an uncached pass.
#[test]
fn cache_hit_renders_identically() {
    let d = three_arrow_doc(true);
    let area = Rect::new(0, 0, 80, 30);
    let view = View::default();
    route_cache::clear();
    let first = {
        let mut b = Buffer::empty(area);
        render(&mut b, area, &d, &view);
        b
    };
    // The second pass is a guaranteed cache hit.
    let mut second = Buffer::empty(area);
    render(&mut second, area, &d, &view);
    let mut diff = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if first[(x, y)].symbol() != second[(x, y)].symbol() {
                diff += 1;
            }
        }
    }
    assert_eq!(diff, 0, "缓存命中后画面变了 {diff} 格");
    assert!(route_cache::hits() >= 1, "第二次渲染并没有命中缓存");
    route_cache::clear();
}

/// With overlapping text nodes, a wide glyph's blank continuation must not erase earlier cells.
/// The later label must not blank out an earlier wide glyph.
#[test]
fn stacked_labels_keep_other_glyphs() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (6, 1), BoxStyle::None),
            bx("b", (20, 0), (4, 1), BoxStyle::None),
        ],
        vec![
            txt("ta", "王夫人", (0, 0), "a"),
            txt("tb", "元春", (20, 0), "b"),
        ],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 8);
    let view = View::default();
    let mut moved = d.clone();
    // Overlap the two glyphs 夫人.
    moved.move_box(1, -17, 0);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (ax, ay) = p.proj.cell((0.0, 0.0));
    let before: String = (0..6)
        .map(|i| buf[((ax + i) as u16, ay as u16)].symbol().to_string())
        .collect();
    assert_eq!(before, "王 夫 人 ", "没叠的时候三个字都在");

    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &moved, &view);
    let (ax, ay) = p.proj.cell((0.0, 0.0));
    let at = |i: i32| buf[((ax + i) as u16, ay as u16)].symbol().to_string();
    assert_eq!(at(0), "王", "第一个字还在");
    // The covered cells must not become blanks: either the old glyph stays or a new one lands.
    for i in [2, 4] {
        assert_ne!(at(i), " ", "第 {i} 格被擦成空白了");
    }
}

/// Test helper: screen rectangles of all boxes at 1:1 with the top-left anchored.
fn rects_of(d: &Document) -> Vec<(i32, i32, i32, i32)> {
    (0..d.boxes.len())
        .map(|i| {
            (
                d.boxes[i].at.0,
                d.boxes[i].at.1,
                d.boxes[i].size.0 as i32,
                d.boxes[i].size.1 as i32,
            )
        })
        .collect()
}

/// A pinned side facing away from the target falls back to auto, avoiding a full detour.
#[test]
fn pinned_side_facing_away_uses_auto() {
    use crate::model::Side;
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 5), (4, 3), BoxStyle::Rounded),
            bx("b", (20, 5), (4, 3), BoxStyle::Rounded),
        ],
        vec![txt("ta", "a", (1, 6), "a"), txt("tb", "b", (21, 6), "b")],
        vec![edg("e1", "a", "b", Dir::None, "g0")],
    );
    // Target on the right: a left pin faces away and is dropped; a right pin still holds.
    assert_eq!(
        end_side(&d, 0, 1, &rects_of(&d), Some(Side::Left)),
        RIGHT,
        "背对着的钉法该让位"
    );
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), Some(Side::Right)), RIGHT);
    assert_eq!(
        end_side(&d, 1, 0, &rects_of(&d), Some(Side::Right)),
        LEFT,
        "反方向同理"
    );
    assert_eq!(end_side(&d, 1, 0, &rects_of(&d), Some(Side::Left)), LEFT);
    // Vertically, down/up pins do not face away from a right-hand target and still apply.
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), Some(Side::Down)), DOWN);
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), Some(Side::Up)), UP);
}

/// On a one-row text node the self loop still shows even when other edges fill the ports.
/// (tcp.gtui's ESTABLISHED does this: all ports taken, the loop degenerates to a straight run.)
#[test]
fn self_loop_visible_when_ports_full() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 10), (11, 1), BoxStyle::None),
            bx("l", (4, 14), (2, 1), BoxStyle::None),
            bx("r", (26, 14), (2, 1), BoxStyle::None),
            bx("u", (13, 6), (2, 1), BoxStyle::None),
            bx("d", (13, 13), (2, 1), BoxStyle::None),
        ],
        vec![
            txt("ta", "ESTABLISHED", (10, 10), "a"),
            txt("tl", "左", (4, 14), "l"),
            txt("tr", "右", (26, 14), "r"),
            txt("tu", "上", (13, 6), "u"),
            txt("td", "下", (13, 13), "d"),
        ],
        vec![
            edg("e1", "l", "a", Dir::Forward, "g0"),
            edg("e2", "r", "a", Dir::Forward, "g0"),
            edg("e3", "u", "a", Dir::Forward, "g0"),
            edg("e4", "d", "a", Dir::Forward, "g0"),
            edg("e5", "a", "a", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ax, ay) = p.proj.cell((10.0, 10.0));
    // The loop's lid: some row above the text holds both rounded corners.
    let mut found = false;
    for y in (ay - 3)..ay {
        let mut open = false;
        let mut closed = false;
        for x in (ax - 4)..(ax + 14) {
            match buf[(x as u16, y as u16)].symbol() {
                "╭" => open = true,
                "╮" => closed = true,
                _ => {}
            }
        }
        if open && closed {
            found = true;
            break;
        }
    }
    assert!(found, "自环的顶盖不见了");
}

/// An 18-row box with one text row ports from border capacity, not from the text.
#[test]
fn tall_box_side_candidates_not_clipped() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("tall", (0, 0), (18, 18), BoxStyle::Single),
            bx("far", (30, 8), (2, 1), BoxStyle::None),
        ],
        vec![
            txt("tt", "一行字", (1, 8), "tall"),
            txt("tf", "x", (30, 8), "far"),
        ],
        vec![],
    );
    let area = Rect::new(0, 0, 60, 30);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let r = rects[0];
    let right = side_capacity(&d, &p.proj, &rects, 0, RIGHT);
    let left = side_capacity(&d, &p.proj, &rects, 0, LEFT);
    assert_eq!(right, (r.3 - 2) as usize, "右侧按边框容量算:{right}");
    assert_eq!(left, (r.3 - 2) as usize, "左侧按边框容量算:{left}");
    assert!(right >= 16, "18 行高的框右侧该有 16 个格:{right}");
}

/// With the target above, overflow prefers the top side, not a fixed clockwise bottom.
#[test]
fn overflow_side_follows_target() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("hub", (20, 20), (6, 3), BoxStyle::Single),
            bx("up1", (10, 4), (4, 1), BoxStyle::None),
            bx("up2", (24, 4), (4, 1), BoxStyle::None),
            bx("lft", (2, 20), (4, 1), BoxStyle::None),
        ],
        vec![
            txt("th", "hub", (21, 21), "hub"),
            txt("t1", "上边", (10, 4), "up1"),
            txt("t2", "上也", (24, 4), "up2"),
            txt("t3", "左边", (2, 20), "lft"),
        ],
        vec![
            edg("e1", "lft", "hub", Dir::Forward, "g0"),
            edg("e2", "up1", "hub", Dir::Forward, "g0"),
            edg("e3", "up2", "hub", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 60, 30);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    // hub's right side has one cell; of 8 endpoints the two coming from above should use UP,
    // they must not be forced clockwise onto DOWN.
    let sides: Vec<u8> = (0..3)
        .map(|e| plan.get(0, true, e).expect("有端口").0)
        .collect();
    assert!(sides.contains(&UP), "上方来的边该从上面进:{sides:?}");
    assert!(
        !sides.iter().all(|&s| s == DOWN),
        "不该全被挤到下面:{sides:?}"
    );
}

/// A thin side (capacity under 2) moves the whole group to a side with room.
#[test]
fn thin_side_moves_whole_group() {
    // 19-wide, 3-high box: left/right have one cell each, top/bottom 17.
    let mut boxes = vec![bx("hub", (10, 20), (19, 3), BoxStyle::Single)];
    let mut texts = vec![txt("th", "hub", (11, 21), "hub")];
    let mut edges = Vec::new();
    for k in 0..6 {
        let id = format!("s{k}");
        boxes.push(bx(&id, (2 + k * 3, 4), (2, 1), BoxStyle::None));
        texts.push(txt(&format!("t{k}"), "服", (2 + k * 3, 4), &id));
        edges.push(edg(&format!("e{k}"), &id, "hub", Dir::Forward, "g0"));
    }
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        boxes,
        texts,
        edges,
    );
    let area = Rect::new(0, 0, 80, 40);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let sides: Vec<u8> = (0..6)
        .map(|e| plan.get(0, true, e).expect("有端口").0)
        .collect();
    assert!(
        sides.iter().all(|&s| s == UP),
        "瘦侧放不下时整组该换到上边:{sides:?}"
    );
}

/// systemd: a high-degree box spreads heads across sides instead of one port or a detour.
#[test]
fn systemd_high_degree_side_holds_arrows() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let area = Rect::new(0, 0, 120, 40);
    let view = View::default();
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    for id in ["basic.target", "dbus.socket"] {
        let Some(i) = d.boxes.iter().position(|b| b.id == id) else {
            continue;
        };
        let (l, t, w, h) = rect_of(&d, i, &p.proj);
        // Top and bottom borders must not accumulate a row of heads.
        let top = (l..l + w)
            .filter(|&x| {
                let c = buf[(x as u16, t as u16)].symbol();
                c == "v" || c == "^"
            })
            .count();
        let bottom = (l..l + w)
            .filter(|&x| {
                let c = buf[(x as u16, (t + h - 1) as u16)].symbol();
                c == "v" || c == "^"
            })
            .count();
        assert!(top <= 2, "{id} 顶边挤了 {top} 个箭头");
        // Below basic only the multi-user -> basic upward head remains.
        assert_eq!(bottom, 1, "{id} 底边该只剩 1 个箭头:{bottom}");
        // The left and right sides must be used: at least half their cells have a tee or head.
        let mut used = 0;
        for y in (t + 1)..(t + h - 1) {
            for x in [l, l + w - 1] {
                let c = buf[(x as u16, y as u16)].symbol();
                if matches!(c, "<" | ">" | "├" | "┤" | "─" | "┬" | "┴") {
                    used += 1;
                }
            }
        }
        assert!(used >= (h - 2) as usize, "{id} 左右两侧没被用上:{used}");
    }
}

/// When a side overflows, plain links move first and the 16 heads stay put.
#[test]
fn overflow_moves_plain_links_first() {
    let mut boxes = vec![bx("hub", (30, 20), (12, 18), BoxStyle::Single)];
    let mut texts = vec![txt("th", "hub", (31, 28), "hub")];
    let mut edges = Vec::new();
    // 16 heads arriving from the right.
    for k in 0..16 {
        let id = format!("s{k}");
        boxes.push(bx(&id, (50, 21 + k), (6, 1), BoxStyle::None));
        texts.push(txt(&format!("t{k}"), "服", (50, 21 + k), &id));
        edges.push(edg(&format!("e{k}"), &id, "hub", Dir::Forward, "g0"));
    }
    // One plain link turning upwards, the 17th right-side endpoint.
    boxes.push(bx("top", (20, 2), (6, 1), BoxStyle::None));
    texts.push(txt("tt", "上边", (20, 2), "top"));
    edges.push(edg("eup", "top", "hub", Dir::None, "g0"));
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        boxes,
        texts,
        edges,
    );
    let area = Rect::new(0, 0, 120, 60);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let arrows: Vec<u8> = (0..16)
        .map(|e| plan.get(0, true, e).expect("箭头端口").0)
        .collect();
    assert!(
        arrows.iter().all(|&s| s == RIGHT),
        "16 个箭头该全留在右侧:{arrows:?}"
    );
    let line = plan.get(0, false, 16).expect("普通连接端口").0;
    assert_ne!(line, RIGHT, "多余的普通连接该挪走:{line}");
    assert_eq!(line, UP, "往上拐的连接该走上边:{line}");
}

/// A one-row none node does not move its group: one cell each side, sharing is normal.
#[test]
fn one_row_text_node_keeps_side() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("txt", (20, 10), (6, 1), BoxStyle::None),
            bx("l1", (4, 8), (2, 1), BoxStyle::None),
            bx("l2", (4, 12), (4, 1), BoxStyle::None),
        ],
        vec![
            txt("tt", "一行字", (20, 10), "txt"),
            txt("t1", "左", (4, 8), "l1"),
            txt("t2", "左二", (4, 12), "l2"),
        ],
        vec![
            edg("e1", "l1", "txt", Dir::None, "g0"),
            edg("e2", "l2", "txt", Dir::None, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 80, 30);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let sides: Vec<u8> = (0..2)
        .map(|e| plan.get(0, false, e).expect("有端口").0)
        .collect();
    assert!(
        sides.iter().all(|&s| s == LEFT),
        "两条水平连线都该留在左边:{sides:?}"
    );
}

/// Explicitly pinned ports never take part in overflow moves.
#[test]
fn explicit_ports_do_not_move() {
    let mut boxes = vec![bx("hub", (30, 20), (12, 18), BoxStyle::Single)];
    let mut texts = vec![txt("th", "hub", (31, 28), "hub")];
    let mut edges = Vec::new();
    for k in 0..18 {
        let id = format!("s{k}");
        boxes.push(bx(&id, (50, 21 + k.min(15)), (6, 1), BoxStyle::None));
        texts.push(txt(&format!("t{k}"), "服", (50, 21 + k.min(15)), &id));
        let mut e = edg(&format!("e{k}"), &id, "hub", Dir::Forward, "g0");
        if k == 17 {
            // The last one is pinned to the right side (capacity 16).
            e.to_side = Some(crate::model::Side::Right);
        }
        edges.push(e);
    }
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        boxes,
        texts,
        edges,
    );
    let area = Rect::new(0, 0, 120, 70);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let forced = plan.get(0, true, 17).expect("钉住的端口").0;
    assert_eq!(forced, RIGHT, "显式端口不该被挪走");
}

/// The short-edge test is relative to box size, not picture size or a fixed cell count.
#[test]
fn short_edge_threshold_is_relative() {
    // (0,0,40,3) and (18,5,4,2): centres 4.5 apart, longest side 40, so short.
    let rects = vec![(0, 0, 40, 3), (18, 5, 4, 2), (0, 0, 4, 2), (30, 0, 4, 2)];
    assert!(
        relative_short_edge(&rects, 0, 1),
        "8 格左右的边配 40 格框该算短边"
    );
    assert!(
        !relative_short_edge(&rects, 2, 3),
        "30 格的边配 4 格框不算短边"
    );
}

/// A relatively short edge: with no pinned sides, the side pair must allow a straight run.
#[test]
fn short_edge_picks_straight_side_pair() {
    // A small box slightly right of a large one's top: short edge with horizontal overlap.
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("big", (0, 0), (40, 3), BoxStyle::None),
            bx("small", (30, 5), (4, 1), BoxStyle::None),
        ],
        vec![
            txt("tb", "大框", (0, 0), "big"),
            txt("ts", "小", (30, 5), "small"),
        ],
        vec![edg("e1", "small", "big", Dir::None, "g0")],
    );
    let area = Rect::new(0, 0, 80, 20);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    assert!(relative_short_edge(&rects, 0, 1), "这条边相对很短");
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let (s0, _) = plan.get(0, false, 0).expect("大框端口");
    let (s1, _) = plan.get(1, false, 0).expect("小框端口");
    let vertical = matches!(s0, UP | DOWN) && matches!(s1, UP | DOWN);
    assert!(vertical, "横向重叠的短边该走上下:{s0}/{s1}");
}

/// Explicit ports are not part of the short-edge side choice.
#[test]
fn explicit_ports_skip_short_edge_side_pick() {
    // Side by side: small box left-pinned towards the big one, big one right-pinned.
    let mut e = edg("e1", "small", "big", Dir::None, "g0");
    e.from_side = Some(crate::model::Side::Left);
    e.to_side = Some(crate::model::Side::Right);
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("big", (0, 0), (40, 3), BoxStyle::None),
            bx("small", (50, 1), (4, 1), BoxStyle::None),
        ],
        vec![
            txt("tb", "大框", (0, 0), "big"),
            txt("ts", "小", (50, 1), "small"),
        ],
        vec![e],
    );
    let area = Rect::new(0, 0, 80, 20);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    assert_eq!(
        plan.get(1, false, 0).expect("有端口").0,
        LEFT,
        "小框钉的是左边"
    );
    assert_eq!(
        plan.get(0, false, 0).expect("有端口").0,
        RIGHT,
        "大框钉的是右边"
    );
}

/// The cell outside a port must not be another box, or the head gets covered by it.
#[test]
fn port_outside_not_another_box() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let area = Rect::new(0, 0, 140, 44);
    let p = panels(&d, area, &View::default());
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let mut bad = Vec::new();
    // A head's outside cell must not fall inside another box (it would cover their text).
    // A line end may be buried one cell under a narrow side, losing only that end.
    for (e, ed) in d.edges.iter().enumerate() {
        for (box_id, arrow) in [(&ed.from, ed.dir.heads().0), (&ed.to, ed.dir.heads().1)] {
            let Some(b) = d.box_idx(box_id) else { continue };
            if !arrow {
                continue;
            }
            let Some((side, cell)) = plan.get(b, arrow, e) else {
                continue;
            };
            let o = Canvas::outside(cell, side);
            if let Some(k) = (0..rects.len()).find(|&k| {
                k != b && {
                    let r = rects[k];
                    o.0 >= r.0 && o.0 < r.0 + r.2 && o.1 >= r.1 && o.1 < r.1 + r.3
                }
            }) {
                bad.push(format!(
                    "{} 端在 {} 外面是 {:?} 的框",
                    ed.id, box_id, d.boxes[k].id
                ));
            }
        }
    }
    assert!(bad.is_empty(), "端口落点被别的框盖住: {bad:?}");
}

/// A one-row target (dbus.service) has one head slot per side but four incoming edges:
/// with no free cell the head is shared, still drawn and counted in every edge's cells.
#[test]
fn one_row_target_shares_arrow_cell() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let area = Rect::new(0, 0, 150, 44);
    let view = View::default();
    let mut buf = Buffer::empty(area);
    let (_map, hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
    let proj = proj_of(&d, &view, 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let plan = plan_ports(&d, &proj, &rects, "");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let (left, top, w, _h) = rects[b7];
    // One head cell per side, just outside the border, and both must hold a head.
    for cell in [(left - 1, top), (left + w, top)] {
        let arrow = buf[(cell.0 as u16, cell.1 as u16)].symbol().to_string();
        assert!(
            "><^v".contains(&arrow),
            "{cell:?} 应该是箭头,实为 {arrow:?}"
        );
    }
    // Every edge into dbus.service needs a port, and the shared head cell counts for each.
    let mut shared = 0;
    for e in d.adj(b7).iter().copied() {
        let ed = &d.edges[e];
        let to = d.box_idx(&ed.to).expect("to");
        let arrow_end = if to == b7 {
            ed.dir.heads().1
        } else {
            ed.dir.heads().0
        };
        let Some((side, cell)) = plan.get(b7, arrow_end, e) else {
            continue;
        };
        if !arrow_end {
            continue;
        }
        let arrow = Canvas::outside(cell, side);
        let cells = hit.edge_cells.get(&e).cloned().unwrap_or_default();
        assert!(
            cells.contains(&arrow),
            "边 {e} 的格子里没有箭头格 {arrow:?}"
        );
        shared += 1;
    }
    assert!(
        shared >= 4,
        "指向 dbus.service 的箭头边不止两条,实际 {shared}"
    );
}

/// Selecting an edge that shares a head lights its cell, repainted in the selection colour.
#[test]
fn selecting_shared_arrow_edge_lights_it() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let area = Rect::new(0, 0, 150, 44);
    let mut view = View::default();
    view.ego = Some(b7);
    let proj = proj_of(&d, &view, 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let plan = plan_ports(&d, &proj, &rects, "");
    // Four incoming edges share the two head cells; selecting any of them lights it.
    for e in [1usize, 26, 40, 50] {
        let ed = &d.edges[e];
        let (side, cell) = plan
            .get(b7, ed.dir.heads().1, e)
            .unwrap_or_else(|| panic!("边 {e} 应该有端口"));
        let arrow = Canvas::outside(cell, side);
        view.pick = Pick::Edge(e);
        let mut buf = Buffer::empty(area);
        let (_map, hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
        let cells = hit.edge_cells.get(&e).cloned().unwrap_or_default();
        assert!(
            cells.contains(&arrow),
            "边 {e} 的格子里没有箭头格 {arrow:?}"
        );
        let c = &buf[(arrow.0 as u16, arrow.1 as u16)];
        let glyph = c.symbol().to_string();
        assert!("<>^v".contains(&glyph), "边 {e} 的箭头格被画成了 {glyph:?}");
        assert!(
            c.modifier.contains(Modifier::BOLD),
            "选中边 {e} 的箭头 {glyph:?} 在 {arrow:?} 没亮"
        );
    }
}

/// When a shared head cell belongs to another lit edge, overlay must not repaint it as a line.
#[test]
fn ego_overlay_keeps_shared_arrow() {
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![],
        vec![],
        vec![],
    );
    let mut cv = Canvas::new(
        (0, 0, 20, 8),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 19, 7);
    cv.pen_edge = 0;
    cv.edge((0, 4), (13, 4), 0, (200, 30, 40), None, (false, false));
    // Another edge's head sits here, but the glyph still belongs to edge 0.
    cv.pen_edge = 1;
    cv.outside_arrow((13, 4), 2, (10, 200, 40));
    let ego = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from([0usize, 1]),
        edge_labels: HashSet::new(),
    };
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
    overlay_ego_edges(&mut buf, &cv, &doc, &ego, None, 0.0);
    assert_eq!(buf[(13, 4)].symbol(), "<", "共用箭头要留着");
}

/// In E mode other edges of the same centre are raised too, but the selected one wins:
/// otherwise it would be covered piecewise by its lit neighbour and look broken.
#[test]
fn selected_edge_beats_other_lit_edges() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let area = Rect::new(0, 0, 150, 44);
    let mut view = View::default();
    view.ego = Some(b7);
    view.pick = Pick::Edge(1);
    let mut buf = Buffer::empty(area);
    let (_map, hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
    let proj = proj_of(&d, &view, 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let in_box = |c: (i32, i32)| {
        rects
            .iter()
            .any(|r| c.0 >= r.0 && c.0 < r.0 + r.2 && c.1 >= r.1 && c.1 < r.1 + r.3)
    };
    let cells = hit.edge_cells.get(&1).cloned().unwrap_or_default();
    assert!(cells.len() > 50, "这条边应该有一大段可见的线");
    let mut dull = Vec::new();
    for (x, y) in cells {
        if in_box((x, y)) {
            continue;
        }
        let cell = &buf[(x as u16, y as u16)];
        if !cell.modifier.contains(Modifier::BOLD) {
            dull.push((
                x,
                y,
                cell.symbol().to_string(),
                hit.edge[(y * 150 + x) as usize],
            ));
        }
    }
    assert!(dull.is_empty(), "选中的边有格子没亮: {dull:?}");
}

/// With one cell per side, a line end must not take the head's cell.
/// (Taking it would show a head at the source too, looking like <->.)
#[test]
fn narrow_side_line_end_avoids_arrow_cell() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 5), (1, 1), BoxStyle::None),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
            bx("c", (13, 1), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "A", (10, 5), "a"),
            txt("tb", "b", (10, 2), "b"),
            txt("tc", "c", (13, 2), "c"),
        ],
        vec![
            // b -> a puts its head on a's top; a -> c is a plain link end, also on a.
            edg("e1", "b", "a", Dir::Forward, "g0"),
            edg("e2", "a", "c", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 40, 16);
    let view = View::default();
    let p = panels(&d, area, &view);
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let arrow_end = plan.get(0, true, 0).expect("e1 在 a 上的箭头端");
    // A line end moves cell or is left to the painter, but never takes the head's cell
    // and never starts outside it, where the head is actually drawn.
    if let Some((side, cell)) = plan.get(0, false, 1) {
        assert_ne!(cell, arrow_end.1, "线端占了箭头格:{arrow_end:?} {cell:?}");
        assert_ne!(
            Canvas::outside(cell, side),
            Canvas::outside(arrow_end.1, arrow_end.0),
            "线端从箭头的外侧格起步:{arrow_end:?} {cell:?}"
        );
    }
}

/// With one cell per side, two plain links may tee into the same port.
#[test]
fn narrow_side_link_ends_merge() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 5), (1, 1), BoxStyle::None),
            bx("b", (10, 1), (1, 1), BoxStyle::Point),
            bx("c", (10, 9), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "A", (10, 5), "a"),
            txt("tb", "b", (10, 2), "b"),
            txt("tc", "c", (10, 10), "c"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "a", "c", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 40, 16);
    let view = View::default();
    let p = panels(&d, area, &view);
    let rects: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let plan = plan_ports(&d, &p.proj, &rects, "");
    let a = plan.get(0, false, 0).expect("e1");
    let b = plan.get(0, false, 1).expect("e2");
    assert_eq!(a.1, b.1, "一格宽的文字节点该共用同一个连接口:{a:?} {b:?}");
}

/// A forward edge (no head at the source) must not look like <-> when selected:
/// a neighbour's head next to the source must not light up with it.
#[test]
fn forward_edge_has_no_double_head() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let e = d
        .edges
        .iter()
        .position(|ed| ed.from == "b7" && ed.to == "b1")
        .expect("dbus.service -> dbus.socket");
    let area = Rect::new(0, 0, 150, 44);
    let mut view = View::default();
    view.ego = Some(b7);
    view.pick = Pick::Edge(e);
    let mut buf = Buffer::empty(area);
    let (_map, _hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
    let proj = proj_of(&d, &view, 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let (l, t, w, h) = rects[b7];
    // The cell outside the source port; in a plain view it is not another edge's head.
    let plan = plan_ports(&d, &proj, &rects, "");
    for (box_i, pe, side, cell) in plan.iter() {
        if pe != e || box_i != b7 {
            continue;
        }
        let out = Canvas::outside(cell, side);
        let glyph = buf[(out.0 as u16, out.1 as u16)].symbol().to_string();
        assert!(
            !"<>^v".contains(&glyph),
            "前向边 {e} 的起点 {out:?} 上是别人的箭头 {glyph:?}"
        );
    }
    // The ring of cells just outside the box.
    let mut lit_arrows = Vec::new();
    for y in (t - 1)..=(t + h) {
        for x in (l - 1)..=(l + w) {
            if x >= l && x < l + w && y >= t && y < t + h {
                continue;
            }
            if x < 0 || y < 0 || x as u16 >= area.width || y as u16 >= area.height {
                continue;
            }
            let cell = &buf[(x as u16, y as u16)];
            let s = cell.symbol().to_string();
            if "<>^v".contains(&s) && cell.modifier.contains(Modifier::BOLD) {
                lit_arrows.push((x, y, s));
            }
        }
    }
    assert!(
        lit_arrows.is_empty(),
        "选中前向边 {e} 时,起点附近别人的箭头跟着亮了:{lit_arrows:?}"
    );
}

/// systemd: with E on fwupd, the shared head into dbus.service uses fwupd's edge colour,
/// not the colour of the first edge.
#[test]
fn ego_on_fwupd_shared_arrow_takes_its_color() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let b8 = d
        .boxes
        .iter()
        .position(|b| b.id == "b8")
        .expect("fwupd.service");
    let e_fwupd = d
        .edges
        .iter()
        .position(|ed| ed.from == "b8" && ed.to == "b7")
        .expect("fwupd -> dbus.service");
    let area = Rect::new(0, 0, 150, 44);
    let proj = proj_of(&d, &View::default(), 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let (l, t, w, _h) = rects[b7];
    let arrow_cell = (l + w, t);
    let shot = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        let mut buf = Buffer::empty(area);
        let (_m, hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
        (buf, hit)
    };
    let (plain, _) = shot(None);
    let (lit, hit) = shot(Some(b8));
    let visible = plain[(arrow_cell.0 as u16, arrow_cell.1 as u16)].fg;
    let active = lit[(arrow_cell.0 as u16, arrow_cell.1 as u16)].fg;
    assert_eq!(
        plain[(arrow_cell.0 as u16, arrow_cell.1 as u16)].symbol(),
        "<",
        "这一格该是箭头"
    );
    // fwupd's own colour: take a stroke cell that belongs only to its edge.
    let sample = hit
        .edge_cells
        .get(&e_fwupd)
        .into_iter()
        .flatten()
        .copied()
        .find(|c| hit.edge[(c.1 * 150 + c.0) as usize] == e_fwupd as i32);
    let Some(sample) = sample else { return };
    let own = lit[(sample.0 as u16, sample.1 as u16)].fg;
    assert_eq!(active, own, "箭头该用 fwupd 那条边的颜色");
    assert_ne!(active, visible, "不该还是可见 owner 的颜色");
}

/// The source's outside cell follows what is actually painted (a full side has no port):
/// a foreign head there must be covered.
#[test]
fn fallback_out_point_covers_foreign_arrow() {
    let Ok(text) = std::fs::read_to_string("examples/real/systemd.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).expect("load");
    let b7 = d
        .boxes
        .iter()
        .position(|b| b.id == "b7")
        .expect("dbus.service");
    let e = d
        .edges
        .iter()
        .position(|ed| ed.from == "b7" && ed.to == "b19")
        .expect("dbus.service -> basic.target");
    let area = Rect::new(0, 0, 150, 44);
    let proj = proj_of(&d, &View::default(), 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let rects: Vec<(i32, i32, i32, i32)> =
        (0..d.boxes.len()).map(|i| rect_of(&d, i, &proj)).collect();
    let (l, t, _w, _h) = rects[b7];
    let start_out = (l - 1, t);
    // In a plain view this cell holds another edge's incoming head.
    let mut plain = Buffer::empty(area);
    render_map_hit(&mut plain, area, &d, &View::default(), 1.0);
    let before = plain[(start_out.0 as u16, start_out.1 as u16)]
        .symbol()
        .to_string();
    assert!("<>^v".contains(&before), "起点外侧格本来是箭头: {before:?}");
    // Selecting this edge, the foreign head yields.
    let mut view = View::default();
    view.ego = Some(b7);
    view.pick = Pick::Edge(e);
    let mut buf = Buffer::empty(area);
    render_map_hit(&mut buf, area, &d, &view, 1.0);
    let after = buf[(start_out.0 as u16, start_out.1 as u16)]
        .symbol()
        .to_string();
    assert!(
        !"<>^v".contains(&after),
        "选中后起点外侧格上还是箭头: {after:?}"
    );
}

/// Shared head colour follows the active owner: selected > only lit in E > the visible edge.
#[test]
fn shared_arrow_color_follows_active_owner() {
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![],
        vec![],
        vec![],
    );
    let mk = || {
        let mut cv = Canvas::new(
            (0, 0, 20, 8),
            vec![false; 2],
            0,
            crate::glyphs::Glyphs::UNICODE,
        );
        cv.frame = (0, 0, 19, 7);
        cv.pen_edge = 0;
        cv.outside_arrow((12, 4), 2, (255, 0, 0));
        cv.pen_edge = 1;
        cv.outside_arrow((12, 4), 2, (0, 0, 255));
        cv
    };
    let ego_of = |edges: Vec<usize>| Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from_iter(edges),
        edge_labels: HashSet::new(),
    };
    let run = |cv: &Canvas, ego: &Ego, picked: Option<usize>| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        overlay_ego_edges(&mut buf, cv, &doc, ego, picked, 0.0);
        let c = &buf[(12, 4)];
        (c.symbol().to_string(), c.fg, c.modifier)
    };
    let cv = mk();
    // 1. Only the blue edge is lit in E: blue.
    let (_, fg, mods) = run(&cv, &ego_of(vec![1]), None);
    assert_eq!(fg, Color::Rgb(0, 0, 255), "箭头该跟亮着的那条边");
    assert!(!mods.contains(Modifier::BOLD), "没选中就不加粗");
    // 2. Both lit: keep the visible colour (red), no random jumps.
    let (_, fg, _) = run(&cv, &ego_of(vec![0, 1]), None);
    assert_eq!(fg, Color::Rgb(255, 0, 0), "多个 owner 同时亮时用可见色");
    // 3. The blue edge is selected: pulse plus bold.
    let (_, _, mods) = run(&cv, &ego_of(vec![0, 1]), Some(1));
    assert!(mods.contains(Modifier::BOLD), "选中那条要呼吸");
    // 4. No E, no selection: the colour lookup falls back to the visible colour.
    assert_eq!(
        arrow_ink_for(&cv, (12, 4), None, None),
        Some((0, (255, 0, 0))),
        "普通模式用可见 owner 的颜色"
    );
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
    let plain = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::new(),
        edge_labels: HashSet::new(),
    };
    overlay_ego_edges(&mut buf, &cv, &doc, &plain, None, 0.0);
    assert_eq!(
        buf[(12, 4)].fg,
        Color::Reset,
        "E 没亮到的箭头不归 overlay 管"
    );
}

/// Mid-path foreign head: overlay leaves it untouched, no pulse, no repaint.
#[test]
fn foreign_arrow_ignores_selection_pulse() {
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![],
        vec![],
        vec![],
    );
    let mut cv = Canvas::new(
        (0, 0, 20, 8),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 19, 7);
    cv.pen_edge = 0;
    cv.edge((0, 4), (12, 4), 0, (200, 30, 40), None, (false, false));
    // Edge 1 puts its head on a mid-path cell.
    cv.pen_edge = 1;
    cv.outside_arrow((6, 4), 2, (10, 200, 40));
    let ego = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from([0usize, 1]),
        edge_labels: HashSet::new(),
    };
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
    overlay_ego_edges(&mut buf, &cv, &doc, &ego, Some(0), 1.0);
    let cell = &buf[(6, 4)];
    assert_eq!(cell.symbol(), "<", "别人的箭头字形要留着");
    assert!(
        !cell.modifier.contains(Modifier::BOLD),
        "别人的箭头不能跟着选中脉动"
    );
}

/// Side choice looks at projections first: a wide box and a small one right below overlap,
/// so route top/bottom; centre difference alone would pick left/right.
#[test]
fn horizontal_overlap_routes_vertically() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("wide", (0, 0), (40, 2), BoxStyle::None),
            bx("low", (28, 4), (4, 2), BoxStyle::None),
            bx("side", (50, 0), (4, 2), BoxStyle::None),
        ],
        vec![
            txt("tw", "一条很长很长的文字", (0, 0), "wide"),
            txt("tl", "小", (28, 4), "low"),
            txt("ts", "旁", (50, 0), "side"),
        ],
        vec![],
    );
    let rs = rects_of(&d);
    // The centre offset is horizontal (dx=10, dy=4) but the spans overlap: route vertically.
    assert_eq!(end_side(&d, 0, 1, &rs, None), DOWN, "宽框该从下面出去");
    assert_eq!(end_side(&d, 1, 0, &rs, None), UP, "小框该从上面进来");
    // Side-by-side boxes still route left/right.
    assert_eq!(end_side(&d, 0, 2, &rs, None), RIGHT);
    assert_eq!(end_side(&d, 2, 0, &rs, None), LEFT);
}

/// Layered layout: edges across layers always run vertically, whatever the centres do.
#[test]
fn layered_edges_route_top_down() {
    let mut d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (5, 3), BoxStyle::Single),
            bx("b", (30, 0), (5, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 1), "a"), txt("tb", "b", (31, 1), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    // Without layer numbers, sides follow the centre offset: left and right.
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), None), RIGHT);
    crate::layout::apply(&mut d, crate::layout::Layout::Layered).expect("分层");
    // With layers, b is below a: leave a's bottom, enter b's top.
    assert_eq!(
        end_side(&d, 0, 1, &rects_of(&d), None),
        DOWN,
        "源端该从下面出去"
    );
    assert_eq!(
        end_side(&d, 1, 0, &rects_of(&d), None),
        UP,
        "目标端该从上面进来"
    );
    // An explicit side still takes precedence.
    assert_eq!(
        end_side(&d, 0, 1, &rects_of(&d), Some(crate::model::Side::Left)),
        LEFT,
        "显式端口压过层号"
    );
}

/// Planning separates two same-group edges between one pair of boxes; they stay off each other.
#[test]
fn plan_separates_same_group_edges() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 10), (4, 3), BoxStyle::Rounded),
            bx("b", (30, 14), (4, 3), BoxStyle::Rounded),
        ],
        vec![txt("ta", "a", (1, 11), "a"), txt("tb", "b", (31, 15), "b")],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "a", "b", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 80, 30);
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &d, &View::default(), &FrameHit::default());
    let cells = |e: usize| -> std::collections::HashSet<(i32, i32)> {
        hit.edge_cells
            .get(&e)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default()
    };
    let (a, b) = (cells(0), cells(1));
    let shared = a.intersection(&b).count();
    assert!(shared <= 2, "两条平行边共线了 {shared} 格:{a:?} {b:?}");
}

/// Tall wall case: both ends need strokes, and a line must not pass through the wall.
#[test]
fn tall_wall_between_nodes_still_connects() {
    let Ok(text) = std::fs::read_to_string("/tmp/wall.gtui") else {
        return;
    };
    let doc = match crate::document::Document::load(&text) {
        Ok(d) => d,
        Err(_) => return,
    };
    let area = Rect::new(0, 0, 80, 60);
    let mut buf = Buffer::empty(area);
    let (p, _) = render_hit(&mut buf, area, &doc, &View::default(), &FrameHit::default());
    // Screen cells of a and b.
    let a = p.proj.cell((0.0, 0.0));
    let b = p.proj.cell((40.0, 0.0));
    let stroke = |x: i32, y: i32| {
        matches!(
            buf[(x as u16, y as u16)].symbol(),
            "─" | "│" | "╭" | "╮" | "╰" | "╯" | ">" | "<" | "^" | "v"
        )
    };
    // Both ends need a stroke in one of the four neighbours; the run may go over the top.
    let adjacent = |x: i32, y: i32| {
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|d| stroke(x + d.0, y + d.1))
    };
    assert!(adjacent(a.0, a.1), "a 端该有线:{a:?}");
    assert!(adjacent(b.0, b.1), "b 端该有线:{b:?}");

    let above = (p.map.y as i32..a.1).any(|y| {
        (p.map.x as i32..(p.map.x + p.map.width) as i32).any(|x| {
            buf[(x as u16, y as u16)].symbol() == "─" || buf[(x as u16, y as u16)].symbol() == "│"
        })
    });
    assert!(above, "该从墙上/下绕过去");
}

#[test]
fn panning_is_pure_translation() {
    let Ok(text) = std::fs::read_to_string("examples/fun/dynasties.gtui") else {
        return; // Sample missing (*.gtui is gitignored), so skip this test.
    };
    let doc = match crate::document::Document::load(&text) {
        Ok(d) => d,
        Err(_) => return,
    };
    let area = Rect::new(0, 0, 100, 30);
    let stroke = |s: &str| {
        matches!(
            s,
            "─" | "│"
                | "╭"
                | "╮"
                | "╯"
                | "╰"
                | "├"
                | "┤"
                | "┬"
                | "┴"
                | "┼"
                | "v"
                | "^"
                | "<"
                | ">"
                | "○"
                | "◎"
                | "●"
                | "◉"
        )
    };
    // (visible canvas rect, canvas cell -> stroke glyph)
    type Shot = (
        (i32, i32, i32, i32),
        std::collections::HashMap<(i32, i32), String>,
    );
    let shot = |cy: f64| -> Shot {
        let mut v = View::default();
        v.zoom = 1.0;
        v.node_labels = false;
        v.center = Some((30.0, cy));
        let mut buf = Buffer::empty(area);
        let (p, _) = render_hit(&mut buf, area, &doc, &v, &FrameHit::default());
        let lo = p.proj.canvas(p.map.x as i32, p.map.y as i32);
        let hi = p.proj.canvas(
            (p.map.x + p.map.width) as i32 - 1,
            (p.map.y + p.map.height) as i32 - 1,
        );
        let mut out = std::collections::HashMap::new();
        for y in p.map.y as i32..(p.map.y + p.map.height) as i32 {
            for x in p.map.x as i32..(p.map.x + p.map.width) as i32 {
                let ch = buf[(x as u16, y as u16)].symbol().to_string();
                if stroke(&ch) {
                    out.insert(p.proj.canvas(x, y), ch);
                }
            }
        }
        ((lo.0, lo.1, hi.0, hi.1), out)
    };
    for base in (6..70).step_by(3) {
        let (a_rect, a) = shot(base as f64);
        assert!(!a.is_empty(), "样例里该有笔画");
        for step in [3.0f64] {
            let cy = base as f64 + step;
            let (b_rect, b) = shot(cy);
            let (x0, y0) = (a_rect.0.max(b_rect.0), a_rect.1.max(b_rect.1));
            let (x1, y1) = (a_rect.2.min(b_rect.2), a_rect.3.min(b_rect.3));
            assert!(x1 > x0 && y1 > y0, "两个视口该有重叠");
            for y in y0..=y1 {
                for x in x0..=x1 {
                    assert_eq!(
                        a.get(&(x, y)),
                        b.get(&(x, y)),
                        "cy={cy}:({x},{y}) 挪视图后不一样"
                    );
                }
            }
        }
    }
}

/// The bottom two rows (status / input / hint) are never bold.
#[test]
fn bottom_rows_not_bold() {
    use ratatui::style::Modifier;
    let d = points();
    let area = Rect::new(0, 0, 100, 24);
    let mut base = View {
        file_display: Some("~/proj/x.gtui".to_string()),
        cwd: "~/proj".to_string(),
        ..View::default()
    };
    let mut cases: Vec<(&str, View)> = Vec::new();
    cases.push(("idle", base.clone()));
    let mut v = base.clone();
    v.note = Some("error: cannot open /tmp/x".to_string());
    cases.push(("note", v));
    let mut v = base.clone();
    v.cmd = Some("w /tmp/a.gtui".to_string());
    v.cmd_hint = Some("/tmp/a.gtui".to_string());
    cases.push(("command", v));
    let mut v = base.clone();
    v.query = Some("ab".to_string());
    cases.push(("search", v));
    let mut v = base.clone();
    v.prompt = Some((crate::app::Prompt::NewBox, "abc".to_string()));
    cases.push(("prompt", v));
    base.sel = Some(crate::app::RectSel {
        anchor: (0, 0),
        end: (2, 2),
        confirmed: false,
    });
    cases.push(("select", base));
    for (name, view) in cases {
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        for y in [area.height - 2, area.height - 1] {
            for x in 0..area.width {
                let cell = &buf[(x, y)];
                assert!(
                    !cell.modifier.contains(Modifier::BOLD),
                    "{name}: ({x},{y}) 加粗了: {:?}",
                    cell.symbol()
                );
            }
        }
    }
}

/// In the English UI the bottom two rows carry no CJK (document content aside).
#[test]
fn english_bottom_rows_have_no_cjk() {
    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| {
            matches!(c as u32,
                0x3000..=0x303f | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xff00..=0xffef)
        })
    }
    // Keep the content ASCII too: this tests chrome, not document names.
    let d = doc_of(
        vec![grp("g0", "line", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (10, 0), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 0), "a"), txt("tb", "b", (10, 0), "b")],
        vec![edg("e1", "a", "b", Dir::None, "g0")],
    );
    let area = Rect::new(0, 0, 120, 30);
    let mut base = View {
        file_display: Some("~/proj/x.gtui".to_string()),
        cwd: "~/proj".to_string(),
        ..View::default()
    };
    assert_eq!(base.lang, Lang::En);
    let mut cases: Vec<(&str, View)> = Vec::new();
    cases.push(("idle", base.clone()));
    base.note = Some("error: cannot open /tmp/x".to_string());
    cases.push(("note", base.clone()));
    let mut v = base.clone();
    v.sel = Some(crate::app::RectSel {
        anchor: (0, 0),
        end: (2, 2),
        confirmed: false,
    });
    cases.push(("select", v));
    let mut v = base.clone();
    v.cmd = Some("w /tmp/a.gtui".to_string());
    cases.push(("command", v));
    let mut v = base.clone();
    v.query = Some("ab".to_string());
    cases.push(("search", v));
    let mut v = base.clone();
    v.prompt = Some((crate::app::Prompt::NewBox, "abc".to_string()));
    cases.push(("prompt", v));
    let mut v = base.clone();
    v.layer = Some("base".to_string());
    cases.push(("layer", v));
    for (name, view) in cases {
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        for row in [status_line(&buf), last_line(&buf)] {
            assert!(!has_cjk(&row), "{name}: 英文界面里出现了 CJK: {row}");
        }
    }
}

/// Two bottom rows: file/mode/cwd/coordinates, then errors; short terminals fall back to one.
#[test]
fn bottom_two_row_layout() {
    let d = points();
    let area = Rect::new(0, 0, 100, 24);
    let mut view = View {
        file_display: Some("~/proj/x.gtui".to_string()),
        cwd: "~/proj".to_string(),
        tab: Some((2, 5)),
        tab_dirty: true,
        cursor: (1, 4),
        note: Some("error: cannot open /tmp/foo.gtui".to_string()),
        ..View::default()
    };
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let top = status_line(&buf);
    assert!(
        top.contains("[2/5] ~/proj/x.gtui*"),
        "上行该有 [n/m] 路径: {top}"
    );
    assert!(top.contains("NORMAL"), "上行该有模式: {top}");
    assert!(top.contains("~/proj | 1,4"), "上行该有 cwd 与坐标: {top}");
    let bottom = last_line(&buf);
    assert!(
        bottom.contains("error: cannot open"),
        "下行该放错误: {bottom}"
    );
    // The map area loses two rows.
    let p = panels(&d, area, &view);
    assert_eq!(p.map.height, area.height - 2, "地图要让出两行");

    // `:` input: the file line stays, the input goes on the lower row.
    view.cmd = Some("w /tmp/a.gtui".to_string());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let top = status_line(&buf);
    assert!(
        top.contains("~/proj/x.gtui*"),
        "输入时上行仍显示路径: {top}"
    );
    assert!(top.contains("COMMAND"), "输入时模式是 COMMAND: {top}");
    assert!(last_line(&buf).contains(":w /tmp/a.gtui"), "输入在下行");

    // Short terminal: one row, the map yields one.
    let small = Rect::new(0, 0, 40, 3);
    let mut view = View::default();
    view.note = Some("boom".to_string());
    let mut buf = Buffer::empty(small);
    render(&mut buf, small, &d, &view);
    assert_eq!(status_rows(small), 1);
    assert_eq!(panels(&d, small, &view).map.height, 2);
    assert!(status_line(&buf).contains("boom"));
    // 10 rows still gets two bottom rows.
    let ten = Rect::new(0, 0, 40, 10);
    assert_eq!(status_rows(ten), 2);
}

/// The status row says VIEW at 1:1 and the factor otherwise; idle it reports the box text.
#[test]
fn status_line_shows_view_and_zoom() {
    let d = sample();
    let area = Rect::new(0, 0, 200, 50);
    let line_of = |view: &View| {
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, view);
        last_line(&buf)
    };
    let mut idle_view = View::default();
    set_at(&d, &mut idle_view, 0);
    let idle = line_of(&idle_view);
    let status = status_line(&buf_status(&d, &idle_view));
    assert!(status.contains("100%"), "状态行该有缩放:{status}");
    assert!(status.contains("NORMAL"), "空闲时该是 NORMAL:{status}");
    let flat: String = status.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(flat.contains("中央"), "状态行该报当前 box 的文字:{status}");
    let mut view = View::default();
    view.zoom = 2.0;
    let zoomed_status = status_line(&buf_status(&d, &view));
    assert!(
        zoomed_status.contains("200%"),
        "缩放后该标百分比:{zoomed_status}"
    );
    assert_ne!(status, zoomed_status, "缩放前后状态行该不同");
    let _ = idle;
}

/// Zoom follows only `zoom`: proj.scale = 1 / zoom, independent of the window size.
#[test]
fn zoom_independent_of_window() {
    let d = points();
    let a = Rect::new(0, 0, 100, 30);
    let b = Rect::new(0, 0, 200, 60);
    let v = View::default();
    let pa = panels(&d, a, &v);
    let pb = panels(&d, b, &v);
    assert_eq!(pa.proj.scale, pb.proj.scale, "缩放不随窗口变");
    assert_eq!(pa.proj.scale, 1.0, "默认一份单元格占一列");
    let q = (10.0, 0.0);
    let ca = pa.proj.cell(q);
    let cb = pb.proj.cell(q);
    let ma = (
        pa.map.x as i32 + pa.map.width as i32 / 2,
        pa.map.y as i32 + pa.map.height as i32 / 2,
    );
    let mb = (
        pb.map.x as i32 + pb.map.width as i32 / 2,
        pb.map.y as i32 + pb.map.height as i32 / 2,
    );
    assert_eq!(
        (ca.0 - ma.0, ca.1 - ma.1),
        (cb.0 - mb.0, cb.1 - mb.1),
        "同一坐标落在同量级的位置"
    );
    for (zoom, want) in [(2.0, 0.5), (0.5, 2.0)] {
        let mut vz = View::default();
        vz.zoom = zoom;
        let pz = panels(&d, a, &vz);
        assert_eq!(pz.proj.scale, want, "zoom {zoom}");
    }
}

/// Two edges on one node pair keep separate cells and are distinguishable when cycling.
#[test]
fn opposite_edges_keep_own_cells() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (6, 3), BoxStyle::Single),
            bx("b", (14, 0), (6, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (1, 1), "a"), txt("tb", "b", (15, 1), "b")],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "b", "a", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 80, 20);
    let mut buf = Buffer::empty(area);
    let (_p, hit) = render_hit(&mut buf, area, &d, &View::default(), &FrameHit::default());
    let c1 = hit.edge_cells.get(&0).cloned().unwrap_or_default();
    let c2 = hit.edge_cells.get(&1).cloned().unwrap_or_default();
    assert!(!c1.is_empty() && !c2.is_empty(), "两条边都要有格子");
    assert_ne!(c1, c2, "两条边不能完全重合");
    // With two candidates on one cell, `E` has something to cycle through.
    let shared = c1.iter().find(|c| c2.contains(c));
    if let Some(c) = shared {
        let list = &hit.candidates[c];
        assert!(
            list.contains(&0) && list.contains(&1),
            "候选该有两条:{list:?}"
        );
    }
}

/// Four edges between point nodes (four groups): one parallel line each, heads on target.
#[test]
fn point_node_edges_one_line_each() {
    let d = doc_of(
        vec![
            grp("g0", "一", (76, 142, 248)),
            grp("g1", "二", (239, 75, 75)),
            grp("g2", "三", (34, 180, 106)),
            grp("g3", "四", (245, 158, 11)),
        ],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 5), (1, 1), BoxStyle::Point),
            bx("b", (20, 5), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 6), "a"), txt("tb", "b", (20, 6), "b")],
        vec![
            edg("e1", "a", "b", Dir::None, "g0"),
            edg("e2", "a", "b", Dir::None, "g1"),
            edg("e3", "a", "b", Dir::None, "g2"),
            edg("e4", "a", "b", Dir::None, "g3"),
        ],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ax, ay) = p.proj.cell((0.0, 5.0));
    let (bx, _) = p.proj.cell((20.0, 5.0));
    let mid = (ax + bx) / 2;
    // The four lines land on four distinct rows.
    let rows: Vec<i32> = (-3..=3)
        .map(|dy| ay + dy)
        .filter(|y| (ax + 2..bx - 2).all(|x| buf[(x as u16, *y as u16)].symbol() == "─"))
        .collect();
    assert_eq!(rows.len(), 4, "四条边该占四条不同的行:{rows:?}");
    assert!(rows.contains(&ay), "其中一条走中间那条直线");
    assert_eq!(buf[(mid as u16, ay as u16)].symbol(), "─");
}

/// E on an edge also highlights the neighbour at the other end; the cursor end already pulses.
#[test]
fn selecting_edge_highlights_neighbour() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
            bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "甲", (1, 1), "a"),
            txt("tb", "乙", (31, 1), "b"),
            txt("tc", "丙", (61, 1), "c"),
        ],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 90, 16);
    let mut view = View::default();
    view.pulse = 0.0;
    view.pick = Pick::Edge(0);
    view.cursor = (1, 1); // Cursor on 甲.
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let border = |i: usize| {
        let (l, t, _, _) = rect_of(&d, i, &p.proj);
        buf[(l as u16, t as u16)].fg
    };
    // Redraw at another pulse phase: the neighbour changes, unconnected boxes do not.
    let mut buf2 = Buffer::empty(area);
    view.pulse = 0.5;
    render(&mut buf2, area, &d, &view);
    let border2 = |i: usize| {
        let (l, t, _, _) = rect_of(&d, i, &p.proj);
        buf2[(l as u16, t as u16)].fg
    };
    assert_ne!(border(1), border2(1), "邻居该跟着呼吸");
    assert_eq!(border(2), border2(2), "没连着的框颜色不变");
}

/// Text-only node: all in/out points sit on its single row, one cell per edge.
#[test]
fn text_node_shares_row_for_both_sides() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (10, 5), (8, 1), BoxStyle::None),
            bx("b", (10, 0), (1, 1), BoxStyle::Point),
            bx("c", (10, 10), (1, 1), BoxStyle::Point),
            bx("d", (25, 5), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "ABCDEFGH", (10, 5), "a"),
            txt("tb", "b", (10, 1), "b"),
            txt("tc", "c", (10, 11), "c"),
            txt("td", "d", (25, 6), "d"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "a", "c", Dir::Forward, "g0"),
            edg("e3", "a", "d", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 60, 20);
    let view = View::default();
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    // a's screen rectangle: each edge's cell inside it is its port.
    let (ax, ay, aw, ah) = box_rects(&d, area, &view)[0];
    let inside = |c: (i32, i32)| c.0 >= ax && c.0 < ax + aw && c.1 >= ay && c.1 < ay + ah;
    let mut ports: Vec<(i32, i32)> = Vec::new();
    for e in 0..3 {
        let cells: Vec<(i32, i32)> = hit
            .edge_cells
            .get(&e)
            .map(|v| v.iter().copied().filter(|&c| inside(c)).collect())
            .unwrap_or_default();
        assert_eq!(cells.len(), 1, "e{e} 该正好占一个端口格:{cells:?}");
        ports.push(cells[0]);
    }
    // One port, one role: a cell is either a head end or a plain link end.
    // e1 (a->b) carries a head at a, e2 (a->c) does not, so the ports must differ.
    assert_ne!(
        ports[0], ports[1],
        "箭头端和连接端不能共用一个格子:{ports:?}"
    );
    assert_ne!(ports[2], ports[0], "另一侧端口分开:{ports:?}");
    assert!(
        ports.iter().all(|c| c.1 == ay),
        "文字节点只有一行,端口都落在这一行:{ports:?}"
    );
}

/// The last bottom row (the hint row in the two-row layout).
fn hint_line(buf: &Buffer) -> String {
    let y = buf.area.height - 1;
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
}

/// Status row: the session hint shows when idle, hidden on echo, dirty disk or overflow.
#[test]
fn status_line_shows_hint_when_idle() {
    let d = sample();
    let mut view = View::default();
    view.tip = Some("5l moves five cells".into());
    let line = hint_line(&buf_status(&d, &view));
    assert!(line.contains("5l moves five cells"), "{line:?}");
    // Two renders give the same string: the hint is session state, not random per frame.
    assert_eq!(line, hint_line(&buf_status(&d, &view)));

    for (name, setup) in [("note", 0usize), ("stale", 1), ("labels", 2)] {
        let mut v = view.clone();
        match setup {
            0 => v.note = Some("saved".into()),
            1 => v.stale = true,
            _ => v.labels_hidden = 3,
        }
        let line = hint_line(&buf_status(&d, &v));
        assert!(
            !line.contains("5l moves five cells"),
            "{name} 的时候不该显示提示:{line:?}"
        );
    }
}

/// A single status row shows no hint; it would crowd out the file and mode.
#[test]
fn single_row_status_has_no_hint() {
    let d = sample();
    let area = Rect::new(0, 0, 120, 6);
    let mut view = View::default();
    view.tip = Some("5l moves five cells".into());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let last: String = (0..area.width)
        .map(|x| buf[(x, area.height - 1)].symbol())
        .collect();
    assert!(!last.contains("5l moves five cells"), "{last:?}");
}

/// Cheatsheet overlay: a TIP row when there is room, omitted on small terminals.
#[test]
fn cheatsheet_overlay_shows_tip() {
    let d = sample();
    let mut view = View::default();
    view.help = true;
    view.tip = Some("5l moves five cells".into());
    let area = Rect::new(0, 0, 120, 60);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let all = screen(&buf).join("\n");
    assert!(
        all.contains("TIP 5l moves five cells"),
        "大终端里要有 TIP 行"
    );
    assert!(
        all.contains("any key closes this"),
        "提示行不能顶掉关掉的说明"
    );
    // Small terminal: even the key table will not fit, so the row is omitted.
    let area = Rect::new(0, 0, 100, 20);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let all = screen(&buf).join("\n");
    assert!(!all.contains("TIP 5l moves"), "小终端不放 TIP:{all}");
    assert!(all.contains("any key closes this"));
}

/// Inline style reaches cells: strike, underline, dim, colour; search fill beats inline colour.
#[test]
fn inline_style_applied_to_cells() {
    let d = doc_of(
        vec![grp("g0", "线", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![bx("b1", (0, 0), (40, 3), BoxStyle::Rounded)],
        vec![txt(
            "t1",
            "~~a~~ <u>b</u> <dim>c</dim> <fg=#ff0000>d</fg> <bg=#001122>e</bg>",
            (1, 1),
            "b1",
        )],
        vec![],
    );
    let area = Rect::new(0, 0, 60, 10);
    let mut view = View::default();
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let (x, y) = p.proj.cell((1.0, 1.0));
    let cell = |k: i32| &buf[((x + k) as u16, y as u16)];
    assert_eq!(cell(0).symbol(), "a");
    assert!(
        cell(0).modifier.contains(Modifier::CROSSED_OUT),
        "~~ 删除线"
    );
    assert_eq!(cell(2).symbol(), "b");
    assert!(
        cell(2).modifier.contains(Modifier::UNDERLINED),
        "<u> 下划线"
    );
    assert_eq!(cell(4).symbol(), "c");
    assert!(cell(4).modifier.contains(Modifier::DIM), "<dim>");
    assert_eq!(cell(6).symbol(), "d");
    assert_eq!(cell(6).fg, Color::Rgb(255, 0, 0), "<fg>");
    assert_eq!(cell(8).symbol(), "e");
    assert_eq!(cell(8).bg, Color::Rgb(0, 17, 34), "<bg>");
    // Search hits switch to a fill (like nvim's Search) and beat inline foreground and fill.
    view.hits = vec![0];
    // Cursor off the hit box: blue fill; on it: orange.
    view.cursor = (0, 20);
    let mut buf2 = Buffer::empty(area);
    render(&mut buf2, area, &d, &view);
    let hit_cell = |k: i32| &buf2[((x + k) as u16, y as u16)];
    // The only hit, so it is the current one: orange fill, dark glyph.
    for k in [0, 6] {
        assert_eq!(
            hit_cell(k).bg,
            Color::Rgb(SEARCH_CUR_BG.0, SEARCH_CUR_BG.1, SEARCH_CUR_BG.2),
            "命中要压过 <fg>/<bg>:{k}"
        );
        assert_eq!(
            hit_cell(k).fg,
            Color::Rgb(SEARCH_CUR_INK.0, SEARCH_CUR_INK.1, SEARCH_CUR_INK.2),
            "橙底配深字:{k}"
        );
    }
    // With the cursor on the hit box it becomes current: orange fill, dark glyph.
    let at = d.boxes[0].at;
    view.cursor = at;
    let mut buf3 = Buffer::empty(area);
    render(&mut buf3, area, &d, &view);
    let cur_cell = |k: i32| &buf3[((x + k) as u16, y as u16)];
    assert_eq!(
        cur_cell(0).bg,
        Color::Rgb(SEARCH_CUR_BG.0, SEARCH_CUR_BG.1, SEARCH_CUR_BG.2),
        "当前命中铺橙底"
    );
    assert_eq!(
        cur_cell(0).fg,
        Color::Rgb(SEARCH_CUR_INK.0, SEARCH_CUR_INK.1, SEARCH_CUR_INK.2)
    );
}

/// Search hits paint only the matched glyphs, not the whole box: current orange, others blue;
/// borders and other lines stay; a name/alias hit with no matching text falls back to the box.
#[test]
fn search_hit_paints_matched_glyphs_only() {
    let d = doc_of(
        vec![grp("g0", "线", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![
            bx("b1", (0, 0), (14, 3), BoxStyle::Rounded),
            bx("b2", (0, 6), (12, 2), BoxStyle::None),
            bx("b3", (0, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "match one", (1, 1), "b1"),
            // Multi-line: the hit is on row two, row one must stay unpainted.
            txt("t2", "row\nmatch two", (0, 6), "b2"),
            txt("t3", "", (0, 10), "b3"),
        ],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 22);
    let blue = Color::Rgb(SEARCH_BG.0, SEARCH_BG.1, SEARCH_BG.2);
    let ink = Color::Rgb(SEARCH_BG_INK.0, SEARCH_BG_INK.1, SEARCH_BG_INK.2);
    let orange = Color::Rgb(SEARCH_CUR_BG.0, SEARCH_CUR_BG.1, SEARCH_CUR_BG.2);
    let cur_ink = Color::Rgb(SEARCH_CUR_INK.0, SEARCH_CUR_INK.1, SEARCH_CUR_INK.2);
    let mut view = View::default();
    view.hits = vec![0, 1, 2];
    view.hit_text = Some("match".to_string());
    let p = panels(&d, area, &view);
    let cell = |x: i32, y: i32| {
        let c = p.proj.cell((x as f64, y as f64));
        (c.0 as u16, c.1 as u16)
    };

    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    // hit_at = 0: b1 is current, all 5 letters get an orange fill.
    for k in 0..5 {
        let (x, y) = cell(1 + k, 1);
        assert_eq!(buf[(x, y)].bg, orange, "当前命中的第 {k} 个字");
        assert_eq!(buf[(x, y)].fg, cur_ink);
    }
    // Unmatched glyphs in the same box stay unpainted.
    let (x, y) = cell(1 + 6, 1); // The space.
    assert_eq!(buf[(x, y)].bg, Color::Reset, "命中的词后面还有字");
    let (x, y) = cell(1 + 7, 1); // 'o'
    assert_eq!(buf[(x, y)].bg, Color::Reset, "one 不在查询里");
    // Borders are not painted.
    let (bx, by) = cell(0, 0);
    assert_eq!(buf[(bx, by)].symbol(), "╭");
    assert_eq!(buf[(bx, by)].bg, Color::Reset, "边框不涂底色");

    // b2 (text only, two rows): the hit on row two is blue.
    for k in 0..5 {
        let (x, y) = cell(k, 7);
        assert_eq!(buf[(x, y)].bg, blue, "非当前命中铺蓝底:{k}");
        assert_eq!(buf[(x, y)].fg, ink, "蓝底配亮字:{k}");
    }
    // Row one's "row" stays unpainted: no fill leaks from the first cell.
    for k in 0..3 {
        let (x, y) = cell(k, 6);
        assert_eq!(buf[(x, y)].bg, Color::Reset, "多行里别的行不涂:{k}");
    }

    // A point node has no glyph: its cell is filled (blue when not current).
    let (px, py) = cell(0, 10);
    assert_eq!(buf[(px, py)].bg, blue, "点节点那一格铺底色");

    // Move hit_at to b2: it turns orange, b1 back to blue.
    view.hit_at = 1;
    let mut buf2 = Buffer::empty(area);
    render(&mut buf2, area, &d, &view);
    let (x, y) = cell(0, 7);
    assert_eq!(buf2[(x, y)].bg, orange, "当前命中跟着 hit_at 走");
    let (x, y) = cell(1, 1);
    assert_eq!(buf2[(x, y)].bg, blue, "b1 不再当前 -> 蓝");
}

/// Wide glyphs with inline style: the continuation cell is kept and colours stay continuous.
#[test]
fn wide_glyph_with_inline_style_kept() {
    let d = doc_of(
        vec![grp("g0", "线", (200, 200, 200))],
        vec![lyr("", 0)],
        vec![bx("b1", (0, 0), (20, 3), BoxStyle::Rounded)],
        vec![txt("t1", "<fg=#ff0000>一二三</fg>", (1, 1), "b1")],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    let mut view = View::default();
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let (x, y) = p.proj.cell((1.0, 1.0));
    for k in 0..6 {
        assert_eq!(
            buf[((x + k) as u16, y as u16)].fg,
            Color::Rgb(255, 0, 0),
            "第 {k} 格该是红字(宽字的续格也算)"
        );
    }
    let _ = &mut view;
}

/// The inline edit draft lays out by display width: a wide glyph takes two cells.
#[test]
fn inline_edit_draft_keeps_wide_glyph() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("b1", (0, 0), (26, 3), BoxStyle::Rounded)],
        vec![txt("t1", "一二三四五六七八", (1, 1), "b1")],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 10);
    let mut view = View::default();
    view.edit = Some(crate::app::BoxEdit {
        box_idx: 0,
        created: false,
        text: "一二三四五六七八".into(),
        caret: 0,
    });
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let (_, y) = p.proj.cell((1.0, 1.0));
    let row = &screen(&buf)[y as usize];
    assert!(
        row.contains("一二三四五六七八"),
        "草稿里八个汉字都要在:{row:?}"
    );
}

/// Isolating an edge label centres on the source end of the edge it is bound to.
#[test]
fn isolate_on_edge_label_uses_source() {
    let mut d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (6, 3), BoxStyle::Single),
            bx("b", (30, 0), (6, 3), BoxStyle::Single),
            bx("c", (0, 10), (6, 3), BoxStyle::Single),
            bx("n1", (12, 8), (6, 3), BoxStyle::None),
        ],
        vec![
            txt("ta", "甲", (1, 1), "a"),
            txt("tb", "乙", (31, 1), "b"),
            txt("tc", "丙", (1, 11), "c"),
            txt("tn", "注", (13, 9), "n1"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "b", "c", Dir::Forward, "g0"),
        ],
    );
    d.rebuild();
    let note = d.box_idx("n1").unwrap();
    d.boxes[note].edge = Some("e1".into());
    let ego = ego_visible(&d, &{
        let mut v = View::default();
        v.ego = Some(note);
        v
    })
    .expect("该算得出隔离集合");
    assert_eq!(ego.center, d.box_idx("a").unwrap(), "注释框按源端当中心");
    assert!(ego.has_edge(0) && !ego.has_edge(1), "只留绑定的那条边");
}

/// Isolating folds regions away, since they mostly enclose hidden nodes.
#[test]
fn isolate_hides_regions() {
    use crate::document::Region;
    let mut d = crate::document::Document::scratch();
    d.add_box("a", (2, 2), BoxStyle::Single).unwrap();
    d.add_box("far", (30, 2), BoxStyle::Single).unwrap();
    let g = d.groups[0].id.clone();
    let (a, far) = (d.boxes[0].id.clone(), d.boxes[1].id.clone());
    d.add_edge(&a, &far, &g, Dir::None).unwrap();
    d.regions.push(Region {
        id: "r1".into(),
        label: "圈".into(),
        color: None,
        style: BoxStyle::Rounded,
        layer: String::new(),
        padding: 1,
        members: vec![a.clone(), far.clone()],
        collapsed: false,
    });
    d.rebuild();
    let area = Rect::new(0, 0, 60, 20);
    let shot = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    };
    assert!(shot(None).contains("圈"), "平时区域标题在");
    let iso = shot(Some(0));
    assert!(!iso.contains("圈"), "隔离时区域不画:\n{iso}");
    assert!(iso.contains('a') || iso.contains('b') || !iso.trim().is_empty());
}

/// Visible edges keep their ports and routes cell for cell: planning still runs in full.
#[test]
fn isolate_keeps_visible_routes() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
            bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
            bx("x", (30, 12), (8, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "甲", (1, 1), "a"),
            txt("tb", "乙", (31, 1), "b"),
            txt("tc", "丙", (61, 1), "c"),
            txt("tx", "旁", (31, 13), "x"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "b", "c", Dir::Forward, "g0"),
            edg("e3", "a", "x", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 90, 24);
    let cells = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        let mut buf = Buffer::empty(area);
        let (_, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        (hit.edge_cells.get(&0).cloned().unwrap_or_default(), hit)
    };
    let (plain, _) = cells(None);
    let (iso, hit) = cells(Some(0));
    assert_eq!(plain, iso, "e1 的路线和端口不能因为开隔离而变");
    assert!(!hit.edge_cells.contains_key(&1), "隐藏的边不进命中表");
    assert!(!iso.is_empty());
}

/// A hidden box is not hittable: its cell behaves as blank.
#[test]
fn hidden_box_not_hittable() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
            bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "甲", (1, 1), "a"),
            txt("tb", "乙", (31, 1), "b"),
            txt("tc", "丙", (61, 1), "c"),
        ],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 90, 16);
    let mut view = View::default();
    let p = panels(&d, area, &view);
    let in_c = p.proj.cell((61.0, 1.0));
    let hit = FrameHit::default();
    assert_eq!(
        pick_at(&d, &hit, (61, 1), in_c, &view),
        Pick::Box(2),
        "平时点得中"
    );
    view.ego = Some(0);
    assert_eq!(
        pick_at(&d, &hit, (61, 1), in_c, &view),
        Pick::None,
        "隔离时隐藏的框当空白"
    );
    assert_eq!(
        pick_at(&d, &hit, (1, 1), p.proj.cell((1.0, 1.0)), &view),
        Pick::Box(0),
        "可见的框照常点得中"
    );
    assert_eq!(box_at(&d, (61, 1), &view), None, "按格查框也查不到");
}

/// Isolating hides point labels of hidden nodes and keeps those of visible ones.
#[test]
fn isolate_hides_hidden_node_labels() {
    let d = points();
    let area = Rect::new(0, 0, 60, 16);
    let shot = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        view.node_labels = true;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    };
    let plain = shot(None);
    assert!(plain.contains('甲') && plain.contains('乙'), "平时三个都标");
    let iso = shot(Some(0));
    assert!(iso.contains('甲') && iso.contains('乙'), "中心和邻居照常标");
    assert!(!iso.contains('丙'), "非邻居的标签不该出现:\n{iso}");
}

/// E isolate: the neighbour and connected edges are drawn, everything else is not.
#[test]
fn neighbour_isolate_hides_everything_else() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
            bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
            bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
        ],
        vec![
            txt("ta", "甲", (1, 1), "a"),
            txt("tb", "乙", (31, 1), "b"),
            txt("tc", "丙", (61, 1), "c"),
        ],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "b", "c", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 90, 16);
    let mut view = View::default();
    view.ego = Some(0); // Light only a and its neighbour b.
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let corner = |i: usize| {
        let (l, t, _, _) = rect_of(&d, i, &p.proj);
        buf[(l as u16, t as u16)].symbol().to_string()
    };
    assert_ne!(corner(0), " ", "自己还在");
    assert_ne!(corner(1), " ", "邻居还在");
    assert_eq!(corner(2), " ", "邻居之外的框该消失");
    // Text in the hidden box is not drawn either.
    let (cx, cy) = p.proj.cell((61.0, 1.0));
    assert_eq!(buf[(cx as u16, cy as u16)].symbol(), " ", "丙 该消失");
    // Edges unrelated to the centre are not drawn: that middle row must be empty.
    let mut strokes = 0;
    for x in 0..90u16 {
        let sym = buf[(x, (p.proj.cell((0.0, 1.0)).1) as u16)].symbol();
        if matches!(sym, "─" | "│" | "╭" | "╮" | "╰" | "╯") && x as i32 > p.map.x as i32
        {
            strokes += 1;
        }
    }
    assert!(strokes > 0, "相连的边要画出来");
    // The b->c edge is not drawn before c: the rightmost cell stays empty.
    let far = p.proj.cell((70.0, 1.0));
    assert_eq!(
        buf[(far.0 as u16, far.1 as u16)].symbol(),
        " ",
        "隐藏的边不画"
    );
}

/// When both ends take one cell (a point node or a one-cell text node), edges must not merge:
/// with no border cells to spread over, lane offsets keep each one hittable.
#[test]
fn many_edges_on_one_cell_node_do_not_overlap() {
    let d = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 5), (1, 1), BoxStyle::None),
            bx("b", (20, 5), (1, 1), BoxStyle::None),
        ],
        vec![txt("ta", "A", (0, 5), "a"), txt("tb", "B", (20, 5), "b")],
        vec![
            edg("e1", "a", "b", Dir::Forward, "g0"),
            edg("e2", "b", "a", Dir::Forward, "g0"),
        ],
    );
    let area = Rect::new(0, 0, 40, 16);
    let view = View::default();
    let mut buf = Buffer::empty(area);
    let (_p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    for e in 0..2 {
        assert!(
            !hit.edge_cells
                .get(&e)
                .cloned()
                .unwrap_or_default()
                .is_empty(),
            "e{e} 该有自己的格子"
        );
    }
    let p = panels(&d, area, &view);
    let (mid, ay) = p.proj.cell((10.0, 5.0));
    // The two lines take one row each in the middle column and are both hittable.
    let lower = hit.edge_at((mid, ay));
    let upper = hit.edge_at((mid, ay - 1));
    assert!(lower.is_some() && upper.is_some(), "上下两行都该有线");
    assert_ne!(lower, upper, "两行不能是同一条边");
}

/// The search row shares the bottom row with the command line.
#[test]
fn search_row_reports_hit_count() {
    let d = points();
    let area = Rect::new(0, 0, 200, 50);
    let _ = &d;
    let mut view = View {
        lang: Lang::Zh,
        ..View::default()
    };
    view.query = Some("甲".to_string());
    view.hits = vec![0, 1, 2];
    view.hit_at = 1;
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let last = last_line(&buf);
    assert!(last.contains("/甲"), "该显示搜索词:{last}");
    assert!(last.contains("3 处匹配"), "该显示命中数:{last}");
    assert!(last.contains("乙"), "该显示当前命中:{last}");
    // Plain digits are a group number.
    let mut view = View {
        lang: Lang::Zh,
        ..View::default()
    };
    view.query = Some("1".to_string());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let last = last_line(&buf);
    assert!(last.contains("没有这个分组"), "样例没有编号分组:{last}");
    // The ":" line owns the status row: it starts at the far left and covers mode and node name.
    let mut view = View::default();
    view.cmd = Some("lang en".to_string());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let last = last_line(&buf);
    assert!(last.starts_with(":lang en"), "命令行该在行首:{last}");
    assert!(!last.contains("VIEW"), "命令行要盖住模式标记:{last}");
    let mut view = View::default();
    view.note = Some("换了布局".to_string());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    assert!(last_line(&buf).contains("换了布局"));
}

/// Help overlay: it shows the key cheatsheet, not the map, and the English one has no Chinese.
#[test]
fn help_overlay_covers_map() {
    let d = points();
    let area = Rect::new(0, 0, 90, 30);
    let mut view = View {
        lang: Lang::Zh,
        ..View::default()
    };
    view.help = true;
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let text = screen(&buf).join("\n");
    assert!(text.contains("键位速查"), "该有标题:\n{text}");
    assert!(text.contains("任意键关掉"), "该提示怎么关:\n{text}");
    assert!(text.contains("半屏往下"), "该列出半屏跳:\n{text}");
    assert!(!text.contains('○'), "浮层打开时不该还在画地图:\n{text}");
    let mut view = View::default();
    view.help = true;
    view.lang = Lang::En;
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let text = screen(&buf).join("\n");
    assert!(text.contains("key reference"), "{text}");
    // The overlay border is box drawing (not UI text), so only scan for CJK.
    let cjk = |c: char| {
        matches!(
            c as u32,
            0x3000..=0x303F | 0x4E00..=0x9FFF | 0xFF01..=0xFF5E
        )
    };
    assert!(!text.chars().any(cjk), "英文速查里不该有中文:\n{text}");
}

/// Click a box to select it; clicks on the status row and legend do not count.
#[test]
fn click_inside_box_rect_hits() {
    let d = points();
    let area = Rect::new(0, 0, 60, 24);
    let p = panels(&d, area, &View::default());
    for i in 0..d.boxes.len() {
        let (x, y) = p
            .proj
            .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
        assert_eq!(
            node_at(&d, &p, area, x as u16, y as u16),
            Some(i),
            "点在框的格子上该选它"
        );
    }
    assert_eq!(
        node_at(&d, &p, area, 5, area.height - 1),
        None,
        "状态行上的点击该忽略"
    );
    // Blank space outside every box rectangle hits nothing.
    let (x, y) = p.proj.cell((5.0, 5.0));
    assert_eq!(node_at(&d, &p, area, x as u16, y as u16), None);
    // Blank space right of the map (no legend panel) does not count either.
    let wide = Rect::new(0, 0, 200, 50);
    let pw = panels(&d, wide, &View::default());
    assert_eq!(pw.map.width, wide.width, "地图铺满整行");
    assert_eq!(
        node_at(&d, &pw, wide, 199, wide.height - 2),
        None,
        "空白处的点击该忽略"
    );
}

/// Wheel panning counts screen cells: n steps move the centre n cells.
#[test]
fn pan_steps_by_screen_cells() {
    let d = points();
    let p = panels(&d, Rect::new(0, 0, 60, 24), &View::default());
    let from = (5.0, 3.0);
    let (c0, r0) = p.proj.cell(from);
    let to = pan_center(&p.proj, from, 3, -2);
    assert_eq!(p.proj.cell(to), (c0 + 3, r0 - 2));
}

/// A nearly empty window still renders without panic and nothing overflows.
#[test]
fn tiny_area_does_not_panic() {
    let d = points();
    for (w, h) in [
        (0u16, 0u16),
        (1, 1),
        (1, 5),
        (5, 1),
        (3, 3),
        (8, 4),
        (12, 3),
    ] {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &View::default());
        let mut view = View::default();
        view.query = Some("甲".to_string());
        view.hits = vec![0];
        render(&mut buf, area, &d, &view);
        let mut view = View::default();
        view.help = true;
        render(&mut buf, area, &d, &view);
        let p = panels(&d, area, &View::default());
        let hit = node_at(&d, &p, area, 0, 0);
        if w == 0 || h <= 1 {
            assert_eq!(hit, None, "{w}x{h} 上没有可点的框");
        }
    }
}

/// Fit to area: the computed zoom makes the whole document fit.
#[test]
fn fit_zoom_fits_whole_document() {
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (100, 40), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (100, 41), "b")],
        vec![],
    );
    let zoom = zoom_for_fit(&d, 60, 24, true);
    assert!(zoom > 0.0 && zoom.is_finite());
    // Smaller zoom shows more; a bigger area allows a bigger zoom.
    assert!(zoom_for_fit(&d, 200, 60, true) > zoom);
}

/// Manual via points are cells: the route passes through them and leaves the straight line.
#[test]
fn manual_via_changes_route() {
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (20, 0), (1, 1), BoxStyle::Point),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 1), "b")],
        vec![{
            let mut e = edg("e1", "a", "b", Dir::None, "g0");
            e.via = vec![(10, 10)];
            e
        }],
    );
    let area = Rect::new(0, 0, 60, 30);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let v = p.proj.cell((10.0, 10.0));
    assert_ne!(
        buf[(v.0 as u16, v.1 as u16)].symbol(),
        " ",
        "线段该经过折点"
    );
    // The via is off the straight line: strokes appear below it, so the route detours.
    let direct = p.proj.cell((10.0, 0.0));
    let mut detour = false;
    for y in (direct.1 + 1)..(p.map.y + p.map.height) as i32 {
        for x in p.map.x as i32..(p.map.x + p.map.width) as i32 {
            let sym = buf[(x as u16, y as u16)].symbol();
            if matches!(sym, "─" | "│" | "╭" | "╮" | "╰" | "╯") {
                detour = true;
                break;
            }
        }
        if detour {
            break;
        }
    }
    assert!(detour, "折点那一段该离开直线");
}

/// `**bold**` markup in a label adds no extra width.
#[test]
fn label_markup_has_no_width() {
    assert_eq!(width("**粗**"), width("粗"));
    assert_eq!(width("*斜*"), width("斜"));
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (4, 3), BoxStyle::Single)],
        vec![txt("ta", "**ab**", (1, 1), "a")],
        vec![],
    );
    let area = Rect::new(0, 0, 40, 12);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (x, y) = p.proj.cell((0.0, 0.0));
    assert_eq!(buf[(x as u16, y as u16)].symbol(), "┌");
    let row: String = (x..x + 4)
        .map(|dx| buf[(dx as u16, (y + 1) as u16)].symbol().to_string())
        .collect();
    assert!(row.contains("ab"), "标记不算宽度:{row}");
}

/// Both directions in one group: two separate lines, each head pointing at its target.
#[test]
fn bidirectional_edges_split_into_two_lines() {
    let d = doc_of(
        vec![grp("g0", "依赖", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![
            bx("u", (0, 1), (1, 1), BoxStyle::Point),
            bx("v", (10, 1), (1, 1), BoxStyle::Point),
        ],
        vec![txt("tu", "u", (0, 0), "u"), txt("tv", "v", (10, 0), "v")],
        vec![
            edg("e1", "u", "v", Dir::Forward, "g0"),
            edg("e2", "v", "u", Dir::Forward, "g0"),
        ],
    );
    assert_eq!(d.edges.len(), 2, "两个方向是两条独立的边");
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &View::default());
    let (ux, y) = p.proj.cell((0.0, 1.0));
    let (vx, _) = p.proj.cell((10.0, 1.0));
    assert_eq!(buf[((vx - 1) as u16, y as u16)].symbol(), ">", "去程指向东");
    // The return run uses the upper parallel line and enters u from above.
    assert_eq!(
        buf[(ux as u16, (y - 1) as u16)].symbol(),
        "v",
        "回程从上面指向 u"
    );
    // The two edges never share a cell: the outbound row is a line right of u.
    assert_eq!(buf[((ux + 1) as u16, y as u16)].symbol(), "─");
}

/// A crossing away from nodes keeps the first group's own shape, the through stroke,
/// and the other group breaks for one cell: never a plus glyph, which suggests a junction.
/// The shape does not follow the cursor: moving the cursor only changes colour.
#[test]
fn crossing_cell_keeps_first_line_shape() {
    let d = doc_of(
        vec![
            grp("gx", "东西", (255, 0, 0)),
            grp("gy", "南北", (0, 255, 0)),
        ],
        vec![lyr("", 0)],
        vec![
            bx("a1", (0, 5), (1, 1), BoxStyle::Point),
            bx("a2", (20, 5), (1, 1), BoxStyle::Point),
            bx("b1", (10, 0), (1, 1), BoxStyle::Point),
            bx("b2", (10, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("t1", "1", (0, 6), "a1"),
            txt("t2", "2", (20, 6), "a2"),
            txt("t3", "3", (11, 0), "b1"),
            txt("t4", "4", (11, 10), "b2"),
        ],
        vec![
            edg("ex", "a1", "a2", Dir::None, "gx"),
            edg("ey", "b1", "b2", Dir::None, "gy"),
        ],
    );
    let area = Rect::new(0, 0, 60, 24);
    let cross = |cursor: usize| {
        let mut view = View::default();
        set_at(&d, &mut view, cursor);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((10.0, 5.0));
        let cell = &buf[(x as u16, y as u16)];
        (cell.symbol().to_string(), cell.fg)
    };
    let (s0, c0) = cross(0);
    let (s1, c1) = cross(2);
    assert_eq!(s0, "─", "交叉格该是先画那一组的直笔画");
    assert_eq!(s0, s1, "挪光标不该改交叉格的字形");
    // Colour follows the visible shape: with the cursor on the north-south group
    // this cell must not turn green, which would read as a break in the red line.
    assert_eq!(c0, Color::Rgb(255, 0, 0), "横向那一格是红的");
    assert_eq!(c1, Color::Rgb(255, 0, 0), "光标换组也不改这一格的颜色");
}

/// The cursor node is solid and lerps between bright white and red by `view.pulse`.
#[test]
fn cursor_node_solid_and_pulses_red() {
    let d = points();
    let area = Rect::new(0, 0, 60, 24);
    let p = panels(&d, area, &View::default());
    let (x, y) = p.proj.cell((0.0, 0.0));
    let cell_of = |pulse: f64| {
        let mut view = View::default();
        view.pulse = pulse;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let cell = &buf[(x as u16, y as u16)];
        (cell.fg, cell.symbol().to_string())
    };
    let (soft, ch) = cell_of(0.0);
    assert_eq!(ch, "●", "普通节点该画成实心");
    assert_eq!(soft, Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2));
    let (red, _) = cell_of(1.0);
    assert_eq!(red, Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2));
    let chan = |c: Color| match c {
        Color::Rgb(r, _, _) => r as i32,
        _ => -1,
    };
    let (half, _) = cell_of(0.5);
    assert!(
        chan(half) > chan(soft) && chan(half) < chan(red),
        "中间的档位该在两头之间"
    );
}

/// A point label goes only on a free cell: the node's own cell keeps its glyph.
#[test]
fn point_label_does_not_cover_node() {
    let d = doc_of(
        vec![grp("g0", "网", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (20, 0), (1, 1), BoxStyle::Point),
            bx("c", (0, 10), (1, 1), BoxStyle::Point),
            bx("d", (20, 10), (1, 1), BoxStyle::Point),
        ],
        vec![
            txt("ta", "很长的名字甲", (0, 0), "a"),
            txt("tb", "很长的名字乙", (20, 0), "b"),
            txt("tc", "很长的名字丙", (0, 10), "c"),
            txt("td", "很长的名字丁", (20, 10), "d"),
        ],
        vec![],
    );
    let area = Rect::new(0, 0, 60, 20);
    let mut view = View::default();
    view.node_labels = true;
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    for (i, b) in d.boxes.iter().enumerate() {
        let (x, y) = p.proj.cell((b.at.0 as f64, b.at.1 as f64));
        let sym = buf[(x as u16, y as u16)].symbol();
        assert!(
            matches!(sym, "○" | "◎" | "●" | "◉"),
            "节点 {i} 那一格被标成了 {sym:?}"
        );
    }
    let text = screen(&buf).join("\n");
    assert!(
        text.contains("很长的名字"),
        "地方够宽,总得标出名字来:\n{text}"
    );
}

/// After fitting the whole document, every box is inside the map area.
#[test]
fn fit_shows_every_box() {
    let d = doc_of(
        vec![grp("g0", "线", (120, 200, 255))],
        vec![lyr("", 0)],
        vec![
            bx("a", (0, 0), (1, 1), BoxStyle::Point),
            bx("b", (100, 40), (4, 3), BoxStyle::Single),
        ],
        vec![txt("ta", "a", (0, 1), "a"), txt("tb", "bb", (101, 41), "b")],
        vec![edg("e1", "a", "b", Dir::Forward, "g0")],
    );
    let area = Rect::new(0, 0, 60, 24);
    let mut view = View::default();
    view.zoom = zoom_for_fit(&d, area.width, area.height, view.edge_labels);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    for (i, b) in d.boxes.iter().enumerate() {
        let (x, y) = p.proj.cell((b.at.0 as f64, b.at.1 as f64));
        assert!(
            x >= p.map.x as i32
                && x < (p.map.x + p.map.width) as i32
                && y >= p.map.y as i32
                && y < (p.map.y + p.map.height) as i32,
            "框 {i} 出了画面:({x}, {y}) / {:?}",
            p.map
        );
    }
}

/// Point edges must not ride each other: 南宋->元 must not run east along 北宋->南宋
/// and merge into one line; with room it detours to a free row or column.
#[test]
fn point_edges_avoid_riding_other_edges() {
    let Ok(text) = std::fs::read_to_string("examples/fun/dynasties.gtui") else {
        return;
    };
    let d = crate::document::Document::load(&text).unwrap();
    let area = Rect::new(0, 0, 102, 98);
    let view = View::default();
    let mut buf = Buffer::empty(area);
    let (_m, hit) = render_map_hit(&mut buf, area, &d, &view, 1.0);
    let a = d.edges.iter().position(|e| e.id == "e50").unwrap(); // 北宋 -> 南宋
    let b = d.edges.iter().position(|e| e.id == "e57").unwrap(); // 南宋 -> 元
                                                                 // The end cells are legitimately shared; count only cells that overlap mid-route.
    let proj = proj_of(&d, &view, 1.0).anchored(
        area.x as i32 + area.width as i32 / 2,
        area.y as i32 + area.height as i32 / 2,
    );
    let nodes: Vec<(i32, i32)> = ["beisong", "nansong", "yuan"]
        .iter()
        .filter_map(|id| d.boxes.iter().position(|b| b.id == *id))
        .map(|i| {
            let (x, y, _, _) = rect_of(&d, i, &proj);
            (x, y)
        })
        .collect();
    let shared: Vec<(i32, i32)> = hit
        .candidates
        .iter()
        .filter(|(cell, c)| c.contains(&a) && c.contains(&b) && !nodes.contains(cell))
        .map(|(cell, _)| *cell)
        .collect();
    assert!(shared.is_empty(), "两条边压在同一批格子上: {shared:?}");
}

// Temporary probe: render each label shape and print which cells carry a search fill.
