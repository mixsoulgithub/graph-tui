//! native 文档模型:框(box)、文字(text)、边(edge)三者分开记录.
//!
//! 坐标就是终端单元格:整数,1 单位 = 1 列 x 1 行,x 向右、y 向下.
//! 视图状态(中心、缩放)不属于几何,存在 `view` 里.
//! 派生结构(id 索引、邻接、分组串联、包围盒)只在内存里算,不落盘.

use std::collections::HashMap;

use crate::model::{Align, BoxStyle, Corner, EdgeStyle};

pub type Rgb = (u8, u8, u8);

pub use crate::model::Dir;

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub color: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub order: i32,
}

/// 一个框.`style` 是 Point 时只占一格,不画框
#[derive(Clone, Debug, PartialEq)]
pub struct Box {
    pub id: String,
    /// 左上角单元格
    pub at: (i32, i32),
    /// (宽, 高);Point 固定 (1, 1)
    pub size: (u16, u16),
    pub style: BoxStyle,
    /// 边框颜色
    pub color: Option<Rgb>,
    /// 框内背景填充;只对带框样式生效
    pub fill: Option<Rgb>,
    pub layer: String,
    pub terms: Vec<String>,
    /// 注释框:绑到哪条边(只有 style=none 的文字框可以带);None = 普通框
    pub edge: Option<String>,
}

/// 框里的文字,严格一对一.`at` 是文字左上角单元格
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub id: String,
    pub text: String,
    pub at: (i32, i32),
    pub box_id: String,
    /// 文字颜色;None = 跟着框的边框色
    pub color: Option<Rgb>,
    /// 文字底色(只铺在文字占的格子上);None = 用框的填充色或终端底色
    pub bg: Option<Rgb>,
    /// 水平对齐;只对带框样式的框内文字生效
    pub align: Align,
    pub layer: String,
}

/// 文字的行数与最长行的显示宽度(多行的框按这个长高、变宽)
pub fn text_metrics(text: &str) -> (usize, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let widest = lines
        .iter()
        .map(|l| crate::text::plain_width(l))
        .max()
        .unwrap_or(0);
    (lines.len().max(1), widest)
}

/// 边.端点用 box id,不存下标
#[derive(Clone, Debug, PartialEq)]
pub struct DocEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub dir: Dir,
    pub group: String,
    pub style: EdgeStyle,
    pub color: Option<Rgb>,
    /// 边线的底色(铺在它经过的格子上)
    pub bg: Option<Rgb>,
    /// 手工折点(单元格,整数)
    pub via: Vec<(i32, i32)>,
    /// 拐角是圆角还是锐角
    pub corner: Corner,
    pub layer: String,
}

/// 视图里的光标位置:老文件存框 id,新文件存格子
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cursor {
    Box(String),
    Cell((i32, i32)),
}

/// 图例里一行的示意字形(画在文字前面)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendGlyph {
    Solid,
    Dashed,
    Dotted,
    Arrow,
}

impl LegendGlyph {
    pub fn name(self) -> &'static str {
        match self {
            LegendGlyph::Solid => "solid",
            LegendGlyph::Dashed => "dashed",
            LegendGlyph::Dotted => "dotted",
            LegendGlyph::Arrow => "arrow",
        }
    }

    pub fn from_name(s: &str) -> Option<LegendGlyph> {
        match s {
            "solid" => Some(LegendGlyph::Solid),
            "dashed" => Some(LegendGlyph::Dashed),
            "dotted" => Some(LegendGlyph::Dotted),
            "arrow" => Some(LegendGlyph::Arrow),
            _ => None,
        }
    }

    /// 画在图例里那两格示意
    pub fn sample(self) -> &'static str {
        match self {
            LegendGlyph::Solid => "──",
            LegendGlyph::Dashed => "╌╌",
            LegendGlyph::Dotted => "┈┈",
            LegendGlyph::Arrow => "─>",
        }
    }
}

/// 图例里的一行.`group` 借分组颜色,`color` 直接指定(两者都有时 color 赢)
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LegendEntry {
    pub text: String,
    pub group: Option<String>,
    pub color: Option<Rgb>,
    pub glyph: Option<LegendGlyph>,
}

/// 文档自带的图例:一个标题 + 任意行.它是面板,不是内容:
/// 不参与布局/命中/选区,相机也不会把它一起缩放
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Legend {
    pub title: Option<String>,
    pub entries: Vec<LegendEntry>,
}

impl Legend {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.entries.is_empty()
    }
}

/// 只属于视图的状态,不影响几何
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    pub center: (f64, f64),
    pub zoom: f64,
    pub layout: String,
    /// 光标位置:v1 存框 id,v2 存格子
    pub cursor: Option<Cursor>,
    pub focus: Vec<String>,
    pub labels: bool,
    pub lang: String,
    pub layer: Option<String>,
    pub side: f64,
    pub cycle: Option<f64>,
    /// 方向:auto 或 0/90/180/270(`r` 与 `:layout` 重排时用)
    pub orientation: String,
    /// 右侧图例开不开.老文件没有这一项,按开读
    pub legend: bool,
    /// 边注释开不开(关掉=不画、不算障碍、不占尺寸).老文件没有这一项,按开读
    pub notes: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            center: (0.0, 0.0),
            zoom: 1.0,
            layout: "layered".to_string(),
            cursor: None,
            focus: Vec::new(),
            labels: false,
            lang: "zh".to_string(),
            layer: None,
            side: 2.0,
            cycle: None,
            orientation: "auto".to_string(),
            legend: true,
            notes: true,
        }
    }
}

impl ViewState {
    /// 光标落到哪个格子:格子照用,框 id 换成它的文字起点,都没有就用整图中心
    pub fn cursor_cell(&self, doc: &Document) -> (i32, i32) {
        match &self.cursor {
            Some(Cursor::Cell(c)) => *c,
            Some(Cursor::Box(id)) => doc
                .box_idx(id)
                .map(|i| {
                    doc.text(i)
                        .map(|t| t.at)
                        .unwrap_or_else(|| (doc.boxes[i].at.0 + 1, doc.boxes[i].at.1 + 1))
                })
                .unwrap_or_else(|| doc_center(doc)),
            None => doc_center(doc),
        }
    }
}

fn doc_center(doc: &Document) -> (i32, i32) {
    if doc.boxes.is_empty() {
        return (0, 0);
    }
    let (x0, y0, x1, y1) = doc.bbox();
    ((x0 + x1) / 2, (y0 + y1) / 2)
}

/// 一份 native 文档
#[derive(Clone)]
pub struct Document {
    /// 写到文件里的来源标记(读的时候只检查有没有)
    pub source: String,
    pub groups: Vec<Group>,
    pub layers: Vec<Layer>,
    pub boxes: Vec<Box>,
    pub texts: Vec<Text>,
    pub edges: Vec<DocEdge>,
    pub view: ViewState,
    /// 文档自带的图例(标题 + 行)
    pub legend: Legend,

    // 下面全是派生结构:rebuild() 重新算,不落盘
    box_index: HashMap<String, usize>,
    group_index: HashMap<String, usize>,
    /// 每个 box 的 text 下标(没有就是 usize::MAX)
    text_of: Vec<usize>,
    /// 每个 box 连着的边下标
    adj: Vec<Vec<usize>>,
    /// 每个 box 经过的分组下标
    box_groups: Vec<Vec<usize>>,
    /// 每个分组串起来的 box 序列(分组下标, box 下标序列)
    series: Vec<(usize, Vec<usize>)>,
    bbox: (i32, i32, i32, i32),
    /// 修订号:每次 rebuild 加一.命中表里存的是数组下标,跨版本就不能用
    rev: u64,
}

/// 判重时的端点顺序:无向边按无序端点算,有向边按书写顺序
fn key_pair(a: &str, b: &str, dir: Dir) -> (String, String) {
    match dir {
        Dir::None if a > b => (b.to_string(), a.to_string()),
        _ => (a.to_string(), b.to_string()),
    }
}

impl PartialEq for Document {
    /// 只比落盘的那些字段:派生结构由它们决定,`source` 只是标记
    fn eq(&self, other: &Self) -> bool {
        self.groups == other.groups
            && self.layers == other.layers
            && self.boxes == other.boxes
            && self.texts == other.texts
            && self.edges == other.edges
            && self.view == other.view
            && self.legend == other.legend
    }
}

impl Document {
    /// 从各部件建文档:先算派生结构,再整体校验.
    // 部件就是这些(分组/图层/框/文字/边/视图/图例),打包成结构体只是换个地方堆
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        source: impl Into<String>,
        groups: Vec<Group>,
        layers: Vec<Layer>,
        boxes: Vec<Box>,
        texts: Vec<Text>,
        mut edges: Vec<DocEdge>,
        view: ViewState,
        legend: Legend,
    ) -> Result<Document, String> {
        // 同一对节点、同一分组的多条边只留一条(与旧模型一致):
        // 有向边按有序端点算,a->b 与 b->a 是两条;无向边按无序端点算
        let mut seen: std::collections::HashSet<(String, String, String)> =
            std::collections::HashSet::new();
        edges.retain(|e| {
            let ordered = match e.dir {
                Dir::None => e.from < e.to,
                _ => true,
            };
            let (a, b) = if ordered {
                (e.from.clone(), e.to.clone())
            } else {
                (e.to.clone(), e.from.clone())
            };
            seen.insert((a, b, e.group.clone()))
        });
        let mut doc = Document {
            source: source.into(),
            groups,
            layers,
            boxes,
            texts,
            edges,
            view,
            legend,
            box_index: HashMap::new(),
            group_index: HashMap::new(),
            text_of: Vec::new(),
            adj: Vec::new(),
            box_groups: Vec::new(),
            series: Vec::new(),
            bbox: (0, 0, 0, 0),
            rev: 0,
        };
        doc.rebuild();
        doc.validate()?;
        Ok(doc)
    }

    /// 重算派生结构.调用方要先保证 id 不重复(load 会校验)
    /// 这一版文档的修订号(命中表校验用)
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn rebuild(&mut self) {
        self.rev = self.rev.wrapping_add(1);
        self.box_index.clear();
        self.group_index.clear();
        for (i, g) in self.groups.iter().enumerate() {
            self.group_index.entry(g.id.clone()).or_insert(i);
        }
        for (i, b) in self.boxes.iter().enumerate() {
            self.box_index.entry(b.id.clone()).or_insert(i);
        }
        self.text_of = vec![usize::MAX; self.boxes.len()];
        for (i, t) in self.texts.iter().enumerate() {
            if let Some(&bi) = self.box_index.get(&t.box_id) {
                if self.text_of[bi] == usize::MAX {
                    self.text_of[bi] = i;
                }
            }
        }
        self.adj = vec![Vec::new(); self.boxes.len()];
        self.box_groups = vec![Vec::new(); self.boxes.len()];
        for (i, e) in self.edges.iter().enumerate() {
            let (Some(&a), Some(&b)) = (self.box_index.get(&e.from), self.box_index.get(&e.to))
            else {
                continue;
            };
            self.adj[a].push(i);
            if b != a {
                // 自环只记一次,不然轮边、找邻居都会重复
                self.adj[b].push(i);
            }
            if let Some(&gi) = self.group_index.get(&e.group) {
                if !self.box_groups[a].contains(&gi) {
                    self.box_groups[a].push(gi);
                }
                if !self.box_groups[b].contains(&gi) {
                    self.box_groups[b].push(gi);
                }
            }
        }
        self.series = self.build_series();
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for b in &self.boxes {
            x0 = x0.min(b.at.0);
            y0 = y0.min(b.at.1);
            x1 = x1.max(b.at.0 + b.size.0 as i32 - 1);
            y1 = y1.max(b.at.1 + b.size.1 as i32 - 1);
        }
        self.bbox = if self.boxes.is_empty() {
            (0, 0, 0, 0)
        } else {
            (x0, y0, x1, y1)
        };
    }

    /// 模型级校验:引用的东西必须都在,id 不重复,文字与框一对一.
    /// 文件格式层面的检查(类型、范围、未知键、路径)由 document_io 负责.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for g in &self.groups {
            if !ids.insert(&g.id) {
                return Err(format!("duplicate id {:?}", g.id));
            }
        }
        if self.groups.is_empty() {
            return Err("no groups".to_string());
        }
        let mut layer_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for l in &self.layers {
            if !layer_names.insert(&l.name) {
                return Err(format!("duplicate layer {:?}", l.name));
            }
        }
        if !layer_names.contains("") {
            return Err("the default layer (\"\") is missing".to_string());
        }
        for b in &self.boxes {
            if !ids.insert(&b.id) {
                return Err(format!("duplicate id {:?}", b.id));
            }
            if !layer_names.contains(b.layer.as_str()) {
                return Err(format!("box {:?}: unknown layer {:?}", b.id, b.layer));
            }
            if b.size.0 == 0 || b.size.1 == 0 {
                return Err(format!("box {:?}: empty size", b.id));
            }
            if b.style.is_boxed() && (b.size.1 < 3 || b.size.0 < 4) {
                return Err(format!(
                    "box {:?}: a boxed node is at least 3 rows tall and 4 wide",
                    b.id
                ));
            }
            if b.style.is_point() && b.size != (1, 1) {
                return Err(format!("box {:?}: a point is 1x1", b.id));
            }
        }
        for t in &self.texts {
            if !ids.insert(&t.id) {
                return Err(format!("duplicate id {:?}", t.id));
            }
            if !layer_names.contains(t.layer.as_str()) {
                return Err(format!("text {:?}: unknown layer {:?}", t.id, t.layer));
            }
            if self.box_idx(&t.box_id).is_none() {
                return Err(format!(
                    "text {:?}: box {:?} does not exist",
                    t.id, t.box_id
                ));
            }
            if t.text.contains('\r') || t.text.contains('\t') {
                return Err(format!("text {:?}: no CR or TAB (use \\n for lines)", t.id));
            }
        }
        for (bi, b) in self.boxes.iter().enumerate() {
            let n = self.texts.iter().filter(|t| t.box_id == b.id).count();
            if n != 1 {
                return Err(format!("box {:?}: needs exactly one text, got {n}", b.id));
            }
            if self.text_of(bi).is_none() {
                return Err(format!("box {:?}: text not attached", b.id));
            }
            if let Some(t) = self.text(bi) {
                let (lines, widest) = text_metrics(&t.text);
                let ok = match b.style {
                    BoxStyle::Point => lines <= 1,
                    BoxStyle::None => {
                        lines <= b.size.1 as usize && widest <= b.size.0 as usize
                    }
                    _ => {
                        lines + 2 <= b.size.1 as usize
                            && widest + 2 <= b.size.0 as usize
                    }
                };
                if !ok {
                    return Err(format!(
                        "box {:?}: the text does not fit ({} lines x {} cols)",
                        b.id, lines, widest
                    ));
                }
            }
            if let Some(e) = &b.edge {
                if !matches!(b.style, BoxStyle::None) {
                    return Err(format!(
                        "box {:?}: only a text-only (none) node can be an edge note",
                        b.id
                    ));
                }
                if !self.edges.iter().any(|x| &x.id == e) {
                    return Err(format!("box {:?}: edge {e:?} does not exist", b.id));
                }
            }
        }
        // 一条边最多一个注释框
        let mut noted: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for b in &self.boxes {
            if let Some(e) = b.edge.as_deref() {
                if !noted.insert(e) {
                    return Err(format!("edge {e:?}: already has a note box"));
                }
            }
        }
        for e in &self.edges {
            if !ids.insert(&e.id) {
                return Err(format!("duplicate id {:?}", e.id));
            }
            if self.box_idx(&e.from).is_none() {
                return Err(format!("edge {:?}: box {:?} does not exist", e.id, e.from));
            }
            if self.box_idx(&e.to).is_none() {
                return Err(format!("edge {:?}: box {:?} does not exist", e.id, e.to));
            }
            if !ids.contains(e.group.as_str()) {
                return Err(format!(
                    "edge {:?}: group {:?} does not exist",
                    e.id, e.group
                ));
            }
            if !layer_names.contains(e.layer.as_str()) {
                return Err(format!("edge {:?}: unknown layer {:?}", e.id, e.layer));
            }
        }
        // 图例:组名要存在,文字单行(和 texts 同一条规矩)
        if let Some(t) = &self.legend.title {
            if t.contains('\n') || t.contains('\t') {
                return Err("legend.title: must be a single line".to_string());
            }
        }
        for (i, e) in self.legend.entries.iter().enumerate() {
            if e.text.contains('\n') || e.text.contains('\t') {
                return Err(format!("legend.entries[{i}].text: must be a single line"));
            }
            if let Some(g) = &e.group {
                if !ids.contains(g.as_str()) {
                    return Err(format!("legend.entries[{i}]: group {g:?} does not exist"));
                }
            }
        }
        if !(self.view.zoom.is_finite() && self.view.zoom > 0.0) {
            return Err("view.zoom must be a finite positive number".to_string());
        }
        if !self.view.center.0.is_finite() || !self.view.center.1.is_finite() {
            return Err("view.center must be finite".to_string());
        }
        Ok(())
    }

    /// 每个分组串一遍:端点优先当起点,画线时方向才接得上
    fn build_series(&self) -> Vec<(usize, Vec<usize>)> {
        let mut out = Vec::new();
        for gi in 0..self.groups.len() {
            let gid = &self.groups[gi].id;
            let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
            for e in self.edges.iter().filter(|e| &e.group == gid) {
                let (Some(&a), Some(&b)) = (self.box_index.get(&e.from), self.box_index.get(&e.to))
                else {
                    continue;
                };
                // 自环不进"线路":它自己绕一圈,不参与串联
                if a == b {
                    continue;
                }
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
            }
            let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
            let mut visited: std::collections::HashSet<(usize, usize)> =
                std::collections::HashSet::new();
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
                        out.push((gi, path));
                    }
                }
            }
        }
        out
    }

    // ---- 查询 ----

    pub fn box_idx(&self, id: &str) -> Option<usize> {
        self.box_index.get(id).copied()
    }

    pub fn group_idx(&self, id: &str) -> Option<usize> {
        self.group_index.get(id).copied()
    }

    pub fn text_of(&self, box_i: usize) -> Option<usize> {
        match self.text_of.get(box_i) {
            Some(&i) if i != usize::MAX => Some(i),
            _ => None,
        }
    }

    pub fn text(&self, box_i: usize) -> Option<&Text> {
        self.text_of(box_i).map(|i| &self.texts[i])
    }

    /// 框的几何中心(单元格坐标,可能带 .5)
    pub fn center(&self, i: usize) -> (f64, f64) {
        let b = &self.boxes[i];
        (
            b.at.0 as f64 + (b.size.0 as f64 - 1.0) / 2.0,
            b.at.1 as f64 + (b.size.1 as f64 - 1.0) / 2.0,
        )
    }

    /// 这个框占的格子
    pub fn cells(&self, i: usize) -> impl Iterator<Item = (i32, i32)> + '_ {
        let b = &self.boxes[i];
        let (w, h) = (b.size.0 as i32, b.size.1 as i32);
        (0..h).flat_map(move |dy| (0..w).map(move |dx| (b.at.0 + dx, b.at.1 + dy)))
    }

    /// 这个格子上的框(点节点也认)
    /// 这一格盖住的框,从最上面(后画的)到最下面
    pub fn boxes_at_cell(&self, cell: (i32, i32)) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for (i, b) in self.boxes.iter().enumerate().rev() {
            let (w, h) = (b.size.0 as i32, b.size.1 as i32);
            if cell.0 >= b.at.0 && cell.0 < b.at.0 + w && cell.1 >= b.at.1 && cell.1 < b.at.1 + h {
                out.push(i);
            }
        }
        out
    }

    /// 这一格盖住的框里最上面的那个(重叠时先选新画的那一份)
    pub fn box_at_cell(&self, cell: (i32, i32)) -> Option<usize> {
        self.boxes_at_cell(cell).first().copied()
    }

    pub fn adj(&self, i: usize) -> &[usize] {
        &self.adj[i]
    }

    pub fn series(&self) -> &[(usize, Vec<usize>)] {
        &self.series
    }

    pub fn box_groups(&self, i: usize) -> &[usize] {
        &self.box_groups[i]
    }

    /// 枢纽:经过两个及以上分组
    pub fn hub(&self, i: usize) -> bool {
        self.box_groups[i].len() > 1
    }

    pub fn bbox(&self) -> (i32, i32, i32, i32) {
        self.bbox
    }

    /// 这个框是不是"边注释"
    pub fn is_note(&self, i: usize) -> bool {
        self.boxes.get(i).is_some_and(|b| b.edge.is_some())
    }

    /// 画面真正占的范围:注释收起来时不把注释框算进去
    pub fn bbox_shown(&self, notes: bool) -> (i32, i32, i32, i32) {
        if notes {
            return self.bbox;
        }
        let mut it = self.boxes.iter().filter(|b| b.edge.is_none());
        let Some(first) = it.next() else {
            return self.bbox;
        };
        let mut r = (
            first.at.0,
            first.at.1,
            first.at.0 + first.size.0 as i32 - 1,
            first.at.1 + first.size.1 as i32 - 1,
        );
        for b in it {
            r.0 = r.0.min(b.at.0);
            r.1 = r.1.min(b.at.1);
            r.2 = r.2.max(b.at.0 + b.size.0 as i32 - 1);
            r.3 = r.3.max(b.at.1 + b.size.1 as i32 - 1);
        }
        r
    }

    /// 给一条边新建一个空的注释框(等用户打字);已经有就返回它
    pub fn new_note(&mut self, e: usize) -> Option<usize> {
        let id = self.edges.get(e)?.id.clone();
        if let Some(b) = self.note_of_edge(&id) {
            return Some(b);
        }
        let at = self.note_spot(e);
        let layer = self.edges[e].layer.clone();
        let b = self.add_box("", at, BoxStyle::None).ok()?;
        self.boxes[b].edge = Some(id);
        self.boxes[b].layer = layer.clone();
        if let Some(ti) = self.text_of(b) {
            self.texts[ti].layer = layer;
        }
        self.rebuild();
        Some(self.note_of_edge(&self.edges[e].id.clone()).unwrap_or(b))
    }

    /// 框实际用的颜色:自己的优先,否则所属第一个分组的
    pub fn box_color(&self, i: usize) -> Option<Rgb> {
        self.boxes[i]
            .color
            .or_else(|| self.box_groups[i].first().map(|g| self.groups[*g].color))
    }

    /// 文字颜色:自己的优先;注释框跟着它绑的那条边;其余跟着框
    pub fn text_color(&self, box_i: usize) -> Option<Rgb> {
        if let Some(t) = self.text(box_i).and_then(|t| t.color) {
            return Some(t);
        }
        if self.boxes.get(box_i).is_some_and(|b| b.edge.is_some()) {
            return self.edge_of_box(box_i).map(|e| self.edge_color(e));
        }
        self.box_color(box_i)
    }

    /// 边 -> 它的注释框(下标)
    pub fn note_of_edge(&self, e: &str) -> Option<usize> {
        self.boxes.iter().position(|b| b.edge.as_deref() == Some(e))
    }

    /// 框 -> 它绑的边(下标)
    pub fn edge_of_box(&self, b: usize) -> Option<usize> {
        let id = self.boxes.get(b)?.edge.as_deref()?;
        self.edges.iter().position(|e| e.id == id)
    }

    /// 注释框的建议位置:两端中心的中点往右一格;被别的框占了就按同心环往外找
    pub fn note_spot(&self, e: usize) -> (i32, i32) {
        let (a, b) = self.edge_ends(e).unwrap_or((0, 0));
        let center = |i: usize| -> (i32, i32) {
            let b = &self.boxes[i];
            (b.at.0 + b.size.0 as i32 / 2, b.at.1 + b.size.1 as i32 / 2)
        };
        let (ca, cb) = (center(a), center(b));
        let base = if a == b {
            (ca.0 + 2, ca.1)
        } else {
            ((ca.0 + cb.0) / 2 + 1, (ca.1 + cb.1) / 2)
        };
        // 别的框(含它们左右各一格)都算占位:注释不要压在字上
        let free = |p: (i32, i32), s: &Self| {
            !s.boxes.iter().any(|b| {
                let (x, y) = (b.at.0, b.at.1);
                let (w, h) = (b.size.0 as i32, b.size.1 as i32);
                p.0 >= x - 1 && p.0 <= x + w && p.1 >= y - 1 && p.1 <= y + h
            })
        };
        if free(base, self) {
            return base;
        }
        for r in 1i32..=24 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let p = (base.0 + dx, base.1 + dy);
                    if free(p, self) {
                        return p;
                    }
                }
            }
        }
        base
    }

    /// 写一条边的注释:空串 = 删掉注释框;没有就新建一个 none 框并绑上.
    /// 返回注释框的下标(删掉时是 None)
    pub fn set_note(&mut self, e: usize, text: &str) -> Option<usize> {
        let id = self.edges.get(e)?.id.clone();
        let text = text.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            if let Some(b) = self.note_of_edge(&id) {
                self.remove_box(&self.boxes[b].id.clone()).ok()?;
            }
            return None;
        }
        if let Some(b) = self.note_of_edge(&id) {
            self.set_text(b, text);
            return Some(b);
        }
        let at = self.note_spot(e);
        let layer = self.edges[e].layer.clone();
        let ok = self.add_box(text, at, BoxStyle::None).ok()?;
        self.boxes[ok].edge = Some(id);
        self.boxes[ok].layer = layer.clone();
        if let Some(ti) = self.text_of(ok) {
            self.texts[ti].layer = layer;
        }
        self.rebuild();
        Some(ok)
    }

    pub fn edge_color(&self, e: usize) -> Rgb {
        let edge = &self.edges[e];
        edge.color.unwrap_or_else(|| {
            self.group_idx(&edge.group)
                .map(|g| self.groups[g].color)
                .unwrap_or((212, 218, 232))
        })
    }

    pub fn edge_ends(&self, e: usize) -> Option<(usize, usize)> {
        let edge = &self.edges[e];
        Some((self.box_idx(&edge.from)?, self.box_idx(&edge.to)?))
    }

    /// 图层名(按 order 排好),默认层在最前
    pub fn layer_names(&self) -> Vec<String> {
        let mut ls: Vec<&Layer> = self.layers.iter().collect();
        ls.sort_by_key(|l| l.order);
        ls.into_iter().map(|l| l.name.clone()).collect()
    }

    pub fn layer_order(&self, name: &str) -> i32 {
        self.layers
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.order)
            .unwrap_or(i32::MAX)
    }

    // ---- 修改(编辑器用;改完要 rebuild) ----

    /// 把 box 连同它的文字一起平移(整数格)
    pub fn move_box(&mut self, i: usize, dx: i32, dy: i32) {
        if i >= self.boxes.len() {
            return;
        }
        self.boxes[i].at.0 += dx;
        self.boxes[i].at.1 += dy;
        if let Some(ti) = self.text_of(i) {
            self.texts[ti].at.0 += dx;
            self.texts[ti].at.1 += dy;
        }
        self.rebuild();
    }

    /// 换文字,并把框尺寸与文字位置同步过去(点节点尺寸不变)
    pub fn set_text(&mut self, i: usize, text: impl Into<String>) {
        if i >= self.boxes.len() {
            return;
        }
        let text = text.into();
        if let Some(ti) = self.text_of(i) {
            self.texts[ti].text = text;
        }
        self.fit_box(i);
        self.rebuild();
    }

    /// 按文字重算框尺寸,并把文字位置对齐到框内左上角 + (1, 1)
    pub fn fit_box(&mut self, i: usize) {
        let Some(ti) = self.text_of(i) else {
            return;
        };
        let (lines, widest) = text_metrics(&self.texts[ti].text);
        if matches!(self.boxes[i].style, BoxStyle::None) {
            // 只有文字:宽 = 最长行,高 = 行数
            self.boxes[i].size = (widest.max(1) as u16, lines.max(1) as u16);
            self.texts[ti].at = self.boxes[i].at;
            return;
        }
        if !self.boxes[i].style.is_boxed() {
            // 点节点只有一个名字,放不下换行
            self.boxes[i].size = (1, 1);
            return;
        }
        let w = (widest.max(2) + 2).max(4) as u16;
        let h = (lines.max(1) + 2) as u16;
        self.boxes[i].size = (w, h);
        self.texts[ti].at = (self.boxes[i].at.0 + 1, self.boxes[i].at.1 + 1);
    }

    /// 把框放到左上角 at(点节点就是那个格),文字跟着走
    pub fn place_box(&mut self, i: usize, at: (i32, i32)) {
        if i >= self.boxes.len() {
            return;
        }
        self.boxes[i].at = at;
        if let Some(ti) = self.text_of(i) {
            self.texts[ti].at = if self.boxes[i].style.is_boxed() {
                (at.0 + 1, at.1 + 1)
            } else {
                at
            };
        }
    }

    /// 绕原点顺时针转 90 度的整数倍(1 = 90). 框的宽高不交换:
    /// 按中心转过去,再用原尺寸重新算左上角
    pub fn rotate_quarters(&mut self, k: i32) {
        let k = k.rem_euclid(4);
        if k == 0 {
            return;
        }
        let rot = |p: (f64, f64)| -> (f64, f64) {
            match k {
                1 => (-p.1, p.0),
                2 => (-p.0, -p.1),
                _ => (p.1, -p.0),
            }
        };
        for b in &mut self.boxes {
            // 宽高不交换:用"矩形角落"的整数映射,四次正好回到原处(不能带中间取整)
            let (w, h) = (b.size.0 as i32 - 1, b.size.1 as i32 - 1);
            let (x, y) = b.at;
            b.at = match k {
                1 => (-(y + h), x),
                2 => (-(x + h), -(y + h)),
                _ => (y, -(x + h)),
            };
            let _ = w;
        }
        // 文字跟着框:带框的回到框内起点,点节点按原点转
        let mut text_at: HashMap<String, (i32, i32)> = HashMap::new();
        for b in &self.boxes {
            text_at.insert(b.id.clone(), b.at);
        }
        for t in &mut self.texts {
            let Some(&at) = text_at.get(&t.box_id) else {
                continue;
            };
            t.at = at;
        }
        for b in &self.boxes {
            if b.style.is_boxed() {
                for t in &mut self.texts {
                    if t.box_id == b.id {
                        t.at = (b.at.0 + 1, b.at.1 + 1);
                    }
                }
            }
        }
        for e in &mut self.edges {
            e.via = e
                .via
                .iter()
                .map(|&(x, y)| {
                    let (rx, ry) = rot((x as f64, y as f64));
                    (rx.round() as i32, ry.round() as i32)
                })
                .collect();
        }
        let (cx, cy) = rot((self.view.center.0, self.view.center.1));
        self.view.center = (cx, cy);
        self.rebuild();
    }

    // ---- 新建 / 删除 ----

    /// 空白文档:一个分组、一个图层、一个框(光标得有地方站)
    pub fn scratch() -> Document {
        let mut d = Document::from_parts(
            "graph-tui",
            vec![Group {
                id: "g0".to_string(),
                label: String::new(),
                color: (76, 142, 248),
            }],
            vec![Layer {
                name: String::new(),
                order: 0,
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ViewState::default(),
            Legend::default(),
        )
        .expect("空白文档不可能是非法的");
        d.add_box("a", (0, 0), BoxStyle::default())
            .expect("空白文档里加框不会失败");
        d
    }

    /// 没被占用的框 id:`b1`、`b2`…(取最小的空号,删了再加不会一路上涨)
    fn free_box_id(&self) -> String {
        let mut n = 1;
        loop {
            let id = format!("b{n}");
            if self.box_idx(&id).is_none() && !self.texts.iter().any(|t| t.id == id) {
                return id;
            }
            n += 1;
        }
    }

    /// 没被占用的文字 id
    fn free_text_id(&self) -> String {
        let mut n = 1;
        loop {
            let id = format!("t{n}");
            if !self.texts.iter().any(|t| t.id == id) && self.box_idx(&id).is_none() {
                return id;
            }
            n += 1;
        }
    }

    /// 新建一个框(带文字),放在左上角 at;尺寸按文字算.返回框下标
    pub fn add_box(
        &mut self,
        label: &str,
        at: (i32, i32),
        style: BoxStyle,
    ) -> Result<usize, String> {
        let id = self.free_box_id();
        self.boxes.push(Box {
            id: id.clone(),
            at,
            size: (4, 3),
            style,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        });
        self.texts.push(Text {
            id: self.free_text_id(),
            text: label.to_string(),
            at,
            box_id: id.clone(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        });
        self.rebuild();
        let i = self
            .box_idx(&id)
            .ok_or_else(|| format!("cannot add box {id}"))?;
        if style.is_boxed() {
            self.boxes[i].at = at;
            self.fit_box(i);
            if let Some(ti) = self.text_of(i) {
                self.texts[ti].at = (at.0 + 1, at.1 + 1);
            }
        } else if matches!(style, BoxStyle::None) {
            self.boxes[i].at = at;
            self.fit_box(i);
        }
        self.rebuild();
        Ok(i)
    }

    /// 新建一条边.重复(同一对端点、同一分组)拒绝;自环允许
    pub fn add_edge(
        &mut self,
        from: &str,
        to: &str,
        group: &str,
        dir: Dir,
    ) -> Result<usize, String> {
        if self.box_idx(from).is_none() || self.box_idx(to).is_none() {
            return Err(format!("no such box: {from} -> {to}"));
        }
        if self.group_idx(group).is_none() {
            return Err(format!("no such group: {group}"));
        }
        let (a, b) = key_pair(from, to, dir);
        let dup = self.edges.iter().any(|e| {
            let (ea, eb) = key_pair(&e.from, &e.to, e.dir);
            ea == a && eb == b && e.group == group
        });
        if dup {
            return Err(format!("edge already exists: {from} -> {to}"));
        }
        let mut n = 1;
        let id = loop {
            let id = format!("e{n}");
            if !self.edges.iter().any(|e| e.id == id) {
                break id;
            }
            n += 1;
        };
        self.edges.push(DocEdge {
            id,
            from: from.to_string(),
            to: to.to_string(),
            dir,
            group: group.to_string(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            corner: Corner::Rounded,
            layer: String::new(),
        });
        self.rebuild();
        Ok(self.edges.len() - 1)
    }

    /// 删掉一个框:连它的文字与所有沾边的边走
    pub fn remove_box(&mut self, id: &str) -> Result<usize, String> {
        let Some(i) = self.box_idx(id) else {
            return Err(format!("no such box: {id}"));
        };
        let id = self.boxes[i].id.clone();
        self.boxes.remove(i);
        self.texts.retain(|t| t.box_id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
        self.rebuild();
        Ok(i)
    }

    /// 删掉一条边(按边 id)
    pub fn remove_edge(&mut self, id: &str) -> Result<(), String> {
        let Some(i) = self.edges.iter().position(|e| e.id == id) else {
            return Err(format!("no such edge: {id}"));
        };
        self.edges.remove(i);
        // 注释框跟着边一起走(同一步撤销)
        let notes: Vec<String> = self
            .boxes
            .iter()
            .filter(|b| b.edge.as_deref() == Some(id))
            .map(|b| b.id.clone())
            .collect();
        for nid in notes {
            self.boxes.retain(|b| b.id != nid);
            self.texts.retain(|t| t.box_id != nid);
        }
        self.rebuild();
        Ok(())
    }

    // ---- 搜索 ----

    /// 数字当分组号,其余当文字/别名;分数越小越准
    pub fn search(&self, text: &str) -> Hits {
        let t = text.trim();
        if t.is_empty() {
            return Hits::Empty;
        }
        if t.chars().all(|c| c.is_ascii_digit()) {
            let hits: Vec<usize> = (0..self.groups.len())
                .filter(|&i| {
                    let label = self.groups[i].label.trim();
                    let lead: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
                    lead == t || label == t
                })
                .collect();
            if !hits.is_empty() {
                return Hits::Groups(hits);
            }
        }
        let lower = t.to_lowercase();
        let latin = crate::search::latin(t);
        let mut scored: Vec<(u8, usize)> = Vec::new();
        for (i, b) in self.boxes.iter().enumerate() {
            let label = self.text(i).map(|x| x.text.as_str()).unwrap_or("");
            if let Some(s) = score(label, &b.terms, t, &lower, &latin) {
                scored.push((s, i));
            }
        }
        scored.sort_unstable();
        let hits: Vec<usize> = scored.into_iter().map(|(_, i)| i).collect();
        if hits.is_empty() {
            Hits::Empty
        } else {
            Hits::Boxes(hits)
        }
    }
}

/// 搜索命中
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hits {
    Groups(Vec<usize>),
    Boxes(Vec<usize>),
    Empty,
}

/// 一个名字/别名与查询串的匹配分,越小越准
fn score(label: &str, terms: &[String], text: &str, lower: &str, latin: &str) -> Option<u8> {
    if label == text {
        return Some(0);
    }
    let ll = crate::text::plain(label).to_lowercase();
    if !lower.is_empty() {
        if ll.starts_with(lower) {
            return Some(1);
        }
        if ll.contains(lower) {
            return Some(2);
        }
    }
    if latin.is_empty() {
        return None;
    }
    let mut best: Option<u8> = None;
    for term in terms {
        let l = crate::search::latin(term);
        if l.is_empty() {
            continue;
        }
        let s = if l == latin {
            3
        } else if l.starts_with(latin) {
            4
        } else if !l.contains(' ') && !latin.contains(' ') && crate::search::initials(term) == latin
        {
            5
        } else if l.contains(latin) {
            6
        } else {
            continue;
        };
        best = Some(best.map_or(s, |b: u8| b.min(s)));
    }
    best
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;

    fn doc() -> Document {
        let g = Group {
            id: "g0".into(),
            label: "1".into(),
            color: (76, 142, 248),
        };
        let l = Layer {
            name: String::new(),
            order: 0,
        };
        let b1 = Box {
            id: "b1".into(),
            at: (2, 2),
            size: (9, 3),
            style: BoxStyle::Rounded,
            color: None,
            fill: None,
            layer: String::new(),
            terms: vec!["central".into()],
            edge: None,
        };
        let b2 = Box {
            id: "b2".into(),
            at: (2, 8),
            size: (1, 1),
            style: BoxStyle::Point,
            color: Some((1, 2, 3)),
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        };
        let t1 = Text {
            id: "t1".into(),
            text: "中央".into(),
            at: (3, 3),
            box_id: "b1".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        };
        let t2 = Text {
            id: "t2".into(),
            text: "点".into(),
            at: (2, 8),
            box_id: "b2".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        };
        let e1 = DocEdge {
            id: "e1".into(),
            from: "b1".into(),
            to: "b2".into(),
            dir: Dir::Forward,
            group: "g0".into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: vec![(4, 6)],
            corner: Corner::Rounded,
            layer: String::new(),
        };
        Document::from_parts(
            "graph-tui 0.2.0",
            vec![g],
            vec![l],
            vec![b1, b2],
            vec![t1, t2],
            vec![e1],
            ViewState::default(),
            crate::document::Legend::default(),
        )
        .unwrap()
    }

    #[test]
    fn 派生结构与包围盒() {
        let d = doc();
        assert_eq!(d.box_idx("b2"), Some(1));
        assert_eq!(d.text_of(0), Some(0));
        assert_eq!(d.text(1).unwrap().text, "点");
        assert_eq!(d.adj(0), &[0]);
        assert!(!d.hub(0));
        assert_eq!(d.bbox(), (2, 2, 10, 8));
        assert_eq!(d.series().len(), 1);
        assert_eq!(d.series()[0].1, vec![0, 1]);
    }

    /// 多行文字:框按行数长高、按最长行长宽;文字装不下要报错
    #[test]
    fn 多行文字按行长大() {
        let mut d = Document::scratch();
        d.set_text(0, "第一行\n第二行更长\n三");
        let b = &d.boxes[0];
        // none:高 = 行数,宽 = 最长行
        assert_eq!(b.size, (10, 3));
        assert_eq!(d.text(0).unwrap().at, b.at);
        assert_eq!(text_metrics("第一行\n第二行更长\n三"), (3, 10));
        // 带框:高 = 行数 + 2,宽 = 最长行 + 2
        d.boxes[0].style = BoxStyle::Rounded;
        d.fit_box(0);
        assert_eq!(d.boxes[0].size, (12, 5));
        assert_eq!(d.text(0).unwrap().at, (d.boxes[0].at.0 + 1, d.boxes[0].at.1 + 1));
        d.validate().expect("装得下就该过");

        // 高度不够:校验拒
        d.boxes[0].size = (12, 4);
        assert!(d.validate().is_err(), "文字放不下要报错");
        d.boxes[0].size = (11, 5);
        assert!(d.validate().is_err(), "宽度不够也要报错");
        d.boxes[0].size = (12, 5);

        // 点节点只有名字:换行不合法
        d.boxes[0].style = BoxStyle::Point;
        d.boxes[0].size = (1, 1);
        assert!(d.validate().is_err(), "点节点不能换行");
    }

    /// 注释框:严格一对一、悬空引用、只允许 none 框、删边级联
    #[test]
    fn 注释框的一对一与级联() {
        let mut d = Document::scratch();
        let id = d.boxes[0].id.clone();
        let g = d.groups[0].id.clone();
        d.add_edge(&id, &id, &g, Dir::Forward).unwrap();
        let b = d.new_note(0).expect("该能建注释框");
        assert_eq!(d.note_of_edge(&d.edges[0].id), Some(b));
        assert_eq!(d.edge_of_box(b), Some(0));
        d.set_text(b, "收到 ACK");
        assert_eq!(d.text(b).unwrap().text, "收到 ACK");

        // 同一条边再来一个框:校验该拒
        let mut second = d.boxes[b].clone();
        second.id = "n2".into();
        let mut t2 = d.texts[d.text_of(b).unwrap()].clone();
        t2.id = "t2".into();
        t2.box_id = "n2".into();
        let mut bad = d.clone();
        bad.boxes.push(second.clone());
        bad.texts.push(t2.clone());
        bad.rebuild();
        assert!(bad.validate().is_err(), "一条边只能有一个注释框");

        // 悬空引用
        let mut bad = d.clone();
        bad.boxes[b].edge = Some("nope".into());
        bad.rebuild();
        assert!(bad.validate().is_err(), "绑到不存在的边要报错");

        // 带框的节点不能当注释框
        let mut bad = d.clone();
        bad.boxes[0].edge = Some(d.edges[0].id.clone());
        assert!(bad.validate().is_err(), "只有 none 框能当注释");

        // 删边连注释框一起走
        let n0 = d.boxes.len();
        let eid = d.edges[0].id.clone();
        d.remove_edge(&eid).unwrap();
        assert_eq!(d.boxes.len(), n0 - 1, "注释框该跟着边删掉");
        assert!(d.note_of_edge(&eid).is_none());
    }

    /// 注释框的建议位置不压在别的框上
    #[test]
    fn 注释位置避开别的框() {
        let mut d = Document::scratch();
        let id = d.boxes[0].id.clone();
        let g = d.groups[0].id.clone();
        d.add_edge(&id, &id, &g, Dir::Forward).unwrap();
        let spot = d.note_spot(0);
        let hit = |p: (i32, i32), s: &Document| {
            s.boxes.iter().any(|b| {
                let (x, y) = (b.at.0, b.at.1);
                p.0 >= x - 1 && p.0 <= x + b.size.0 as i32 && p.1 >= y - 1 && p.1 <= y + b.size.1 as i32
            })
        };
        assert!(!hit(spot, &d), "建议位置不能压在框上:{spot:?}");
    }

    /// 自环:邻接只记一次,不参与串联,校验放行
    #[test]
    fn 自环可以写进来() {
        let mut d = Document::scratch();
        let id = d.boxes[0].id.clone();
        let g = d.groups[0].id.clone();
        d.add_edge(&id, &id, &g, Dir::Forward).expect("自环该收下");
        assert_eq!(d.edges.len(), 1);
        assert_eq!(d.edges[0].from, d.edges[0].to);
        assert_eq!(d.adj(0), &[0], "邻接表里只记一次,不能重复");
        assert!(d.series().is_empty(), "自环不进线路");
        // 同一个自环再来一次还是重复
        assert!(d.add_edge(&id, &id, &g, Dir::Forward).is_err());
    }

    #[test]
    fn 颜色回退到分组() {
        let d = doc();
        assert_eq!(d.box_color(0), Some((76, 142, 248)));
        assert_eq!(d.box_color(1), Some((1, 2, 3)));
        assert_eq!(d.edge_color(0), (76, 142, 248));
    }

    /// 转四份回到原样,文字始终在框里
    #[test]
    fn 旋转四次回到原点且文字跟着框() {
        // 用带框的节点:文字要跟着框的内侧走(默认样式 none 下文字就压在框的左上角)
        let mut d = Document::scratch();
        d.boxes[0].style = BoxStyle::Rounded;
        d.fit_box(0);
        let b0 = d.boxes[0].clone();
        let t0 = d.texts[0].clone();
        let mut seen = Vec::new();
        for _ in 0..4 {
            d.rotate_quarters(1);
            seen.push(d.boxes[0].at);
            let b = &d.boxes[0];
            let t = &d.texts[0];
            assert_eq!(t.at, (b.at.0 + 1, b.at.1 + 1), "文字该在框内起点");
            assert_eq!(b.size, b0.size, "宽高不交换");
        }
        assert_eq!(d.boxes[0].at, b0.at, "转四份该回到原处");
        assert_eq!(d.texts[0].at, t0.at);
        assert!(seen.iter().any(|at| *at != b0.at), "中间该动过");
    }

    /// 折点跟着转
    #[test]
    fn 旋转带着折点走() {
        let mut d = Document::scratch();
        d.add_box("b", (20, 0), BoxStyle::Rounded).unwrap();
        d.add_edge("b1", "b2", "g0", Dir::Forward).unwrap();
        d.edges[0].via = vec![(30, 5)];
        d.rotate_quarters(1);
        assert_eq!(d.edges[0].via, vec![(-5, 30)], "(x,y) -> (-y,x)");
    }

    #[test]
    fn 移动框时文字跟着走() {
        let mut d = doc();
        d.move_box(0, 3, -1);
        assert_eq!(d.boxes[0].at, (5, 1));
        assert_eq!(d.texts[0].at, (6, 2));
        assert_eq!(d.bbox(), (2, 1, 13, 8));
    }

    #[test]
    fn 换文字会重算框并同步文字位置() {
        let mut d = doc();
        d.set_text(0, "**很长** 的名字");
        // 去掉标记后 6 个全角/半角混排:宽度按 plain_width 算
        let w = crate::text::plain_width("**很长** 的名字");
        assert_eq!(d.boxes[0].size, ((w.max(2) + 2).max(4) as u16, 3));
        assert_eq!(d.texts[0].at, (3, 3));
        // 点节点尺寸不变
        d.set_text(1, "很长的点名字");
        assert_eq!(d.boxes[1].size, (1, 1));
    }

    #[test]
    fn 同组同一对节点的边会被去重() {
        let base = doc();
        let (groups, layers, boxes, texts) = (
            base.groups.clone(),
            base.layers.clone(),
            base.boxes.clone(),
            base.texts.clone(),
        );
        let mk = |id: &str, from: &str, to: &str, dir: Dir| DocEdge {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            dir,
            group: "g0".to_string(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            corner: Corner::Rounded,
            layer: String::new(),
        };
        // 无向:a-b 与 b-a 算同一条
        let d = Document::from_parts(
            "t",
            groups.clone(),
            layers.clone(),
            boxes.clone(),
            texts.clone(),
            vec![
                mk("e1", "b1", "b2", Dir::None),
                mk("e2", "b2", "b1", Dir::None),
            ],
            ViewState::default(),
            crate::document::Legend::default(),
        )
        .unwrap();
        assert_eq!(d.edges.len(), 1);
        // 有向:a->b 与 b->a 是两条
        let d = Document::from_parts(
            "t",
            groups,
            layers,
            boxes,
            texts,
            vec![
                mk("e1", "b1", "b2", Dir::Forward),
                mk("e2", "b2", "b1", Dir::Forward),
            ],
            ViewState::default(),
            crate::document::Legend::default(),
        )
        .unwrap();
        assert_eq!(d.edges.len(), 2);
    }

    #[test]
    fn 点节点只有一个格子() {
        let d = doc();
        assert_eq!(d.cells(1).collect::<Vec<_>>(), vec![(2, 8)]);
        assert_eq!(d.cells(0).count(), 9 * 3);
        assert_eq!(d.box_at_cell((3, 3)), Some(0));
    }

    #[test]
    fn 搜索命中分组与框() {
        let d = doc();
        assert_eq!(d.search("1"), Hits::Groups(vec![0]));
        assert_eq!(d.search("中央"), Hits::Boxes(vec![0]));
        assert_eq!(d.search("central"), Hits::Boxes(vec![0]));
        assert_eq!(d.search("zzz"), Hits::Empty);
    }
}
