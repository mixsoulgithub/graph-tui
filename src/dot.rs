//! Parser for a `.dot` subset (general graphs, not metro-specific).
//!
//! These forms are recognised; everything else is ignored:
//!   strict? graph|digraph name { ... }      graph type (digraph -> edges carry arrows)
//!   node [k=v, ...];  /  edge [k=v, ...];  default attributes (for later elements only)
//!   "甲" [pos="x,y", label="显示名", terms="a|b"];      node
//!   "甲" [box="single", color="#rrggbb"];                 own box and colour
//!   "甲" [fill="#rrggbb", align="center"];                box fill and text align (custom)
//!   "甲" -- "乙" [color="#rrggbb", group="主线"];        undirected edge
//!   "甲" -> "乙" [color="#rrggbb", group="主线"];        directed edge
//!   "甲" -> "乙" [style="dashed", via="1,2 3,4"];        line style and manual via points
//!   "甲" -> "乙" [dir="none|forward|back|both"];         four directions override defaults
//!   "甲" -> "乙" [corner="rounded|sharp"];                edge corner (custom attribute)
//!   "甲" <- "乙" [..];                                    directed edge, swapped then forward
//!   "甲" "乙" [..];                                       no arrow: direction from graph type
//! Values are UTF-8 strings; inside quotes only `\"` and `\\` are recognised (writing back
//! produces only these). A subgraph is a transparent container: nodes and edges inside are
//! still collected.
//! An undeclared edge endpoint creates an implicit node (graphviz semantics), so
//! `"a" -> "b";` suffices; node order is always first appearance, names are case-sensitive,
//! and one name maps to one node.
//!
//! Two passes: build the node table first (edge endpoints count as appearances), then
//! connect edges; edges are written by name and resolve to Builder indices via that table.

use std::collections::HashMap;

use crate::document::{Document, Rgb, ViewState};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Graph};

use crate::convert::ImportStyles;

/// Attribute table: keeps insertion order; a repeated key keeps its last value.
#[derive(Clone, Debug, Default)]
struct Attrs {
    kv: Vec<(String, String)>,
}

impl Attrs {
    fn get(&self, key: &str) -> Option<&str> {
        self.kv
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.kv.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.kv.push((key.to_string(), value.to_string())),
        }
    }

    /// Fill only missing keys: a node/edge's own attributes take precedence over defaults.
    fn fill_from(&mut self, other: &Attrs) {
        for (k, v) in &other.kv {
            if self.get(k).is_none() {
                self.kv.push((k.clone(), v.clone()));
            }
        }
    }
}

/// Parse-time node: label defaults to the name as display name.
struct RawNode {
    name: String,
    label: Option<String>,
    pos: Option<(f64, f64)>,
    /// Search terms (excluding name).
    terms: Vec<String>,
    /// Box style; None = attribute absent (a later definition can still fill it when names
    /// merge).
    box_style: Option<BoxStyle>,
    color: Option<(u8, u8, u8)>,
    /// Box background fill; None = unfilled or absent.
    fill: Option<Rgb>,
    /// Text horizontal align; None = attribute absent.
    align: Option<Align>,
    shift: Option<(f64, f64)>,
    layer: Option<String>,
    /// `size="WxH"`: box size (written by our own export for manually resized boxes).
    size: Option<(u16, u16)>,
    /// Text colour / background.
    text_color: Option<Rgb>,
    text_bg: Option<Rgb>,
    /// `edge_label="N"`: this node is the edge label of the Nth edge.
    note: Option<usize>,
}

/// Parse-time edge: endpoints are node indices.
struct RawEdge {
    a: usize,
    b: usize,
    dir: Dir,
    style: EdgeStyle,
    color: Option<(u8, u8, u8)>,
    via: Vec<(f64, f64)>,
    group: String,
    corner: Corner,
    layer: String,
    /// Custom port attributes (from_side / to_side).
    from_side: Option<crate::model::Side>,
    to_side: Option<crate::model::Side>,
    /// Edge label (graphviz's label attribute).
    label: Option<String>,
    /// Arrowhead glyph (custom attribute head).
    head: Option<crate::model::ArrowStyle>,
    /// Line background colour.
    bg: Option<Rgb>,
}

pub fn parse(text: &str) -> Result<Graph, String> {
    parse_all(text).map(|(g, _, _)| g)
}

/// `.dot` text -> native document, importing custom style attributes too.
pub fn parse_document(text: &str, view: ViewState, source: &str) -> Result<Document, String> {
    let (g, styles, clusters) = parse_all(text)?;
    let mut doc = crate::convert::from_graph_styles(&g, &styles, view, source)?;
    // `subgraph cluster_x { ... }` -> region (cluster). Members map from .dot node indices
    // to box ids
    if !clusters.is_empty() {
        for (k, c) in clusters.iter().enumerate() {
            let members: Vec<String> = c.nodes.iter().map(|&i| format!("b{}", i + 1)).collect();
            if members.is_empty() {
                continue;
            }
            doc.regions.push(crate::document::Region {
                id: if c.name.is_empty() {
                    format!("r{}", k + 1)
                } else {
                    c.name.clone()
                },
                label: c.label.clone(),
                color: c.color,
                style: c.style.unwrap_or(crate::model::BoxStyle::Rounded),
                layer: c.layer.clone(),
                padding: c.padding.unwrap_or(1),
                members,
                collapsed: c.collapsed,
            });
        }
        // The layer a region (box= or layer=) references may have no nodes/edges, so add it to
        // the layer table; otherwise validate rejects the whole document for an unknown layer
        for c in &clusters {
            if c.layer.is_empty() || doc.layers.iter().any(|l| l.name == c.layer) {
                continue;
            }
            let order = doc.layers.len() as i32;
            doc.layers.push(crate::document::Layer {
                name: c.layer.clone(),
                order,
            });
        }
        doc.rebuild();
        doc.validate()?;
    }
    Ok(doc)
}

/// One cluster in a `.dot`: name (as written, before stripping `cluster_`), label, member
/// node indices.
struct Cluster {
    name: String,
    label: String,
    nodes: Vec<usize>,
    /// The region's own style (our export writes it; `.dot` from elsewhere usually lacks it).
    color: Option<Rgb>,
    style: Option<BoxStyle>,
    padding: Option<u16>,
    collapsed: bool,
    layer: String,
}

/// Scan the source and extract membership of `subgraph cluster_* { ... }`.
/// Only subgraphs starting with `cluster` count; ordinary subgraphs remain transparent.
fn clusters(text: &str, index: &HashMap<String, usize>) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = Vec::new();
    let mut stack: Vec<usize> = Vec::new(); // index in out of the cluster currently being scanned
                                            // Split statements with braces: like
                                            // statements(), just keeping `{` `}` as
                                            // separate statements
    let mut stmts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for line in text.lines() {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if quoted && c == '\\' {
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
                continue;
            }
            if !quoted && c == '/' && chars.peek() == Some(&'/') {
                break;
            }
            match c {
                '"' => {
                    quoted = !quoted;
                    cur.push(c);
                }
                ';' if !quoted => stmts.push(std::mem::take(&mut cur)),
                '{' | '}' if !quoted => {
                    if !cur.trim().is_empty() {
                        stmts.push(std::mem::take(&mut cur));
                    }
                    stmts.push(c.to_string());
                }
                _ => cur.push(c),
            }
        }
        cur.push(' ');
    }
    if !cur.trim().is_empty() {
        stmts.push(cur);
    }

    for stmt in stmts {
        let tok = tokens(&stmt);
        if tok.is_empty() {
            continue;
        }
        match tok[0].as_str() {
            "}" => {
                stack.pop();
                continue;
            }
            "{" => continue,
            "subgraph" => {
                // `subgraph cluster_a {` / `subgraph {`
                if let Some(name) = tok.get(1) {
                    if name.starts_with("cluster") {
                        let id = name.strip_prefix("cluster_").unwrap_or(name).to_string();
                        out.push(Cluster {
                            name: id,
                            label: String::new(),
                            nodes: Vec::new(),
                            color: None,
                            style: None,
                            padding: None,
                            collapsed: false,
                            layer: String::new(),
                        });
                        stack.push(out.len() - 1);
                    }
                }
                continue;
            }
            "node" | "edge" | "graph" | "strict" => {
                // Clusters may set default attributes; label = "..." becomes the cluster title
                if stack.is_empty() {
                    continue;
                }
                if let Some(v) = tok.iter().position(|t| t == "label") {
                    if let Some(eq) = tok.get(v + 1).filter(|t| *t == "=") {
                        let _ = eq;
                        if let Some(raw) = tok.get(v + 2) {
                            if let Some(r) = stack.last() {
                                out[*r].label = raw.trim_matches('"').to_string();
                            }
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
        // Node names in a statement: the first word and the one after the arrow
        let mut names: Vec<&str> = Vec::new();
        if tok.get(1).map(String::as_str) == Some("=") {
            // Graph-level attributes inside a cluster: label is the title, the rest are this
            // project's region style
            let Some(r) = stack.last().copied() else {
                continue;
            };
            let raw = tok.get(2).map(|v| v.trim_matches('"')).unwrap_or("");
            match tok[0].as_str() {
                "label" => out[r].label = raw.to_string(),
                "color" => out[r].color = parse_color(raw),
                "box" => out[r].style = BoxStyle::parse(raw),
                "padding" => out[r].padding = raw.trim().parse().ok(),
                "collapsed" => {
                    out[r].collapsed = matches!(raw, "1" | "true" | "yes" | "on");
                }
                "layer" => out[r].layer = raw.to_string(),
                _ => {}
            }
            continue;
        }
        names.push(tok[0].as_str());
        if let Some(arrow) = tok
            .iter()
            .position(|t| matches!(t.as_str(), "->" | "--" | "<-"))
        {
            if let Some(b) = tok.get(arrow + 1) {
                names.push(b.as_str());
            }
        }
        if stack.is_empty() {
            continue;
        }
        for name in names {
            if let Some(&i) = index.get(name) {
                if let Some(r) = stack.last() {
                    if !out[*r].nodes.contains(&i) {
                        out[*r].nodes.push(i);
                    }
                }
            }
        }
    }
    out
}

/// Dedup key for edges: undirected edges use unordered endpoints, others ordered (same as model).
fn edge_key(a: usize, b: usize, dir: Dir, group: &str) -> (usize, usize, String) {
    let (a, b) = if dir != Dir::None || a < b {
        (a, b)
    } else {
        (b, a)
    };
    (a, b, group.to_string())
}

/// Parse the intermediate graph and style overrides; `parse` uses only the former.
fn parse_all(text: &str) -> Result<(Graph, ImportStyles, Vec<Cluster>), String> {
    let stmts = statements(text);
    let mut digraph = false;
    let mut node_def = Attrs::default();
    let mut edge_def = Attrs::default();
    let mut graph_attrs: Vec<(String, String)> = Vec::new();
    let mut nodes: Vec<RawNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<RawEdge> = Vec::new();
    // strict / graph / digraph in the first non-empty statement sets the graph type.
    // Leading comments or empty statements mean the loop index is not "first"
    let mut first = true;

    for stmt in stmts.iter() {
        let tok = tokens(stmt);
        if tok.is_empty() {
            continue;
        }
        if first {
            first = false;
            if matches!(tok[0].as_str(), "strict" | "graph" | "digraph") {
                digraph = tok.iter().any(|t| t == "digraph");
                continue;
            }
        }
        match tok[0].as_str() {
            // A subgraph is a transparent container: only this statement is consumed; nodes and
            // edges inside are collected by later statements, so no node named subgraph appears
            "subgraph" | "strict" => continue,
            // Default attributes: later nodes/edges fill missing keys from here
            "node" | "edge" => {
                let attrs = parse_attrs(&tok, 1);
                if tok[0] == "node" {
                    node_def = attrs;
                } else {
                    edge_def = attrs;
                }
                continue;
            }
            // Graph-level attributes: `graph [pos_unit="cell"]` and the like are kept verbatim;
            // conversion decides
            "graph" => {
                for (k, v) in parse_attrs(&tok, 1).kv {
                    graph_attrs.retain(|(key, _): &(String, String)| *key != k);
                    graph_attrs.push((k, v));
                }
                continue;
            }
            // Stray punctuation is not a node
            _ if matches!(tok[0].as_str(), "[" | "]" | "=" | "--" | "->" | "<-") => continue,
            // Graph-level assignment statements (rankdir=LR and the like) are not nodes either
            _ if tok.get(1).map(String::as_str) == Some("=") => continue,
            _ => {}
        }
        // Edge: the middle word is the arrow spelling
        if let Some((a, b, dir, at)) = edge_parts(&tok, digraph)? {
            let mut attrs = parse_attrs(&tok, at);
            attrs.fill_from(&edge_def);
            // A bad style errors out instead of silently drawing a solid line
            let style = match attrs.get("style") {
                Some(v) => EdgeStyle::parse(v)
                    .ok_or_else(|| format!("bad edge style {v:?} between {a:?} and {b:?}"))?,
                None => EdgeStyle::Solid,
            };
            // dir overrides the direction from arrow/graph type; unknown values keep the default,
            // never silently changing semantics
            let dir = match attrs.get("dir") {
                Some(v) => Dir::parse(v).unwrap_or(dir),
                None => dir,
            };
            let corner = attrs
                .get("corner")
                .and_then(Corner::parse)
                .unwrap_or_default();
            // group and line both name the group; when both are present, group wins
            let group = attrs
                .get("group")
                .filter(|s| !s.is_empty())
                .or_else(|| attrs.get("line").filter(|s| !s.is_empty()))
                .unwrap_or("");
            // Undeclared endpoints create an implicit node via `"name";`, still in first-appearance
            // order
            let a = touch(&mut nodes, &mut index, &a, &node_def)?;
            let b = touch(&mut nodes, &mut index, &b, &node_def)?;
            let layer = attrs
                .get("layer")
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            // Edge label: multi-line `\n` is already a real newline, which the label box can hold
            // (it grows with the line count); a lone `\r` is rejected
            let label = match attrs.get("label") {
                Some(v) if !v.is_empty() => {
                    if v.contains('\r') {
                        return Err(format!(
                            "an edge label must not contain CR (between {a:?} and {b:?})"
                        ));
                    }
                    Some(v.to_string())
                }
                _ => None,
            };
            edges.push(RawEdge {
                a,
                b,
                dir,
                style,
                color: color_of(&attrs),
                via: attrs.get("via").map(parse_via).unwrap_or_default(),
                group: group.to_string(),
                corner,
                from_side: attrs.get("from_side").and_then(crate::model::Side::parse),
                to_side: attrs.get("to_side").and_then(crate::model::Side::parse),
                layer,
                label,
                head: attrs.get("head").and_then(crate::model::ArrowStyle::parse),
                bg: attrs.get("bg").and_then(parse_color),
            });
            continue;
        }
        // Node: name + optional attributes
        let mut attrs = parse_attrs(&tok, 1);
        attrs.fill_from(&node_def);
        merge_node(&mut nodes, &mut index, raw_of(&tok[0], &attrs)?);
    }

    // Nodes enter the graph in index order, then edges are connected
    let node_styles: Vec<crate::convert::NodeStyle> = nodes
        .iter()
        .map(|n| crate::convert::NodeStyle {
            fill: n.fill,
            align: n.align.unwrap_or_default(),
            text_color: n.text_color,
            text_bg: n.text_bg,
            size: n.size,
            note: n.note,
        })
        .collect();
    // `.dot` from our own export uses box ids as node names (`node_names="id"`); such
    // names are identity only and must not become search terms
    let names_are_ids = graph_attrs
        .iter()
        .any(|(k, v)| k == "node_names" && v == "id");
    let mut b = Graph::builder();
    for node in &nodes {
        let label = node.label.clone().unwrap_or_else(|| node.name.clone());
        let i = b.node(label, node.pos);
        b.node_box(i, node.box_style.unwrap_or_default());
        b.node_color(i, node.color);
        if let Some(sh) = node.shift {
            b.node_shift(i, sh);
        }
        if let Some(l) = &node.layer {
            b.node_layer(i, l.clone());
        }
        let mut terms: Vec<&str> = Vec::new();
        // After the display name becomes label, the original node name is still searchable
        if !names_are_ids {
            if let Some(l) = &node.label {
                if l != &node.name {
                    terms.push(&node.name);
                }
            }
        }
        for t in &node.terms {
            if !terms.contains(&t.as_str()) {
                terms.push(t);
            }
        }
        if !terms.is_empty() {
            b.terms(i, &terms);
        }
    }
    for e in &edges {
        // Group colours always come from the Builder palette: dot's color only means this one
        // edge's colour and must not decide the group colour (otherwise the first edge would
        // infect the whole group). Thus same-group edges writing the same color stay
        // same-coloured.
        let g = b.group(&e.group, None);
        let idx = b.edge(e.a, e.b, g, e.dir);
        b.edge_style(idx, e.style);
        b.edge_color(idx, e.color);
        b.edge_via(idx, e.via.clone());
        if !e.layer.is_empty() {
            b.edge_layer(idx, e.layer.clone());
        }
    }
    for (k, v) in graph_attrs {
        b.attr(k, v);
    }
    let graph = b.build()?;

    // The Builder dedups edges, so each edge's style is re-keyed to the deduped edge; for a
    // given endpoint pair only the first occurrence is kept
    let mut seen: HashMap<(usize, usize, String), crate::convert::EdgeOverride> = HashMap::new();
    for e in &edges {
        seen.entry(edge_key(e.a, e.b, e.dir, &e.group))
            .or_insert_with(|| crate::convert::EdgeOverride {
                corner: e.corner,
                ports: (e.from_side, e.to_side),
                head: e.head,
                bg: e.bg,
                label: e.label.clone(),
            });
    }
    let edge_styles: Vec<crate::convert::EdgeOverride> = graph
        .edges
        .iter()
        .map(|e| {
            let group = &graph.groups[e.group].label;
            seen.get(&edge_key(e.a, e.b, e.dir, group))
                .cloned()
                .unwrap_or_default()
        })
        .collect();

    // Regions (clusters): only `subgraph cluster_*`; names as written in .dot, node indices
    // resolved
    let clusters = clusters(text, &index);
    Ok((
        graph,
        ImportStyles {
            nodes: node_styles,
            edges: edge_styles,
        },
        clusters,
    ))
}

/// Record this name and return its node index: undeclared names create an implicit node
/// via `"name";`, so implicit nodes take the current node defaults like explicit declarations.
fn touch(
    nodes: &mut Vec<RawNode>,
    index: &mut HashMap<String, usize>,
    name: &str,
    def: &Attrs,
) -> Result<usize, String> {
    let mut attrs = Attrs::default();
    attrs.fill_from(def);
    let fresh = raw_of(name, &attrs)?;
    Ok(merge_node(nodes, index, fresh))
}

/// Keep one node per name: a later definition only fills keys the earlier one lacks.
fn merge_node(
    nodes: &mut Vec<RawNode>,
    index: &mut HashMap<String, usize>,
    fresh: RawNode,
) -> usize {
    match index.get(&fresh.name) {
        Some(&i) => {
            let old = &mut nodes[i];
            if old.pos.is_none() {
                old.pos = fresh.pos;
            }
            if old.label.is_none() {
                old.label = fresh.label;
            }
            if old.box_style.is_none() {
                old.box_style = fresh.box_style;
            }
            if old.color.is_none() {
                old.color = fresh.color;
            }
            if old.fill.is_none() {
                old.fill = fresh.fill;
            }
            if old.align.is_none() {
                old.align = fresh.align;
            }
            for t in fresh.terms {
                if !old.terms.contains(&t) {
                    old.terms.push(t);
                }
            }
            i
        }
        None => {
            index.insert(fresh.name.clone(), nodes.len());
            nodes.push(fresh);
            nodes.len() - 1
        }
    }
}

/// Extract everything except the name from the attribute table. A bad box errors
/// instead of silently degrading.
fn raw_of(name: &str, attrs: &Attrs) -> Result<RawNode, String> {
    let box_style = match attrs.get("box") {
        Some(v) => Some(
            BoxStyle::parse(v).ok_or_else(|| format!("bad box style {v:?} on node {name:?}"))?,
        ),
        // graphviz's shape=point / shape=none are also recognised: many .dot files use
        // them to say "point node / just a line of text"
        None => match attrs.get("shape") {
            Some("point") => Some(BoxStyle::Point),
            Some("none") | Some("plaintext") | Some("text") => Some(BoxStyle::None),
            _ => None,
        },
    };
    let mut terms = Vec::new();
    for key in ["terms", "pinyin", "pinyin2"] {
        split_terms(attrs.get(key), &mut terms);
    }
    let shift = attrs.get("shift").and_then(parse_pos);
    Ok(RawNode {
        name: name.to_string(),
        // Use `label` when present; an empty string counts too ("empty text" differs from "absent")
        label: attrs.get("label").map(str::to_string),
        pos: attrs.get("pos").and_then(parse_pos),
        terms,
        box_style,
        color: color_of(attrs),
        fill: attrs.get("fill").and_then(parse_color),
        align: attrs.get("align").and_then(Align::parse),
        shift,
        layer: attrs
            .get("layer")
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        size: attrs.get("size").and_then(parse_size),
        text_color: attrs.get("textcolor").and_then(parse_color),
        text_bg: attrs.get("textbg").and_then(parse_color),
        note: attrs
            .get("edge_label")
            .or_else(|| attrs.get("note"))
            .and_then(|v| v.trim().parse().ok()),
    })
}

/// `"WxH"` -> (width, height); a bad value counts as absent.
fn parse_size(s: &str) -> Option<(u16, u16)> {
    let (w, h) = s.trim().split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Split an edge statement into (start, end, default dir, attr index in tokens);
/// Ok(None) if not an edge.
/// Arrow spelling must match the graph type -- Graphviz is this strict and other tools
/// reject mixes.
/// `<-` swaps endpoints and is then treated as forward.
fn edge_parts(
    tok: &[String],
    digraph: bool,
) -> Result<Option<(String, String, Dir, usize)>, String> {
    let (a, b, dir, at) = match tok.get(1).map(String::as_str) {
        // "--" is always undirected, "->" always directed, "<-" points from right to left
        arrow @ Some("--" | "->" | "<-") => {
            let arrow = arrow.unwrap();
            let wants_directed = arrow != "--";
            if wants_directed != digraph {
                let (kind, want) = if digraph {
                    ("digraph", "-> for a directed edge")
                } else {
                    ("graph", "-- for an undirected edge")
                };
                return Err(format!(
                    "{kind} does not take {arrow}; Graphviz wants {want}"
                ));
            }
            let other = tok.get(2).cloned().unwrap_or_default();
            let dir = if arrow == "--" {
                Dir::None
            } else {
                Dir::Forward
            };
            if arrow == "<-" {
                (other, tok[0].clone(), dir, 3)
            } else {
                (tok[0].clone(), other, dir, 3)
            }
        }
        // Two names only, no arrow: direction comes from the graph type
        Some(t) if t != "[" && t != "=" => {
            let dir = if digraph { Dir::Forward } else { Dir::None };
            (tok[0].clone(), tok[1].clone(), dir, 2)
        }
        _ => return Ok(None),
    };
    // A missing endpoint (e.g. `"甲" --;`) means an incomplete statement; ignore it
    // rather than create a nameless node
    if a.is_empty() || b.is_empty() {
        return Ok(None);
    }
    Ok(Some((a, b, dir, at)))
}

/// Split text into "statements": strip comments, split on ';' outside quotes, braces are
/// separators.
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for line in text.lines() {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            // Inside quotes `\"` and `\\` do not close the string; keep them for
            // tokenising to unescape
            if quoted && c == '\\' {
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
                continue;
            }
            // Inline comment: drop everything after "//" outside quotes to end of line
            if !quoted && c == '/' && chars.peek() == Some(&'/') {
                break;
            }
            match c {
                '"' => {
                    quoted = !quoted;
                    cur.push(c);
                }
                ';' if !quoted => out.push(std::mem::take(&mut cur)),
                '{' | '}' if !quoted => {
                    if !cur.trim().is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            }
        }
        // Always a blank between lines, otherwise two lines would stick into one word
        cur.push(' ');
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Tokenise: strip the shell off quoted strings; '=' '[' ']' ',' each become tokens;
/// '--' '->' '<-' each become tokens (they differ in direction and must not be conflated).
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut quoted = false;
    // Backslash escapes inside quotes: only `\"` and `\\`; other backslashes are kept verbatim
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if escaped {
                // graphviz escapes: \n \l \r are newlines, \\ is a backslash, \" is a quote;
                // other \x keep the backslash (dropping it would turn `\n` into `n`)
                match c {
                    'n' | 'l' | 'r' => buf.push('\n'),
                    '\\' => buf.push('\\'),
                    '"' => buf.push('"'),
                    other => {
                        buf.push('\\');
                        buf.push(other);
                    }
                }
                escaped = false;
            } else {
                match c {
                    '\\' => escaped = true,
                    '"' => {
                        quoted = false;
                        out.push(std::mem::take(&mut buf));
                    }
                    c => buf.push(c),
                }
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '=' | '[' | ']' | ',' => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                if c != ',' {
                    out.push(c.to_string());
                }
            }
            '-' | '<' => {
                let arrow = match (c, chars.peek().copied()) {
                    ('-', Some('-')) => Some("--"),
                    ('-', Some('>')) => Some("->"),
                    ('<', Some('-')) => Some("<-"),
                    _ => None,
                };
                match arrow {
                    Some(op) => {
                        if !buf.is_empty() {
                            out.push(std::mem::take(&mut buf));
                        }
                        chars.next();
                        out.push(op.to_string());
                    }
                    None => buf.push(c),
                }
            }
            c if c.is_whitespace() => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            c => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Parse an attribute table starting at '['; without one the table is empty.
fn parse_attrs(tok: &[String], mut i: usize) -> Attrs {
    let mut attrs = Attrs::default();
    if tok.get(i).map(String::as_str) != Some("[") {
        return attrs;
    }
    i += 1;
    while i < tok.len() {
        match tok[i].as_str() {
            "]" => break,
            key => {
                let mut value = "";
                if tok.get(i + 1).map(String::as_str) == Some("=") {
                    value = tok.get(i + 2).map(String::as_str).unwrap_or("");
                    i += 3;
                } else {
                    i += 1;
                }
                if !key.is_empty() {
                    attrs.set(key, value);
                }
            }
        }
    }
    attrs
}

/// "x,y" -> coordinate; absent or malformed counts as no coordinate.
fn parse_pos(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
}

/// Manual via points: coordinates in pairs, comma or whitespace both separate, empty
/// items dropped. A trailing unpaired coordinate is meaningless and discarded.
fn parse_via(s: &str) -> Vec<(f64, f64)> {
    let nums: Vec<f64> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect();
    let (pairs, _) = nums.as_chunks::<2>();
    pairs.iter().map(|p| (p[0], p[1])).collect()
}

/// Split term attributes on '|': trim, drop empty items, keep one copy of duplicates.
fn split_terms(value: Option<&str>, out: &mut Vec<String>) {
    if let Some(v) = value {
        for part in v.split('|') {
            let part = part.trim();
            if !part.is_empty() && !out.iter().any(|t| t == part) {
                out.push(part.to_string());
            }
        }
    }
}

/// "#rrggbb" -> true colour; '#' optional, incomplete values count as absent.
fn parse_color(v: &str) -> Option<(u8, u8, u8)> {
    let hex = v.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

/// color="#rrggbb" -> true colour; '#' optional, incomplete values count as absent.
fn color_of(attrs: &Attrs) -> Option<(u8, u8, u8)> {
    parse_color(attrs.get("color")?)
}

// In tests, starting from default and overriding a few fields reads better than a long
// struct literal
#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::model::{BoxStyle, EdgeStyle, PALETTE};

    #[test]
    fn parse_nodes_and_edges() {
        let g = parse(
            "// 注释\n\
             graph g {\n\
             node [shape=point];\n\
             edge [color=\"#112233\", line=\"2号线\"];\n\
             \"甲\" [pos=\"10,20\"];\n\
             \"乙\" [pos=\"30.5,-40\"];\n\
             \"甲\" -- \"乙\" [color=\"#df4661\", line=\"1号线\"];\n\
             \"甲\" -- \"乙\";\n\
             }\n",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].label, "甲");
        assert_eq!(g.nodes[0].pos, Some((10.0, 20.0)));
        assert_eq!(g.nodes[1].pos, Some((30.5, -40.0)));
        assert_eq!(g.edges.len(), 2);
        // The edge's own attributes override edge defaults
        assert_eq!(g.groups[g.edges[0].group].label, "1号线");
        assert_eq!(g.edge_color(&g.edges[0]), (0xdf, 0x46, 0x61));
        assert_eq!(g.groups[g.edges[1].group].label, "2号线");
        assert_eq!(g.edge_color(&g.edges[1]), (0x11, 0x22, 0x33));
        // When a group has one edge that wrote its own colour, the group adopts it (the legend
        // needs it); groups fall back to the palette colour only when colours disagree
        assert_eq!(g.group_color(g.edges[0].group), (0xdf, 0x46, 0x61));
        assert_eq!(g.group_color(g.edges[1].group), (0x11, 0x22, 0x33));
    }

    #[test]
    fn node_without_pos_is_none() {
        let g = parse("graph g {\n \"甲\";\n}").unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn edge_without_color_gets_palette_color() {
        let g = parse("graph g { \"甲\"; \"乙\"; \"甲\" -- \"乙\"; }").unwrap();
        assert_eq!(g.groups[g.edges[0].group].color, PALETTE[0]);
    }

    #[test]
    fn group_and_line_both_name_group_with_group_priority() {
        let g = parse(
            "graph g {\n\
             \"甲\"; \"乙\"; \"丙\";\n\
             \"甲\" -- \"乙\" [group=\"主线\", line=\"1号线\"];\n\
             \"乙\" -- \"丙\" [line=\"支线\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.groups[g.edges[0].group].label, "主线");
        assert_eq!(g.groups[g.edges[1].group].label, "支线");
    }

    #[test]
    fn digraph_arrow_edge_is_forward() {
        let g = parse("digraph deps { \"a\"; \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
    }

    #[test]
    fn double_dash_parses_as_undirected() {
        let g = parse("graph g { \"a\" -- \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::None);
        // dir overrides the default from arrow/graph type
        let g = parse("graph g { \"a\" -- \"b\" [dir=\"forward\"]; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
    }

    #[test]
    fn arrow_spelling_must_match_graph_type() {
        // Graphviz's hard rule: digraph accepts only ->, graph only --; mixing is a syntax error
        // (other tools reject it outright, so reject here too, lest the file be readable
        // only by us)
        let e = parse("digraph g { \"a\" -- \"b\"; }").unwrap_err();
        assert!(e.contains("--"), "{e}");
        let e = parse("graph g { \"a\" -> \"b\"; }").unwrap_err();
        assert!(e.contains("->"), "{e}");
        assert!(parse("graph g { \"a\" -- \"b\"; }").is_ok());
        assert!(parse("digraph g { \"a\" -> \"b\"; }").is_ok());
    }

    #[test]
    fn arrowless_edge_direction_follows_graph_type() {
        let g = parse("digraph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
        let g = parse("graph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::None);
    }

    #[test]
    fn left_arrow_points_from_right_to_left() {
        let g = parse("digraph g { \"甲\"; \"乙\"; \"甲\" <- \"乙\"; }").unwrap();
        let e = &g.edges[0];
        assert_eq!(g.nodes[e.a].label, "乙");
        assert_eq!(g.nodes[e.b].label, "甲");
        assert_eq!(e.dir, Dir::Forward);
    }

    #[test]
    fn subgraph_is_transparent_container() {
        let g = parse(
            "graph g {\n\
             subgraph cluster_a {\n\
               \"甲\"; \"乙\";\n\
               \"甲\" -- \"乙\";\n\
             }\n\
             \"丙\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert!(g.nodes.iter().all(|n| n.label != "subgraph"));
        assert!(g.nodes.iter().all(|n| n.label != "cluster_a"));
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn own_attributes_override_defaults() {
        let g = parse(
            "graph g {\n\
             node [pos=\"1,1\", label=\"默认\"];\n\
             edge [group=\"默认组\"];\n\
             \"甲\" [label=\"甲显示\"];\n\
             \"乙\" [pos=\"2,2\"];\n\
             \"甲\" -- \"乙\" [group=\"自己的组\"];\n\
             \"乙\" -- \"甲\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes[0].label, "甲显示");
        assert_eq!(g.nodes[0].pos, Some((1.0, 1.0)));
        assert_eq!(g.nodes[1].pos, Some((2.0, 2.0)));
        assert_eq!(g.groups[g.edges[0].group].label, "自己的组");
        assert_eq!(g.groups[g.edges[1].group].label, "默认组");
    }

    #[test]
    fn label_becomes_display_name_and_name_becomes_term() {
        let g = parse(
            "graph g {\n\
             \"station\" [label=\"站点\", terms=\"a|b\", pinyin=\"zhan dian\", pinyin2=\"cham|\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes[0].label, "站点");
        let terms = &g.nodes[0].terms;
        for t in ["station", "a", "b", "zhan dian", "cham"] {
            assert!(terms.iter().any(|s| s == t), "缺别名 {t}: {terms:?}");
        }
    }

    #[test]
    fn same_name_nodes_merge() {
        let g = parse(
            "graph g {\n\
             \"甲\";\n\
             \"甲\" [pos=\"3,4\", terms=\"jia\"];\n\
             \"乙\";\n\
             \"甲\" -- \"乙\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].label, "甲");
        assert_eq!(g.nodes[0].pos, Some((3.0, 4.0)));
        assert!(g.nodes[0].terms.iter().any(|t| t == "jia"));
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn undeclared_endpoints_create_nodes_in_first_appearance_order() {
        let g = parse("digraph g { \"a\" -> \"b\"; \"b\" -> \"c\"; }").unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].label, "a");
        assert_eq!(g.nodes[1].label, "b");
        assert_eq!(g.nodes[2].label, "c");
        assert_eq!(g.edges.len(), 2);
        // Implicitly created nodes have no coordinate
        assert!(!g.has_pos());
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn declaration_after_edge_still_applies() {
        let g = parse(
            "digraph g {\n\
             \"a\" -> \"b\";\n\
             \"a\" [pos=\"1,2\", terms=\"ay\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].pos, Some((1.0, 2.0)));
        assert!(g.nodes[0].terms.iter().any(|t| t == "ay"));
    }

    #[test]
    fn node_without_style_defaults_to_text_only() {
        // Default style is none: a line of text, width follows the text
        let d = parse_document(
            "graph g { \"甲\" [pos=\"0,0\"]; \"乙\" [pos=\"8,0\"]; \"甲\" -- \"乙\"; }",
            ViewState::default(),
            "dot",
        )
        .unwrap();
        assert_eq!(d.boxes[0].style, BoxStyle::None);
        assert_eq!(d.boxes[0].size.1, 1);
        assert_eq!(d.texts[0].at, d.boxes[0].at);

        // graphviz's shape=point / shape=none are also recognised
        let d = parse_document(
            "graph g { \n\
             node [shape=point]; \"甲\"; \"乙\"; \"甲\" -- \"乙\"; \n\
             }",
            ViewState::default(),
            "dot",
        )
        .unwrap();
        assert_eq!(d.boxes[0].style, BoxStyle::Point);
        assert_eq!(d.boxes[0].size, (1, 1));

        let d = parse_document(
            "graph g { \"甲\" [shape=none]; \"乙\"; \"甲\" -- \"乙\"; }",
            ViewState::default(),
            "dot",
        )
        .unwrap();
        assert_eq!(d.boxes[0].style, BoxStyle::None);
        assert_eq!(d.boxes[1].style, BoxStyle::None);
    }

    #[test]
    fn names_are_case_sensitive() {
        let g = parse("graph g { \"A\"; \"a\"; \"A\" -- \"a\"; }").unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn empty_graph_errors() {
        assert!(parse("graph g { }").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn strict_digraph_is_recognised() {
        let g =
            parse("strict digraph g { \"a\"; \"b\"; \"a\" -> \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn node_box_and_color() {
        let g = parse(
            "graph g {\n\
             node [box=\"single\"];\n\
             \"甲\";\n\
             \"乙\" [box=\"double\", color=\"#0a0b0c\"];\n\
             }",
        )
        .unwrap();
        // Defaults apply; a node's own attributes override them
        assert_eq!(g.nodes[0].box_style, BoxStyle::Single);
        assert_eq!(g.nodes[0].color, None);
        assert_eq!(g.nodes[1].box_style, BoxStyle::Double);
        assert_eq!(g.nodes[1].color, Some((0x0a, 0x0b, 0x0c)));
    }

    #[test]
    fn edge_label_import_and_escaping() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"收到 SYN\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .edge_label_of(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(note.as_deref(), Some("收到 SYN"), "label 变成注释框");
        // Quotes and backslashes are escaped per the rules
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"say \\\"hi\\\" \\\\\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .edge_label_of(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(note.as_deref(), Some("say \"hi\" \\"));
    }

    /// `\n` / `\l` / `\r` are newlines; unknown escapes keep their backslash.
    #[test]
    fn newline_and_unknown_escapes() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"一\\n二\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .edge_label_of(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(note.as_deref(), Some("一\n二"), "\\n 是换行");
        let text = "digraph g {\n\
             \"a\" [box=\"single\", terms=\"x\\qy\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\";\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(
            d.boxes[0].terms,
            vec!["x\\qy".to_string()],
            "未知转义保留反斜杠"
        );
    }

    #[test]
    fn custom_style_attributes_import_with_document() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\", fill=\"#102030\", align=\"center\"];\n\
             \"b\" [box=\"rounded\"];\n\
             \"a\" -> \"b\" [corner=\"sharp\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(d.boxes[0].fill, Some((0x10, 0x20, 0x30)));
        assert_eq!(d.texts[0].align, Align::Center);
        assert_eq!(d.edges[0].corner, Corner::Sharp);
        // Absent items take defaults
        assert_eq!(d.boxes[1].fill, None);
        assert_eq!(d.texts[1].align, Align::Left);
    }

    #[test]
    fn edge_style_and_color_land_on_edge() {
        let g = parse(
            "graph g {\n\
             \"a\"; \"b\";\n\
             \"a\" -- \"b\" [style=\"dashed\", color=\"#df4661\"];\n\
             }",
        )
        .unwrap();
        let e = &g.edges[0];
        assert_eq!(e.style, EdgeStyle::Dashed);
        assert_eq!(e.color, Some((0xdf, 0x46, 0x61)));
        assert_eq!(g.edge_color(e), (0xdf, 0x46, 0x61));
    }

    #[test]
    fn via_accepts_commas_and_spaces() {
        let g = parse(
            "graph g {\n\
             \"a\"; \"b\"; \"c\";\n\
             \"a\" -- \"b\" [via=\"1,2 3,4\"];\n\
             \"b\" -- \"c\" [via=\"5 6, 7 8\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.edges[0].via, vec![(1.0, 2.0), (3.0, 4.0)]);
        assert_eq!(g.edges[1].via, vec![(5.0, 6.0), (7.0, 8.0)]);
    }

    #[test]
    fn all_four_dir_values_recognised() {
        let g = parse(
            "digraph g {\n\
             \"a\" -> \"b\" [dir=\"none\"];\n\
             \"b\" -> \"c\" [dir=\"forward\"];\n\
             \"c\" -> \"d\" [dir=\"back\"];\n\
             \"d\" -> \"e\" [dir=\"both\"];\n\
             }",
        )
        .unwrap();
        let dirs: Vec<Dir> = g.edges.iter().map(|e| e.dir).collect();
        assert_eq!(dirs, vec![Dir::None, Dir::Forward, Dir::Back, Dir::Both]);
    }

    #[test]
    fn same_group_edges_with_same_color_stay_same_colored() {
        let g = parse(
            "graph g {\n\
             \"a\"; \"b\"; \"c\";\n\
             \"a\" -- \"b\" [group=\"主线\", color=\"#112233\"];\n\
             \"b\" -- \"c\" [group=\"主线\", color=\"#112233\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.edges[0].group, g.edges[1].group);
        assert_eq!(g.edge_color(&g.edges[0]), g.edge_color(&g.edges[1]));
    }

    #[test]
    fn bad_box_style_errors() {
        assert!(parse("graph g { \"a\" [box=\"wavy\"]; }").is_err());
    }

    #[test]
    fn bad_edge_style_errors() {
        assert!(parse("graph g { \"a\" -- \"b\" [style=\"wavy\"]; }").is_err());
    }
}

#[cfg(test)]
mod port_tests {
    use super::*;

    /// `graph [..]` is kept verbatim (`pos_unit` is recognised by the conversion layer);
    /// clusters are unaffected
    #[test]
    fn graph_attributes_enter_model() {
        let g = parse("digraph {\n graph [pos_unit=\"cell\", rankdir=\"LR\"];\n \"a\"; \"b\"; \"a\" -> \"b\";\n}").unwrap();
        assert_eq!(g.attr("pos_unit"), Some("cell"));
        assert_eq!(g.attr("rankdir"), Some("LR"));
        assert_eq!(g.attr("nope"), None);
        // Repeated occurrences keep the last value
        let g2 = parse("graph { graph [pos_unit=\"cell\"]; graph [pos_unit=\"pt\"]; a; }").unwrap();
        assert_eq!(g2.attr("pos_unit"), Some("pt"));
        // Absent means empty
        assert!(parse("graph { a; }").unwrap().attrs.is_empty());
    }

    /// Custom port attributes: imported into the document and written back on export
    #[test]
    fn port_round_trip() {
        let text = "digraph g {\n\
             a [label=\"A\", shape=box];\n\
             b [label=\"B\", shape=box];\n\
             a -> b [from_side=\"left\", to_side=\"top\"];\n\
           }\n";
        let doc = parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(doc.edges[0].from_side, Some(crate::model::Side::Left));
        assert_eq!(doc.edges[0].to_side, Some(crate::model::Side::Up));
        let (out, _w) = crate::dot_write::export(&doc).unwrap();
        assert!(out.contains("from_side=\"left\""), "{out}");
        assert!(out.contains("to_side=\"up\""), "{out}");
        // Save as .gtui and read back: ports persist
        let body = doc.save();
        assert!(body.contains("\"version\": 11"), "端口要升 v11");
        let back = crate::document::Document::load(&body).unwrap();
        assert_eq!(back.edges[0].from_side, Some(crate::model::Side::Left));
        assert_eq!(back.edges[0].to_side, Some(crate::model::Side::Up));
        // No attribute: auto
        let plain = parse_document("digraph g { a -> b; }", ViewState::default(), "dot").unwrap();
        assert!(plain.edges[0].from_side.is_none());
        assert!(plain.edges[0].to_side.is_none());
        // Ports are written to .gtui as "auto" strings
        assert!(back.save().contains("\"from_side\": \"left\""));
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    /// `subgraph cluster_x` imports as a region and exports back as `subgraph cluster_x`
    #[test]
    fn cluster_import_export() {
        let text = "digraph g {\n\
             subgraph cluster_setup {\n\
               label = \"setup\";\n\
               a [label=\"A\", shape=box];\n\
             b [label=\"B\", shape=box];\n\
               a -> b;\n\
             }\n\
             c [label=\"C\", shape=box];\n\
             b -> c;\n\
           }\n";
        let doc = parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(doc.regions.len(), 1, "该有一个区域");
        let r = &doc.regions[0];
        assert_eq!(r.id, "setup");
        assert_eq!(r.label, "setup");
        assert_eq!(r.members, vec!["b1", "b2"], "两个成员");
        // Export: writes back subgraph cluster_setup
        let (out, _warns) = crate::dot_write::export(&doc).unwrap();
        assert!(out.contains("subgraph \"cluster_setup\""), "{out}");
        assert!(out.contains("label=\"setup\""), "{out}");
        // Collapsed state can be written and read back
        let mut d2 = doc.clone();
        d2.regions[0].collapsed = true;
        let (out2, _w2) = crate::dot_write::export(&d2).unwrap();
        assert!(out2.contains("collapsed=\"1\""), "{out2}");
        let back = crate::dot::parse_document(&out2, ViewState::default(), "dot").unwrap();
        assert!(back.regions[0].collapsed);
        assert_eq!(back.regions[0].members, vec!["b1", "b2"]);
    }
}
