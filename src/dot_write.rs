//! Export a native document to `.dot`. Explicit export only; not an archive format.
//!
//! Export is best-effort: whatever can be expressed (nodes, edges, direction, line
//! style, via points, colours, groups) is written as-is, and `convert::to_graph`
//! returns a warning for what cannot be expressed or would be lossy. The header states
//! that this is a lossy view exported from `.gtui`, not the source document.
//!
//! Directed/undirected syntax has to match the graph kind: a single edge with an
//! arrowhead (forward/back/both) makes the whole file a digraph; forward is the digraph
//! default and gets no attribute, while the other directions write `[dir=none|back|both]`
//! (Graphviz rejects `--` inside a digraph).
//! `fill` / `align` / `corner` are style attributes of this project, written only for
//! round trips; Graphviz ignores them and each one is called out in the export warnings.

use std::collections::HashMap;

use crate::document::Document;
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Graph};

/// Node name: the box id (`b1`...). `.dot` references nodes by it; text goes in `label`
fn box_name(doc: &Document, i: usize) -> &str {
    doc.boxes.get(i).map(|b| b.id.as_str()).unwrap_or("b?")
}

/// Numeric text for coordinates: `0` must not turn into `-0`; `11.5` is kept as is
fn num(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}

/// Dedupe key for document edges on export, matching Builder: undirected edges use the
/// unordered endpoints, the rest the ordered ones.
fn edge_key(a: usize, b: usize, dir: Dir, group: &str) -> (usize, usize, String) {
    let (a, b) = if dir != Dir::None || a < b {
        (a, b)
    } else {
        (b, a)
    };
    (a, b, group.to_string())
}

/// Document -> `.dot` text plus lossy warnings. The caller reports the warnings
pub fn export(doc: &Document) -> Result<(String, Vec<String>), String> {
    let (g, mut warnings) = crate::convert::to_graph(doc)?;
    // box fill, text align/colour, box size and label binding are custom attributes:
    // written for round trips, so Graphviz sees them as unknown and ignores them
    if doc.boxes.iter().any(|b| b.fill.is_some()) {
        warnings.push("fill 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    if doc.texts.iter().any(|t| t.align != Align::Left) {
        warnings.push("align 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    if doc.edges.iter().any(|e| e.corner != Corner::Rounded) {
        warnings.push("corner 是自定义属性,可能被 Graphviz 忽略".to_string());
    }
    // region in region: only one cluster level is written, so the level whose members
    // are regions is lost
    if doc.regions.iter().any(|r| {
        r.members
            .iter()
            .any(|m| doc.regions.iter().any(|x| &x.id == m))
    }) {
        warnings.push("区域套区域无法表达:嵌在里面的那一层会丢".to_string());
    }
    // a group with no edges has nowhere to carry its colour
    if doc
        .groups
        .iter()
        .any(|grp| !doc.edges.iter().any(|e| e.group == grp.id))
    {
        warnings.push("没有边的分组无法携带配色".to_string());
    }
    Ok((write_graph(&g, doc), warnings))
}

/// Intermediate graph -> `.dot` text. Nodes by index, edges by dedupe key; newer style
/// fields are filled in from the document
fn write_graph(g: &Graph, doc: &Document) -> String {
    let digraph = g.edges.iter().any(|e| e.dir.is_arrow());
    // Builder dedupes edges, so corners are looked up by the deduped key; the first
    // occurrence supplies the style
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
    // edge label: the label box's text is the label (strictly one-to-one, nothing to pick)
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
    // arrowhead style and edge background are looked up per deduped edge too
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
    // ports are looked up per deduped edge: the first occurrence wins
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
    // spell out the pos unit and orientation: one cell = one unit, y up (the old
    // Graphviz convention), values at the box centre; the importer restores them verbatim
    // through pos_unit instead of re-projecting
    out.push_str("  graph [pos_unit=\"cell\", node_names=\"id\"];\n");

    for (i, n) in g.nodes.iter().enumerate() {
        let mut attrs: Vec<String> = Vec::new();
        // node name is the box id (`b1`...): text goes in `label`, so equal labels never
        // merge nodes
        let name = doc.boxes.get(i).map(|b| b.id.as_str()).unwrap_or("b?");
        // always write `label` (may be empty): the name is only an id, label is separate
        attrs.push(format!("label={}", quote(&n.label)));
        if !n.terms.is_empty() {
            attrs.push(format!("terms={}", quote(&n.terms.join("|"))));
        }
        if let Some((x, y)) = n.pos {
            attrs.push(format!("pos={}", quote(&format!("{},{}", num(x), num(-y)))));
        }
        // point nodes use Graphviz's own shape=point (the default style is none, which
        // would not round-trip); other styles go in the custom box=
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
            // box size: `.dot` does not care, but a hand-resized box only round-trips when
            // it is written out
            attrs.push(format!(
                "size={}",
                quote(&format!("{}x{}", b.size.0, b.size.1))
            ));
            // edge label box: which edge it binds to (1-based). On import, edge_label=
            // means no new label is created from the edge's label
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

    // regions (cluster): members are listed by node name again, which Graphviz resolves
    // to the same node
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
        // region style: a foreign .dot without these attributes takes the defaults
        // (rounded, padding 1, not collapsed)
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
        // always write a colour: None means "follow the group colour", so the group colour
        // is written out; importing can then infer the group colour from "every edge in
        // the group shares one colour"
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
        // there is no `--` in a digraph, so undirected edges use dir=none; forward is the
        // default and takes no attribute
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
        // ports are our own attributes: Graphviz does not know them but preserves them
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

/// `[k=v, k=v]`; omitted when there are no attributes
fn tail(attrs: &[String]) -> String {
    if attrs.is_empty() {
        String::new()
    } else {
        format!(" [{}]", attrs.join(", "))
    }
}

/// Quoted string: escape only backslash and double quote (the parser knows no others)
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            // newlines become Graphviz's \n
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Colour -> `#rrggbb`
fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak a default; clearer than a long literal
mod tests {
    use super::*;
    use crate::document::{Dir, Document, ViewState};
    use crate::model::{Align, BoxStyle, Corner, EdgeStyle, Graph};

    /// A document with boxes and points, directed and undirected edges, via points,
    /// layers and groups
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
    fn header_states_the_lossy_view() {
        let (text, _) = export(&sample()).unwrap();
        assert!(text.contains("有损视图"), "{text}");
        assert!(text.contains("不是源文档"), "{text}");
        assert!(text.contains("digraph {"), "{text}");
    }

    #[test]
    fn exported_dot_round_trips_through_the_parser() {
        let doc = sample();
        let (text, warnings) = export(&doc).unwrap();
        assert!(!warnings.is_empty());
        let back = Graph::from_dot(&text).unwrap();
        assert_eq!(back.nodes.len(), doc.boxes.len());
        assert_eq!(back.edges.len(), doc.edges.len());
        assert_eq!(back.groups[0].label, "主线");
        // direction semantics survive
        assert!(back.edges.iter().any(|e| e.dir.is_arrow()));
        assert!(back.edges.iter().any(|e| e.dir == Dir::None));
        // style and via points survive too (positions are best-effort, not asserted)
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
    fn undirected_edge_in_digraph_gets_dir_none() {
        let (text, _) = export(&sample()).unwrap();
        assert!(text.contains("dir=\"none\""), "{text}");
        assert!(text.contains("\"b2\" -> \"b3\""), "{text}");
        // directed edges carry no dir
        let line = text
            .lines()
            .find(|l| l.contains("\"b1\" -> \"b2\""))
            .unwrap();
        assert!(!line.contains("dir="), "{line}");
    }

    #[test]
    fn all_undirected_exports_as_graph() {
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
    fn four_direction_states_become_attrs() {
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

    /// Point nodes are written as shape=point and read back as point nodes (the default
    /// style is none)
    #[test]
    fn point_nodes_round_trip_via_shape_point() {
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
    fn edge_labels_round_trip() {
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
        // round-trips verbatim: positions and colours are the lossy parts, labels are not
        let d2 = crate::dot::parse_document(&text, ViewState::default(), "dot").unwrap();
        let note = |doc: &crate::document::Document, e: usize| {
            doc.edge_label_of(&doc.edges[e].id)
                .and_then(|b| doc.text(b).map(|t| t.text.clone()))
        };
        assert_eq!(note(&d2, 0).as_deref(), Some("收到 SYN"));
        assert_eq!(note(&d2, 1).as_deref(), Some("say \"hi\" \\"));
    }

    #[test]
    fn style_fields_go_to_custom_attrs_and_reimport() {
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
    fn labels_with_quotes_backslashes_and_bold_parse() {
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
