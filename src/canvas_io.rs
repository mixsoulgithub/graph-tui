//! JSON Canvas (Obsidian's `.canvas`) import/export. For interop only, not an archive.
//!
//! Canvas coordinates map to pixels at a fixed rate: 1 cell = 8x16 pixels. Import divides,
//! export multiplies. Layers do not exist in JSON Canvas, so export adds a `layer` field
//! and reads it back (other tools do not know the field and ignore it).
//!
//! Import first parses into the intermediate graph (Graph), then hands it to
//! `convert::from_graph_cells` to make a native document; export reads the document's
//! boxes / texts / edges directly.
//!
//! Only forward can be expressed exactly: imports are always forward; export writes back
//! after swapping endpoints, and degrades both / none to forward with one warning each.

use std::collections::HashMap;

use crate::document::{Dir, Document, ViewState};
use crate::json::{self, Value};
use crate::model::{Builder, Graph};

/// JSON Canvas's six preset colours: red/orange/yellow/green/cyan/purple
const PRESETS: [(&str, (u8, u8, u8)); 6] = [
    ("1", (0xef, 0x4b, 0x4b)),
    ("2", (0xf5, 0x9e, 0x0b)),
    ("3", (0xe6, 0xc7, 0x00)),
    ("4", (0x22, 0xb4, 0x6a)),
    ("5", (0x14, 0xb8, 0xa6)),
    ("6", (0xa8, 0x55, 0xf7)),
];

/// One cell = 8 pixels wide
const CELL_W: f64 = 8.0;
/// One cell = 16 pixels high
const CELL_H: f64 = 16.0;
/// Very narrow boards look bad in JSON Canvas, so set a lower bound
const MIN_W: f64 = 80.0;
const MIN_H: f64 = 40.0;

fn preset_color(key: &str) -> Option<(u8, u8, u8)> {
    PRESETS.iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
}

/// The colour value may be the string "1".."6"; the number 1..6 is accepted too
fn color_value(v: &Value) -> Option<(u8, u8, u8)> {
    match v {
        Value::Str(s) => preset_color(s.trim()),
        Value::Num(n) => {
            if n.fract() == 0.0 && *n >= 1.0 && *n <= 6.0 {
                preset_color(&format!("{}", *n as i64))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Inverse: nearest preset colour. On a tie, the earlier entry wins
fn closest_preset(c: (u8, u8, u8)) -> &'static str {
    let mut best = PRESETS[0].0;
    let mut best_d = i32::MAX;
    for (key, p) in PRESETS {
        let dr = c.0 as i32 - p.0 as i32;
        let dg = c.1 as i32 - p.1 as i32;
        let db = c.2 as i32 - p.2 as i32;
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = key;
        }
    }
    best
}

/// First line (trailing `\r` stripped); an empty line counts as none
fn first_line(text: &str) -> Option<&str> {
    let line = text.split('\n').next().unwrap_or("");
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// Node label: ordinary nodes take the first line of text, group nodes prefer label,
/// falling back to the id.
fn node_label(v: &Value, id: &str) -> String {
    let is_group = v.get("type").and_then(Value::as_str) == Some("group");
    let text = v.get("text").and_then(Value::as_str).and_then(first_line);
    let label = if is_group {
        v.get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or(text)
    } else {
        text
    };
    match label {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => id.to_string(),
    }
}

/// JSON Canvas text -> intermediate graph.
///
/// Edges with missing endpoints are skipped; Builder errors propagate as-is.
fn import_graph(text: &str) -> Result<Graph, String> {
    let root = json::parse(text).map_err(|e| format!("canvas: {e}"))?;
    let nodes = root
        .get("nodes")
        .and_then(Value::as_arr)
        .ok_or_else(|| "canvas: missing 'nodes' array".to_string())?;

    let mut b = Builder::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for (i, v) in nodes.iter().enumerate() {
        if v.as_obj().is_none() {
            return Err(format!("canvas: node {i} is not an object"));
        }
        let id = v
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("canvas: node {i} has no string id"))?;
        let x = v.get("x").and_then(Value::as_num).unwrap_or(0.0);
        let y = v.get("y").and_then(Value::as_num).unwrap_or(0.0);
        let label = node_label(v, id);
        // Pixels / (8, 16) give cell coordinates, then convert through the "already cells"
        // entry
        let ni = b.node(label, Some((x / CELL_W, y / CELL_H)));
        if let Some(c) = v.get("color").and_then(color_value) {
            b.node_color(ni, Some(c));
        }
        if let Some(layer) = v
            .get("layer")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            b.node_layer(ni, layer);
        }
        // For duplicate ids the last one wins, so edges still point at a real node
        ids.insert(id.to_string(), ni);
    }

    let empty: &[Value] = &[];
    let edges = root.get("edges").and_then(Value::as_arr).unwrap_or(empty);
    for (i, v) in edges.iter().enumerate() {
        if v.as_obj().is_none() {
            return Err(format!("canvas: edge {i} is not an object"));
        }
        let from = v.get("fromNode").and_then(Value::as_str);
        let to = v.get("toNode").and_then(Value::as_str);
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        let (Some(&a), Some(&b_node)) = (ids.get(from), ids.get(to)) else {
            continue;
        };
        let label = v.get("label").and_then(Value::as_str).unwrap_or("");
        let gi = b.group(label, None);
        let ei = b.edge(a, b_node, gi, Dir::Forward);
        if let Some(layer) = v
            .get("layer")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            b.edge_layer(ei, layer);
        }
    }

    b.build()
}

/// JSON Canvas -> native document. An empty canvas (no nodes) is an error.
pub fn import(text: &str) -> Result<Document, String> {
    let g = import_graph(text)?;
    crate::convert::from_graph_cells(&g, ViewState::default(), "json-canvas")
}

/// Native document -> JSON Canvas text, plus warnings about direction loss.
///
/// Node ids are the box ids and edges point back via fromNode/toNode; colours map to
/// presets. JSON Canvas edges only have forward semantics: `back` is written with swapped
/// endpoints, `both`/`none` have no exact form, so they are written as forward with one
/// warning each.
pub fn export(doc: &Document) -> (String, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let mut out_nodes: Vec<Value> = Vec::with_capacity(doc.boxes.len());
    for (i, b) in doc.boxes.iter().enumerate() {
        let text = doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
        let mut fields: Vec<(String, Value)> = vec![
            ("id".to_string(), Value::Str(b.id.clone())),
            ("type".to_string(), Value::Str("text".to_string())),
            ("text".to_string(), Value::Str(text)),
            ("x".to_string(), Value::Num(b.at.0 as f64 * CELL_W)),
            ("y".to_string(), Value::Num(b.at.1 as f64 * CELL_H)),
            (
                "width".to_string(),
                Value::Num((b.size.0 as f64 * CELL_W).max(MIN_W)),
            ),
            (
                "height".to_string(),
                Value::Num((b.size.1 as f64 * CELL_H).max(MIN_H)),
            ),
        ];
        // Write only the box's own colour; a group colour would become "this node is that
        // colour", changing the meaning. Same as the .dot exporter
        if let Some(c) = b.color {
            fields.push((
                "color".to_string(),
                Value::Str(closest_preset(c).to_string()),
            ));
        }
        if !b.layer.is_empty() {
            fields.push(("layer".to_string(), Value::Str(b.layer.clone())));
        }
        out_nodes.push(Value::Obj(fields));
    }

    let mut out_edges: Vec<Value> = Vec::with_capacity(doc.edges.len());
    for e in &doc.edges {
        let (from, to) = match e.dir {
            Dir::Forward => (e.from.clone(), e.to.clone()),
            Dir::Back => (e.to.clone(), e.from.clone()),
            Dir::Both | Dir::None => {
                warnings.push(format!(
                    "边 {} 的方向 {} 无法在 JSON Canvas 中表达,按 forward 写出",
                    e.id,
                    e.dir.name()
                ));
                (e.from.clone(), e.to.clone())
            }
        };
        let mut fields: Vec<(String, Value)> = vec![
            ("id".to_string(), Value::Str(e.id.clone())),
            ("fromNode".to_string(), Value::Str(from)),
            ("toNode".to_string(), Value::Str(to)),
        ];
        if let Some(gi) = doc.group_idx(&e.group) {
            let label = &doc.groups[gi].label;
            if !label.is_empty() {
                fields.push(("label".to_string(), Value::Str(label.clone())));
            }
        }
        if !e.layer.is_empty() {
            fields.push(("layer".to_string(), Value::Str(e.layer.clone())));
        }
        out_edges.push(Value::Obj(fields));
    }

    // JSON Canvas edges have no text field: export labels as plain text, drop the binding,
    // and warn
    if doc.boxes.iter().any(|b| b.edge.is_some()) {
        warnings.push(
            "edge notes are exported as plain text nodes (.canvas has no binding)".to_string(),
        );
    }

    let root = Value::Obj(vec![
        ("nodes".to_string(), Value::Arr(out_nodes)),
        ("edges".to_string(), Value::Arr(out_edges)),
    ]);
    (json::write(&root), warnings)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // build defaults then tweak fields
mod tests {
    use super::*;
    use crate::document::Dir;
    use crate::model::{BoxStyle, Builder};

    const SAMPLE: &str = r#"{
        "nodes": [
            {"id":"a","type":"text","text":"hello\nworld","x":16,"y":32,"width":200,"height":60,"color":"1"},
            {"id":"b","type":"group","label":"Box","text":"ignored","x":-80,"y":100,"width":200,"height":60},
            {"id":"c","type":"text","x":0,"y":-16,"width":200,"height":60}
        ],
        "edges": [
            {"id":"e1","fromNode":"a","toNode":"b","fromSide":"right","toSide":"left","label":"flow"},
            {"id":"e2","fromNode":"c","toNode":"missing","label":"lost"},
            {"id":"e3","fromNode":"missing","toNode":"a"}
        ]
    }"#;

    #[test]
    fn rejects_control_characters_in_text() {
        let src = r#"{"nodes":[{"id":"a","type":"text","text":"x\u001b]0;PWNED\u0007","x":0,"y":0}],"edges":[]}"#;
        let e = match import(src) {
            Ok(_) => panic!("control character should be rejected"),
            Err(e) => e,
        };
        assert!(e.contains("control character"), "{e}");
    }

    #[test]
    fn import_basic() {
        let d = import(SAMPLE).unwrap();
        assert_eq!(d.boxes.len(), 3);
        assert_eq!(d.texts.len(), 3);
        assert_eq!(d.edges.len(), 1);
        assert_eq!(d.edges[0].dir, Dir::Forward);
        // Pixels / 8 and / 16 give cells; node labels take the first line of text
        assert_eq!(d.texts[0].text, "hello");
        assert_eq!(d.boxes[0].at, (2, 2));
        assert_eq!(d.boxes[0].color, Some((0xef, 0x4b, 0x4b)));
        assert_eq!(d.texts[1].text, "Box");
        assert_eq!(d.boxes[1].at, (-10, 6));
        assert_eq!(d.texts[2].text, "c");
        assert_eq!(d.boxes[2].at, (0, -1));
        let gi = d.group_idx(&d.edges[0].group).unwrap();
        assert_eq!(d.groups[gi].label, "flow");
    }

    #[test]
    fn export_roundtrip() {
        let mut b = Builder::new();
        let a = b.node("alpha", Some((1.5, 2.0)));
        b.node_color(a, Some((0x22, 0xb4, 0x6a)));
        b.node_layer(a, "one");
        let c = b.node("alpha", Some((10.0, -3.0)));
        b.node_box(c, BoxStyle::Single);
        b.node_layer(c, "two");
        let gi = b.group("line", None);
        let e = b.edge(a, c, gi, Dir::Forward);
        b.edge_layer(e, "one");
        let g = b.build().unwrap();
        let doc = crate::convert::from_graph(&g, ViewState::default(), "dot").unwrap();

        let (text, _) = export(&doc);
        let v = json::parse(&text).unwrap();
        assert_eq!(v.get("nodes").and_then(Value::as_arr).unwrap().len(), 2);
        assert_eq!(v.get("edges").and_then(Value::as_arr).unwrap().len(), 1);
        // Edges point at box ids
        let edge = &v.get("edges").and_then(Value::as_arr).unwrap()[0];
        assert_eq!(edge.get("fromNode").and_then(Value::as_str), Some("b1"));
        assert_eq!(edge.get("toNode").and_then(Value::as_str), Some("b2"));

        let back = import(&text).unwrap();
        assert_eq!(back.boxes.len(), doc.boxes.len());
        assert_eq!(back.edges.len(), doc.edges.len());
        assert_eq!(back.edges[0].dir, Dir::Forward);
        assert_eq!(back.edges[0].layer, "one");
        assert!(back.layers.iter().any(|l| l.name == "one"));
        assert!(back.layers.iter().any(|l| l.name == "two"));
    }

    #[test]
    fn cell_coordinates_export_as_8x16_pixels() {
        let mut b = Builder::new();
        b.node("a", Some((2.0, 3.0)));
        b.node("b", Some((5.0, 7.0)));
        let g = b.build().unwrap();
        let doc = crate::convert::from_graph_cells(&g, ViewState::default(), "canvas").unwrap();

        let (text, _) = export(&doc);
        let v = json::parse(&text).unwrap();
        let node = &v.get("nodes").and_then(Value::as_arr).unwrap()[0];
        assert_eq!(node.get("x").and_then(Value::as_num), Some(16.0));
        assert_eq!(node.get("y").and_then(Value::as_num), Some(48.0));
        assert_eq!(
            node.get("width").and_then(Value::as_num),
            Some((1.0 * CELL_W).max(MIN_W))
        );
        assert_eq!(
            node.get("height").and_then(Value::as_num),
            Some((1.0 * CELL_H).max(MIN_H))
        );

        let back = import(&text).unwrap();
        assert_eq!(back.boxes[0].at, (2, 3));
        assert_eq!(back.boxes[1].at, (5, 7));
        assert_eq!(back.boxes.len(), 2);
    }

    #[test]
    fn back_edges_swap_endpoints_on_export() {
        let mut b = Builder::new();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((5.0, 5.0)));
        let gi = b.group("g", None);
        b.edge(a, c, gi, Dir::Back);
        let g = b.build().unwrap();
        let doc = crate::convert::from_graph(&g, ViewState::default(), "dot").unwrap();

        let (text, warnings) = export(&doc);
        assert!(warnings.is_empty(), "{warnings:?}");
        let v = json::parse(&text).unwrap();
        let edge = &v.get("edges").and_then(Value::as_arr).unwrap()[0];
        // A back arrowhead sits at the from end: after swapping, canvas's to end is the
        // arrow end
        assert_eq!(edge.get("fromNode").and_then(Value::as_str), Some("b2"));
        assert_eq!(edge.get("toNode").and_then(Value::as_str), Some("b1"));
    }

    #[test]
    fn both_and_none_export_as_forward_with_warnings() {
        let mut b = Builder::new();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((5.0, 5.0)));
        let g1 = b.group("one", None);
        let g2 = b.group("two", None);
        b.edge(a, c, g1, Dir::Both);
        b.edge(a, c, g2, Dir::None);
        let g = b.build().unwrap();
        let doc = crate::convert::from_graph(&g, ViewState::default(), "dot").unwrap();

        let (text, warnings) = export(&doc);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings
            .iter()
            .all(|w| w.contains("无法在 JSON Canvas 中表达")));
        let v = json::parse(&text).unwrap();
        for edge in v.get("edges").and_then(Value::as_arr).unwrap() {
            assert_eq!(edge.get("fromNode").and_then(Value::as_str), Some("b1"));
            assert_eq!(edge.get("toNode").and_then(Value::as_str), Some("b2"));
        }
        // Imports are always forward
        let back = import(&text).unwrap();
        assert!(back.edges.iter().all(|e| e.dir == Dir::Forward));
    }

    #[test]
    fn empty_canvas_is_an_error() {
        assert!(import("{\"nodes\":[]}").is_err());
    }
}
