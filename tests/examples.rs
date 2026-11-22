//! End to end: the graphs in examples draw under every layout, and on a large enough canvas
//! every box gets its own cell.
//!
//! It goes through the new pipeline: `.dot` -> Graph -> convert::from_graph -> Document ->
//! layout::apply -> render.

// Tests build a default View and tweak a few fields, which reads better than a long literal.
#![allow(clippy::field_reassign_with_default)]

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

/// Draw one frame (filled up front, like the App's first paint); returns the buffer and the map
/// area
fn shot(doc: &Document, w: u16, h: u16) -> (Buffer, Rect) {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    let mut view = View::default();
    view.zoom = render::zoom_for_fit(doc, w, h, view.edge_labels);
    let p = render::render(&mut buf, area, doc, &view);
    (buf, p.map)
}

/// Where each node is "visible" on screen: point nodes by their node glyph, boxed ones by the
/// top-left corner, text-only (none) ones by their first character
fn visible_nodes(doc: &Document, buf: &Buffer) -> Vec<(usize, (i32, i32))> {
    let area = Rect::new(0, 0, buf.area.width, buf.area.height);
    let mut view = View::default();
    // The zoom_for_fit flags must match the ones panels uses below, otherwise the map comes out
    // narrower than computed and the right side gets cut
    view.zoom = render::zoom_for_fit(doc, area.width, area.height, view.edge_labels);
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
        // At low zoom LOD drops the text, leaving only the node glyph on a text-only (none)
        // node, so these glyphs count as "visible" for every non-boxed style
        let node_glyph = matches!(sym, "○" | "◎" | "●" | "◉");
        let ok = if b.style.is_boxed() {
            matches!(sym, "╭" | "┌" | "╔" | "┏")
        } else if b.style.is_point() {
            node_glyph
        } else {
            node_glyph
                || match doc.text(i).and_then(|t| t.text.chars().next()) {
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

/// Every node is drawn and each takes its own cell
fn assert_all_nodes_visible(doc: &Document, buf: &Buffer, what: &str) {
    let seen = visible_nodes(doc, buf);
    assert_eq!(
        seen.len(),
        doc.boxes.len(),
        "{what}:只画出 {} 个节点(共 {} 个)",
        seen.len(),
        doc.boxes.len()
    );
    // "Its own cell" only means something at 1:1 or above: when a node shrinks below one cell,
    // two adjacent rows naturally land on the same cell
    let area = Rect::new(0, 0, buf.area.width, buf.area.height);
    let mut view = View::default();
    view.zoom = render::zoom_for_fit(doc, area.width, area.height, view.edge_labels);
    if view.zoom >= 1.0 {
        let cells: HashSet<(i32, i32)> = seen.iter().map(|(_, c)| *c).collect();
        assert_eq!(cells.len(), seen.len(), "{what}:有节点挤在同一格");
    }
}

/// For every boxed box the top-left cell must be a corner glyph (bent edges use ╭ too, so
/// counting is not enough)
fn corners(buf: &Buffer, map: Rect, doc: &Document) -> usize {
    let area = Rect::new(0, 0, buf.area.width, buf.area.height);
    let mut view = View::default();
    view.zoom = render::zoom_for_fit(doc, area.width, area.height, view.edge_labels);
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
fn each_layout_gives_each_point_node_its_own_cell() {
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
fn every_edge_has_a_stroke_in_one_frame() {
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
fn every_boxed_box_is_drawn() {
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

/// examples/limits holds graphs that deliberately stress the engine: how they look is secondary,
/// but nothing may blow up
#[test]
fn extreme_inputs_do_not_panic() {
    let mut files: Vec<String> = std::fs::read_dir("examples/limits")
        .unwrap()
        .map(|e| e.unwrap().path().to_string_lossy().to_string())
        .collect();
    files.sort();
    assert!(files.len() >= 16, "压力用例应该有十几张:{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        // Input the engine explicitly rejects (self loops and the like) fails to parse; skip it
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

/// Graphs from real data fit too
#[test]
fn real_data_graphs_fit() {
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

/// Native examples: they load, draw at 1:1, and survive a save + reload unchanged
#[test]
fn native_examples_draw_and_round_trip() {
    // The top level holds layout examples, fun/ holds hand-placed pictures: both must load,
    // draw, and save back byte for byte. The fun/ .gtui files are force-added to the repo and
    // tested like the top-level examples.
    let mut files: Vec<String> = ["examples", "examples/fun"]
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|dir| {
            dir.filter_map(|e| {
                let p = e.ok()?.path();
                (p.extension()? == "gtui").then(|| p.to_string_lossy().into_owned())
            })
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
        // At least some box lines in the picture: not a blank frame
        let drawn = (0..area.width)
            .flat_map(|x| (0..area.height).map(move |y| (x, y)))
            .filter(|&(x, y)| !matches!(buf[(x, y)].symbol(), " " | ""))
            .count();
        // A dot picture (one glyph per node) counts too, so at least one cell per box
        assert!(drawn >= doc.boxes.len(), "{path}: 画面几乎是空的");
        let again = doc.save();
        let back = Document::load(&again).unwrap_or_else(|e| panic!("{path} 存回来读不了: {e}"));
        assert_eq!(back.boxes, doc.boxes, "{path}: 框不一致");
        assert_eq!(back.texts, doc.texts, "{path}: 文字不一致");
        assert_eq!(back.edges, doc.edges, "{path}: 边不一致");
        assert_eq!(back.save(), again, "{path}: 第二次保存不逐字节相同");
    }
}

/// Acceptance 6: after importing a .dot and saving as .gtui, a reload keeps position, style,
/// group and via points
#[test]
fn dot_import_then_gtui_round_trip_keeps_geometry() {
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

/// Port alignment: in a boxed graph the junctions are T glyphs and directed edges end in an
/// arrowhead embedded in the border (picture and export share one source)
#[test]
fn boxed_routing_lands_on_ports() {
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

/// No two box rectangles intersect
fn no_overlap(doc: &Document) -> bool {
    for i in 0..doc.boxes.len() {
        for j in (i + 1)..doc.boxes.len() {
            let (a, b) = (&doc.boxes[i], &doc.boxes[j]);
            let ax1 = a.at.0 + a.size.0 as i32;
            let ay1 = a.at.1 + a.size.1 as i32;
            let bx1 = b.at.0 + b.size.0 as i32;
            let by1 = b.at.1 + b.size.1 as i32;
            if a.at.0 < bx1 && b.at.0 < ax1 && a.at.1 < by1 && b.at.1 < ay1 {
                return false;
            }
        }
    }
    true
}

/// Structure detection: auto picks the right layout on real samples and reports the matching
/// warning codes
#[test]
fn auto_picks_layout_by_structure() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("examples/org.dot", "tree", &[]),
        ("examples/limits/star40.dot", "star", &[]),
        ("examples/timeline.dot", "chain", &[]),
        ("examples/limits/k12.dot", "relax", &["dense_graph"]),
        (
            "examples/limits/components.dot",
            "layered",
            &["components_packed"],
        ),
    ];
    for &(path, profile, codes) in cases {
        let mut doc = load_dot(path);
        let r = layout::apply(&mut doc, Layout::Auto).expect("auto 布局");
        assert_eq!(r.profile, profile, "{path}");
        for code in codes {
            assert!(
                r.warnings.iter().any(|w| w.code == *code),
                "{path} 少了 {code}:{:?}",
                r.warnings
            );
        }
        // Every input must still get a non-overlapping layout
        assert!(no_overlap(&doc), "{path} 有重叠框");
    }
}

/// Star sample: auto lays it out as a fan (four columns) instead of one row over 500 cells wide
#[test]
fn star_sample_becomes_fan() {
    let mut doc = load_dot("examples/limits/star40.dot");
    let r = layout::apply(&mut doc, Layout::Auto).expect("布局");
    assert_eq!(r.profile, "star");
    assert!(no_overlap(&doc), "扇子不该有重叠");
    let (x0, y0, w, h) = doc.bbox();
    assert!(w < 80, "扇子该窄: {w} 格宽");
    assert!(h > 10, "扇子该高: {h} 行");
    // Four leaf columns of 10 leaves each with identical x; two columns sit on either side of
    // the hub
    let hub = (0..doc.boxes.len())
        .find(|&i| doc.text(i).map(|t| t.text.as_str()) == Some("hub"))
        .expect("枢纽");
    let mut columns: Vec<(i32, usize)> = Vec::new();
    for (i, b) in doc.boxes.iter().enumerate() {
        if i == hub {
            continue;
        }
        match columns.iter_mut().find(|(x, _)| *x == b.at.0) {
            Some(c) => c.1 += 1,
            None => columns.push((b.at.0, 1)),
        }
    }
    assert_eq!(columns.len(), 4, "该分成四列:{columns:?}");
    for (_, n) in &columns {
        assert_eq!(*n, 10, "{columns:?}");
    }
    // Picture: every leaf draws and all of them fall inside the map area
    let _ = (x0, y0);
    let (buf, _) = shot(&doc, (w + 40) as u16, (h + 8) as u16);
    assert_all_nodes_visible(&doc, &buf, "star40");
}

/// Pure chain: auto picks chain and the spine stays on one row
#[test]
fn chain_sample_becomes_one_line() {
    let mut doc = load_dot("examples/timeline.dot");
    let r = layout::apply(&mut doc, Layout::Auto).expect("布局");
    assert_eq!(r.profile, "chain");
    let ys: Vec<i32> = doc.boxes.iter().map(|b| b.at.1).collect();
    assert!(ys.windows(2).all(|w| w[0] == w[1]), "主线该在一行:{ys:?}");
}

/// org.dot: clusters import as regions, explicit ports survive, .dot export writes them back;
/// saving as .gtui and reloading agrees
#[test]
fn org_sample_regions_and_ports() {
    // Regions and ports only come from the full import (dot::parse_document); Graph::from_dot
    // is a bare graph
    let text = std::fs::read_to_string("examples/org.dot").unwrap();
    let doc = graph_tui::dot::parse_document(&text, ViewState::default(), "dot").unwrap();
    assert_eq!(doc.regions.len(), 2, "两个 cluster");
    let mut labels: Vec<&str> = doc.regions.iter().map(|r| r.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["engineering", "operations"]);
    for r in &doc.regions {
        assert!(!r.members.is_empty(), "区域 {} 该有成员", r.id);
    }
    // Ports: CEO -> CTO forces left/right
    let text_of = |id: &str| doc.text(doc.box_idx(id).unwrap()).map(|t| t.text.clone());
    let ceo = doc.boxes[0].id.clone();
    let e = doc
        .edges
        .iter()
        .find(|e| e.from == ceo && text_of(&e.to).as_deref() == Some("CTO"))
        .expect("CEO -> CTO");
    assert_eq!(e.from_side, Some(graph_tui::model::Side::Left));
    assert_eq!(e.to_side, Some(graph_tui::model::Side::Right));

    // Export: both the subgraph and the port attributes are present
    let (out, _w) = graph_tui::dot_write::export(&doc).unwrap();
    assert!(out.contains("subgraph \"cluster_eng\""), "{out}");
    assert!(out.contains("label=\"engineering\""), "{out}");
    assert!(out.contains("from_side=\"left\""), "{out}");
    assert!(out.contains("to_side=\"right\""), "{out}");

    // .gtui round trip: regions and ports are still there
    let body = doc.save();
    let back = Document::load(&body).unwrap();
    assert_eq!(back.regions, doc.regions);
    assert_eq!(back.edges[0].from_side, doc.edges[0].from_side);
}

/// Both samples render as a tree/chain without overlap
#[test]
fn new_samples_render() {
    for (path, want_rows_ok) in [("examples/org.dot", false), ("examples/timeline.dot", true)] {
        let mut doc = load_dot(path);
        layout::apply(&mut doc, Layout::Auto).unwrap();
        let (buf, _map) = shot(&doc, 120, 40);
        assert_all_nodes_visible(&doc, &buf, path);
        if want_rows_ok {
            let ys: HashSet<i32> = doc.boxes.iter().map(|b| b.at.1).collect();
            assert_eq!(ys.len(), 1, "{path} 该排成一行");
        }
    }
}
