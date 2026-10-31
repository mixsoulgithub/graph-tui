//! JSON Canvas(Obsidian 的 `.canvas`)导入导出.只用于互操作,不是存档.
//!
//! 画布坐标与像素的换算是固定的:1 格 = 8x16 像素.导入时除回去,
//! 导出时乘回来.图层是 JSON Canvas 没有的概念,导出时额外带上 `layer` 字段,
//! 读回来还在(别的软件不认识这个字段,会忽略).
//!
//! 导入先按老逻辑解析成中间图(Graph),再交给 `convert::from_graph_cells`
//! 变成 native 文档;导出直接读文档的 box / text / edge.
//!
//! 方向只有 forward 能精确表达:导入一律是 forward;导出时 back 交换端点后写,
//! both / none 退化成 forward 并在警告里逐条说明.

use std::collections::HashMap;

use crate::document::{Dir, Document, ViewState};
use crate::json::{self, Value};
use crate::model::{Builder, Graph};

/// JSON Canvas 的六个预设色:红/橙/黄/绿/青/紫
const PRESETS: [(&str, (u8, u8, u8)); 6] = [
    ("1", (0xef, 0x4b, 0x4b)),
    ("2", (0xf5, 0x9e, 0x0b)),
    ("3", (0xe6, 0xc7, 0x00)),
    ("4", (0x22, 0xb4, 0x6a)),
    ("5", (0x14, 0xb8, 0xa6)),
    ("6", (0xa8, 0x55, 0xf7)),
];

/// 一格 = 8 像素宽
const CELL_W: f64 = 8.0;
/// 一格 = 16 像素高
const CELL_H: f64 = 16.0;
/// JSON Canvas 里太窄的板子不好看,给个下限
const MIN_W: f64 = 80.0;
const MIN_H: f64 = 40.0;

fn preset_color(key: &str) -> Option<(u8, u8, u8)> {
    PRESETS.iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
}

/// 颜色值可以是字符串 "1".."6";也宽容地接受数字 1..6
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

/// 反过来:最接近哪个预设色.距离相同就取前面的
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

/// 第一行(去掉行尾的 `\r`);空行当没有
fn first_line(text: &str) -> Option<&str> {
    let line = text.split('\n').next().unwrap_or("");
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// 节点标签:普通节点取 text 的第一行,group 节点优先 label,
/// 都没有就用 id.
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

/// JSON Canvas 文本 -> 中间图.
///
/// 端点找不到的边跳过;Builder 的错误原样上传.
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
        // 像素 / (8, 16) 就是单元格坐标,后面按"已经是格子"的入口转换
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
        // 重复 id 以最后一个为准,边仍能指到某个真实节点
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

/// JSON Canvas -> native 文档.空画布(没有节点)报错.
pub fn import(text: &str) -> Result<Document, String> {
    let g = import_graph(text)?;
    crate::convert::from_graph_cells(&g, ViewState::default(), "json-canvas")
}

/// native 文档 -> JSON Canvas 文本,附带方向失真的警告.
///
/// 节点 id 直接用 box id,边用 fromNode/toNode 指回它;颜色回查预设色.
/// JSON Canvas 的边只有 forward 语义:`back` 交换端点后写,`both`/`none`
/// 没有精确表达,按 forward 写出并逐条警告.
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
        // 只写框自己的颜色;分组色写出来会变成"这个节点就是这个颜色",
        // 语义被改掉了,和 .dot 那边一致
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

    // JSON Canvas 的边没有文字字段:注释框当普通文字导出,绑定关系丢掉,说一声
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
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
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
    fn import_basic() {
        let d = import(SAMPLE).unwrap();
        assert_eq!(d.boxes.len(), 3);
        assert_eq!(d.texts.len(), 3);
        assert_eq!(d.edges.len(), 1);
        assert_eq!(d.edges[0].dir, Dir::Forward);
        // 像素 / 8、/ 16 得到单元格;节点标签取 text 第一行
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
        // 边用 box id 指人
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
    fn 单元格坐标按_8x16_像素导出() {
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
    fn back_边导出时交换端点() {
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
        // back 的箭头在 from 端:交换端点后 canvas 的 to 端正好是箭头端
        assert_eq!(edge.get("fromNode").and_then(Value::as_str), Some("b2"));
        assert_eq!(edge.get("toNode").and_then(Value::as_str), Some("b1"));
    }

    #[test]
    fn both_与_none_导出取_forward_并警告() {
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
        // 导回来一律是 forward
        let back = import(&text).unwrap();
        assert!(back.edges.iter().all(|e| e.dir == Dir::Forward));
    }

    #[test]
    fn 空画布报错() {
        assert!(import("{\"nodes\":[]}").is_err());
    }
}
