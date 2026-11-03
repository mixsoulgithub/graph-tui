//! 结构识别:一次扫描算出图的"形状指标",给 `auto` 布局挑候选.
//!
//! 判定只决定"要不要生成某个候选",最终由候选评分决定;显式写法永远优先.
//! 所有指标 O(n + m),不看坐标.

use std::collections::HashMap;

use crate::document::Document;

/// 一次扫描得到的结构指标(只算数学结构,不掺领域语义)
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// 框数
    pub n: usize,
    /// 边数(不含自环)
    pub m: usize,
    /// 弱连通块数
    pub components: usize,
    /// 无向唯一边数
    pub undirected: usize,
    /// 自环数
    pub loops: usize,
    /// 环秩 u - n + c(0 = 森林)
    pub cyclo: usize,
    /// 树缺陷:多出来的边数
    pub extra: usize,
    /// 最大无向度
    pub max_deg: usize,
    /// 分支点数(无向度 > 2)
    pub branch_nodes: usize,
    /// 有向边的条数
    pub arcs: usize,
    /// 有向图里是否有环(无向图恒为 false)
    pub has_cycle: bool,
    /// 边密度 2u / (n(n-1))
    pub density: f64,
}

impl Shape {
    /// 纯链:连通、无环、最大度 <= 2、至少两个框
    pub fn is_chain(&self) -> bool {
        self.components == 1 && self.n >= 2 && self.cyclo == 0 && self.max_deg <= 2
    }

    /// 纯环:连通、每个点度 2、边数等于点数
    pub fn is_cycle(&self) -> bool {
        self.components == 1 && self.n >= 3 && self.cyclo == 1 && self.undirected == self.n
    }

    /// 纯树 / 森林(无环)
    pub fn is_tree(&self) -> bool {
        self.cyclo == 0 && self.n >= 2
    }

    /// 多连通块
    pub fn is_multi(&self) -> bool {
        self.components > 1
    }

    /// 稠密到正交走线没救.光看密度不行:树天然是 2/n,小树上就超过 0.25.
    /// 还要"独立环很多"(环秩至少 n/2)
    pub fn is_dense(&self) -> bool {
        self.n >= 4 && self.density > 0.25 && (self.cyclo as f64) >= self.n as f64 / 2.0
    }

    /// 大部分是树:多出来的边很少,而且没有有向环
    pub fn mostly_tree(&self) -> bool {
        if self.n < 3 || self.has_cycle || self.arcs == 0 {
            return false;
        }
        let budget = ((self.n as f64 * 0.05).ceil() as usize).max(1);
        self.cyclo == 0 && self.extra <= budget
    }

    /// 大部分是链:分支点少、最长路径覆盖大部分节点
    pub fn mostly_chain(&self) -> bool {
        if self.n < 3 || self.components != 1 {
            return false;
        }
        let budget = ((self.n as f64 * 0.1).ceil() as usize).max(1);
        self.branch_nodes <= budget && self.cyclo == 0 && self.max_deg <= 3
    }

    /// 一句话描述,给状态栏 / 报告用
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

/// 扫一遍文档,算出结构指标
pub fn analyze(doc: &Document) -> Shape {
    let n = doc.boxes.len();
    let mut deg = vec![0usize; n];
    let mut loops = 0usize;
    let mut arcs = 0usize;
    // 无向边去重:同一个无序端点对只算一条
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
    // 环秩 = u - n + c(森林为 0,每个独立环 +1)
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

/// 弱连通块数(孤立点各算一块)
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

    /// 直接摆好 n 个框(ids 是 b0..b(n-1))与若干边,不经过 add_box 的自动 id
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
    fn 指标与判定() {
        // 链:a-b-c
        let chain = doc_of(3, &[(0, 1, true), (1, 2, true)]);
        let s = analyze(&chain);
        assert_eq!((s.n, s.undirected, s.components), (3, 2, 1));
        assert!(s.is_chain() && s.is_tree() && !s.is_cycle());
        assert_eq!(s.cyclo, 0);
        assert_eq!(s.kind(), "chain");

        // 环:a-b-c-a
        let cyc = doc_of(3, &[(0, 1, true), (1, 2, true), (2, 0, true)]);
        let s = analyze(&cyc);
        assert!(s.is_cycle(), "{s:?}");
        assert_eq!(s.cyclo, 1);

        // 树:一个根三个子(度 3 > 2,所以不算链)
        let tree = doc_of(4, &[(0, 1, true), (0, 2, true), (0, 3, true)]);
        let s = analyze(&tree);
        assert!(s.is_tree() && !s.is_chain(), "{s:?}");
        assert_eq!(s.branch_nodes, 1);
        assert_eq!(s.kind(), "tree");

        // 两个连通块
        let multi = doc_of(4, &[(0, 1, true), (2, 3, true)]);
        let s = analyze(&multi);
        assert_eq!(s.components, 2);
        assert!(s.is_multi());
        assert_eq!(s.kind(), "multi");

        // 稠密:K4
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
        // 稠密优先于多块/树:状态栏先报 dense
        assert_eq!(s.kind(), "dense");
    }
}
