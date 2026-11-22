//! Graph model: nodes, edges, edge groups. Drawing-agnostic; layout and render
//! read only this module.
//!
//! Build a graph with Builder (node index comes back from push). Indices are only
//! stable after build(), so edges address nodes by index, never by name.

use std::collections::HashMap;

/// A set of edges: one metro line / one dependency class / one swim lane.
/// Sets the colour and the "walk the line, previous/next stop" order.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub label: String,
    pub color: (u8, u8, u8),
}

/// Node outline: Point draws a one-cell glyph (`○` / `◎`), every other variant
/// draws a 3-row-tall box with the text inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxStyle {
    /// One cell, no box (called "point" in native documents)
    Point,
    /// No box and no node glyph: the region holds text only. Edges still route
    /// around it but draw no T-junction or arrowhead on the border (there is no
    /// room); the arrowhead lands in the cell just outside. Also the default:
    /// with nothing specified a node is one line of text.
    #[default]
    None,
    Single,
    Rounded,
    Double,
    Heavy,
}

impl BoxStyle {
    pub fn parse(s: &str) -> Option<BoxStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "point" => Some(BoxStyle::Point),
            // empty string means "unspecified": take the default style
            "" => Some(BoxStyle::default()),
            "none" | "off" | "empty" => Some(BoxStyle::None),
            "single" | "box" | "on" => Some(BoxStyle::Single),
            "rounded" => Some(BoxStyle::Rounded),
            "double" => Some(BoxStyle::Double),
            "heavy" | "bold" => Some(BoxStyle::Heavy),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            BoxStyle::Point => "point",
            BoxStyle::None => "none",
            BoxStyle::Single => "single",
            BoxStyle::Rounded => "rounded",
            BoxStyle::Double => "double",
            BoxStyle::Heavy => "heavy",
        }
    }

    /// Whether a box is drawn
    pub fn is_boxed(&self) -> bool {
        matches!(
            self,
            BoxStyle::Single | BoxStyle::Rounded | BoxStyle::Double | BoxStyle::Heavy
        )
    }

    /// Whether this is a one-cell node glyph, i.e. a point node
    pub fn is_point(&self) -> bool {
        matches!(self, BoxStyle::Point)
    }

    /// (horizontal, vertical, top-left, top-right, bottom-left, bottom-right)
    pub fn glyphs(&self) -> (char, char, char, char, char, char) {
        match self {
            BoxStyle::Point | BoxStyle::None => (' ', ' ', ' ', ' ', ' ', ' '),
            BoxStyle::Single => ('─', '│', '┌', '┐', '└', '┘'),
            BoxStyle::Rounded => ('─', '│', '╭', '╮', '╰', '╯'),
            BoxStyle::Double => ('═', '║', '╔', '╗', '╚', '╝'),
            BoxStyle::Heavy => ('━', '┃', '┏', '┓', '┗', '┛'),
        }
    }
}

/// Edge port: force the side to enter and leave on (auto derives it from the two
/// node centres)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "l" | "west" => Some(Side::Left),
            "right" | "r" | "east" => Some(Side::Right),
            "up" | "u" | "top" | "north" => Some(Side::Up),
            "down" | "d" | "bottom" | "south" => Some(Side::Down),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Up => "up",
            Side::Down => "down",
        }
    }

    /// Direction bit (canvas LEFT/RIGHT/UP/DOWN)
    pub fn bit(&self) -> u8 {
        match self {
            Side::Left => 1,
            Side::Right => 2,
            Side::Up => 4,
            Side::Down => 8,
        }
    }

    /// The side facing this one
    pub fn opposite(&self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Up => Side::Down,
            Side::Down => Side::Up,
        }
    }
}

/// Arrow glyph style
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ArrowStyle {
    /// `^ v < >` (default; present in every monospace font)
    #[default]
    Ascii,
    /// `↑ ↓ ← →` (Ambiguous in East Asian Width; some terminals draw them 2 cells wide)
    Arrows,
    /// `▲ ▼ ◀ ▶`
    Triangles,
}

impl ArrowStyle {
    pub fn parse(s: &str) -> Option<ArrowStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ascii" => Some(ArrowStyle::Ascii),
            "arrows" => Some(ArrowStyle::Arrows),
            "triangles" => Some(ArrowStyle::Triangles),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ArrowStyle::Ascii => "ascii",
            ArrowStyle::Arrows => "arrows",
            ArrowStyle::Triangles => "triangles",
        }
    }

    /// Arrow glyph for the direction the edge enters from. Any glyph that is not
    /// exactly one cell wide falls back to ASCII.
    pub fn arrow(&self, side: u8) -> char {
        let ch = match (self, side) {
            (ArrowStyle::Ascii, 4) => 'v',
            (ArrowStyle::Ascii, 8) => '^',
            (ArrowStyle::Ascii, 1) => '>',
            (ArrowStyle::Ascii, 2) => '<',
            (ArrowStyle::Arrows, 4) => '↓',
            (ArrowStyle::Arrows, 8) => '↑',
            (ArrowStyle::Arrows, 1) => '→',
            (ArrowStyle::Arrows, 2) => '←',
            (ArrowStyle::Triangles, 4) => '▼',
            (ArrowStyle::Triangles, 8) => '▲',
            (ArrowStyle::Triangles, 1) => '▶',
            (ArrowStyle::Triangles, 2) => '◀',
            _ => '?',
        };
        if unicode_width::UnicodeWidthChar::width(ch).unwrap_or(2) == 1 {
            ch
        } else {
            match side {
                4 => 'v',
                8 => '^',
                1 => '>',
                _ => '<',
            }
        }
    }
}

/// Edge direction: four states. Arrow glyphs stay `< > ^ v`; there is no other style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dir {
    /// No arrowhead
    None,
    /// Arrowhead at the to end
    #[default]
    Forward,
    /// Arrowhead at the from end
    Back,
    /// Arrowheads at both ends
    Both,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "" => Some(Dir::None),
            "forward" | "->" => Some(Dir::Forward),
            "back" | "<-" => Some(Dir::Back),
            "both" | "<->" => Some(Dir::Both),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Dir::None => "none",
            Dir::Forward => "forward",
            Dir::Back => "back",
            Dir::Both => "both",
        }
    }

    /// (arrowhead at the from end, arrowhead at the to end)
    pub fn heads(&self) -> (bool, bool) {
        match self {
            Dir::None => (false, false),
            Dir::Forward => (false, true),
            Dir::Back => (true, false),
            Dir::Both => (true, true),
        }
    }

    /// Whether the edge counts as directed (used by layering and cycle checks)
    pub fn is_arrow(&self) -> bool {
        !matches!(self, Dir::None)
    }
}

/// Horizontal align of the text inside the box (point nodes ignore it)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn parse(s: &str) -> Option<Align> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "" => Some(Align::Left),
            "center" | "centre" => Some(Align::Center),
            "right" => Some(Align::Right),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// Edge corner: rounded `╭╮╯╰` or sharp `┌┐└┘`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corner {
    #[default]
    Rounded,
    Sharp,
}

impl Corner {
    pub fn parse(s: &str) -> Option<Corner> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rounded" | "" => Some(Corner::Rounded),
            "sharp" => Some(Corner::Sharp),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Corner::Rounded => "rounded",
            Corner::Sharp => "sharp",
        }
    }
}

/// Edge line style. Dashed/dotted only break straight runs; corners and crossings
/// stay solid (otherwise the turn direction is unreadable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl EdgeStyle {
    pub fn parse(s: &str) -> Option<EdgeStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "solid" | "" => Some(EdgeStyle::Solid),
            "dashed" | "dash" => Some(EdgeStyle::Dashed),
            "dotted" | "dot" => Some(EdgeStyle::Dotted),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            EdgeStyle::Solid => "solid",
            EdgeStyle::Dashed => "dashed",
            EdgeStyle::Dotted => "dotted",
        }
    }

    /// Whether this cell is painted (dashed skips every other cell, dotted every
    /// third). The caller keeps corners solid.
    pub fn keeps(&self, x: i32, y: i32) -> bool {
        match self {
            EdgeStyle::Solid => true,
            EdgeStyle::Dashed => (x + y).rem_euclid(2) == 0,
            EdgeStyle::Dotted => (x + y).rem_euclid(3) != 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub group: usize,
    /// Direction (four states)
    pub dir: Dir,
    pub style: EdgeStyle,
    /// Colour of this edge alone; None = use the group colour
    pub color: Option<(u8, u8, u8)>,
    /// Manual via points (canvas coords): the edge runs a -> via.. -> b, turning at
    /// each point
    pub via: Vec<(f64, f64)>,
    /// Layer name (empty = default layer). Painting stacks layers in order; editing
    /// can show or act on one layer only
    pub layer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub label: String,
    /// Coordinates: x right, y up. Units arbitrary (only relative positions
    /// matter). None = this node has no coordinates and layout must compute them
    pub pos: Option<(f64, f64)>,
    /// Search aliases (pinyin, abbreviation, English name), tried in order
    pub terms: Vec<String>,
    /// Outline style; None = no box
    pub box_style: BoxStyle,
    /// Colour of this node alone (box and text); None = use the group colour
    pub color: Option<(u8, u8, u8)>,
    /// Manual offset (canvas coords) added to the layout position; dragging edits it
    pub shift: (f64, f64),
    /// Layer name (empty = default layer)
    pub layer: String,
}

impl Node {
    pub fn new(label: impl Into<String>, pos: Option<(f64, f64)>) -> Node {
        Node {
            label: label.into(),
            pos,
            terms: Vec::new(),
            box_style: BoxStyle::default(),
            color: None,
            shift: (0.0, 0.0),
            layer: String::new(),
        }
    }

    /// Footprint (character cells): 1x1 without a box, (text width + 2, 3) with one
    pub fn size(&self) -> (usize, usize) {
        match self.box_style {
            BoxStyle::Point => (1, 1),
            _ => (crate::text::plain_width(&self.label).max(2) + 2, 3),
        }
    }
}

/// Handed out in this order when no colour is given
pub const PALETTE: [(u8, u8, u8); 12] = [
    (0x4c, 0x8e, 0xf8),
    (0xef, 0x4b, 0x4b),
    (0x22, 0xb4, 0x6a),
    (0xf5, 0x9e, 0x0b),
    (0xa8, 0x55, 0xf7),
    (0x14, 0xb8, 0xa6),
    (0xf4, 0x72, 0xb6),
    (0xa3, 0xe6, 0x35),
    (0xfb, 0x92, 0x3c),
    (0x60, 0xa5, 0xfa),
    (0x8d, 0x6e, 0x63),
    (0x78, 0x90, 0x9c),
];

/// Graph builder: create nodes and groups first, keep their indices, then connect
/// edges
pub struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    groups: Vec<Group>,
    /// Graph-level attributes (`graph [k=v]`), carried verbatim; the reader decides
    /// which ones it honours
    attrs: Vec<(String, String)>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        Builder {
            nodes: Vec::new(),
            edges: Vec::new(),
            groups: Vec::new(),
            attrs: Vec::new(),
        }
    }

    /// Add a node, returning its index
    pub fn node(&mut self, label: impl Into<String>, pos: Option<(f64, f64)>) -> usize {
        self.nodes.push(Node::new(label, pos));
        self.nodes.len() - 1
    }

    /// Set a graph-level attribute; the last write wins for the same key
    pub fn attr(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let (key, value) = (key.into(), value.into());
        match self.attrs.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((key, value)),
        }
    }

    /// Set search aliases (pinyin / abbreviation / English name)
    pub fn terms(&mut self, i: usize, terms: &[&str]) {
        self.nodes[i].terms = terms.iter().map(|s| s.to_string()).collect();
    }

    /// Find a group with the same label, creating one if absent. The group created
    /// first keeps its colour
    pub fn group(&mut self, label: impl Into<String>, color: Option<(u8, u8, u8)>) -> usize {
        let label = label.into();
        if let Some(i) = self.groups.iter().position(|g| g.label == label) {
            return i;
        }
        let color = color.unwrap_or(PALETTE[self.groups.len() % PALETTE.len()]);
        self.groups.push(Group { label, color });
        self.groups.len() - 1
    }

    /// Add an edge, returning its index (later tuned via edge_style / edge_via)
    pub fn edge(&mut self, a: usize, b: usize, group: usize, dir: Dir) -> usize {
        self.edges.push(Edge {
            a,
            b,
            group,
            dir,
            style: EdgeStyle::Solid,
            color: None,
            via: Vec::new(),
            layer: String::new(),
        });
        self.edges.len() - 1
    }

    /// Node outline (pass BoxStyle::None for no box)
    pub fn node_box(&mut self, i: usize, box_style: BoxStyle) {
        self.nodes[i].box_style = box_style;
    }

    /// Colour of this node alone
    pub fn node_color(&mut self, i: usize, color: Option<(u8, u8, u8)>) {
        self.nodes[i].color = color;
    }

    pub fn edge_style(&mut self, e: usize, style: EdgeStyle) {
        self.edges[e].style = style;
    }

    pub fn edge_color(&mut self, e: usize, color: Option<(u8, u8, u8)>) {
        self.edges[e].color = color;
    }

    /// Manual via points: the edge runs a -> via.. -> b
    pub fn edge_via(&mut self, e: usize, via: Vec<(f64, f64)>) {
        self.edges[e].via = via;
    }

    /// Manual offset for the node, added to the layout position
    pub fn node_shift(&mut self, i: usize, shift: (f64, f64)) {
        self.nodes[i].shift = shift;
    }

    pub fn node_layer(&mut self, i: usize, layer: impl Into<String>) {
        self.nodes[i].layer = layer.into();
    }

    pub fn edge_layer(&mut self, e: usize, layer: impl Into<String>) {
        self.edges[e].layer = layer.into();
    }

    /// Finish: validate, drop duplicate edges, derive structures (adjacency, node
    /// groups, bbox, node series per group)
    pub fn build(mut self) -> Result<Graph, String> {
        if self.nodes.is_empty() {
            return Err("the graph has no nodes".to_string());
        }
        let n = self.nodes.len();
        for e in &self.edges {
            if e.a >= n || e.b >= n {
                return Err(format!("edge refers to a missing node {} -- {}", e.a, e.b));
            }
            if e.group >= self.groups.len() {
                return Err(format!("edge refers to a missing group {}", e.group));
            }
        }
        if self.groups.is_empty() {
            self.groups.push(Group {
                label: String::new(),
                color: PALETTE[0],
            });
        }
        // Keep one copy per (endpoints, group). Directed edges dedupe on the ordered
        // endpoints: a->b and b->a are distinct (a 2-cycle), not duplicates
        let mut seen: HashMap<(usize, usize, usize), ()> = HashMap::new();
        self.edges.retain(|e| {
            let (a, b) = if e.dir != Dir::None || e.a < e.b {
                (e.a, e.b)
            } else {
                (e.b, e.a)
            };
            seen.insert((a, b, e.group), ()).is_none()
        });

        let mut node_groups = vec![Vec::new(); n];
        let mut adj = vec![Vec::new(); n];
        for e in &self.edges {
            for (me, other) in [(e.a, e.b), (e.b, e.a)] {
                if !node_groups[me].contains(&e.group) {
                    node_groups[me].push(e.group);
                }
                adj[me].push((other, e.group));
            }
        }
        for v in node_groups.iter_mut() {
            v.sort_unstable();
        }

        let mut bbox = None;
        if self.nodes.iter().all(|nd| nd.pos.is_some()) {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in self.nodes.iter().filter_map(|nd| nd.pos) {
                x0 = x0.min(p.0);
                y0 = y0.min(p.1);
                x1 = x1.max(p.0);
                y1 = y1.max(p.1);
            }
            bbox = Some((x0, y0, x1, y1));
        }

        // Group colour: when every edge of a group carries the same edge colour, adopt
        // it as the group colour. The legend in the adjust-layer view and ":config" ask
        // "what colour is this group", but color is a per-edge attribute, so without this
        // fold the legend has no colour to show
        for gi in 0..self.groups.len() {
            let mut seen: Option<(u8, u8, u8)> = None;
            let mut same = true;
            for e in self.edges.iter().filter(|e| e.group == gi) {
                match e.color {
                    Some(c) if seen.is_none() => seen = Some(c),
                    Some(c) if seen != Some(c) => same = false,
                    Some(_) => {}
                    None => same = false,
                }
            }
            if same {
                if let Some(c) = seen {
                    self.groups[gi].color = c;
                }
            }
        }

        let mut graph = Graph {
            nodes: self.nodes,
            edges: self.edges,
            groups: self.groups,
            attrs: self.attrs,
            node_groups,
            adj,
            bbox,
            series: Vec::new(),
        };
        graph.series = graph.build_series();
        Ok(graph)
    }
}

/// One graph. Nodes are referenced by index; edges hang off groups
#[derive(Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    /// Graph-level attributes (`graph [k=v]`); collected when reading `.dot` and
    /// written back verbatim
    pub attrs: Vec<(String, String)>,
    /// Groups each node passes through (ascending, deduped)
    pub node_groups: Vec<Vec<usize>>,
    /// Neighbours of each node: (neighbour, group)
    pub adj: Vec<Vec<(usize, usize)>>,
    /// bbox (x0, y0, x1, y1) present only when every node has coordinates
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// Node series walked per group (may split into several at forks)
    series: Vec<(usize, Vec<usize>)>,
}

impl Graph {
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Build a graph from .dot text (see dot.rs)
    pub fn from_dot(text: &str) -> Result<Graph, String> {
        crate::dot::parse(text)
    }

    /// Hub: a node on two or more groups (a metro interchange)
    pub fn hub(&self, i: usize) -> bool {
        self.node_groups[i].len() > 1
    }

    pub fn degree(&self, i: usize) -> usize {
        self.adj[i].len()
    }

    /// Look up a graph-level attribute
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Do all nodes have coordinates?
    pub fn has_pos(&self) -> bool {
        self.bbox.is_some()
    }

    pub fn pos(&self, i: usize) -> (f64, f64) {
        self.nodes[i].pos.unwrap_or((0.0, 0.0))
    }

    pub fn group_color(&self, g: usize) -> (u8, u8, u8) {
        self.groups[g].color
    }

    /// Colour this edge actually uses: its own if set, otherwise the group's
    pub fn edge_color(&self, e: &Edge) -> (u8, u8, u8) {
        e.color.unwrap_or_else(|| self.groups[e.group].color)
    }

    /// Colour this node actually uses: its own, else its first group's; None if neither
    pub fn node_color(&self, i: usize) -> Option<(u8, u8, u8)> {
        self.nodes[i]
            .color
            .or_else(|| self.node_groups[i].first().map(|g| self.groups[*g].color))
    }

    /// Layers seen in the graph, in first-appearance order; the default layer ("")
    /// comes first
    pub fn layers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for node in &self.nodes {
            let l = &node.layer;
            if !out.contains(l) {
                out.push(l.clone());
            }
        }
        for e in &self.edges {
            if !out.contains(&e.layer) {
                out.push(e.layer.clone());
            }
        }
        out.sort_by_key(|l| !l.is_empty());
        out
    }

    /// Footprint width of the widest node (layout spaces by it so boxes never overlap)
    pub fn max_box_w(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| n.size().0)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// Node series walked per group: (group index, node series).
    /// Computed once at build time and cached: rendering needs it every frame (to keep
    /// direction continuous at corners), and recomputing it per frame is waste.
    pub fn series(&self) -> &[(usize, Vec<usize>)] {
        &self.series
    }

    /// Chain each group's edges into paths, splitting at forks. A cyclic component has
    /// no node of degree != 2, so as a fallback the walk starts at any node and closes
    /// the cycle
    fn build_series(&self) -> Vec<(usize, Vec<usize>)> {
        let mut out = Vec::new();
        for g in 0..self.groups.len() {
            let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
            for e in self.edges.iter().filter(|e| e.group == g) {
                adj.entry(e.a).or_default().push(e.b);
                adj.entry(e.b).or_default().push(e.a);
            }
            let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
            let mut visited: std::collections::HashSet<(usize, usize)> =
                std::collections::HashSet::new();
            // nodes of degree != 2 are natural path ends, so start there first
            let mut starts: Vec<usize> = adj
                .iter()
                .filter(|(_, v)| v.len() != 2)
                .map(|(k, _)| *k)
                .collect();
            starts.sort_unstable();
            // Fallback starts must be ordered too: when the whole component is a cycle of
            // degree 2, starts is empty and HashMap iteration order would give the same
            // graph different series orderings
            let mut all: Vec<usize> = adj.keys().copied().collect();
            all.sort_unstable();
            for &start in starts.iter().chain(all.iter()) {
                for &nb in &adj[&start] {
                    if visited.contains(&key(start, nb)) {
                        continue;
                    }
                    let mut path = vec![start, nb];
                    visited.insert(key(start, nb));
                    let (mut prev, mut cur) = (start, nb);
                    while let Some(&next) = adj[&cur]
                        .iter()
                        .find(|&&n| n != prev && !visited.contains(&key(cur, n)))
                    {
                        visited.insert(key(cur, next));
                        path.push(next);
                        prev = cur;
                        cur = next;
                    }
                    if path.len() > 1 {
                        out.push((g, path));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arrow glyphs: three styles, anything not 1 cell wide falls back to ASCII
    #[test]
    fn arrow_styles() {
        assert_eq!(ArrowStyle::Ascii.arrow(4), 'v');
        assert_eq!(ArrowStyle::Ascii.arrow(8), '^');
        assert_eq!(ArrowStyle::Ascii.arrow(1), '>');
        assert_eq!(ArrowStyle::Ascii.arrow(2), '<');
        assert_eq!(ArrowStyle::Arrows.arrow(2), '←');
        assert_eq!(ArrowStyle::Triangles.arrow(4), '▼');
        for st in [ArrowStyle::Arrows, ArrowStyle::Triangles] {
            for side in [1u8, 2, 4, 8] {
                let ch = st.arrow(side);
                assert_eq!(
                    unicode_width::UnicodeWidthChar::width(ch),
                    Some(1),
                    "{} 的 {ch:?} 该是一格宽",
                    st.name()
                );
            }
        }
        assert_eq!(ArrowStyle::parse("ASCII"), Some(ArrowStyle::Ascii));
        assert_eq!(ArrowStyle::parse("nope"), None);
    }

    fn sample() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((1.0, 0.0)));
        let d = b.node("c", Some((2.0, 0.0)));
        let g = b.group("主线", Some((0xff, 0, 0)));
        b.edge(a, c, g, Dir::None);
        b.edge(c, d, g, Dir::None);
        b.build().unwrap()
    }

    #[test]
    fn build_derives_adjacency_and_groups() {
        let g = sample();
        assert_eq!(g.adj[1].len(), 2);
        assert!(!g.hub(1));
        assert_eq!(g.series().len(), 1);
        assert_eq!(g.series()[0].1, vec![0, 1, 2]);
        assert_eq!(g.bbox, Some((0.0, 0.0, 2.0, 0.0)));
    }

    #[test]
    fn missing_coords_mean_no_bbox() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", Some((1.0, 0.0)));
        let g = b.group("g", None);
        b.edge(a, c, g, Dir::None);
        let g = b.build().unwrap();
        assert!(!g.has_pos());
    }

    #[test]
    fn self_loops_are_accepted() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let g = b.group("g", None);
        b.edge(a, a, g, Dir::None);
        let g = b.build().expect("自环该收下");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].a, g.edges[0].b);
    }

    #[test]
    fn duplicate_edges_keep_one() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g = b.group("g", None);
        b.edge(a, c, g, Dir::None);
        b.edge(c, a, g, Dir::None);
        let g = b.build().unwrap();
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn opposite_directed_edges_stay_distinct() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g = b.group("g", None);
        b.edge(a, c, g, Dir::Forward);
        b.edge(c, a, g, Dir::Forward);
        let g = b.build().unwrap();
        assert_eq!(g.edges.len(), 2);
        // adj records both ends of every edge, so two parallel edges are recorded twice
        // for this node pair
        assert_eq!(g.adj[0].len(), 2);
        assert_eq!(g.adj[1].len(), 2);
    }

    /// When every edge of a group shares one colour, the group adopts it (for the legend)
    #[test]
    fn group_adopts_uniform_edge_color() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let d = b.node("c", None);
        let g = b.group("g", None);
        let e1 = b.edge(a, c, g, Dir::None);
        let e2 = b.edge(c, d, g, Dir::None);
        b.edge_color(e1, Some((1, 2, 3)));
        b.edge_color(e2, Some((1, 2, 3)));
        let g = b.build().unwrap();
        assert_eq!(g.groups[0].color, (1, 2, 3), "组内一致就该采纳");

        // Mixed edge colours leave the palette colour in place
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let d = b.node("c", None);
        let g = b.group("g", None);
        let e1 = b.edge(a, c, g, Dir::None);
        let e2 = b.edge(c, d, g, Dir::None);
        b.edge_color(e1, Some((1, 2, 3)));
        b.edge_color(e2, Some((4, 5, 6)));
        let g = b.build().unwrap();
        assert_eq!(g.groups[0].color, PALETTE[0]);
    }

    #[test]
    fn uncoloured_groups_follow_the_palette() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g1 = b.group("g1", None);
        let g2 = b.group("g2", None);
        b.edge(a, c, g1, Dir::None);
        b.edge(a, c, g2, Dir::None);
        let g = b.build().unwrap();
        assert_eq!(g.groups[0].color, PALETTE[0]);
        assert_eq!(g.groups[1].color, PALETTE[1]);
        assert!(g.hub(0) && g.hub(1));
    }
}

#[cfg(test)]
mod default_style_tests {
    use super::*;

    #[test]
    fn default_box_style_is_text_only() {
        assert_eq!(BoxStyle::default(), BoxStyle::None);
        assert_eq!(BoxStyle::parse(""), Some(BoxStyle::None));
        assert_eq!(Node::new("a", None).box_style, BoxStyle::None);
    }
}
