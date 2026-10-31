//! 端到端:examples 里的图在每一套布局下都画得出来,而且在足够大的画布上每个框各占一格.
//!
//! 走的是新管线:`.dot` -> Graph -> convert::from_graph -> Document -> layout::apply -> render.

#![allow(clippy::field_reassign_with_default)] // 测试里先造默认 View 再改几项,比堆一长串字面量清楚

use std::collections::HashSet;

use graph_tui::app::View;
use graph_tui::document::{Document, ViewState};
use graph_tui::layout::{self, Layout};
use graph_tui::model::Graph;
use graph_tui::{convert, render};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn load_dot(path: &str) -> Document {
    let text = std::fs::read_to_string(path).unwrap();
    let g = Graph::from_dot(&text).unwrap();
    convert::from_graph(&g, ViewState::default(), "dot").unwrap()
}

fn load_dot_text(text: &str) -> Document {
    let g = Graph::from_dot(text).unwrap();
    convert::from_graph(&g, ViewState::default(), "dot").unwrap()
}

/// 画一帧(开局铺满,和 App 首次绘制一致),返回画面与地图区
fn shot(doc: &Document, w: u16, h: u16) -> (Buffer, Rect) {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    let mut view = View::default();
    view.zoom = render::zoom_for_fit(doc, w, h, view.legend, view.notes);
    let p = render::render(&mut buf, area, doc, &view);
    (buf, p.map)
}

/// 每个节点在画面上"看得见"的位置:点节点看节点字形,带框的看左上角拐角,
/// 只有文字的(none)看首字
fn visible_nodes(doc: &Document, buf: &Buffer) -> Vec<(usize, (i32, i32))> {
    let area = Rect::new(0, 0, buf.area.width, buf.area.height);
    let mut view = View::default();
    // zoom_for_fit 的开关要和下面 panels 用的一致,否则地图比算出来的窄,右边会被切
    view.zoom = render::zoom_for_fit(doc, area.width, area.height, view.legend, view.notes);
    let p = render::panels(doc, area, &view);
    let mut out = Vec::new();
    for (i, b) in doc.boxes.iter().enumerate() {
        let (x, y) = p.proj.cell((b.at.0 as f64, b.at.1 as f64));
        if x < p.map.x as i32
            || x >= (p.map.x + p.map.width) as i32
            || y < p.map.y as i32
            || y >= (p.map.y + p.map.height) as i32
        {
            continue;
        }
        let sym = buf[(x as u16, y as u16)].symbol();
        let ok = if b.style.is_boxed() {
            matches!(sym, "╭" | "┌" | "╔" | "┏")
        } else if b.style.is_point() {
            matches!(sym, "○" | "◎" | "●" | "◉")
        } else {
            match doc.text(i).and_then(|t| t.text.chars().next()) {
                Some(c) => sym.starts_with(c),
                None => false,
            }
        };
        if ok {
            out.push((i, (x, y)));
        }
    }
    out
}

/// 每个节点都画出来了,而且各占一格
fn assert_all_nodes_visible(doc: &Document, buf: &Buffer, what: &str) {
    let seen = visible_nodes(doc, buf);
    assert_eq!(
        seen.len(),
        doc.boxes.len(),
        "{what}:只画出 {} 个节点(共 {} 个)",
        seen.len(),
        doc.boxes.len()
    );
    let cells: HashSet<(i32, i32)> = seen.iter().map(|(_, c)| *c).collect();
    assert_eq!(cells.len(), seen.len(), "{what}:有节点挤在同一格");
}

/// 每个带框的框,它左上角那一格必须是某种拐角字形(边拐弯也用 ╭,所以不能靠数量数)
fn corners(buf: &Buffer, map: Rect, doc: &Document) -> usize {
    let area = Rect::new(0, 0, buf.area.width, buf.area.height);
    let mut view = View::default();
    view.zoom = render::zoom_for_fit(doc, area.width, area.height, view.legend, view.notes);
    let p = render::panels(doc, area, &view);
    let mut n = 0;
    for b in &doc.boxes {
        if !b.style.is_boxed() {
            continue;
        }
        let (x, y) = p.proj.cell((b.at.0 as f64, b.at.1 as f64));
        if x >= map.x as i32
            && x < (map.x + map.width) as i32
            && y >= map.y as i32
            && y < (map.y + map.height) as i32
            && matches!(buf[(x as u16, y as u16)].symbol(), "╭" | "┌" | "╔" | "┏")
        {
            n += 1;
        }
    }
    n
}

#[test]
fn 每套布局每个点节点各占一格() {
    for (path, l) in [
        ("examples/metro.dot", Layout::Layered),
        ("examples/metro.dot", Layout::Force),
        ("examples/deps.dot", Layout::Force),
        ("examples/deps.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Force),
    ] {
        let mut doc = load_dot(path);
        layout::apply(&mut doc, l).unwrap();
        let (buf, _map) = shot(&doc, 200, 50);
        assert_all_nodes_visible(&doc, &buf, &format!("{path} 走 {} 布局", l.name()));
    }
}

#[test]
fn 一帧里每条边都画出了笔画() {
    for (path, l) in [
        ("examples/metro.dot", Layout::Layered),
        ("examples/deps.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Layered),
    ] {
        let mut doc = load_dot(path);
        layout::apply(&mut doc, l).unwrap();
        let (buf, map) = shot(&doc, 200, 50);
        let mut strokes = 0;
        for y in map.y..map.y + map.height {
            for x in map.x..map.x + map.width {
                if matches!(
                    buf[(x, y)].symbol(),
                    "─" | "│" | "╭" | "╮" | "╯" | "╰" | "├" | "┤" | "┬" | "┴" | "┼"
                ) {
                    strokes += 1;
                }
            }
        }
        assert!(strokes > 0, "{path} 走 {} 布局时一条边都没画出来", l.name());
    }
}

#[test]
fn 带框的图每个框都画出来() {
    let text = std::fs::read_to_string("examples/boxed.dot").unwrap();
    for l in Layout::all() {
        let mut doc = load_dot_text(&text);
        layout::apply(&mut doc, l).unwrap();
        let (buf, map) = shot(&doc, 120, 40);
        assert_eq!(
            corners(&buf, map, &doc),
            doc.boxes.len(),
            "{} 布局下框的左上角数量不对",
            l.name()
        );
    }
    for (w, h) in [(40u16, 12u16), (8, 4), (200, 60)] {
        let mut doc = load_dot_text(&text);
        layout::apply(&mut doc, Layout::Layered).unwrap();
        let _ = shot(&doc, w, h);
    }
}

/// examples/limits 里全是故意为难引擎的图:画出来好不好看是另一回事,但不能炸
#[test]
fn 极端输入不炸() {
    let mut files: Vec<String> = std::fs::read_dir("examples/limits")
        .unwrap()
        .map(|e| e.unwrap().path().to_string_lossy().to_string())
        .collect();
    files.sort();
    assert!(files.len() >= 16, "压力用例应该有十几张:{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        // 自环这类引擎明确不收的输入:解析就报错,跳过
        let doc = match Graph::from_dot(&text)
            .and_then(|g| convert::from_graph(&g, ViewState::default(), "dot"))
        {
            Ok(d) => d,
            Err(_) => continue,
        };
        for l in Layout::all() {
            let mut d = doc.clone();
            layout::apply(&mut d, l).unwrap();
            for (w, h) in [(200u16, 50u16), (40, 12), (8, 4)] {
                let (buf, map) = shot(&d, w, h);
                assert!(map.width <= w && map.height <= h, "{path} 地图区越界");
                if (w, h) == (200, 50) {
                    assert!(
                        !visible_nodes(&d, &buf).is_empty(),
                        "{path} 走 {} 布局时一个节点都没画",
                        l.name()
                    );
                }
            }
        }
    }
}

/// 真实数据的图也画得下
#[test]
fn 真实数据的图也画得下() {
    let mut files: Vec<String> = std::fs::read_dir("examples/real")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_string_lossy().to_string())
        .filter(|p| p.ends_with(".dot"))
        .collect();
    files.sort();
    assert!(files.len() >= 5, "真实用例应该有五张:{files:?}");
    for path in files {
        for l in Layout::all() {
            let mut doc = load_dot(&path);
            layout::apply(&mut doc, l).unwrap();
            let (buf, _map) = shot(&doc, 200, 50);
            assert_all_nodes_visible(&doc, &buf, &format!("{path} 走 {} 布局", l.name()));
        }
    }
}

/// native 示例:能加载、能按 1:1 画出来、存一遍再读回来不变
#[test]
fn native_示例画得出来且往返一致() {
    let mut files: Vec<String> = std::fs::read_dir("examples")
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "gtui").then(|| p.to_string_lossy().into_owned())
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "examples 里该有 .gtui 示例");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let doc = Document::load(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
        let (x0, y0, x1, y1) = doc.bbox();
        let area = Rect::new(0, 0, (x1 - x0 + 8) as u16, (y1 - y0 + 4) as u16);
        let mut buf = Buffer::empty(area);
        let mut view = View::default();
        view.zoom = 1.0;
        render::render_map(&mut buf, area, &doc, &view, 1.0);
        // 画面里至少有框线:不是一张空白图
        let drawn = (0..area.width)
            .flat_map(|x| (0..area.height).map(move |y| (x, y)))
            .filter(|&(x, y)| !matches!(buf[(x, y)].symbol(), " " | ""))
            .count();
        assert!(drawn > doc.boxes.len() * 2, "{path}: 画面几乎是空的");
        let again = doc.save();
        let back = Document::load(&again).unwrap_or_else(|e| panic!("{path} 存回来读不了: {e}"));
        assert_eq!(back.boxes, doc.boxes, "{path}: 框不一致");
        assert_eq!(back.texts, doc.texts, "{path}: 文字不一致");
        assert_eq!(back.edges, doc.edges, "{path}: 边不一致");
        assert_eq!(back.save(), again, "{path}: 第二次保存不逐字节相同");
    }
}

/// 验收 6:.dot 导入后存成 .gtui,再加载,位置/样式/分组/折点保持
#[test]
fn dot_导入后_gtui_往返保持几何() {
    for path in [
        "examples/metro.dot",
        "examples/deps.dot",
        "examples/boxed.dot",
    ] {
        let mut doc = load_dot(path);
        layout::apply(&mut doc, Layout::Layered).unwrap();
        let text = doc.save();
        let back = Document::load(&text).unwrap();
        assert_eq!(back.boxes, doc.boxes, "{path}: 框不一致");
        assert_eq!(back.texts, doc.texts, "{path}: 文字不一致");
        assert_eq!(back.edges, doc.edges, "{path}: 边不一致");
        assert_eq!(back.layers, doc.layers, "{path}: 图层不一致");
    }
}

/// 端口对齐:带框的图里,连接点是 T 字形、有向边末端是嵌进边框的箭头(`--dump` 与导出同源)
#[test]
fn 带框图的走线落在连接点上() {
    const TEES: [&str; 15] = [
        "┬", "┴", "├", "┤", "╦", "╩", "╠", "╣", "┳", "┻", "┣", "┫", "┼", "╬", "╋",
    ];
    for path in ["examples/boxed.dot", "examples/styles.dot"] {
        let mut doc = load_dot(path);
        layout::apply(&mut doc, Layout::Layered).unwrap();
        let (x0, y0, x1, y1) = doc.bbox();
        let area = Rect::new(
            0,
            0,
            (x1 - x0 + 8).max(20) as u16,
            (y1 - y0 + 4).max(10) as u16,
        );
        let mut buf = Buffer::empty(area);
        let view = View::default();
        render::render_map(&mut buf, area, &doc, &view, 1.0);
        let mut tees = 0;
        let mut arrows = 0;
        for y in 0..area.height {
            for x in 0..area.width {
                let sym = buf[(x, y)].symbol();
                if TEES.contains(&sym) {
                    tees += 1;
                }
                if matches!(sym, "v" | "^" | ">" | "<") {
                    arrows += 1;
                }
            }
        }
        assert!(tees > 0, "{path}: 一条连接点都没有");
        assert!(arrows > 0, "{path}: 一个嵌入箭头都没有");
    }
}
