//! Shape recognition: one scan computes shape metrics used to pick `auto`
//!
//! layout candidates. Metrics only decide whether a candidate is generated; the
//! score picks the winner, and an explicit layout always wins. Every metric is
//! O(n + m) and ignores coordinates.

use std::collections::HashMap;

use crate::document::Document;

/// Metrics from a single scan (mathematical structure only, no domain rules)
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// Box count
    pub n: usize,
    /// Edge count (self-loops excluded)
    pub m: usize,
    /// Weakly connected components
    pub components: usize,
    /// Number of unique undirected edges
    pub undirected: usize,
    /// Self-loop count
    pub loops: usize,
    /// Cycle rank u - n + c (0 = forest)
    pub cyclo: usize,
    /// Tree defect: extra edges beyond a spanning forest
    pub extra: usize,
    /// Maximum undirected degree
    pub max_deg: usize,
    /// Branch nodes (undirected degree > 2)
    pub branch_nodes: usize,
    /// Number of directed edges
    pub arcs: usize,
    /// Whether the directed graph has a cycle (always false when undirected)
    pub has_cycle: bool,
    /// Edge density 2u / (n(n-1))
    pub density: f64,
}

impl Shape {
    /// Pure chain: connected, acyclic, max degree <= 2, at least two boxes
    pub fn is_chain(&self) -> bool {
        self.components == 1 && self.n >= 2 && self.cyclo == 0 && self.max_deg <= 2
    }

    /// Pure cycle: connected, every node has degree 2, edges == nodes
    pub fn is_cycle(&self) -> bool {
        self.components == 1 && self.n >= 3 && self.cyclo == 1 && self.undirected == self.n
    }

    /// Pure tree / forest (acyclic)
    pub fn is_tree(&self) -> bool {
        self.cyclo == 0 && self.n >= 2
    }

    /// Multiple components
    pub fn is_multi(&self) -> bool {
        self.components > 1
    }

    /// Too dense for orthogonal routing. Density alone is not enough: a tree is
    /// inherently 2/n and exceeds 0.25 when small; also require cycle rank >= n/2.
    pub fn is_dense(&self) -> bool {
        self.n >= 4 && self.density > 0.25 && (self.cyclo as f64) >= self.n as f64 / 2.0
    }

    /// Mostly a tree: few extra edges and no directed cycle
    pub fn mostly_tree(&self) -> bool {
        if self.n < 3 || self.has_cycle || self.arcs == 0 {
            return false;
        }
        let budget = ((self.n as f64 * 0.05).ceil() as usize).max(1);
        self.cyclo == 0 && self.extra <= budget
    }

    /// Mostly a chain: few branch nodes and the longest path covers most nodes
    pub fn mostly_chain(&self) -> bool {
        if self.n < 3 || self.components != 1 {
            return false;
        }
        let budget = ((self.n as f64 * 0.1).ceil() as usize).max(1);
        self.branch_nodes <= budget && self.cyclo == 0 && self.max_deg <= 3
    }

    /// One-line description for the status line and reports
    pub fn kind(&self) -> &'static str {
        if self.n == 0 {
            "empty"
        } else if self.is_dense() {
            "dense"
        } else if self.is_cycle() {
            "cycle"
        } else if self.is_chain() {
            "chain"
        } else if self.is_multi() {
            "multi"
        } else if self.is_tree() {
            "tree"
        } else {
            "dag"
        }
    }
}

/// Scan the document and compute its shape metrics
pub fn analyze(doc: &Document) -> Shape {
    let n = doc.boxes.len();
    let mut deg = vec![0usize; n];
    let mut loops = 0usize;
    let mut arcs = 0usize;
    // Deduplicate undirected edges: one entry per unordered endpoint pair.
    let mut seen: HashMap<(usize, usize), ()> = HashMap::new();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            loops += 1;
            continue;
        }
        if e.dir.is_arrow() {
            arcs += 1;
        }
        let key = (a.min(b), a.max(b));
        if seen.insert(key, ()).is_none() {
            deg[a] += 1;
            deg[b] += 1;
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    let undirected = seen.len();
    let components = weak_components(&adj);
    // Cycle rank = u - n + c (0 for a forest, +1 per independent cycle).
    let cyclo = (undirected + components).saturating_sub(n);
    let extra = undirected.saturating_sub(n.saturating_sub(components));
    let max_deg = deg.iter().copied().max().unwrap_or(0);
    let branch_nodes = deg.iter().filter(|&&d| d > 2).count();
    let density = if n > 1 {
        2.0 * undirected as f64 / (n as f64 * (n as f64 - 1.0))
    } else {
        0.0
    };
    let has_cycle = crate::layout::directed_cycle(doc);
    Shape {
        n,
        m: doc.edges.len(),
        components,
        undirected,
        loops,
        cyclo,
        extra,
        max_deg,
        branch_nodes,
        arcs,
        has_cycle,
        density,
    }
}

/// Number of weakly connected components (isolated nodes count as one each)
fn weak_components(adj: &[Vec<usize>]) -> usize {
    let n = adj.len();
    let mut seen = vec![false; n];
    let mut c = 0;
    for s in 0..n {
        if seen[s] {
            continue;
        }
        c += 1;
        let mut stack = vec![s];
        seen[s] = true;
        while let Some(v) = stack.pop() {
            for &w in &adj[v] {
                if !seen[w] {
                    seen[w] = true;
                    stack.push(w);
                }
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BoxStyle;

    /// Build `n` boxes (ids b0..b(n-1)) and edges directly, bypassing add_box ids
    fn doc_of(n: usize, edges: &[(usize, usize, bool)]) -> Document {
        use crate::document::{DocEdge, Group, Layer, Text};
        use crate::model::{Corner, Dir, EdgeStyle};
        let groups = vec![Group {
            id: "g0".into(),
            label: "g".into(),
            color: (200, 200, 200),
        }];
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let mut boxes = Vec::new();
        let mut texts = Vec::new();
        for i in 0..n {
            boxes.push(crate::document::Box {
                id: format!("b{i}"),
                at: (i as i32 * 8, 0),
                size: (6, 3),
                style: BoxStyle::Single,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            });
            texts.push(Text {
                id: format!("t{i}"),
                text: format!("n{i}"),
                at: (i as i32 * 8 + 1, 1),
                box_id: format!("b{i}"),
                color: None,
                bg: None,
                align: Default::default(),
                layer: String::new(),
            });
        }
        let edges: Vec<DocEdge> = edges
            .iter()
            .enumerate()
            .map(|(k, &(a, b, directed))| DocEdge {
                id: format!("e{k}"),
                from: format!("b{a}"),
                to: format!("b{b}"),
                dir: if directed { Dir::Forward } else { Dir::None },
                group: "g0".to_string(),
                style: EdgeStyle::Solid,
                color: None,
                bg: None,
                via: Vec::new(),
                corner: Corner::Rounded,
                layer: String::new(),
                from_side: None,
                to_side: None,
                head: None,
            })
            .collect();
        Document::from_parts(
            "test",
            groups,
            layers,
            boxes,
            texts,
            edges,
            Default::default(),
        )
        .unwrap()
    }

    #[test]
    fn metrics_and_classification() {
        // chain: a-b-c
        let chain = doc_of(3, &[(0, 1, true), (1, 2, true)]);
        let s = analyze(&chain);
        assert_eq!((s.n, s.undirected, s.components), (3, 2, 1));
        assert!(s.is_chain() && s.is_tree() && !s.is_cycle());
        assert_eq!(s.cyclo, 0);
        assert_eq!(s.kind(), "chain");

        // cycle: a-b-c-a
        let cyc = doc_of(3, &[(0, 1, true), (1, 2, true), (2, 0, true)]);
        let s = analyze(&cyc);
        assert!(s.is_cycle(), "{s:?}");
        assert_eq!(s.cyclo, 1);

        // tree: one root with three children (degree 3 > 2, so not a chain)
        let tree = doc_of(4, &[(0, 1, true), (0, 2, true), (0, 3, true)]);
        let s = analyze(&tree);
        assert!(s.is_tree() && !s.is_chain(), "{s:?}");
        assert_eq!(s.branch_nodes, 1);
        assert_eq!(s.kind(), "tree");

        // two components
        let multi = doc_of(4, &[(0, 1, true), (2, 3, true)]);
        let s = analyze(&multi);
        assert_eq!(s.components, 2);
        assert!(s.is_multi());
        assert_eq!(s.kind(), "multi");

        // dense: K4
        let k4 = doc_of(
            4,
            &[
                (0, 1, true),
                (0, 2, true),
                (0, 3, true),
                (1, 2, true),
                (1, 3, true),
                (2, 3, true),
            ],
        );
        let s = analyze(&k4);
        assert!(s.is_dense(), "{s:?}");
        // dense wins over multi/tree: the status line reports dense first
        assert_eq!(s.kind(), "dense");
    }
}
