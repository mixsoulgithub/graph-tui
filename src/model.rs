//! 图模型:节点、边、边分组.与画法无关,布局与渲染都只看这里.
//!
//! 用 Builder 造图(索引由 push 返回),build() 之后索引才是稳定的,
//! 所以边一律用节点索引、而不是名字来指人.

use std::collections::HashMap;

/// 一组边:同一条地铁线 / 同一类依赖 / 同一条泳道.
/// 决定颜色,也决定"沿线走上一站下一站"的顺序.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub label: String,
    pub color: (u8, u8, u8),
}

/// 节点外框:None 还是画 `○` / `◎` 那种一格字形,其余画一个 3 行高的框,文字在框里
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxStyle {
    /// 只占一格,不画框(native 文档里叫 point)
    Point,
    /// 不画框也不要节点字形:整块区域只有文字. 边照样避开这块区域,
    /// 但不往边框上画 T/箭头(没地方画),箭头落在区域外面那一格.
    /// 这也是默认样式:什么都不说时节点就是一行字
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
            // 空串跟"没写"一样:默认样式
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

    /// 画不画框
    pub fn is_boxed(&self) -> bool {
        matches!(
            self,
            BoxStyle::Single | BoxStyle::Rounded | BoxStyle::Double | BoxStyle::Heavy
        )
    }

    /// 是不是"一格一个节点字形"的点节点
    pub fn is_point(&self) -> bool {
        matches!(self, BoxStyle::Point)
    }

    /// (横, 竖, 左上, 右上, 左下, 右下)
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

/// 箭头字形风格
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ArrowStyle {
    /// `^ v < >`(默认;所有等宽字体都有)
    #[default]
    Ascii,
    /// `↑ ↓ ← →`(East Asian Width 里是 Ambiguous,有的终端按两格画)
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

    /// 按进入的方向取箭头字形.宽度不是 1 的字符一律退回 ASCII
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

/// 边的方向:四态.箭头字形仍是 `< > ^ v`,没有别的样式
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dir {
    /// 无箭头
    None,
    /// 箭头在 to 端
    #[default]
    Forward,
    /// 箭头在 from 端
    Back,
    /// 两端都有
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

    /// (from 端有没有箭头, to 端有没有箭头)
    pub fn heads(&self) -> (bool, bool) {
        match self {
            Dir::None => (false, false),
            Dir::Forward => (false, true),
            Dir::Back => (true, false),
            Dir::Both => (true, true),
        }
    }

    /// 算不算"有方向"(分层、环判定用)
    pub fn is_arrow(&self) -> bool {
        !matches!(self, Dir::None)
    }
}

/// 框内文字的水平对齐(点节点忽略)
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

/// 边的拐角:圆角 `╭╮╯╰` 或锐角 `┌┐└┘`
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

/// 边的线型.虚线/点线只断直笔,拐角与交叉点仍是实的(不然看不出线往哪拐)
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

    /// 这一格画不画(虚线隔一格断,点线隔两格断).拐角由调用方保证不断
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
    /// 方向(四态)
    pub dir: Dir,
    pub style: EdgeStyle,
    /// 这一条边自己的颜色;None = 用分组的颜色
    pub color: Option<(u8, u8, u8)>,
    /// 手工折点(画布坐标):边按 a -> via.. -> b 走,每个点都是一次拐弯
    pub via: Vec<(f64, f64)>,
    /// 图层名(空 = 默认层).画的时候按层的先后叠,编辑时可以只看/只动一层
    pub layer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub label: String,
    /// 坐标:x 向右、y 向上.单位随意(只有相对关系有意义).
    /// None = 这一站没有坐标,布局得自己算
    pub pos: Option<(f64, f64)>,
    /// 搜索别名(拼音、缩写、英文名),按顺序试
    pub terms: Vec<String>,
    /// 外框样式;None = 不画框
    pub box_style: BoxStyle,
    /// 节点自己的颜色(框与文字);None = 用所属分组的颜色
    pub color: Option<(u8, u8, u8)>,
    /// 手工偏移(画布坐标):布局给的位置再加这个量.拖节点就是在改它
    pub shift: (f64, f64),
    /// 图层名(空 = 默认层)
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

    /// 占位尺寸(字符格):没框就是 1x1;有框就是 (文字宽 + 2, 3)
    pub fn size(&self) -> (usize, usize) {
        match self.box_style {
            BoxStyle::Point => (1, 1),
            _ => (crate::text::plain_width(&self.label).max(2) + 2, 3),
        }
    }
}

/// 没指定颜色时按这个顺序发
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

/// 图的构造器:先把节点和分组建出来,拿到索引,再连边
pub struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    groups: Vec<Group>,
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
        }
    }

    /// 加一个节点,返回它的索引
    pub fn node(&mut self, label: impl Into<String>, pos: Option<(f64, f64)>) -> usize {
        self.nodes.push(Node::new(label, pos));
        self.nodes.len() - 1
    }

    /// 补搜索别名(pinyin / 缩写 / 英文名)
    pub fn terms(&mut self, i: usize, terms: &[&str]) {
        self.nodes[i].terms = terms.iter().map(|s| s.to_string()).collect();
    }

    /// 找一个同名分组,没有就建一个.同名分组颜色以先建的为准
    pub fn group(&mut self, label: impl Into<String>, color: Option<(u8, u8, u8)>) -> usize {
        let label = label.into();
        if let Some(i) = self.groups.iter().position(|g| g.label == label) {
            return i;
        }
        let color = color.unwrap_or(PALETTE[self.groups.len() % PALETTE.len()]);
        self.groups.push(Group { label, color });
        self.groups.len() - 1
    }

    /// 加一条边,返回它的下标(后面要用 edge_style / edge_via 调它)
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

    /// 节点外框(不画框就传 BoxStyle::None)
    pub fn node_box(&mut self, i: usize, box_style: BoxStyle) {
        self.nodes[i].box_style = box_style;
    }

    /// 节点自己的颜色
    pub fn node_color(&mut self, i: usize, color: Option<(u8, u8, u8)>) {
        self.nodes[i].color = color;
    }

    pub fn edge_style(&mut self, e: usize, style: EdgeStyle) {
        self.edges[e].style = style;
    }

    pub fn edge_color(&mut self, e: usize, color: Option<(u8, u8, u8)>) {
        self.edges[e].color = color;
    }

    /// 手工折点:边按 a -> via.. -> b 走
    pub fn edge_via(&mut self, e: usize, via: Vec<(f64, f64)>) {
        self.edges[e].via = via;
    }

    /// 节点的手工偏移(布局给的位置再加这个量)
    pub fn node_shift(&mut self, i: usize, shift: (f64, f64)) {
        self.nodes[i].shift = shift;
    }

    pub fn node_layer(&mut self, i: usize, layer: impl Into<String>) {
        self.nodes[i].layer = layer.into();
    }

    pub fn edge_layer(&mut self, e: usize, layer: impl Into<String>) {
        self.edges[e].layer = layer.into();
    }

    /// 收尾:校验、去掉重复边、算派生结构(邻接表、节点的分组、包围盒、每个分组的节点序列)
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
        // 同一条边(同分组)只留一条.有向边按有序端点去重:a->b 与 b->a 是两条边
        // (有向图里那是个二元环),不能当成重复
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

        // 分组的颜色:如果这一组的边都写了同一个自己的颜色,就把它当分组色.
        // 每图调整层的图例、":config" 那些地方要的都是"这一组是什么颜色",
        // 而 color 现在是单条边的属性,不这么收一下图例就没颜色可用了
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
            node_groups,
            adj,
            bbox,
            series: Vec::new(),
        };
        graph.series = graph.build_series();
        Ok(graph)
    }
}

/// 一张图.节点用索引指,边挂在分组上
#[derive(Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    /// 每个节点经过的分组(升序去重)
    pub node_groups: Vec<Vec<usize>>,
    /// 每个节点的邻居:(邻居, 分组)
    pub adj: Vec<Vec<(usize, usize)>>,
    /// 全部节点都有坐标时的包围盒 (x0, y0, x1, y1)
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// 每个分组走一遍的节点序列(可能因为分叉而分成几条)
    series: Vec<(usize, Vec<usize>)>,
}

impl Graph {
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// 从 .dot 文本建图(见 dot.rs)
    pub fn from_dot(text: &str) -> Result<Graph, String> {
        crate::dot::parse(text)
    }

    /// 枢纽:经过两条及以上分组的节点(地铁的换乘站)
    pub fn hub(&self, i: usize) -> bool {
        self.node_groups[i].len() > 1
    }

    pub fn degree(&self, i: usize) -> usize {
        self.adj[i].len()
    }

    /// 每个节点都有坐标吗
    pub fn has_pos(&self) -> bool {
        self.bbox.is_some()
    }

    pub fn pos(&self, i: usize) -> (f64, f64) {
        self.nodes[i].pos.unwrap_or((0.0, 0.0))
    }

    pub fn group_color(&self, g: usize) -> (u8, u8, u8) {
        self.groups[g].color
    }

    /// 这条边实际用的颜色:自己的优先,否则用分组的
    pub fn edge_color(&self, e: &Edge) -> (u8, u8, u8) {
        e.color.unwrap_or_else(|| self.groups[e.group].color)
    }

    /// 这个节点实际用的颜色:自己的优先,否则所属第一个分组的;都没有就 None
    pub fn node_color(&self, i: usize) -> Option<(u8, u8, u8)> {
        self.nodes[i]
            .color
            .or_else(|| self.node_groups[i].first().map(|g| self.groups[*g].color))
    }

    /// 图里出现过的图层,按首次出现顺序;默认层("")排最前
    pub fn layers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in 0..self.nodes.len() {
            let l = &self.nodes[i].layer;
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

    /// 最宽的那个节点的占位宽度(布局按它留间距,框才不会叠在一起)
    pub fn max_box_w(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| n.size().0)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// 每个分组走一遍的节点序列:(分组号, 节点序列).
    /// 建图时算一次存下来:渲染每帧都要用(照着顺序走,拐弯处方向才接得上),
    /// 几百个节点的图每帧重算不划算
    pub fn series(&self) -> &[(usize, Vec<usize>)] {
        &self.series
    }

    /// 按分组把边串成路径.有分叉就分成几条;
    /// 有环的连通块没有度数 != 2 的起点,所以最后兜底从任意点起走(闭合成环)
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
            // 度 != 2 的节点是天然的路径端点,先当起点
            let mut starts: Vec<usize> = adj
                .iter()
                .filter(|(_, v)| v.len() != 2)
                .map(|(k, _)| *k)
                .collect();
            starts.sort_unstable();
            for &start in starts.iter().chain(adj.keys()) {
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

    /// 箭头字形:三套风格,宽度不是 1 的回退 ASCII
    #[test]
    fn 箭头字形风格() {
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
    fn 建图算出邻接与分组() {
        let g = sample();
        assert_eq!(g.adj[1].len(), 2);
        assert!(!g.hub(1));
        assert_eq!(g.series().len(), 1);
        assert_eq!(g.series()[0].1, vec![0, 1, 2]);
        assert_eq!(g.bbox, Some((0.0, 0.0, 2.0, 0.0)));
    }

    #[test]
    fn 缺坐标时没有包围盒() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", Some((1.0, 0.0)));
        let g = b.group("g", None);
        b.edge(a, c, g, Dir::None);
        let g = b.build().unwrap();
        assert!(!g.has_pos());
    }

    #[test]
    fn 自己连自己也收() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let g = b.group("g", None);
        b.edge(a, a, g, Dir::None);
        let g = b.build().expect("自环该收下");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].a, g.edges[0].b);
    }

    #[test]
    fn 重复边只留一条() {
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
    fn 反向的有向边是两条边() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g = b.group("g", None);
        b.edge(a, c, g, Dir::Forward);
        b.edge(c, a, g, Dir::Forward);
        let g = b.build().unwrap();
        assert_eq!(g.edges.len(), 2);
        // adj 是"每条边的两端各记一次",所以两条平行边在这一对节点上各记两次
        assert_eq!(g.adj[0].len(), 2);
        assert_eq!(g.adj[1].len(), 2);
    }

    /// 组内每条边都写了同一个颜色时,分组也采用它(图例要用)
    #[test]
    fn 分组采纳组内一致的颜色() {
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

        // 组内颜色不一致就保持调色板色
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
    fn 没给颜色的分组自动排在调色板上() {
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
    fn 默认框样式是只有文字() {
        assert_eq!(BoxStyle::default(), BoxStyle::None);
        assert_eq!(BoxStyle::parse(""), Some(BoxStyle::None));
        assert_eq!(Node::new("a", None).box_style, BoxStyle::None);
    }
}
