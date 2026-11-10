use super::*;
use crate::document::{DocEdge, Group, Layer, Text, ViewState};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

/// 渲染一帧(只用于读底部两行)
fn buf_status(d: &Document, view: &View) -> Buffer {
    let area = Rect::new(0, 0, 200, 50);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, d, view);
    buf
}

/// 状态下那一行(两行底部时是倒数第二行;矮终端就是最后一行)
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

/// 提示太长时从左边截,留住文件名;放不下就干脆不画
#[test]
fn 提示左侧截断() {
    assert_eq!(fit_tail("abc", 5).as_deref(), Some("abc"));
    assert_eq!(fit_tail("abc", 3).as_deref(), Some("abc"));
    // 截断:保留尾部,前面加 "..."
    assert_eq!(
        fit_tail("/very/long/path.gtui", 10).as_deref(),
        Some("...th.gtui")
    );
    // 宽度按显示列算(CJK 占两列)
    assert_eq!(fit_tail("中文文件名", 7).as_deref(), Some("...件名"));
    // 连 "..." 都放不下就不画
    assert_eq!(fit_tail("abcdef", 3), None);
    assert_eq!(fit_tail("abcdef", 0), None);
}

/// 把光标放到第 i 个框的文字起点(渲染测试用)
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

/// 一个带框的节点 + 一个点节点,一条带折点的有向边
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

/// 三个点节点排成 L:甲的乙一根线,丙在下面
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

/// 把画面读成文本:全角字占的第二格要跨过去
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

/// 框直接画在 at 上:左上角那一格就是左上角字形,不是从中心推出来的
#[test]
fn 框画在左上角_at() {
    let d = sample();
    let area = Rect::new(0, 0, 80, 30);
    let mut view = View::default();
    set_at(&d, &mut view, 1); // 光标挪到点节点上,好看清框本身
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    let (x, y) = p.proj.cell((2.0, 2.0));
    let at = |dx: i32, dy: i32| buf[((x + dx) as u16, (y + dy) as u16)].symbol().to_string();
    assert_eq!(at(0, 0), "╭", "左上角就是 at");
    assert_eq!(at(8, 0), "╮");
    assert_eq!(at(0, 2), "╰");
    assert_eq!(at(8, 2), "╯");
    // 文字画在自己的 at 上(框内左上角 + (1, 1))
    let (tx, ty) = p.proj.cell((3.0, 3.0));
    assert_eq!((tx, ty), (x + 1, y + 1));
    assert_eq!(
        buf[(tx as u16, ty as u16)].symbol(),
        "中",
        "文字该在它自己的 at 上"
    );
    // 如果按中心推,框会被挪到 (6, 3) 那儿去
    let (cx, cy) = p.proj.cell((6.0, 3.0));
    assert_ne!((cx, cy), (x, y));
}

/// 点节点只占一格,画 ○;经过两个分组的画 ◎
#[test]
fn 点节点一格并画圆() {
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
    // 只占一格:左右相邻格不是节点字形
    for dx in [-1i32, 1] {
        let sym = buf[((x + dx) as u16, y as u16)].symbol();
        assert!(!matches!(sym, "○" | "◎" | "●" | "◉"), "点节点只占一格");
    }
}

/// 有向边:箭头按 dir 画;目标带框时落在框外紧挨着的那一格
/// 每条边自己的箭头字形:同一条图里可以一条 ▲▼◀▶、一条 ↑↓←→
#[test]
fn 每条边自己的箭头字形() {
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
    // 没写 head 的边走全局设置
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
    // 全局换成三角时,没写 head 的边跟着变
    let mut view = View::default();
    view.arrow_style = crate::model::ArrowStyle::Triangles;
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &plain, &view);
    let (ex, ey) = p.proj.cell((10.0, 1.0));
    assert_eq!(buf[((ex - 1) as u16, ey as u16)].symbol(), "▶");
}

#[test]
fn 箭头按dir落在边框上() {
    let area = Rect::new(0, 0, 60, 20);
    // 点节点目标:箭头在终点前一格
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
    // 目标带框:箭头嵌在左边界那一格
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
    // 无向边不画箭头
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

/// 连边模式:源框稳定高亮,目标框呼吸,状态栏报数量,预览线不落 T/箭头
#[test]
fn 连边模式的三种标记() {
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
    // 源框:左上角是稳定高亮色
    let src = &buf[at(2, 3)];
    assert_eq!(src.symbol(), "┌");
    assert_eq!(src.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
    // 目标框:同一个格子是呼吸色(和源不同)
    let dst = &buf[at(2, 12)];
    assert_eq!(dst.symbol(), "┌");
    assert_ne!(dst.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
    // 预览线:暗蓝灰,而且目标边框没被 T/箭头改掉
    let mut preview = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if buf[(x, y)].fg == Color::Rgb(PREVIEW.0, PREVIEW.1, PREVIEW.2) {
                preview += 1;
            }
        }
    }
    assert!(preview > 0, "该画出预览线");
    // 状态行报数量(两行底部时它在倒数第二行)
    // 宽字会占两格,第二条是空的:去掉空白再比
    let row: String = status_line(&buf)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(row.contains("连边:源1->目标1"), "状态栏该有源和目标:{row}");
    assert!(row.contains("将建1条"), "状态栏该报数量:{row}");
}

/// 预览最多画 64 条:再多只报数量
#[test]
fn 连边预览最多六十四条() {
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

/// none:不画框,只有文字;边照样停在这块区域外,箭头落在外面那一格
#[test]
fn none_节点只有文字() {
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
    // 文字就在区域左上角,没有边框(宽字占两格,第二格是空的)
    assert_eq!(at(10, 3), "文");
    assert_eq!(at(12, 3), "字");
    // 边从左边进来:箭头落在区域外面那一格
    assert_eq!(at(9, 3), ">", "箭头该在区域外");
    // 区域本身不该出现框线
    for x in 10..14 {
        let ch = at(x, 3);
        assert!(
            !"┌┐└┘├┤┬┴┼─│╭╮╯╰".contains(&ch),
            "({x},3) 出现了框线 {ch:?}"
        );
    }
}

/// 文字底色与边底色各自独立,互不覆盖
#[test]
fn 文字底色与边底色互不影响() {
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
    // 文字:自己的底色
    assert_eq!(at(10, 3).symbol(), "文");
    assert_eq!(at(10, 3).bg, Color::Rgb(0x33, 0x44, 0x55), "文字底色");
    // 边经过的格子:边的底色
    let (ex, ey) = (9, 3);
    assert_eq!(at(ex, ey).bg, Color::Rgb(0x55, 0x22, 0x11), "边底色");
    // 框里:填充色不变,没被边的底色串到
    assert_eq!(at(1, 4).bg, Color::Rgb(0x11, 0x11, 0x11), "框的填充");
}

/// 端口走线:奇数宽上下对齐,0 折,T 与箭头同列
/// 算一遍 PortPlan 里某个端点的格子(测试用)
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

/// 换框型不改变端口:锚点是从文字内容算的,框只换字形
#[test]
fn 换框型不改变端口() {
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

/// 没有框的文字节点:多条边在够宽时各占一个虚拟格,箭头落在文字外一格
#[test]
fn 文字节点的端口各占一格() {
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

/// 三条边从同一侧进来、框够宽:各占一个格
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

/// 加一条边不挪动已有端口:分配只按数组顺序往下走
#[test]
fn 加边不重排端口() {
    let two = three_arrow_doc(false);
    let three = three_arrow_doc(true);
    let before: Vec<_> = (0..3).map(|e| plan_cell(&two, 4, true, e)).collect();
    let after: Vec<_> = (0..3).map(|e| plan_cell(&three, 4, true, e)).collect();
    assert_eq!(
        before, after,
        "加边把已有端口挤走了:{before:?} -> {after:?}"
    );
}

/// 删掉最后一条边:剩下的端口一个都不动
#[test]
fn 删边不重排端口() {
    let two = three_arrow_doc(false);
    let three = three_arrow_doc(true);
    // 删掉 e4 就是回到只有三条边的文档
    let before: Vec<_> = (0..3).map(|e| plan_cell(&three, 4, true, e)).collect();
    let after: Vec<_> = (0..3).map(|e| plan_cell(&two, 4, true, e)).collect();
    assert_eq!(
        before, after,
        "删边把别的端口挤走了:{before:?} -> {after:?}"
    );
}

/// 同一份文档跑两次,端口表逐格相同
#[test]
fn 端口分配是确定的() {
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

/// 一侧的格挤满时:箭头换到别的侧,线跟最近的线 T 合并
#[test]
fn 端口挤满时箭头换侧线合并() {
    // 目标框 4 格宽:上边只有 2 个内格,却要接 3 条边
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
    // 三支箭头:上边两个格用完后,第三条换到别的侧
    let arrows = plan_of(&make(Dir::Forward));
    let sides: Vec<u8> = (0..3)
        .map(|e| arrows.get(3, true, e).expect("有端口").0)
        .collect();
    assert!(sides.iter().any(|&s| s != UP), "一侧挤满该换侧:{sides:?}");
    // 三条普通连接:方向性弱,超容量时先挪走一条,其余留在上边
    let lines = plan_of(&make(Dir::None));
    let got: Vec<(u8, (i32, i32))> = (0..3)
        .map(|e| lines.get(3, false, e).expect("有端口"))
        .collect();
    let stay = got.iter().filter(|(s, _)| *s == UP).count();
    assert_eq!(stay, 2, "上边两个格,应该留两条、挪走一条:{got:?}");
}

/// 同一侧三条边:框够宽时每条各占一个格
#[test]
fn 三条边各占一格() {
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
    // 三个箭头各占一格:上边这一行能数出三个 v
    let arrows = (r.0..r.0 + r.2)
        .filter(|&x| buf[(x as u16, r.1 as u16)].symbol() == "v")
        .count();
    assert_eq!(arrows, 3, "三个箭头该各占一格");
}

/// 同一侧一条进(箭头)一条出(普通连接):各占一格,箭头不被 T 顶掉
#[test]
fn 箭头和普通连接各占一格() {
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
    // 同侧一组一个口:再放一条进线,和前面那条共用
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

/// 显式端口仍然生效:两条边都钉在同一边时按"每条自己一格"
#[test]
fn 显式端口按两条规则() {
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

/// 端口落在"文字内容"的正中    /// 端口落在"文字内容"的正中:单字框哪怕宽 5,口也在字下面那一格
#[test]
fn 端口走线_奇数宽对齐() {
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
    // 文字在 (1,4):下边最近的格是 (1,5),不是框的正中列
    assert_eq!(at(1, 5), "┬", "起点 T");
    assert_eq!(at(1, 9), "v", "终点箭头");
    for y in 6..9 {
        assert_eq!(at(1, y), "│", "第 {y} 行该是直线");
    }
    assert_eq!(at(2, 5), "─", "底边其余是边框");
}

/// 端口走线:偶数宽上下差一格,2 折,jog 贴着目标端
/// 面对面但内容列错开:两端对齐到同一列,中间一笔直下,不再多一个 jog
#[test]
fn 端口走线_面对面错开也对齐() {
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
    // 出口:上面那个框底边上的 T;入口:下面那个框顶边的箭头,必须同一列
    let port = (1..5).find(|&x| at(x, 5) == "┬").expect("出口有个 T");
    let arrow = (1..5).find(|&x| at(x, 12) == "v").expect("目标那边有箭头");
    assert_eq!(port, arrow, "两端该对齐到同一列");
    for y in 6..12 {
        assert_eq!(at(port, y), "│", "第 {y} 行该是直的");
    }
    assert_eq!(at(port + 1, 9), " ", "旁边不该有线");
}

/// 两头都按内容中心取口:两个字的列不一样时,长直线最后错一格接上去
#[test]
fn 端口走线_水平错格() {
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
    // a 右边取 top+h/2 = 3;b 左边取 top+(h-1)/2 = 6
    assert_eq!(at(7, 3), "├", "起点 T");
    assert_eq!(at(20, 6), ">", "箭头嵌在左边框");
    assert_eq!(at(18, 3), "╮", "竖直 jog 的起点");
    assert_eq!(at(18, 6), "╰", "竖直 jog 的终点");
    for x in 8..18 {
        assert_eq!(at(x, 3), "─", "第 {x} 列走在源行");
    }
}

/// 端口走线:左右互换,选侧跟着翻转
#[test]
fn 端口走线_左右互换() {
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

/// 端口走线:上下互换,选侧翻到上边/下边
/// 上下互换:两端都落在各自文字那一列
#[test]
fn 端口走线_上下互换() {
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

/// 手工折点:首段沿源法线出去,横着到 via 那一列,末段竖着进目标
#[test]
fn 端口走线_via() {
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
    // 源端口在框底下,先笔直往下到 via 那一行
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

/// 有向边:起点边框画 T,终点把箭头嵌进边框
#[test]
fn 边框上的连接点与嵌入箭头() {
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

/// 粗线框的 T 用粗线字形
#[test]
fn 粗线框的连接点用粗线() {
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

/// 颜色:连接点跟框的边框色,嵌入箭头跟边的颜色;两者都保留框的填充底色
#[test]
fn 连接点与箭头的颜色和填充() {
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

/// 同侧放不下第二个箭头时直接跳过,不报错也不画到别处
#[test]
fn 没空位就不画箭头() {
    let area = Rect::new(0, 0, 60, 20);
    // 框高固定 3,左右两侧各只有一格:第三个箭头无处可放
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
    // 左边只有中间一行能放箭头,其余两个跳过;箭头数不超过格子数
    let mut n = 0;
    for y in 5..8 {
        let (sx, sy) = p.proj.cell((12.0, y as f64));
        if buf[(sx as u16, sy as u16)].symbol() == ">" {
            n += 1;
        }
    }
    assert!(n <= 1, "左边只有一格,画了 {n} 个箭头");
}

/// 框被推到画面外:T 在画面外不画,可见的那一侧照常
#[test]
fn 框出画面不崩() {
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
    // 把视图固定在 a 上:b 落在画面外,它的箭头格不该被画
    let view = View {
        center: Some((2.0, 3.5)),
        ..Default::default()
    };
    render(&mut buf, area, &d, &view);
    let t = (0..area.height).any(|y| (0..area.width).any(|x| buf[(x, y)].symbol() == "├"));
    assert!(t, "可见那侧照常画 T");
}

/// 单字框:候选按边框容量算(6 格宽的上边有 4 个格),两个箭头各占一格,
/// 不再因为"内容只有一个字"就挤成一个口
#[test]
fn 单字框的两个箭头各占一格() {
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
/// 同侧多个箭头:首选格被占就找最近的空格,不再只画一个
#[test]
fn 同侧多个箭头各占一格() {
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
    // b 的上边:13..16,离内容中心 (14,5) 最近的是 14、13
    let mut arrows = Vec::new();
    for x in 13..17 {
        let (sx, sy) = p.proj.cell((x as f64, 4.0));
        if buf[(sx as u16, sy as u16)].symbol() == "v" {
            arrows.push(x);
        }
    }
    assert_eq!(arrows.len(), 2, "两个箭头该各占一格: {arrows:?}");
    // 每条箭头都对齐自己的目标那一列:12 是角格,退到 13;另一个正好 16
    assert_eq!(arrows, vec![13, 16], "箭头该对齐各自的目标列:{arrows:?}");
}

/// via 折点:箭头按最后一段的方向落在对应的边上
#[test]
fn via_决定进入侧() {
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
    // 最后一段从 (6,8) 横着进 b 的左边
    let (lx, my) = p.proj.cell((10.0, 9.0));
    assert_eq!(buf[(lx as u16, my as u16)].symbol(), ">", "从左边进来");
}

/// 双线框的 T 用双线字形
#[test]
fn 双线框的连接点用双线() {
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

/// 取格按宽高奇偶:宽 4 的框上边取右中列
/// 连接格按内容中心对齐:宽 4 的框、两个字的标签,口在 (11,6)
#[test]
fn 连接格按内容中心() {
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

/// 方向四态:0/1/1/2 个箭头,位置分别在 from 端、to 端
#[test]
fn 四态方向箭头() {
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

/// 框内填充:整个矩形每一格都有背景,框外没有;点节点忽略 fill
#[test]
fn 框内填充铺满矩形() {
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
    // 文字格背景也在,字画在背景上面
    let cell = &buf[((x + 1) as u16, (y + 1) as u16)];
    assert_eq!(cell.symbol(), "a");
    assert_eq!(cell.bg, fill);
    // 点节点没有框,不铺填充
    let (px, py) = p.proj.cell((20.0, 8.0));
    assert_ne!(buf[(px as u16, py as u16)].bg, fill, "点节点忽略 fill");
}

/// 光标框只换前景(呼吸色),填充不能被冲掉
#[test]
fn 光标框保留填充() {
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

/// 宽字的续格也要有框内填充:文字只换前景,不能把底色冲掉
#[test]
fn 宽字续格保留填充() {
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
    // 内区 8 格:中 文 各占两列,续格不能是终端底色
    for dx in 1..9 {
        assert_eq!(
            buf[((x + dx) as u16, (y + 1) as u16)].bg,
            Color::Rgb(1, 2, 3),
            "内区第 {dx} 格该有填充"
        );
    }
}

/// 选中元素用红白呼吸(不铺底色):框闪边框,没框的点节点闪字
#[test]
fn 选中元素红白呼吸() {
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
        view.labels = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let (px, py) = p.proj.cell((20.0, 9.0));
        (
            buf[(x as u16, y as u16)].clone(),
            buf[(px as u16, py as u16)].clone(),
        )
    };
    // 选中框:pulse=0 时白,=1 时红;底色不变(不铺高亮底)
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

    // 选中点节点:闪的是它的名字
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

/// 自环:自己绕节点画一圈,格子记进命中表,点得到
#[test]
fn 自环画得出来也点得到() {
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
        // 至少拐了两下,不是一根直线
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

/// 多行文字:每行落在框内相邻的行上,对齐逐行生效
#[test]
fn 多行文字逐行画() {
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
    // 三行都在框内、行距 1
    let rows: Vec<String> = (0..5)
        .map(|k| screen(&buf)[(y + 1 + k) as usize].clone())
        .collect();
    assert!(rows[0].contains('一'), "{rows:?}");
    assert!(rows[1].contains("二二"), "{rows:?}");
    assert!(rows[2].contains("三三三"), "{rows:?}");
    let _ = x;
}

/// 注释框就是普通 none 框:画在自己坐标上,关掉开关就整块消失(尺寸也变小)
#[test]
fn 注释框随开关显示() {
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
    let b = d.new_note(0).expect("该能建注释框");
    d.set_text(b, "收到 ACK");
    d.place_box(b, (18, 9));
    let area = Rect::new(0, 0, 60, 20);
    let mut view = View::default();
    let mut buf = Buffer::empty(area);
    let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
    let (x, y) = p.proj.cell((18.0, 9.0));
    let text = screen(&buf).join("\n");
    assert!(text.contains("收到 ACK"), "{text}");
    // 注释框和普通框一样能点
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

    // 关掉:不画、也点不到
    view.notes = false;
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

/// E 模式:亮着的边绑的注释一起亮;注释关着时它临时露出来
#[test]
fn 邻居高亮带上相关注释() {
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
    let b = d.new_note(0).expect("该能建注释框");
    d.set_text(b, "收到 ACK");
    let area = Rect::new(0, 0, 60, 30);
    let mut view = View::default();
    view.notes = false;
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

/// 命中优先级:框 > 边 > 空
#[test]
fn 命中优先级() {
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
    // 框:画布格与屏幕格各算一份
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
    // 边:命中表记的是屏幕格,画布格随便给一个不在框里的
    let s = screen((11, 6));
    let got = pick_at(&d, &hit, (11, 6), s, &view);
    assert!(matches!(got, Pick::Edge(_)), "中间那格命中边:{got:?}");
    let s2 = screen((11, 14));
    assert_eq!(pick_at(&d, &hit, (11, 14), s2, &view), Pick::None);
}

/// 文字里的链接:命中表覆盖显示文字每一格(宽字两格),目标不占宽度
#[test]
fn 链接命中表() {
    let base = sample();
    let area = Rect::new(0, 0, 80, 24);
    let v = View::default();
    let mut buf = Buffer::empty(area);
    let (_, hit) = render_hit(&mut buf, area, &base, &v, &FrameHit::default());
    assert!(hit.links.is_empty(), "样例没有链接");

    // 换成带链接的文字:一个全角字 + 一个 ASCII
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

/// 一帧的命中表:边笔画格能查到是哪条边,空白格查不到
#[test]
fn 命中表记得边和它的格子() {
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
    // 两个点之间那一格是这条边画的
    let (x, y) = p.proj.cell((11.0, 5.0));
    assert_eq!(hit.edge_at((x, y)), Some(0), "中间那格该属于 e1");
    // 空白处没有边
    let (bx, by) = p.proj.cell((11.0, 12.0));
    assert_eq!(hit.edge_at((bx, by)), None);
    // 可见格子清单里有这条边,而且不止一格
    assert!(hit.edge_cells.get(&0).map(|v| v.len() > 5).unwrap_or(false));
    // 候选里也有它
    assert!(hit.candidates_at((x, y)).contains(&0));
}

/// 被聚焦过滤压暗的框不画填充,保持终端底色
#[test]
fn 压暗的框不画填充() {
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
    set_at(&d, &mut view, 1); // 光标别落在被压暗的框上,免得高亮盖掉判断
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

/// 边框色和文字色各用各的
#[test]
fn 边框与文字颜色分离() {
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
    // 光标放到空白处:这条测的是"颜色各自独立",不是选中时的呼吸色
    view.cursor = (-5, -5);
    view.pick = Pick::None;
    view.labels = true; // 点节点名平时未必画,这里强制标出来
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

/// 文字水平对齐:left 从左边 1 格,center 居中,right 靠右留 1 格
#[test]
fn 文字按align偏移() {
    let area = Rect::new(0, 0, 80, 30);
    // 框宽 12,可用宽度 10,文字 "abc" 宽 3
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

/// 拐角样式:sharp 画 ┌┐└┘,rounded 画 ╭╮╯╰
#[test]
fn 边拐角按corner画() {
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

/// 虚线:直笔按线型断开,拐角处仍是实画
#[test]
fn 虚线断直笔不断拐角() {
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
    // 拐弯的虚线:拐角必须有字形
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

/// 按图层看时:不在当前层的节点与边压暗,当前层的正常画
#[test]
fn 非当前层压暗() {
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

/// 区块选择画连续的灰边:只改底色,不盖字,整条边不留断口
#[test]
fn 区块选择画连续灰边() {
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
    // 框里的字还在:左上角那格本来就是框的角字
    assert_eq!(buf[(x0 as u16, y0 as u16)].symbol(), "╭");
    // 四条边都是连续的:中间每一格都盖着灰底,不留断口
    for y in y0..=y1 {
        assert!(gray(x0, y) && gray(x1, y), "竖边在 y={y} 处断了");
    }
    for x in x0..=x1 {
        assert!(gray(x, y0) && gray(x, y1), "横边在 x={x} 处断了");
    }
    // 框内不画
    assert!(buf[(x0 as u16 + 1, y0 as u16 + 1)].bg != SEL_GRAY);
}

/// 矩形选择:按格矩形收集 box,选中的框按 pulse 呼吸(不反显)
#[test]
fn 矩形选择按格子收集框() {
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
    view.pulse = 1.0; // 固定到红,便于断言
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

/// 只画图的入口:没有图例、没有状态行,zoom 参数生效
#[test]
fn 只画图不带图例状态行() {
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
    // zoom 翻倍,两个节点在屏幕上离得也翻倍
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

/// 显式端口:左/右/上/下各有自己的出口格;auto 与改动前逐格一致
#[test]
fn 强制端口落在指定边() {
    use crate::model::Side;
    let d = sample();
    let p = panels(&d, Rect::new(0, 0, 80, 30), &View::default());
    let r = rect_of(&d, 0, &p.proj);
    let rs: Vec<(i32, i32, i32, i32)> = (0..d.boxes.len())
        .map(|i| rect_of(&d, i, &p.proj))
        .collect();
    let auto = end_mark(&d, 0, 1, &rs, None).expect("带框节点有端口");
    // auto:两个框上下排,应该是从下面出去
    assert_eq!(auto.side, DOWN, "auto 该按中心差选");
    assert_eq!(auto.cell, Canvas::side_base(r, DOWN));
    // 这个几何里目标是左下:left / down 与方位一致,right / up 是背对着的
    // (背对的钉法按自动走,另有 `钉住的侧指向背后就按自动走` 盯着)
    for (side, bit) in [(Side::Left, LEFT), (Side::Down, DOWN)] {
        let m = end_mark(&d, 0, 1, &rs, Some(side)).expect("端口");
        assert_eq!(m.side, bit, "{} 端口", side.name());
        assert_eq!(m.cell, Canvas::side_base(r, bit));
    }
    for side in [Side::Right, Side::Up] {
        let m = end_mark(&d, 0, 1, &rs, Some(side)).expect("端口");
        assert_eq!(m.side, DOWN, "{} 背对目标,按自动走", side.name());
    }
    // 点节点没有边框:强制端口也不改变(交给中心格)
    let d2 = points();
    let p2 = panels(&d2, Rect::new(0, 0, 80, 30), &View::default());
    let rs2: Vec<(i32, i32, i32, i32)> = (0..d2.boxes.len())
        .map(|i| rect_of(&d2, i, &p2.proj))
        .collect();
    assert!(end_mark(&d2, 0, 1, &rs2, Some(Side::Left)).is_none());
}

/// 十字交叉的两条边:先画的占住交叉格,E 模式亮起来的那条会压上去
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
        // 竖的那条先画:交叉格归它
        vec![
            edg("ev", "v1", "v2", Dir::None, "g0"),
            edg("eh", "h1", "h2", Dir::Forward, "g0"),
        ],
    )
}

/// E 模式亮着的边盖在别的边上面(交叉处不断成一格缺口)
#[test]
fn e模式把边抬到最上层() {
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
    // 光标压在竖边的头上开 E:竖的那条亮起来,交叉格跟着它变成竖笔
    view.pick = Pick::Box(2);
    view.cursor = (8, 2);
    view.ego = Some(2);
    let (buf, s) = draw(&view);
    assert_eq!(buf[s].symbol(), "│", "E 亮起来的边该压上来");
}

/// E 模式的 overlay:高亮边即使在某个格子上被文字盖过,也要画回边字形和边色
#[test]
fn 模式叠加把边画回文字之上() {
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
        notes: HashSet::new(),
    };
    // 模拟"文字后来盖上去":把中间那格写成字
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

/// E 模式叠加层:选中的那条边保持呼吸色,不被普通边色盖掉
#[test]
fn 模式叠加层保留选中边的呼吸色() {
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

/// E 隔离:邻居框上只有相连边那一端的端口还在,别的边的端口根本不画
#[test]
fn 模式只画相连边的端口() {
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
    view.ego = Some(1); // n 是 hub 的邻居
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

/// systemd:E 在某个节点上时,只剩相连边的端口;别人的端口消失(不是压暗)
#[test]
fn 模式邻居框只剩相连的端口() {
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

/// 箭头落在一格上,而另一条边正好从这一格穿过:屏幕上仍然是箭头
#[test]
fn 箭头压在线上面() {
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
        // 竖线先画:横线的箭头落在它身上的那一格
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

/// 带框的节点换成 none:结果和"本来就是 none"完全一致(文字不动,边按文字节点重走)
#[test]
fn 换成none等于原生文字节点() {
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

    // 手写一份"本来就是 none"的文档:位置、尺寸、文字都一样
    let mut native = d.clone();
    native.boxes[0].style = BoxStyle::None;
    native.boxes[0].size = (3, 1);
    native.boxes[0].at = text_at;
    native.texts[0].at = text_at;
    native.rebuild();
    assert_eq!(shot(&converted), shot(&native), "该和原生文字节点画得一样");
}

/// 拖框期间不画选区矩形;松手后照旧
#[test]
fn 拖动时藏起选区矩形() {
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

/// 区域:边框包住成员、标题可见;折叠后成员不画,只剩标题条
#[test]
fn 区域边框与折叠() {
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
    // 折叠:成员框的字消失,区域还在
    let mut folded = d.clone();
    folded.regions[0].collapsed = true;
    folded.rebuild();
    let text = shot(&folded);
    assert!(text.contains("grp"), "折叠后标题还在");
    assert!(!text.contains("b2"), "折叠后成员不画");
    assert_ne!(open, text);
}

/// 低倍率 LOD:文字先消失,再小就标 LOD
#[test]
fn 低倍率不画文字() {
    let d = sample();
    let area = Rect::new(0, 0, 120, 40);
    let text_of = |zoom: f64| {
        let mut view = View::default();
        view.zoom = zoom;
        view.labels = true;
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
    // 宽字符的第二格是空 symbol,按"有没有这个字"判,不按连续子串判
    let count = |text: &str, ch: char| text.matches(ch).count();
    assert!(count(&text_of(1.0), '中') > 0, "1:1 该有文字");
    // 0.4:文字整块不画,但框和边还在
    let mid = text_of(0.4);
    assert_eq!(count(&mid, '中'), 0, "0.4 不该画文字");
    assert!(mid.contains("─") || mid.contains("│"), "0.4 还该有框线");
    // 0.2:状态行标 LOD
    let small = text_of(0.2);
    assert_eq!(count(&small, '中'), 0, "0.2 不该画文字");
    assert!(small.contains("LOD"), "0.2 状态行该标 LOD");
}

/// 放不下的点节点名字要记账:小窗口有、大窗口没有,顺序还得稳定
#[test]
fn 记下没放下的标签() {
    let d = points();
    let shot = |w: u16, h: u16| {
        let area = Rect::new(0, 0, w, h);
        let mut view = View::default();
        view.zoom = zoom_for_fit(&d, w, h, view.notes);
        let mut buf = Buffer::empty(area);
        let (_, hit) = render_picture_hit(&mut buf, area, &d, &view);
        hit.labels_skipped
    };
    // 大窗口:三个点都有名字
    assert!(shot(120, 30).is_empty(), "大窗口不该丢标签");
    // 小窗口:挤不下,至少要报出来,而且两次渲染一致
    let a = shot(4, 3);
    let b = shot(4, 3);
    assert!(!a.is_empty(), "小窗口该报出没放下的标签");
    assert_eq!(a, b, "同尺寸两次渲染要一致");
    for s in &a {
        assert!(!s.text.is_empty() && !s.box_id.is_empty());
    }
}

/// 选中的边经过别人的箭头格时,那格要显示自己的笔画,而不是别人的箭头
#[test]
fn 选中的边压过别人的箭头() {
    let mut e2 = edg("e2", "b", "q", Dir::Forward, "g0");
    e2.via = vec![(12, 6)]; // 从 b 出发先往右,正好踩别人的箭头格
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
        v.labels = false;
        v.pick = pick;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &v);
        let (x, y) = p.proj.cell((11.0, 6.0));
        buf[(x as u16, y as u16)].symbol().to_string()
    };
    assert_eq!(shot(Pick::None), "<", "没选中时那是 e1 的箭头");
    assert_eq!(shot(Pick::Edge(1)), "─", "选中 e2 时该显示自己的笔画");
}

/// 挪视图只是平移:重叠区域里的"笔画"必须逐格相同.
/// 点节点的名字是自动找位摆的(和视口有关),不算笔画,所以只比框线、边和节点字形
/// 文字被画面边缘裁掉时,留在画面里的那部分还要画出来(以前整块跳过 -> 消失)
#[test]
fn 文字被边缘裁掉也留可见部分() {
    let long: String = (0..60).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let doc = doc_of(
        vec![grp("g0", "线", (76, 142, 248))],
        vec![lyr("", 0)],
        vec![bx("a", (0, 0), (60, 1), BoxStyle::None)],
        vec![txt("ta", &long, (0, 0), "a")],
        Vec::new(),
    );
    let area = Rect::new(0, 0, 30, 10);
    // 视图往右挪 40 格:文字前 40 列在画面外,后 20 列要在画面里
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

/// 多行文字:上面几行在画面外时,下面几行照样画
#[test]
fn 多行文字被上边缘裁掉也留可见部分() {
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

/// 还没瞄到目标框时,预览也要画一条虚线到光标(以前 targets 空就直接返回)
#[test]
fn 预览_没有目标框也画一条线() {
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
    // 线是虚线:沿途至少有一个笔画格
    let strokes = (p.map.y..p.map.y + p.map.height)
        .flat_map(|y| (p.map.x..p.map.x + p.map.width).map(move |x| (x, y)))
        .filter(|(x, y)| matches!(buf2[(*x, *y)].symbol(), "─" | "│" | "╭" | "╮" | "╰" | "╯"))
        .count();
    assert!(strokes > 0, "该画出虚线");
    // 没有目标框:不该出现箭头
    assert!(
        !(p.map.y..p.map.y + p.map.height)
            .flat_map(|y| (p.map.x..p.map.x + p.map.width).map(move |x| (x, y)))
            .any(|(x, y)| matches!(buf2[(x, y)].symbol(), ">" | "<" | "^" | "v")),
        "自由预览不画箭头"
    );
}

/// 同组两条边不许长距离共线:后画的那条会整段看不见
#[test]
fn 同组边不长距离共线() {
    // a 在左,两个目标都在右:直着走两条会重叠同一行,必须自己错开
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

/// 命中缓存的画面必须和不带缓存的一次渲染逐格相同
#[test]
fn 缓存命中不改变画面() {
    let d = three_arrow_doc(true);
    let area = Rect::new(0, 0, 80, 30);
    let view = View::default();
    route_cache::clear();
    let first = {
        let mut b = Buffer::empty(area);
        render(&mut b, area, &d, &view);
        b
    };
    // 第二次必然命中
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

/// 文字节点叠在一起时,先用别人的格子不会被宽字的续格空格擦掉
/// (以前后画的那个会把前一个汉字抹成空格,看起来就是"某个字没了")
#[test]
fn 叠在一起的中文名字不会把别人的字擦成空白() {
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
    // 叠到"夫人"两个字上
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
    // 被压住的那两个位置不许变成空格——要么留着原字,要么被新字替掉
    for i in [2, 4] {
        assert_ne!(at(i), " ", "第 {i} 格被擦成空白了");
    }
}

/// 测试用:一个文档所有框在 1:1、左上角对齐时的屏幕矩形
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

/// 钉住的侧指向目标背后时按自动走,免得绕一整圈
#[test]
fn 钉住的侧指向背后就按自动走() {
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
    // 目标在右边:钉 left 是背对着,丢掉;钉 right 方向不矛盾,保留
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
    // 竖直方向:目标在右边时钉 down/up 都不算背对,照旧生效
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), Some(Side::Down)), DOWN);
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), Some(Side::Up)), UP);
}

/// 一行高的文字节点上,自环被别的边挤满端口后也得画出来
/// (tcp.gtui 的 ESTABLISHED 就是这样:端口全被占,环退化成一条直线,看着像没了)
#[test]
fn 端口被占满时自环仍然看得见() {
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
    // 环的顶盖:文字上方某一整行里必须同时有左右两个圆角
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

/// 18 行高、只有一行文字的框,右/左侧的候选格按边框容量算,不按文字内容
#[test]
fn 高框的侧边候选不被内容截断() {
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

/// 目标在上方时,溢出优先走上边,不是固定顺时针的下边
#[test]
fn 溢出按目标方向选侧() {
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
    // hub 的右侧只有 1 个格,8 个端点里的两个"从上面来"的箭头应该走 UP,
    // 至少不该被顺时针逼到 DOWN
    let sides: Vec<u8> = (0..3)
        .map(|e| plan.get(0, true, e).expect("有端口").0)
        .collect();
    assert!(sides.contains(&UP), "上方来的边该从上面进:{sides:?}");
    assert!(
        !sides.iter().all(|&s| s == DOWN),
        "不该全被挤到下面:{sides:?}"
    );
}

/// 瘦侧(容量不到 2)放不下时整组换到容量够的侧
#[test]
fn 瘦侧整组换侧() {
    // 宽 19、高 3 的框:右/左侧只有 1 个格,上/下各有 17 个
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

/// systemd:高 degree 的框不再把箭头挤到一个口、也不从对侧绕
#[test]
fn systemd_高_degree_侧边放得下箭头() {
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
        // 顶边 / 底边上不该再挤出一排箭头
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
        // basic 底下只剩 multi-user -> basic 那一个向上箭头
        assert_eq!(bottom, 1, "{id} 底边该只剩 1 个箭头:{bottom}");
        // 左右两侧该用上:至少一半的边框格有接点/箭头
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

/// 普通侧超容量时,先挪走普通连接,16 个箭头留在原地
#[test]
fn 普通侧超容量时先挪普通连接() {
    let mut boxes = vec![bx("hub", (30, 20), (12, 18), BoxStyle::Single)];
    let mut texts = vec![txt("th", "hub", (31, 28), "hub")];
    let mut edges = Vec::new();
    // 16 条从右边过来的箭头
    for k in 0..16 {
        let id = format!("s{k}");
        boxes.push(bx(&id, (50, 21 + k), (6, 1), BoxStyle::None));
        texts.push(txt(&format!("t{k}"), "服", (50, 21 + k), &id));
        edges.push(edg(&format!("e{k}"), &id, "hub", Dir::Forward, "g0"));
    }
    // 一条往上拐的普通连接,第 17 个右侧端点
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

/// 一行 `none` 节点不整组换侧:左右各一格,多条普通连接共用是正常的
#[test]
fn 一行文字节点不整组换侧() {
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

/// 显式钉住的端口不参与超容量挪动
#[test]
fn 显式端口不参与挪动() {
    let mut boxes = vec![bx("hub", (30, 20), (12, 18), BoxStyle::Single)];
    let mut texts = vec![txt("th", "hub", (31, 28), "hub")];
    let mut edges = Vec::new();
    for k in 0..18 {
        let id = format!("s{k}");
        boxes.push(bx(&id, (50, 21 + k.min(15)), (6, 1), BoxStyle::None));
        texts.push(txt(&format!("t{k}"), "服", (50, 21 + k.min(15)), &id));
        let mut e = edg(&format!("e{k}"), &id, "hub", Dir::Forward, "g0");
        if k == 17 {
            // 最后一条钉死在右边(右侧容量 16)
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

/// 短边判定按两端框的尺寸比例,不看图规模,也不用固定格数
#[test]
fn 短边判定按比例() {
    // (0,0,40,3) 与 (18,5,4,2):中心相距 4.5,框最大边 40 -> 短
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

/// 相对短的边:两端都不认死侧时,侧对要能直着走
#[test]
fn 短边选能直着走的侧对() {
    // 大框正上方偏右一点放一个小框:边短、横向也重叠
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

/// 显式端口不参与短边选侧
#[test]
fn 显式端口不参与短边选侧() {
    // 并排:小框在右边,钉 left(朝大框)/ 大框钉 right,都不背对目标
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

/// 端口外面一格不能是别的框:否则线和箭头会被那个框盖住(箭头看着就没了)
#[test]
fn 端口外面不会是别的框() {
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
    // 箭头端不能把外侧格落在别的框里(箭头会直接盖在别人的文字上).
    // 线端在窄侧实在没地方时允许埋在下面一格:只丢一个线头,见任务 33
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

/// 只有一行文字的目标(dbus.service)左右各只有一个箭头位,指进来的边却有四条:
/// 没有空格时同侧共用箭头格,箭头要画出来,并且算进每一条边的格子里
#[test]
fn 一行文字的目标能共用箭头位() {
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
    // 左右各一个箭头格(框外紧挨着的那一格),两边都得真有箭头
    for cell in [(left - 1, top), (left + w, top)] {
        let arrow = buf[(cell.0 as u16, cell.1 as u16)].symbol().to_string();
        assert!(
            "><^v".contains(&arrow),
            "{cell:?} 应该是箭头,实为 {arrow:?}"
        );
    }
    // 指向 dbus.service 的那几条边都要有端口,而且共享的箭头格算进它们的格子
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

/// 选中共用箭头的那条边时,箭头格要跟着亮(它在 edge_cells 里,overlay 按选中色重画)
#[test]
fn 选中共享箭头边会亮() {
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
    // 四条指进来的边共用左右两个箭头格,任意选中一条都得亮
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

/// 共用的箭头格属于同中心另一条亮边时,overlay 重画线不能把箭头盖成横线
#[test]
fn e_叠加层保留共用箭头() {
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
    // 另一条边的箭头落在这一格,形状仍归 0 号边
    cv.pen_edge = 1;
    cv.outside_arrow((13, 4), 2, (10, 200, 40));
    let ego = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from([0usize, 1]),
        notes: HashSet::new(),
    };
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
    overlay_ego_edges(&mut buf, &cv, &doc, &ego, None, 0.0);
    assert_eq!(buf[(13, 4)].symbol(), "<", "共用箭头要留着");
}

/// E 模式下同中心的其他边也抬起来了,但选中的那条要压过它们:
/// 不然选一条边时,它会被旁边那条亮着的边一段段盖掉,看着像断的
#[test]
fn 选中的边不会被同时亮着的别的边盖住() {
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

/// 一侧只有一格时:线端不能占掉箭头端那一格
/// (占掉之后选这条边就会看到起点也有箭头,像 <->)
#[test]
fn 窄侧线端不占箭头格() {
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
            // b -> a:箭头落在 a 的上侧;c ... 用 a -> c:普通的连接端(也在 a 上)
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
    // 线端要么换一格,要么干脆不在这里规划(交给绘制兜底),但绝不能占箭头格,
    // 也不能从箭头的外侧格起步(外侧格才是箭头真正画出来的地方)
    if let Some((side, cell)) = plan.get(0, false, 1) {
        assert_ne!(cell, arrow_end.1, "线端占了箭头格:{arrow_end:?} {cell:?}");
        assert_ne!(
            Canvas::outside(cell, side),
            Canvas::outside(arrow_end.1, arrow_end.0),
            "线端从箭头的外侧格起步:{arrow_end:?} {cell:?}"
        );
    }
}

/// 一侧只有一格时,两条普通连接可以 T 合并到同一个口上
#[test]
fn 窄侧两个连接端可以合并() {
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

/// 前向边(起点没有箭头)选中后不能看着像 <->:
/// 起点旁边那一格别人的箭头不能跟着这条边亮
#[test]
fn 前向边不出现双向箭头() {
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
    // 端口外侧格:起点就是这一格,正常视图里它不该是别人的箭头
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
    // 框外紧挨着的一圈
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

/// systemd: E 在 fwupd 上时, 指向 dbus.service 的共享箭头用 fwupd 那条边的颜色,
/// 不再是第一条边的颜色
#[test]
fn e_在_fwupd_上时共享箭头用它那条边的颜色() {
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
    // fwupd 那条边自己的颜色:取它一个只属于它的笔画格
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

/// 选中边的起点外侧格以**实际落笔**为准(这一端挤满时 plan 里没有端口):
/// 落在那里的别人的箭头要被压住
#[test]
fn 回退端口的_out_点也能压住外来箭头() {
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
    // 普通模式下这一格是别人进来的箭头
    let mut plain = Buffer::empty(area);
    render_map_hit(&mut plain, area, &d, &View::default(), 1.0);
    let before = plain[(start_out.0 as u16, start_out.1 as u16)]
        .symbol()
        .to_string();
    assert!("<>^v".contains(&before), "起点外侧格本来是箭头: {before:?}");
    // 选中这条边之后,别人的箭头让位
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

/// 共享箭头:颜色跟当前激活的 owner(选中的 > E 里唯一亮着的 > 可见的那条)
#[test]
fn 共享箭头颜色跟随激活的_owner() {
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
        notes: HashSet::new(),
    };
    let run = |cv: &Canvas, ego: &Ego, picked: Option<usize>| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        overlay_ego_edges(&mut buf, cv, &doc, ego, picked, 0.0);
        let c = &buf[(12, 4)];
        (c.symbol().to_string(), c.fg, c.modifier)
    };
    let cv = mk();
    // 1. E 里只亮蓝色那条 -> 蓝色
    let (_, fg, mods) = run(&cv, &ego_of(vec![1]), None);
    assert_eq!(fg, Color::Rgb(0, 0, 255), "箭头该跟亮着的那条边");
    assert!(!mods.contains(Modifier::BOLD), "没选中就不加粗");
    // 2. 两条都亮 -> 保持可见色(红),不随机跳
    let (_, fg, _) = run(&cv, &ego_of(vec![0, 1]), None);
    assert_eq!(fg, Color::Rgb(255, 0, 0), "多个 owner 同时亮时用可见色");
    // 3. 选中蓝色那条 -> pulse + 加粗
    let (_, _, mods) = run(&cv, &ego_of(vec![0, 1]), Some(1));
    assert!(mods.contains(Modifier::BOLD), "选中那条要呼吸");
    // 4. 没有 E、没有选中 -> 取色函数回退到可见色(overlay 自己不画这一格)
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
        notes: HashSet::new(),
    };
    overlay_ego_edges(&mut buf, &cv, &doc, &plain, None, 0.0);
    assert_eq!(
        buf[(12, 4)].fg,
        Color::Reset,
        "E 没亮到的箭头不归 overlay 管"
    );
}

/// 路径中段碰到别人的箭头:overlay 原样留着,不 pulse、不盖字形
#[test]
fn 外来箭头不跟随选中脉动() {
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
    // 1 号边把箭头落在路径中段那一格
    cv.pen_edge = 1;
    cv.outside_arrow((6, 4), 2, (10, 200, 40));
    let ego = Ego {
        center: 0,
        nodes: HashSet::new(),
        edges: HashSet::from([0usize, 1]),
        notes: HashSet::new(),
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

/// 选侧先看投影:很宽的框和它正下方的小框,横向投影重叠就该从上/下走,
/// 只看中心差会挑成左/右(线要横跨一大段再拐下来)
#[test]
fn 横向投影重叠的框从上往下连() {
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
    // 中心差是横向的(dx=10, dy=4),但两个框的横向范围重叠:从上/下走
    assert_eq!(end_side(&d, 0, 1, &rs, None), DOWN, "宽框该从下面出去");
    assert_eq!(end_side(&d, 1, 0, &rs, None), UP, "小框该从上面进来");
    // 并排的两个框照旧走左右
    assert_eq!(end_side(&d, 0, 2, &rs, None), RIGHT);
    assert_eq!(end_side(&d, 2, 0, &rs, None), LEFT);
}

/// 分层布局:跨层的边一律从上/下走,不受"两个中心谁偏左偏右"影响
#[test]
fn 分层布局的边从上往下走() {
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
    // 没算层号时按中心差选侧:一左一右
    assert_eq!(end_side(&d, 0, 1, &rects_of(&d), None), RIGHT);
    crate::layout::apply(&mut d, crate::layout::Layout::Layered).expect("分层");
    // 算过层号:b 在下一层,从 a 的底下出去,到 b 的上面进来
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
    // 显式指定的侧仍然优先
    assert_eq!(
        end_side(&d, 0, 1, &rects_of(&d), Some(crate::model::Side::Left)),
        LEFT,
        "显式端口压过层号"
    );
}

/// 规划那一遍的效果:同一对框之间的两条同组边各走各的,不共线
#[test]
fn 规划把同组两条边分开() {
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

/// 高墙用例:两端都要有线,线不能穿墙(以前会停在墙边)
#[test]
fn 高墙夹在两點之间也连得通() {
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
    // 找 a 和 b 的屏幕格
    let a = p.proj.cell((0.0, 0.0));
    let b = p.proj.cell((40.0, 0.0));
    let stroke = |x: i32, y: i32| {
        matches!(
            buf[(x as u16, y as u16)].symbol(),
            "─" | "│" | "╭" | "╮" | "╰" | "╯" | ">" | "<" | "^" | "v"
        )
    };
    // 两端四邻都要有笔画(线可能从上面翻过去,不一定是横着走)
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
fn 挪视图是纯平移() {
    let Ok(text) = std::fs::read_to_string("examples/fun/dynasties.gtui") else {
        return; // 样例不在(仓库里 *.gtui 是忽略的),这条就跳过
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
    // (可见的画布范围, 画布格 -> 笔画字形)
    type Shot = (
        (i32, i32, i32, i32),
        std::collections::HashMap<(i32, i32), String>,
    );
    let shot = |cy: f64| -> Shot {
        let mut v = View::default();
        v.zoom = 1.0;
        v.labels = false;
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

/// 底部两行(状态 / 输入 / 提示)一律不加粗    /// 底部两行(状态 / 输入 / 提示)一律不加粗    /// 底部两行(状态 / 输入 / 提示)一律不加粗
#[test]
fn 底部两行不加粗() {
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

/// 英文界面下,底部两行不能出现任何 CJK(文档内容除外)
#[test]
fn 英文底部不出现中文() {
    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| {
            matches!(c as u32,
                0x3000..=0x303f | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xff00..=0xffef)
        })
    }
    // 内容也用 ASCII:这条测的是界面文案,不是文档里的名字
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

/// 两行底部:上行文件/模式/cwd/坐标,下行放错误;矮终端退回一行
#[test]
fn 底部两行布局() {
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
    // map 区少两行
    let p = panels(&d, area, &view);
    assert_eq!(p.map.height, area.height - 2, "地图要让出两行");

    // `:` 输入:上行仍是文件,输入在下行
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

    // 矮终端:一行,地图让出一行
    let small = Rect::new(0, 0, 40, 3);
    let mut view = View::default();
    view.note = Some("boom".to_string());
    let mut buf = Buffer::empty(small);
    render(&mut buf, small, &d, &view);
    assert_eq!(status_rows(small), 1);
    assert_eq!(panels(&d, small, &view).map.height, 2);
    assert!(status_line(&buf).contains("boom"));
    // 10 行:仍然是两行
    let ten = Rect::new(0, 0, 40, 10);
    assert_eq!(status_rows(ten), 2);
}

/// 状态行只在 1:1 标 VIEW,缩放后标倍数;空闲时只报当前 box 的文字
#[test]
fn 状态行区分查看与缩放() {
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

/// 缩放只跟 zoom 走:proj.scale = 1 / zoom,窗口大小不改变它
#[test]
fn 缩放不随窗口变() {
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

/// 同一对节点上的两条边:命中表里各有自己的格子,点击轮换也认得出两条
#[test]
fn 互逆两条边各自记格子() {
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
    // 同一格上有两条候选时,`E` 才有得轮换
    let shared = c1.iter().find(|c| c2.contains(c));
    if let Some(c) = shared {
        let list = &hit.candidates[c];
        assert!(
            list.contains(&0) && list.contains(&1),
            "候选该有两条:{list:?}"
        );
    }
}

/// 点节点之间的四条边(四个分组):各占一条平行线,箭头各指各的目标端
#[test]
fn 点节点之间多条边各占一条线() {
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
    // 四条线分别落在四行上
    let rows: Vec<i32> = (-3..=3)
        .map(|dy| ay + dy)
        .filter(|y| (ax + 2..bx - 2).all(|x| buf[(x as u16, *y as u16)].symbol() == "─"))
        .collect();
    assert_eq!(rows.len(), 4, "四条边该占四条不同的行:{rows:?}");
    assert!(rows.contains(&ay), "其中一条走中间那条直线");
    assert_eq!(buf[(mid as u16, ay as u16)].symbol(), "─");
}

/// E 选中一条边:另一头的邻居节点也一起高亮(光标那一头本来就在闪)
#[test]
fn 选中边时邻居节点一起高亮() {
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
    view.cursor = (1, 1); // 光标在 甲 上
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let p = panels(&d, area, &view);
    let border = |i: usize| {
        let (l, t, _, _) = rect_of(&d, i, &p.proj);
        buf[(l as u16, t as u16)].fg
    };
    // 换个呼吸相位再画一次:邻居跟着变,没连着的框不变
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

/// 只有文字的节点:几条边各走各的进出点(不会全挤在同一格)
#[test]
fn 文字节点上下两侧共用同一行() {
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
    // a 在屏幕上的矩形:三条边落在里面的那一格就是各自的端口
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
    // 端口角色唯一:一格要么是箭头端,要么是普通连接端.
    // e1 (a->b) 在 a 这一端带箭头,e2 (a->c) 不带,所以两个端口不能落在同一格
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

/// 就地改字的草稿按显示宽度排:汉字占两格,后面的字不会被右半边吃掉
#[test]
fn 就地改字的草稿不丢宽字() {
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

/// 隔离的中心:注释框按它绑的那条边的源端算
#[test]
fn 隔离在注释框上取边的源端() {
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

/// 隔离时区域整个收起来(它多半框着已经消失的节点)
#[test]
fn 隔离时不画区域() {
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

/// 隔离前后可见边的端口与路线一格不变:规划照旧全量算,只是不落笔
#[test]
fn 隔离前后可见边路线不变() {
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

/// 隐藏的框点不中:那一格按空白处理
#[test]
fn 隐藏的框点不中() {
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

/// 隔离时隐藏节点的点标签不画,可见节点的照常标
#[test]
fn 隔离时不画隐藏节点的标签() {
    let d = points();
    let area = Rect::new(0, 0, 60, 16);
    let shot = |ego: Option<usize>| {
        let mut view = View::default();
        view.ego = ego;
        view.labels = true;
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

/// E 隔离:邻居与相连的边正常画,其余元素直接不画
#[test]
fn 邻居隔离时别的消失() {
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
    view.ego = Some(0); // 只亮 a 和它的邻居 b
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
    // 隐藏框里的字也不画
    let (cx, cy) = p.proj.cell((61.0, 1.0));
    assert_eq!(buf[(cx as u16, cy as u16)].symbol(), " ", "丙 该消失");
    // 和中心无关的边也不画:中间那一整行不该有线
    let mut strokes = 0;
    for x in 0..90u16 {
        let sym = buf[(x, (p.proj.cell((0.0, 1.0)).1) as u16)].symbol();
        if matches!(sym, "─" | "│" | "╭" | "╮" | "╰" | "╯") && x as i32 > p.map.x as i32
        {
            strokes += 1;
        }
    }
    assert!(strokes > 0, "相连的边要画出来");
    // b->c 那条边在 c 前面那段没有画:最右边的单元格子是空的
    let far = p.proj.cell((70.0, 1.0));
    assert_eq!(
        buf[(far.0 as u16, far.1 as u16)].symbol(),
        " ",
        "隐藏的边不画"
    );
}

/// 两端都只有一格(点节点 / 一格的文字节点)时,多条边不能叠成一条:
/// 没有边框格可以顺延,就靠 lane 错开,每条都点得到
#[test]
fn 一格节点上多条边不重叠() {
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
    // 两条线在中间列上各占一行,各点得到
    let lower = hit.edge_at((mid, ay));
    let upper = hit.edge_at((mid, ay - 1));
    assert!(lower.is_some() && upper.is_some(), "上下两行都该有线");
    assert_ne!(lower, upper, "两行不能是同一条边");
}

/// 搜索行与命令行共用底行
#[test]
fn 搜索行报命中数() {
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
    // 纯数字:分组号
    let mut view = View {
        lang: Lang::Zh,
        ..View::default()
    };
    view.query = Some("1".to_string());
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &d, &view);
    let last = last_line(&buf);
    assert!(last.contains("没有这个分组"), "样例没有编号分组:{last}");
    // ":" 命令行独占状态行:从最左边起,盖掉模式标记与节点名
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

/// 帮助浮层:画的是键位速查,不是地图;英文那份不许留中文
#[test]
fn 帮助浮层盖住地图() {
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
    // 浮层边框是制表符(非 ASCII 但不算界面文字),所以只查有没有 CJK
    let cjk = |c: char| {
        matches!(
            c as u32,
            0x3000..=0x303F | 0x4E00..=0x9FFF | 0xFF01..=0xFF5E
        )
    };
    assert!(!text.chars().any(cjk), "英文速查里不该有中文:\n{text}");
}

/// 点哪个框就选哪个;状态行和图例上的点击不算
#[test]
fn 点框矩形内命中() {
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
    // 空白处(不在任何框矩形里)没有命中
    let (x, y) = p.proj.cell((5.0, 5.0));
    assert_eq!(node_at(&d, &p, area, x as u16, y as u16), None);
    // 地图右边的空白(不再有图例面板)同样不算
    let wide = Rect::new(0, 0, 200, 50);
    let pw = panels(&d, wide, &View::default());
    assert_eq!(pw.map.width, wide.width, "地图铺满整行");
    assert_eq!(
        node_at(&d, &pw, wide, 199, wide.height - 2),
        None,
        "空白处的点击该忽略"
    );
}

/// 滚轮平移按屏幕格算:挪几格,中心就落在原来那一格加几格上
#[test]
fn 平移按屏幕格算() {
    let d = points();
    let p = panels(&d, Rect::new(0, 0, 60, 24), &View::default());
    let from = (5.0, 3.0);
    let (c0, r0) = p.proj.cell(from);
    let to = pan_center(&p.proj, from, 3, -2);
    assert_eq!(p.proj.cell(to), (c0 + 3, r0 - 2));
}

/// 窗口小到几乎没有:照画不炸,状态行与浮层都不越界
#[test]
fn 极小画面也不炸() {
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

/// 铺满整个区域:缩放算出来之后整幅文档塞得下
#[test]
fn 铺满时刚好装下整幅文档() {
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
    // zoom 越小看得越全;区域越大一样大小的图能放得更大
    assert!(zoom_for_fit(&d, 200, 60, true) > zoom);
}

/// 手工折点直接是单元格:线段经过折点,并且离开直线
#[test]
fn 手工折点改变走线() {
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
    // 折点在直线之外:直线下面也得有笔画,说明不是直接横穿
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

/// 标签里的 `**粗**` 不占额外宽度
#[test]
fn 标签标记不占宽度() {
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

/// 同一个分组里两个方向都有边:两条各自占一条线,箭头各指各的目标端
#[test]
fn 双向边分成两条线() {
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
    // 回程走上面那条平行线,最后竖着进 u:箭头落在 u 头顶那一格
    assert_eq!(
        buf[(ux as u16, (y - 1) as u16)].symbol(),
        "v",
        "回程从上面指向 u"
    );
    // 两条边不能落在同一格上:去程那一行在 u 右边是横线
    assert_eq!(buf[((ux + 1) as u16, y as u16)].symbol(), "─");
}

/// 非节点处的交叉:交叉格照"先画的那一组"自己的形状画(直通的那一笔),
/// 另一组在这里断一格 —— 不能画成 ┼,不然看起来像这里有枢纽.
/// 这个形状**不跟光标走**:挪光标只换颜色
#[test]
fn 交叉格只画先到那条线的形状() {
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
    // 颜色跟"看得见的那个形状"走:光标在南北组上时,也不能把这一格染成绿的,
    // 否则红线中间会冒出一个绿格,读起来像断线
    assert_eq!(c0, Color::Rgb(255, 0, 0), "横向那一格是红的");
    assert_eq!(c1, Color::Rgb(255, 0, 0), "光标换组也不改这一格的颜色");
}

/// 光标那个节点是实心的,并按 view.pulse 在亮白和红之间插值
#[test]
fn 光标节点实心并按脉冲闪红() {
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

/// 点节点的名标注只写在空位上:标出来之后,节点自己那一格还是节点字形
#[test]
fn 点节点标注不压节点() {
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
    view.labels = true;
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    for i in 0..d.boxes.len() {
        let (x, y) = p
            .proj
            .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
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

/// 铺满整幅文档之后,每个框都在地图区里
#[test]
fn 铺满后每个框都在画面里() {
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
    view.zoom = zoom_for_fit(&d, area.width, area.height, view.notes);
    let mut buf = Buffer::empty(area);
    let p = render(&mut buf, area, &d, &view);
    for i in 0..d.boxes.len() {
        let (x, y) = p
            .proj
            .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
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

/// 点节点之间不该"压着走":南宋->元 原来从南宋沿着 北宋->南宋 那一行往东爬,
/// 两条边叠成一条线。空间够时该绕到空着的列/行上
#[test]
fn 点节点的边不压着别的边走() {
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
                                                                 // 两端的节点格本来就该共用(进出都在那一格),只数线路上重合的格子
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
