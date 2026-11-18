//! 把 native 文档导成 `.dot`.只用于显式导出,不是存档格式.
//!
//! 导出是尽力而为的:能表达的部分(节点、边、方向、线型、折点、颜色、分组)
//! 照写,表达不了或会失真的部分由 `convert::to_graph` 返回警告.
//! 文件头会明确写出这是从 `.gtui` 导出的有损视图,不是源文档.
//!
//! 有向/无向的写法必须跟图类型一致:只要有一条边带箭头(forward/back/both)
//! 就整张图按 digraph 写;digraph 里 forward 是默认值不写属性,其余方向写
//! `[dir=none|back|both]`(Graphviz 不接受 digraph 里写 `--`).
//! `fill` / `align` / `corner` 是本项目自定义的样式属性,写出来只为 roundtrip,
//! Graphviz 会忽略,导出的警告里会逐项说明.

use std::collections::HashMap;

use crate::document::Document;
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Graph};

/// 节点名:框 id(`b1`…),`.dot` 里用它指节点,文字走 `label`
fn box_name(doc: &Document, i: usize) -> &str {
    doc.boxes.get(i).map(|b| b.id.as_str()).unwrap_or("b?")
}

/// 坐标写成文本:`0` 不要写成 `-0`,`11.5` 原样保留
fn num(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}

/// 文档边在导出时的去重键,和 Builder 的规则一致:
/// 无向边按无序端点算,其余按有序端点算.
fn edge_key(a: usize, b: usize, dir: Dir, group: &str) -> (usize, usize, String) {
    let (a, b) = if dir != Dir::None || a < b {
        (a, b)
    } else {
        (b, a)
    };
    (a, b, group.to_string())
}

/// 文档 -> `.dot` 文本,附带失真警告.调用方负责把警告报给用户
pub fn export(doc: &Document) -> Result<(String, Vec<String>), String> {
    let (g, mut warnings) = crate::convert::to_graph(doc)?;
    // 边框填充、文字对齐/颜色、框尺寸、注释绑定这些是自定义属性:
    // 写出去是为了 roundtrip,Graphviz 会当不认识而忽略
    if doc.boxes.iter().any(|b| b.fill.is_some()) {
        warnings.push("fill 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    if doc.texts.iter().any(|t| t.align != Align::Left) {
        warnings.push("align 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    if doc.edges.iter().any(|e| e.corner != Corner::Rounded) {
        warnings.push("corner 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    // 区域套区域:cluster 只写一层,成员是区域的那一层会丢
    if doc.regions.iter().any(|r| {
        r.members
            .iter()
            .any(|m| doc.regions.iter().any(|x| &x.id == m))
    }) {
        warnings.push("区域套区域无法表达:嵌在里面的那一层会丢".to_string());
    }
    // 没有任何边的分组没有地方携带配色
    if doc
        .groups
        .iter()
        .any(|grp| !doc.edges.iter().any(|e| e.group == grp.id))
    {
        warnings.push("没有边的分组无法携带配色".to_string());
    }
    Ok((write_graph(&g, doc), warnings))
}

/// 中间图 -> `.dot` 文本.节点按索引、边按去重后的键,从文档补读新样式字段
fn write_graph(g: &Graph, doc: &Document) -> String {
    let digraph = g.edges.iter().any(|e| e.dir.is_arrow());
    // 边会被 Builder 去重,拐角按去重后的边查;同一条边只留第一次出现的样式
    let mut corner_of: HashMap<(usize, usize, String), Corner> = HashMap::new();
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        let label = doc
            .group_idx(&e.group)
            .map(|gi| doc.groups[gi].label.clone())
            .unwrap_or_default();
        corner_of
            .entry(edge_key(a, b, e.dir, &label))
            .or_insert(e.corner);
    }
    // 边注释:注释框的文字就是 label(严格一对一,不用挑)
    let mut label_of: HashMap<(usize, usize, String), Option<String>> = HashMap::new();
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        let label = doc
            .group_idx(&e.group)
            .map(|gi| doc.groups[gi].label.clone())
            .unwrap_or_default();
        let note = doc
            .edge_label_of(&e.id)
            .and_then(|b| doc.text(b).map(|t| t.text.clone()));
        label_of
            .entry(edge_key(a, b, e.dir, &label))
            .or_insert(note);
    }
    // 箭头字形与线的底色也按去重后的边查
    let mut head_of: HashMap<(usize, usize, String), Option<crate::model::ArrowStyle>> =
        HashMap::new();
    let mut bg_of: HashMap<(usize, usize, String), Option<crate::document::Rgb>> = HashMap::new();
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        let label = doc
            .group_idx(&e.group)
            .map(|gi| doc.groups[gi].label.clone())
            .unwrap_or_default();
        let key = edge_key(a, b, e.dir, &label);
        head_of.entry(key.clone()).or_insert(e.head);
        bg_of.entry(key).or_insert(e.bg);
    }
    // 端口按去重后的边查:同一条边只留第一次出现的
    let mut port_of: HashMap<(usize, usize, String), crate::convert::EdgePorts> = HashMap::new();
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        let label = doc
            .group_idx(&e.group)
            .map(|gi| doc.groups[gi].label.clone())
            .unwrap_or_default();
        port_of
            .entry(edge_key(a, b, e.dir, &label))
            .or_insert((e.from_side, e.to_side));
    }
    let mut out = String::new();
    out.push_str("// 从 .gtui 导出的有损视图,不是源文档;不要把它写回存档.\n");
    out.push_str("// lossy view exported from .gtui; not the source document.\n");
    out.push_str(if digraph { "digraph {\n" } else { "graph {\n" });
    // pos 的单位与方向写清楚:一格 = 一个单位、y 向上(Graphviz 的老规矩),
    // 值取框的中心;本项目的导入按 pos_unit 原样还原,不再重新投影
    out.push_str("  graph [pos_unit=\"cell\", node_names=\"id\"];\n");

    for (i, n) in g.nodes.iter().enumerate() {
        let mut attrs: Vec<String> = Vec::new();
        // 节点名用框 id(`b1`…):文字放 `label`,名字才不会因为同名而被合并
        let name = doc.boxes.get(i).map(|b| b.id.as_str()).unwrap_or("b?");
        // 文字一律写 `label`(可为空):名字只是 id,写不写 label 是两回事
        attrs.push(format!("label={}", quote(&n.label)));
        if !n.terms.is_empty() {
            attrs.push(format!("terms={}", quote(&n.terms.join("|"))));
        }
        if let Some((x, y)) = n.pos {
            attrs.push(format!("pos={}", quote(&format!("{},{}", num(x), num(-y)))));
        }
        // 点节点写成 graphviz 自己的 shape=point(默认样式是 none,不写就回不来了);
        // 其余样式写自定义 box=
        if n.box_style == BoxStyle::Point {
            attrs.push("shape=\"point\"".to_string());
        } else {
            attrs.push(format!("box={}", quote(n.box_style.name())));
        }
        if let Some(c) = n.color {
            attrs.push(format!("color={}", quote(&hex(c))));
        }
        if let Some(b) = doc.boxes.get(i) {
            if let Some(c) = b.fill {
                attrs.push(format!("fill={}", quote(&hex(c))));
            }
            // 框尺寸:`.dot` 不在乎大小,但手工改过的框只有写下来才能原样回来
            attrs.push(format!(
                "size={}",
                quote(&format!("{}x{}", b.size.0, b.size.1))
            ));
            // 边标签框:绑到第几条边(1 起).导入端见到 edge_label= 就不再从边的 label 新建
            if let Some(eid) = &b.edge {
                if let Some(n) = doc.edges.iter().position(|e| &e.id == eid) {
                    attrs.push(format!("edge_label={}", quote(&(n + 1).to_string())));
                }
            }
        }
        if let Some(t) = doc.text(i) {
            if t.align != Align::Left {
                attrs.push(format!("align={}", quote(t.align.name())));
            }
            if let Some(c) = t.color {
                attrs.push(format!("textcolor={}", quote(&hex(c))));
            }
            if let Some(c) = t.bg {
                attrs.push(format!("textbg={}", quote(&hex(c))));
            }
        }
        if !n.layer.is_empty() {
            attrs.push(format!("layer={}", quote(&n.layer)));
        }
        out.push_str(&format!("  {}{};\n", quote(name), tail(&attrs)));
    }

    // 区域(cluster):成员用节点名再列一遍,Graphviz 会把它当成同一个节点
    for r in &doc.regions {
        let mut names: Vec<&str> = Vec::new();
        for m in &r.members {
            if let Some(bi) = doc.box_idx(m) {
                if g.nodes.get(bi).is_some() {
                    names.push(m.as_str());
                }
            }
        }
        if names.is_empty() {
            continue;
        }
        let name = if r.id.starts_with("cluster_") {
            r.id.clone()
        } else {
            format!("cluster_{}", r.id)
        };
        out.push_str(&format!("  subgraph {} {{\n", quote(&name)));
        if !r.label.is_empty() {
            out.push_str(&format!("    label={};\n", quote(&r.label)));
        }
        // 区域自己的样式:别人的 .dot 里没有属性时按默认(圆角、留 1 格、不折叠)
        if let Some(c) = r.color {
            out.push_str(&format!("    color={};\n", quote(&hex(c))));
        }
        if r.style != BoxStyle::Rounded {
            out.push_str(&format!("    box={};\n", quote(r.style.name())));
        }
        if r.padding != 1 {
            out.push_str(&format!("    padding={};\n", quote(&r.padding.to_string())));
        }
        if r.collapsed {
            out.push_str("    collapsed=\"1\";\n");
        }
        if !r.layer.is_empty() {
            out.push_str(&format!("    layer={};\n", quote(&r.layer)));
        }
        for n in names {
            out.push_str(&format!("    {};\n", quote(n)));
        }
        out.push_str("  }\n");
    }

    for e in &g.edges {
        let mut attrs: Vec<String> = Vec::new();
        let group = &g.groups[e.group].label;
        if !group.is_empty() {
            attrs.push(format!("group={}", quote(group)));
        }
        if !e.layer.is_empty() {
            attrs.push(format!("layer={}", quote(&e.layer)));
        }
        if e.style != EdgeStyle::Solid {
            attrs.push(format!("style={}", quote(e.style.name())));
        }
        // 颜色总是写:None 表示"跟分组色走",这里把分组色写实,
        // 导入时才能从"同一组边同一个颜色"反推出分组配色
        let color = e.color.unwrap_or_else(|| g.groups[e.group].color);
        attrs.push(format!("color={}", quote(&hex(color))));
        let key = edge_key(e.a, e.b, e.dir, group);
        if let Some(Some(c)) = bg_of.get(&key) {
            attrs.push(format!("bg={}", quote(&hex(*c))));
        }
        if let Some(Some(h)) = head_of.get(&key) {
            attrs.push(format!("head={}", quote(h.name())));
        }
        if !e.via.is_empty() {
            let pts: Vec<String> = e
                .via
                .iter()
                .map(|&(x, y)| format!("{},{}", num(x), num(-y)))
                .collect();
            attrs.push(format!("via={}", quote(&pts.join(" "))));
        }
        // digraph 里没有 `--`,无向边靠 dir=none 表示;forward 是默认值,不写属性
        if digraph && e.dir != Dir::Forward {
            attrs.push(format!("dir={}", quote(e.dir.name())));
        }
        let key = edge_key(e.a, e.b, e.dir, group);
        if let Some(Some(label)) = label_of.get(&key) {
            attrs.push(format!("label={}", quote(label)));
        }
        if let Some(corner) = corner_of.get(&key) {
            if *corner != Corner::Rounded {
                attrs.push(format!("corner={}", quote(corner.name())));
            }
        }
        // 端口是我们自己的属性,Graphviz 不认识但会原样保留
        if let Some((from_side, to_side)) = port_of.get(&key) {
            if let Some(s) = from_side {
                attrs.push(format!("from_side={}", quote(s.name())));
            }
            if let Some(s) = to_side {
                attrs.push(format!("to_side={}", quote(s.name())));
            }
        }
        let arrow = if digraph { "->" } else { "--" };
        let (an, bn) = (box_name(doc, e.a), box_name(doc, e.b));
        out.push_str(&format!(
            "  {} {arrow} {}{};\n",
            quote(an),
            quote(bn),
            tail(&attrs)
        ));
    }

    out.push_str("}\n");
    out
}

/// `[k=v, k=v]`;没有属性就没有这一段
fn tail(attrs: &[String]) -> String {
    if attrs.is_empty() {
        String::new()
    } else {
        format!(" [{}]", attrs.join(", "))
    }
}

/// 引号字符串:只转义反斜杠与双引号(解析器也只认这两种)
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            // 多行写成 graphviz 的 \n
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 颜色 -> `#rrggbb`
fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;
    use crate::document::{Dir, Document, ViewState};
    use crate::model::{Align, BoxStyle, Corner, EdgeStyle, Graph};

    /// 一张有框/点、有向/无向、带折点与图层分组的文档
    fn sample() -> Document {
        let text = r##"digraph {
            "甲" [label="甲站", pos="10,20", box="single", color="#112233", terms="jia", layer="one"];
            "乙" [label="乙站", pos="30,40", box="rounded"];
            "丙" [pos="50,60"];
            "甲" -> "乙" [group="主线", style="dashed", color="#df4661", via="20,30 25,35", layer="one"];
            "乙" -> "丙" [group="支线", dir="none"];
        }"##;
        let g = Graph::from_dot(text).unwrap();
        crate::convert::from_graph(&g, ViewState::default(), "dot").unwrap()
    }

    #[test]
    fn 文件头写清楚是有损视图() {
        let (text, _) = export(&sample()).unwrap();
        assert!(text.contains("有损视图"), "{text}");
        assert!(text.contains("不是源文档"), "{text}");
        assert!(text.contains("digraph {"), "{text}");
    }

    #[test]
    fn 导出的_dot_能被内置解析器读入() {
        let doc = sample();
        let (text, warnings) = export(&doc).unwrap();
        assert!(!warnings.is_empty());
        let back = Graph::from_dot(&text).unwrap();
        assert_eq!(back.nodes.len(), doc.boxes.len());
        assert_eq!(back.edges.len(), doc.edges.len());
        assert_eq!(back.groups[0].label, "主线");
        // 有向/无向的语义能读回来
        assert!(back.edges.iter().any(|e| e.dir.is_arrow()));
        assert!(back.edges.iter().any(|e| e.dir == Dir::None));
        // 样式、折点也读得回来(位置是尽力而为,不断言相等)
        assert!(back
            .edges
            .iter()
            .any(|e| e.style == EdgeStyle::Dashed && e.via.len() == 2));
        assert!(back
            .nodes
            .iter()
            .any(|n| n.box_style == BoxStyle::Single && n.label == "甲站"));
    }

    #[test]
    fn digraph_里的无向边带_dir_none() {
        let (text, _) = export(&sample()).unwrap();
        assert!(text.contains("dir=\"none\""), "{text}");
        assert!(text.contains("\"b2\" -> \"b3\""), "{text}");
        // 有向边不带 dir
        let line = text
            .lines()
            .find(|l| l.contains("\"b1\" -> \"b2\""))
            .unwrap();
        assert!(!line.contains("dir="), "{line}");
    }

    #[test]
    fn 全无向导出成_graph() {
        let mut d = sample();
        for e in &mut d.edges {
            e.dir = Dir::None;
        }
        let (text, _) = export(&d).unwrap();
        assert!(text.contains("graph {"), "{text}");
        assert!(!text.contains("->"), "{text}");
        assert!(text.contains(" -- "), "{text}");
        assert_eq!(Graph::from_dot(&text).unwrap().edges.len(), d.edges.len());
    }

    #[test]
    fn 四态方向按属性写出() {
        let mut d = sample();
        d.edges[0].dir = Dir::Back;
        d.edges[1].dir = Dir::Both;
        let (text, _) = export(&d).unwrap();
        assert!(text.contains("dir=\"back\""), "{text}");
        assert!(text.contains("dir=\"both\""), "{text}");
        let back = Graph::from_dot(&text).unwrap();
        assert_eq!(back.edges[0].dir, Dir::Back);
        assert_eq!(back.edges[1].dir, Dir::Both);
    }

    /// 点节点写成 shape=point,再读回来还是点节点(默认样式已经是 none 了)
    #[test]
    fn 点节点走_shape_point_往返() {
        let mut d = sample();
        d.boxes[0].style = BoxStyle::Point;
        d.boxes[0].size = (1, 1);
        d.texts[0].at = d.boxes[0].at;
        let (text, _) = export(&d).unwrap();
        assert!(text.contains("shape=\"point\""), "{text}");
        assert!(!text.contains("box=\"point\""), "{text}");
        let d2 = crate::dot::parse_document(&text, ViewState::default(), "dot").unwrap();
        assert_eq!(d2.boxes[0].style, BoxStyle::Point);
    }

    #[test]
    fn 边注释写出去还能读回来() {
        let mut d = sample();
        for (i, text) in [(0usize, "收到 SYN"), (1usize, "say \"hi\" \\")] {
            let b = d.new_edge_label(i).expect("该能建注释框");
            d.set_text(b, text);
        }
        let (text, _) = export(&d).unwrap();
        assert!(text.contains("label=\"收到 SYN\""), "{text}");
        assert!(text.contains(r#"label="say \"hi\" \\""#), "{text}");
        let back = Graph::from_dot(&text).unwrap();
        assert_eq!(back.edges.len(), d.edges.len());
        // 逐字回来:位置、颜色是有损项,标签不是
        let d2 = crate::dot::parse_document(&text, ViewState::default(), "dot").unwrap();
        let note = |doc: &crate::document::Document, e: usize| {
            doc.edge_label_of(&doc.edges[e].id)
                .and_then(|b| doc.text(b).map(|t| t.text.clone()))
        };
        assert_eq!(note(&d2, 0).as_deref(), Some("收到 SYN"));
        assert_eq!(note(&d2, 1).as_deref(), Some("say \"hi\" \\"));
    }

    #[test]
    fn 样式字段写进自定义属性并可再导入() {
        let mut d = sample();
        d.boxes[0].fill = Some((0x01, 0x02, 0x03));
        d.texts[0].align = Align::Center;
        d.edges[0].corner = Corner::Sharp;
        let (text, warnings) = export(&d).unwrap();
        assert!(text.contains("fill=\"#010203\""), "{text}");
        assert!(text.contains("align=\"center\""), "{text}");
        assert!(text.contains("corner=\"sharp\""), "{text}");
        assert!(warnings.iter().any(|w| w.contains("fill")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("align")), "{warnings:?}");
        assert!(
            warnings.iter().any(|w| w.contains("corner")),
            "{warnings:?}"
        );

        let back = crate::dot::parse_document(&text, ViewState::default(), "dot").unwrap();
        assert_eq!(back.boxes[0].fill, Some((0x01, 0x02, 0x03)));
        assert_eq!(back.texts[0].align, Align::Center);
        assert_eq!(back.edges[0].corner, Corner::Sharp);
    }

    #[test]
    fn label_带引号反斜杠与粗体也能读入() {
        let mut b = Graph::builder();
        let a = b.node("他说\"你好\"", Some((0.0, 0.0)));
        let c = b.node("反斜杠\\路径", Some((5.0, 5.0)));
        b.node_box(c, BoxStyle::Single);
        let d = b.node("**粗体**", Some((9.0, 9.0)));
        let grp = b.group("g", None);
        b.edge(a, c, grp, Dir::Forward);
        b.edge(c, d, grp, Dir::None);
        let g = b.build().unwrap();
        let doc = crate::convert::from_graph(&g, ViewState::default(), "dot").unwrap();

        let (text, _) = export(&doc).unwrap();
        let back = Graph::from_dot(&text).unwrap();
        assert_eq!(back.nodes.len(), 3);
        assert_eq!(back.nodes[0].label, "他说\"你好\"");
        assert_eq!(back.nodes[1].label, "反斜杠\\路径");
        assert_eq!(back.nodes[2].label, "**粗体**");
    }
}
