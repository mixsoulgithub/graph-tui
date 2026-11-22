//! Conversion between the intermediate graph (Graph) of `.dot` / `.canvas` and the native
//! document (Document).
//!
//! Import has two entry points:
//!   - `from_graph` for `.dot`: node coordinates are metres; project them linearly onto cells
//!     using the nominal canvas from `scene::nominal_for`, compressing y by 2:1 and rounding.
//!   - `from_graph_cells` for `.canvas`: coordinates are already cells after the pixel
//!     conversion.
//!
//! Export has a single path, `to_graph`: preserve boxes, text, edges, groups and layers in the
//! Graph as best it can; whatever cannot be expressed is returned to the caller as warnings for
//! the user.

use std::collections::HashSet;

use crate::document::{
    Box as DocBox, DocEdge, Document, Group as DocGroup, Layer, Rgb, Text, ViewState,
};
use crate::model::{Align, BoxStyle, Corner, Graph, PALETTE};
use crate::scene::nominal_for;

/// Forced ports at both ends of an edge (from, to); None = auto
pub type EdgePorts = (Option<crate::model::Side>, Option<crate::model::Side>);

#[derive(Clone, Copy, Debug, Default)]
pub struct NodeStyle {
    /// Box fill
    pub fill: Option<Rgb>,
    /// Text horizontal alignment
    pub align: Align,
    /// Text colour
    pub text_color: Option<Rgb>,
    /// Text background colour
    pub text_bg: Option<Rgb>,
    /// Box size `"WxH"`; derived from the text when absent
    pub size: Option<(u16, u16)>,
    /// Edge label box: bound to edge N (1-based, in file order)
    pub note: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct EdgeOverride {
    /// Corner style
    pub corner: Corner,
    /// Forced ports at both ends
    pub ports: EdgePorts,
    /// Arrowhead glyph
    pub head: Option<crate::model::ArrowStyle>,
    /// Line background colour
    pub bg: Option<Rgb>,
    /// Label text written on the edge (a `.dot` without a label box creates one from it)
    pub label: Option<String>,
}

/// Optional style overrides for import, indexed by Graph node/edge position.
/// Missing entries use the document defaults: `fill` None, `align` Left, `corner` Rounded.
/// Only entry points that can read these fields from the source format fill it in (`.dot`
/// custom attributes).
#[derive(Clone, Debug, Default)]
pub struct ImportStyles {
    /// Per-node style (same index as `Graph::nodes`)
    pub nodes: Vec<NodeStyle>,
    /// Per-edge style (same index as `Graph::edges`)
    pub edges: Vec<EdgeOverride>,
}

/// Projection: map input coordinates to cells. `.dot` metre coordinates need scaling;
/// `.canvas` is already in cells.
#[derive(Clone, Copy)]
enum Projection {
    Geo(Geo),
    Cells,
    /// This project's own `.dot` export: `pos` is the cell at the box centre, y upward
    /// (document coordinates once flipped)
    CentreUp,
}

/// Linear projection parameters for `.dot` coordinates: first translate the bbox top-left to
/// the origin, multiply x by s and y by s/2 (a character cell is one row tall and two columns
/// wide), then flip to screen orientation (y downward).
#[derive(Clone, Copy)]
struct Geo {
    x0: f64,
    /// Top of the bbox (max y); used for the y flip
    y1: f64,
    s: f64,
    ox: f64,
    oy: f64,
}

impl Geo {
    fn point(&self, p: (f64, f64)) -> (f64, f64) {
        (
            self.ox + (p.0 - self.x0) * self.s,
            self.oy + (self.y1 - p.1) * self.s * 0.5,
        )
    }

    fn cell(&self, p: (f64, f64)) -> (i32, i32) {
        let (cx, cy) = self.point(p);
        (round_i32(cx), round_i32(cy))
    }
}

impl Projection {
    fn cell(&self, p: (f64, f64)) -> (i32, i32) {
        match self {
            Projection::Geo(g) => g.cell(p),
            Projection::Cells => (round_i32(p.0), round_i32(p.1)),
            Projection::CentreUp => (round_i32(p.0), round_i32(-p.1)),
        }
    }

    /// Floating-point result of the projection (before rounding): for CentreUp it is the cell
    /// of the box centre
    fn point(&self, p: (f64, f64)) -> (f64, f64) {
        match self {
            Projection::Geo(g) => g.point(p),
            Projection::Cells => p,
            Projection::CentreUp => (p.0, -p.1),
        }
    }

    /// Does the projected cell hold the box centre rather than its top-left?
    fn centre_anchored(&self) -> bool {
        matches!(self, Projection::CentreUp)
    }
}

/// Round finite values; non-finite values fall back to 0 so NaN never becomes a random i32.
fn round_i32(v: f64) -> i32 {
    if v.is_finite() {
        v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
    } else {
        0
    }
}

/// Compute the linear projection from the nominal canvas. An empty point set gets safe
/// degenerate parameters.
fn geo_projection(points: &[(f64, f64)], n: usize) -> Geo {
    let (w, h) = nominal_for(n.max(1));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if points.is_empty() {
        (x0, y0, x1, y1) = (0.0, 0.0, 0.0, 0.0);
    }
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    let cols = (w - 6.0).max(1.0);
    let rows = (h - 4.0).max(1.0);
    // Vertical capacity is rows, and after compression each metre takes only s/2 rows, so the
    // height allows s to grow up to rows*2/gh
    let s = (cols / gw).min(rows * 2.0 / gh);
    Geo {
        x0,
        y1,
        s,
        ox: (w - gw * s) / 2.0,
        oy: (h - gh * s * 0.5) / 2.0,
    }
}

/// When two node centres land on the same cell, walk outward along square rings to the nearest
/// free cell. Only a separation of at least one cell is required; overlap of box extents is out
/// of scope.
fn free_center(start: (i32, i32), used: &mut HashSet<(i32, i32)>) -> (i32, i32) {
    if used.insert(start) {
        return start;
    }
    let mut ring = 1i32;
    loop {
        let mut cand = None;
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let p = (start.0.saturating_add(dx), start.1.saturating_add(dy));
                if !used.contains(&p) {
                    cand = Some(p);
                    break;
                }
            }
            if cand.is_some() {
                break;
            }
        }
        if let Some(p) = cand {
            used.insert(p);
            return p;
        }
        ring = ring.saturating_add(1);
    }
}

/// `.dot` import: one box plus one text per node, coordinates projected onto cells by geo.
pub fn from_graph(g: &Graph, view: ViewState, source: &str) -> Result<Document, String> {
    from_graph_styles(g, &ImportStyles::default(), view, source)
}

/// Same as `from_graph` but with the style overrides read from the source format (box fill,
/// text alignment, edge corners).
pub fn from_graph_styles(
    g: &Graph,
    styles: &ImportStyles,
    view: ViewState,
    source: &str,
) -> Result<Document, String> {
    let points: Vec<(f64, f64)> = g
        .nodes
        .iter()
        .map(|n| n.pos.unwrap_or((0.0, 0.0)))
        .collect();
    // Our own .dot export carries `pos_unit="cell"`: the coordinates are already cells, so
    // restore them verbatim -- no scaling to the nominal canvas and no separating overlapping
    // centres (overlap is what the document really looks like)
    let ours = g.attr("pos_unit") == Some("cell");
    let proj = if ours {
        Projection::CentreUp
    } else {
        Projection::Geo(geo_projection(&points, g.nodes.len()))
    };
    let centers: Vec<(f64, f64)> = if ours {
        points.iter().map(|&p| proj.point(p)).collect()
    } else {
        let mut used = HashSet::new();
        points
            .iter()
            .map(|&p| {
                let (x, y) = free_center(proj.cell(p), &mut used);
                (x as f64, y as f64)
            })
            .collect()
    };
    let doc = build_doc(g, view, source, &centers, proj, styles)?;
    // A .dot without coordinates can only start from a default arrangement: run the layout
    // once on import, otherwise every node piles up at the origin. (Native documents are not
    // relaid out on open; this is a one-shot conversion.)
    if !g.has_pos() {
        return laid_out(doc);
    }
    Ok(doc)
}

/// Run the layout once on import: pick a suitable algorithm and write the integer cell
/// coordinates back.
fn laid_out(mut doc: Document) -> Result<Document, String> {
    let l = crate::layout::pick(&doc);
    crate::layout::apply(&mut doc, l)?;
    Ok(doc)
}

/// `.canvas` import: coordinates are already cells, used directly (rounding only).
pub fn from_graph_cells(g: &Graph, view: ViewState, source: &str) -> Result<Document, String> {
    let proj = Projection::Cells;
    let centers: Vec<(f64, f64)> = g
        .nodes
        .iter()
        .map(|n| proj.point(n.pos.unwrap_or((0.0, 0.0))))
        .collect();
    build_doc(g, view, source, &centers, proj, &ImportStyles::default())
}

/// Conversion shared by both entry points. `centers` are the projected points before rounding
/// (top-left for Geo/Cells, centre for CentreUp).
fn build_doc(
    g: &Graph,
    view: ViewState,
    source: &str,
    centers: &[(f64, f64)],
    proj: Projection,
    styles: &ImportStyles,
) -> Result<Document, String> {
    if g.nodes.is_empty() {
        return Err("cannot import a graph with no nodes".to_string());
    }
    if centers.len() != g.nodes.len() {
        return Err("internal error: center count does not match node count".to_string());
    }

    // Groups: one Graph group maps to one document group. Ids are stably `gN`, order is copied
    let mut groups: Vec<DocGroup> = Vec::with_capacity(g.groups.len().max(1));
    for (i, grp) in g.groups.iter().enumerate() {
        groups.push(DocGroup {
            id: format!("g{i}"),
            label: grp.label.clone(),
            color: grp.color,
        });
    }
    if groups.is_empty() {
        groups.push(DocGroup {
            id: "g0".to_string(),
            label: String::new(),
            color: PALETTE[0],
        });
    }
    // Edges without a group name (and edges whose group index is out of range) fall back to the
    // default group
    let default_group = groups
        .iter()
        .position(|x| x.label.is_empty())
        .map(|i| groups[i].id.clone())
        .unwrap_or_else(|| groups[0].id.clone());

    // Layers: the default layer must exist and come first; the rest follow first appearance
    let mut layer_names: Vec<String> = vec![String::new()];
    for name in g.layers() {
        if !layer_names.contains(&name) {
            layer_names.push(name);
        }
    }
    let layers: Vec<Layer> = layer_names
        .into_iter()
        .enumerate()
        .map(|(order, name)| Layer {
            name,
            order: order as i32,
        })
        .collect();

    let mut boxes: Vec<DocBox> = Vec::with_capacity(g.nodes.len());
    let mut texts: Vec<Text> = Vec::with_capacity(g.nodes.len());
    for (i, node) in g.nodes.iter().enumerate() {
        let st = styles.nodes.get(i).copied().unwrap_or_default();
        let box_id = format!("b{}", i + 1);
        let text_id = format!("t{}", i + 1);
        let c = centers[i];
        // Centre a box of this `size` on the projected point (rounding happens in the last step
        // so an even-width box is not shifted by half a cell through early rounding)
        let centred = |size: (u16, u16)| {
            (
                round_i32(c.0 - (size.0 as f64 - 1.0) / 2.0),
                round_i32(c.1 - (size.1 as f64 - 1.0) / 2.0),
            )
        };
        let corner = (round_i32(c.0), round_i32(c.1));
        // Box size: use an explicit size from `.dot` when present (manually adjusted width and
        // height), otherwise derive it from the text -- a text-only box is "widest line x line
        // count", a boxed one adds one cell per side
        let (lines, widest) = crate::document::text_metrics(&node.label);
        let derived = if matches!(node.box_style, BoxStyle::None) {
            (widest.max(1) as u16, lines as u16)
        } else if node.box_style.is_boxed() {
            ((widest.max(2) + 2).max(4) as u16, (lines + 2) as u16)
        } else {
            (1, 1)
        };
        let size = st.size.unwrap_or(derived);
        // Text-only point/box: the top-left lands on the projected cell (in our own .dot export
        // the projected cell is the centre, so derive the top-left back from it); a boxed box is
        // centred on the projected point
        let at = if node.box_style.is_boxed() || proj.centre_anchored() {
            centred(size)
        } else {
            corner
        };
        let text_at = if node.box_style.is_boxed() {
            (at.0 + 1, at.1 + 1)
        } else {
            at
        };
        boxes.push(DocBox {
            id: box_id.clone(),
            at,
            size,
            style: node.box_style,
            color: node.color,
            fill: st.fill,
            layer: node.layer.clone(),
            terms: node.terms.clone(),
            edge: None,
        });
        texts.push(Text {
            id: text_id,
            text: node.label.clone(),
            at: text_at,
            box_id,
            color: st.text_color,
            bg: st.text_bg,
            align: st.align,
            layer: node.layer.clone(),
        });
    }

    let mut edges: Vec<DocEdge> = Vec::with_capacity(g.edges.len());
    // An edge label becomes a text box bound to this edge: record (edge index, text, colour)
    // first and place them all afterwards
    let mut notes: Vec<(usize, String)> = Vec::new();
    // When a node declares itself an edge label box (`note="N"`), that edge no longer creates
    // one from its label, otherwise the same annotation would produce two boxes
    let claimed: HashSet<usize> = styles.nodes.iter().filter_map(|n| n.note).collect();
    let mut next_id = 1usize;
    for (i, e) in g.edges.iter().enumerate() {
        if e.a >= g.nodes.len() || e.b >= g.nodes.len() {
            continue;
        }
        let group = if e.group < groups.len() {
            groups[e.group].id.clone()
        } else {
            default_group.clone()
        };
        let eo = styles.edges.get(i).cloned().unwrap_or_default();
        let (from_side, to_side) = eo.ports;
        edges.push(DocEdge {
            id: format!("e{next_id}"),
            from: format!("b{}", e.a + 1),
            to: format!("b{}", e.b + 1),
            dir: e.dir,
            group,
            style: e.style,
            color: e.color,
            bg: eo.bg,
            via: e.via.iter().map(|&p| proj.cell(p)).collect(),
            corner: eo.corner,
            layer: e.layer.clone(),
            from_side,
            to_side,
            head: eo.head,
        });
        if !claimed.contains(&(i + 1)) {
            if let Some(text) = eo.label.as_deref().filter(|t| !t.is_empty()) {
                notes.push((edges.len() - 1, text.to_string()));
            }
        }
        next_id += 1;
    }

    // `note="N"`: bind that box to edge N (the members were created above in file order)
    for (i, st) in styles.nodes.iter().enumerate() {
        let Some(n) = st.note else { continue };
        let Some(idx) = n.checked_sub(1).filter(|k| *k < edges.len()) else {
            continue;
        };
        if let Some(bx) = boxes.get_mut(i) {
            bx.edge = Some(edges[idx].id.clone());
        }
    }

    // Edge label: place it one cell right of the midpoint between both ends' centres; the
    // position is computed once and the user moves it afterwards
    for (k, (ei, text)) in notes.into_iter().enumerate() {
        let e = &edges[ei];
        let (Some(a), Some(b)) = (
            boxes.iter().position(|x| x.id == e.from),
            boxes.iter().position(|x| x.id == e.to),
        ) else {
            continue;
        };
        let center = |i: usize| -> (i32, i32) {
            let b = &boxes[i];
            (b.at.0 + b.size.0 as i32 / 2, b.at.1 + b.size.1 as i32 / 2)
        };
        let (ca, cb) = (center(a), center(b));
        let at = ((ca.0 + cb.0) / 2 + 1, (ca.1 + cb.1) / 2);
        let id = format!("b{}", boxes.len() + k + 1);
        let tid = format!("t{}", texts.len() + k + 1);
        let (lines, widest) = crate::document::text_metrics(&text);
        boxes.push(DocBox {
            id: id.clone(),
            at,
            size: (widest.max(1) as u16, lines.max(1) as u16),
            style: BoxStyle::None,
            color: None,
            fill: None,
            layer: e.layer.clone(),
            terms: Vec::new(),
            edge: Some(e.id.clone()),
        });
        texts.push(Text {
            id: tid,
            text,
            at,
            box_id: id,
            color: None,
            bg: None,
            align: Align::Left,
            layer: e.layer.clone(),
        });
    }

    Document::from_parts(source, groups, layers, boxes, texts, edges, view)
}

/// Native document -> intermediate graph (for export). Lossy parts are returned as warnings and
/// do not block the export.
pub fn to_graph(doc: &Document) -> Result<(Graph, Vec<String>), String> {
    if doc.boxes.is_empty() {
        return Err("cannot export a document with no boxes".to_string());
    }
    let mut b = Graph::builder();
    for (i, bx) in doc.boxes.iter().enumerate() {
        let label = doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
        // Node coordinates are the box centre (cells); .dot's pos only expresses it as best it
        // can
        let (cx, cy) = doc.center(i);
        let n = b.node(label, Some((cx, cy)));
        b.node_box(n, bx.style);
        b.node_color(n, bx.color);
        if !bx.terms.is_empty() {
            let terms: Vec<&str> = bx.terms.iter().map(String::as_str).collect();
            b.terms(n, &terms);
        }
        b.node_layer(n, bx.layer.clone());
    }

    // Register groups in document order, remembering each document group's Graph index.
    // Merging groups of the same name is a limitation of Graph itself and matches .dot
    // semantics
    let mut group_of: Vec<usize> = Vec::with_capacity(doc.groups.len());
    for grp in &doc.groups {
        group_of.push(b.group(grp.label.clone(), Some(grp.color)));
    }

    for e in &doc.edges {
        let a = doc
            .box_idx(&e.from)
            .ok_or_else(|| format!("edge {:?}: box {:?} does not exist", e.id, e.from))?;
        let c = doc
            .box_idx(&e.to)
            .ok_or_else(|| format!("edge {:?}: box {:?} does not exist", e.id, e.to))?;
        let gi = doc
            .group_idx(&e.group)
            .ok_or_else(|| format!("edge {:?}: group {:?} does not exist", e.id, e.group))?;
        // Groups of the same name collapse to one index; the first registered one wins
        let model_group = group_of.get(gi).copied().unwrap_or(0);
        let ei = b.edge(a, c, model_group, e.dir);
        b.edge_style(ei, e.style);
        b.edge_color(ei, e.color);
        if !e.via.is_empty() {
            b.edge_via(
                ei,
                e.via.iter().map(|&(x, y)| (x as f64, y as f64)).collect(),
            );
        }
        b.edge_layer(ei, e.layer.clone());
    }

    let graph = b.build()?;
    Ok((graph, warnings()))
}

/// Lossy items always present when exporting `.dot`. Reported one by one on every export; the
/// caller decides how to show them.
fn warnings() -> Vec<String> {
    vec![
        "pos 用单元格单位(框中心,y 向上,pos_unit=\"cell\"),Graphviz 只在 neato -n 下认它"
            .to_string(),
        "group、layer、terms、size、note、head、textcolor 这些是本项目的自定义属性,\
         Graphviz 会忽略或原样保留"
            .to_string(),
        "相机、缩放、光标、图层、朝向这些视图状态无法表达".to_string(),
        "同名分组会合并成一个;手写 .dot 里同名的节点也只有一个".to_string(),
    ]
}

#[cfg(test)]
// Tests build defaults and tweak a few fields, which reads better than a long struct literal.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::document::{Dir, Hits};
    use crate::model::{Align, BoxStyle, Corner, Edge, EdgeStyle};
    use crate::text::plain_width;

    const SAMPLE: &str = r##"digraph {
        "甲" [label="甲站", pos="0,0", box="single", color="#112233", terms="jia|a", layer="one"];
        "乙" [label="乙站", pos="10,10", box="rounded", terms="yi"];
        "丙" [pos="20,-5", layer="two"];
        "甲" -> "乙" [group="主线", style="dashed", color="#df4661", via="3,4 5,6", layer="one"];
        "乙" -> "丙" [group="支线", dir="none"];
    }"##;

    fn sample() -> Document {
        let g = Graph::from_dot(SAMPLE).unwrap();
        from_graph(&g, ViewState::default(), "dot").unwrap()
    }

    #[test]
    fn dot_imports_boxes_text_and_edges() {
        let d = sample();
        assert_eq!(d.boxes.len(), 3);
        assert_eq!(d.texts.len(), 3);
        assert_eq!(d.edges.len(), 2);

        // Boxed node: width is the displayed text width + 2 and at least 4, height fixed at 3;
        // text sits at the box top-left + (1,1)
        assert_eq!(d.boxes[0].style, BoxStyle::Single);
        let w0 = (plain_width("甲站").max(2) + 2).max(4) as u16;
        assert_eq!(d.boxes[0].size, (w0, 3));
        assert_eq!(d.texts[0].text, "甲站");
        assert_eq!(d.texts[0].at, (d.boxes[0].at.0 + 1, d.boxes[0].at.1 + 1));
        assert_eq!(d.boxes[0].color, Some((0x11, 0x22, 0x33)));
        assert!(d.boxes[0].terms.contains(&"jia".to_string()));
        assert!(d.boxes[0].terms.contains(&"甲".to_string()));
        assert_eq!(d.boxes[0].layer, "one");

        // Node without box=: the default style is none (one text line, width follows the text)
        assert_eq!(d.boxes[2].style, BoxStyle::None);
        assert_eq!(d.boxes[2].size.1, 1);
        assert_eq!(d.texts[2].at, d.boxes[2].at);
        assert_eq!(d.boxes[2].layer, "two");

        // Edge: direction, style, colour, via points, layer
        assert_eq!(d.edges[0].dir, Dir::Forward);
        assert_eq!(d.edges[1].dir, Dir::None);
        assert_eq!(d.edges[0].style, EdgeStyle::Dashed);
        assert_eq!(d.edges[0].color, Some((0xdf, 0x46, 0x61)));
        assert_eq!(d.edges[0].via.len(), 2);
        assert_eq!(d.edges[0].layer, "one");
        assert!(d.edges[0].via.iter().all(|&(x, y)| x > 0 && y > 0));

        // Groups and layers
        assert_eq!(d.groups.len(), 2);
        assert_eq!(d.groups[0].label, "主线");
        assert_eq!(d.groups[1].label, "支线");
        assert_eq!(d.edges[0].group, d.groups[0].id);
        assert_eq!(d.edges[1].group, d.groups[1].id);
        assert!(d.layers.iter().any(|l| l.name.is_empty() && l.order == 0));
        assert!(d.layer_names().contains(&"one".to_string()));
        assert!(d.layer_names().contains(&"two".to_string()));
    }

    #[test]
    fn four_way_directions_survive_conversion() {
        let text = r#"digraph {
            "a"; "b"; "c"; "d"; "e";
            "a" -> "b";
            "a" -> "c" [dir="none"];
            "a" -> "d" [dir="back"];
            "a" -> "e" [dir="both"];
        }"#;
        let g = Graph::from_dot(text).unwrap();
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        let dirs: Vec<Dir> = d.edges.iter().map(|e| e.dir).collect();
        assert_eq!(dirs, vec![Dir::Forward, Dir::None, Dir::Back, Dir::Both]);
        // Exporting back to the graph passes the directions through unchanged
        let (back, _) = to_graph(&d).unwrap();
        let dirs: Vec<Dir> = back.edges.iter().map(|e| e.dir).collect();
        assert_eq!(dirs, vec![Dir::Forward, Dir::None, Dir::Back, Dir::Both]);
    }

    #[test]
    fn imported_style_fields_take_defaults() {
        let d = sample();
        assert!(d.boxes.iter().all(|b| b.fill.is_none()));
        assert!(d.texts.iter().all(|t| t.align == Align::Left));
        assert!(d.edges.iter().all(|e| e.corner == Corner::Rounded));
    }

    #[test]
    fn import_can_override_styles() {
        let g = Graph::from_dot(SAMPLE).unwrap();
        let styles = ImportStyles {
            nodes: vec![NodeStyle {
                fill: Some((0x01, 0x02, 0x03)),
                align: Align::Center,
                ..NodeStyle::default()
            }],
            edges: vec![EdgeOverride {
                corner: Corner::Sharp,
                label: Some("收到 SYN".to_string()),
                ..EdgeOverride::default()
            }],
        };
        let d = from_graph_styles(&g, &styles, ViewState::default(), "dot").unwrap();
        assert_eq!(d.boxes[0].fill, Some((0x01, 0x02, 0x03)));
        assert_eq!(d.texts[0].align, Align::Center);
        assert_eq!(d.edges[0].corner, Corner::Sharp);
        let note = d
            .edge_label_of(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(
            note.as_deref(),
            Some("收到 SYN"),
            "label 变成绑在边上的注释框"
        );
        // Entries without an override keep their defaults
        assert_eq!(d.boxes[1].fill, None);
        assert_eq!(d.texts[1].align, Align::Left);
        assert_eq!(d.edges[1].corner, Corner::Rounded);
    }

    #[test]
    fn geometric_projection_yields_integer_separated_cells() {
        let text = r#"graph {
            "a" [pos="0,0"];
            "b" [pos="0,0"];
            "c" [pos="100000,50000"];
            "a" -- "b";
            "b" -- "c";
        }"#;
        let g = Graph::from_dot(text).unwrap();
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        // Point nodes each take one cell and all positions differ (by at least one cell)
        assert_ne!(d.boxes[0].at, d.boxes[1].at);
        assert_ne!(d.boxes[1].at, d.boxes[2].at);
        // The projection lands inside a sensible canvas instead of treating metres as cells
        for b in &d.boxes {
            assert!(b.at.0.abs() < 1000, "{:?}", b.at);
            assert!(b.at.1.abs() < 1000, "{:?}", b.at);
        }
    }

    #[test]
    fn imported_doc_is_searchable_by_text_and_terms() {
        let d = sample();
        assert_eq!(d.search("甲站"), Hits::Boxes(vec![0]));
        assert_eq!(d.search("jia"), Hits::Boxes(vec![0]));
    }

    #[test]
    fn save_load_round_trip_keeps_key_fields() {
        let d = sample();
        let back = Document::load(&d.save()).unwrap();
        assert_eq!(back.boxes, d.boxes);
        assert_eq!(back.texts, d.texts);
        assert_eq!(back.edges, d.edges);
        assert_eq!(back.groups, d.groups);
        assert_eq!(back.layers, d.layers);
    }

    #[test]
    fn self_loop_is_imported() {
        let mut b = Graph::builder();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((5.0, 5.0)));
        let grp = b.group("g", None);
        let e = b.edge(a, c, grp, Dir::Forward);
        let mut g = b.build().unwrap();
        assert_eq!(g.edges.len(), 1);
        g.edges.push(Edge {
            a,
            b: e,
            group: grp,
            dir: Dir::Forward,
            style: EdgeStyle::Solid,
            color: None,
            via: Vec::new(),
            layer: String::new(),
        });
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        assert_eq!(d.edges.len(), 2, "自环也要收进来");
        assert_eq!(d.edges[0].from, "b1");
        assert_eq!(d.edges[0].to, "b2");
        assert_eq!(d.edges[1].from, d.edges[1].to, "第二条是自环");
    }

    /// Our own .dot export carries `pos_unit="cell"`: restored verbatim, the same for negative
    /// coordinates, even widths and boxed/point nodes
    #[test]
    fn exported_dot_restores_coordinates_verbatim() {
        let mut d = Document::scratch();
        // Negative coordinates, an even width (centre lands on half a cell), one of each style
        d.add_box("abcd", (-7, -3), BoxStyle::None).unwrap();
        d.add_box("框", (12, 4), BoxStyle::Single).unwrap();
        // Point nodes are 1x1: `add_box` creates a default box, so switch to point style and
        // place it manually
        let pi = d.add_box("", (0, 0), BoxStyle::Point).unwrap();
        d.boxes[pi].size = (1, 1);
        d.place_box(pi, (30, -20));
        d.rebuild();

        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        assert!(dot.contains("pos_unit=\"cell\""), "{dot}");
        let g = Graph::from_dot(&dot).unwrap();
        let back = from_graph(&g, ViewState::default(), "dot").unwrap();
        assert_eq!(back.boxes.len(), d.boxes.len());
        for (a, b) in d.boxes.iter().zip(back.boxes.iter()) {
            assert_eq!((a.at, a.size, a.style), (b.at, b.size, b.style), "{dot}");
        }
        for (a, b) in d.texts.iter().zip(back.texts.iter()) {
            assert_eq!((a.at, a.text.as_str()), (b.at, b.text.as_str()), "{dot}");
        }
    }

    /// Multi-line text: export writes newlines as `\n` and import derives the box height from
    /// the line count, so nothing fails to fit
    #[test]
    fn multiline_text_round_trip() {
        let mut d = Document::scratch();
        d.add_box("一\n二二\n三三三", (-4, -4), BoxStyle::None)
            .unwrap();
        d.add_box("上\n下", (20, 0), BoxStyle::Single).unwrap();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        assert_eq!(back.boxes.len(), 3, "{dot}");
        for (a, b) in d.boxes.iter().zip(back.boxes.iter()) {
            assert_eq!((a.at, a.size, a.style), (b.at, b.size, b.style), "{dot}");
        }
        for (a, b) in d.texts.iter().zip(back.texts.iter()) {
            assert_eq!((a.at, a.text.as_str()), (b.at, b.text.as_str()), "{dot}");
        }
    }

    /// Boxes with the same name or empty text stay separate: export uses the box id as node name
    /// and passes the text through `label`
    #[test]
    fn same_name_and_empty_text_boxes_stay_separate() {
        let mut d = Document::scratch();
        d.add_box("same", (0, 0), BoxStyle::Single).unwrap();
        d.add_box("same", (20, 0), BoxStyle::Single).unwrap();
        d.add_box("", (40, 0), BoxStyle::None).unwrap();
        d.add_box("", (50, 0), BoxStyle::None).unwrap();
        d.add_box("", (60, 0), BoxStyle::None).unwrap();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        assert!(dot.contains("node_names=\"id\""), "{dot}");
        assert!(dot.contains("\"b2\" [label=\"same\""), "{dot}");
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        // scratch already contains box a, plus the five added here
        assert_eq!(back.boxes.len(), 6, "六框一个都不该少:{dot}");
        let got: Vec<String> = back.texts.iter().map(|t| t.text.clone()).collect();
        assert_eq!(got, vec!["a", "same", "same", "", "", ""], "{dot}");
        // Names are ids only and never become search terms
        assert!(
            back.boxes
                .iter()
                .all(|b| !b.terms.iter().any(|t| t.starts_with('b'))),
            "{dot}"
        );
    }

    /// A hand-written multi-line `.dot` (no size=) derives height from the line count: text only
    /// = line count, boxed = line count + 2
    #[test]
    fn sizeless_multiline_nodes_grow_by_line_count() {
        let text = "digraph { a [label=\"x\\ny\\nz\", pos=\"0,0\"]; \
                    b [label=\"p\\nq\", pos=\"20,10\", box=\"single\"]; }";
        let d = crate::dot::parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(d.boxes[0].size, (1, 3), "三行文字就是三行高");
        assert_eq!(
            d.boxes[1].size,
            (4, 4),
            "带框的多行:文字 + 上下边框(宽最小 4)"
        );
        assert_eq!(d.texts[1].text, "p\nq");
    }

    /// Boxes with manually changed sizes come back verbatim through `size=` (text-only widened,
    /// boxed heightened)
    #[test]
    fn box_size_round_trip() {
        let mut d = Document::scratch();
        d.add_box("x", (0, 0), BoxStyle::None).unwrap();
        let wide = d.boxes.len() - 1;
        d.boxes[wide].size = (12, 1);
        d.add_box("框", (20, 0), BoxStyle::Single).unwrap();
        let tall = d.boxes.len() - 1;
        d.boxes[tall].size = (6, 7);
        d.rebuild();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        assert!(dot.contains("size=\"12x1\""), "{dot}");
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        assert_eq!(back.boxes[wide].size, (12, 1));
        assert_eq!(back.boxes[tall].size, (6, 7));
        assert_eq!(back.boxes[wide].at, d.boxes[wide].at);
    }

    /// Text colour, text background, arrowhead glyph and line background all travel through
    /// custom attributes and read back
    #[test]
    fn text_and_arrow_style_round_trip() {
        let mut d = Document::scratch();
        d.add_box("b", (10, 0), BoxStyle::Single).unwrap();
        d.texts[0].color = Some((0x11, 0x22, 0x33));
        d.texts[0].bg = Some((0x44, 0x55, 0x66));
        d.add_edge("b1", "b2", "g0", Dir::Forward).unwrap();
        d.edges[0].head = Some(crate::model::ArrowStyle::Triangles);
        d.edges[0].bg = Some((0x77, 0x88, 0x99));
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        assert_eq!(back.texts[0].color, Some((0x11, 0x22, 0x33)), "{dot}");
        assert_eq!(back.texts[0].bg, Some((0x44, 0x55, 0x66)), "{dot}");
        assert_eq!(
            back.edges[0].head,
            Some(crate::model::ArrowStyle::Triangles)
        );
        assert_eq!(back.edges[0].bg, Some((0x77, 0x88, 0x99)));
    }

    /// The edge label box is not duplicated: it is a node carrying `note=N`, so that edge no
    /// longer creates one from its label
    #[test]
    fn edge_label_round_trip_does_not_duplicate() {
        let mut d = Document::scratch();
        d.add_box("乙", (14, 0), BoxStyle::Single).unwrap();
        d.add_edge("b1", "b2", "g0", Dir::Forward).unwrap();
        let n = d.new_edge_label(0).expect("新建注释框");
        if let Some(ti) = d.text_of(n) {
            d.texts[ti].text = "收到 SYN".to_string();
        }
        d.fit_box(n);
        d.rebuild();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        assert!(dot.contains("edge_label=\"1\""), "{dot}");
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        assert_eq!(back.boxes.len(), d.boxes.len(), "注释框不该多出一个:{dot}");
        assert_eq!(
            back.boxes[back.boxes.len() - 1].edge,
            Some(back.edges[0].id.clone())
        );
        // A foreign `.dot` (label only) still creates an edge label box
        let foreign = "digraph { a [pos=\"0,0\"]; b [pos=\"10,0\"]; a -> b [label=\"hi\"]; }";
        let fd = crate::dot::parse_document(foreign, ViewState::default(), "dot").unwrap();
        assert_eq!(fd.boxes.len(), 3, "label 该变出一个注释框");
    }

    /// Group colour: when an edge has no colour of its own, export writes the group colour and
    /// import infers it back from "same group, same colour"
    #[test]
    fn group_colour_round_trip() {
        let mut d = Document::scratch();
        d.add_box("乙", (14, 0), BoxStyle::Single).unwrap();
        d.add_edge("b1", "b2", "g0", Dir::Forward).unwrap();
        d.groups[0].color = (0x12, 0x34, 0x56);
        d.groups[0].label = "主线".to_string();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        assert!(dot.contains("color=\"#123456\""), "{dot}");
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        assert_eq!(back.groups[0].color, (0x12, 0x34, 0x56));
        assert_eq!(back.groups[0].label, "主线");
    }

    /// Region colour, border, padding, collapse and layer all go into cluster attributes
    #[test]
    fn region_style_round_trip() {
        let mut d = Document::scratch();
        d.add_box("乙", (14, 0), BoxStyle::Single).unwrap();
        let mut r = crate::document::Region {
            id: "core".to_string(),
            label: "核心".to_string(),
            color: Some((0x0a, 0x0b, 0x0c)),
            style: BoxStyle::Double,
            layer: "hl".to_string(),
            padding: 2,
            members: vec!["b1".to_string(), "b2".to_string()],
            collapsed: true,
        };
        r.members.retain(|m| d.box_idx(m).is_some());
        d.layers.push(crate::document::Layer {
            name: "hl".to_string(),
            order: 1,
        });
        d.regions.push(r);
        d.rebuild();
        let (dot, _w) = crate::dot_write::export(&d).unwrap();
        let back = crate::dot::parse_document(&dot, ViewState::default(), "dot").unwrap();
        let got = back.regions.first().expect("区域该回来:{dot}");
        assert_eq!(got.label, "核心");
        assert_eq!(got.color, Some((0x0a, 0x0b, 0x0c)), "{dot}");
        assert_eq!(got.style, BoxStyle::Double);
        assert_eq!(got.padding, 2);
        assert!(got.collapsed);
        assert_eq!(got.layer, "hl");
        assert_eq!(got.members.len(), 2);
    }

    /// A foreign .dot without pos_unit is still re-projected (scaled to the nominal canvas, y
    /// flipped) and never treated as cells
    #[test]
    fn foreign_dot_coordinates_are_reprojected() {
        let text = "digraph { \"a\" [pos=\"0,0\"]; \"b\" [pos=\"40,20\"]; \"a\" -> \"b\"; }";
        let g = Graph::from_dot(text).unwrap();
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        // The two centres end up far apart (40 x 20 units), so scaling was applied; y was
        // flipped too (b, which is above, stays above)
        let dx = (d.boxes[1].at.0 - d.boxes[0].at.0).abs();
        assert!(dx > 10, "该被放大到画布:{dx}");
        assert!(d.boxes[0].at.1 > d.boxes[1].at.1, "y 向上的 b 该在 a 上面");
    }

    #[test]
    fn to_graph_keeps_box_styles_and_text() {
        let d = sample();
        let (g, warnings) = to_graph(&d).unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.nodes[0].label, "甲站");
        assert_eq!(g.nodes[0].box_style, BoxStyle::Single);
        assert_eq!(g.nodes[0].color, Some((0x11, 0x22, 0x33)));
        assert_eq!(g.nodes[0].layer, "one");
        assert_eq!(g.nodes[0].terms, d.boxes[0].terms);
        assert_eq!(g.edges[0].dir, Dir::Forward);
        assert_eq!(g.edges[1].dir, Dir::None);
        assert_eq!(g.edges[0].via.len(), d.edges[0].via.len());
        assert_eq!(g.groups[0].label, "主线");

        // Every lossy item must be listed individually
        assert!(warnings.iter().any(|w| w.contains("pos")));
        assert!(warnings.iter().any(|w| w.contains("Graphviz 会忽略")));
        assert!(warnings.iter().any(|w| w.contains("缩放")));
        assert!(warnings.iter().any(|w| w.contains("同名分组会合并")));
    }

    #[test]
    fn empty_document_export_fails() {
        let d = Document::from_parts(
            "t",
            vec![DocGroup {
                id: "g0".into(),
                label: String::new(),
                color: PALETTE[0],
            }],
            vec![Layer {
                name: String::new(),
                order: 0,
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ViewState::default(),
        )
        .unwrap();
        assert!(to_graph(&d).is_err());
    }
}
