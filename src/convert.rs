//! `.dot` / `.canvas` 的中间图(Graph)与 native 文档(Document)之间的转换.
//!
//! 导入有两条入口:
//!   - `from_graph` 给 `.dot`:节点坐标是米,先按 `scene::nominal_for` 的标称画布
//!     线性投影成单元格,y 方向按 2:1 压缩,四舍五入.
//!   - `from_graph_cells` 给 `.canvas`:坐标已经除过像素换算,直接就是单元格.
//!
//! 导出只有一条 `to_graph`:尽力而为地在 Graph 里保留框、文字、边、分组、图层,
//! 表达不了的部分交给调用方作为警告报给用户.

use std::collections::HashSet;

use crate::document::{
    Box as DocBox, DocEdge, Document, Group as DocGroup, Layer, Rgb, Text, ViewState,
};
use crate::model::{Align, BoxStyle, Corner, Graph, PALETTE};
use crate::scene::nominal_for;

/// 导入时可选的样式覆盖,按 Graph 的节点/边下标对齐.
/// 缺项一律用文档默认值:`fill` 为 None、`align` 为 Left、`corner` 为 Rounded.
/// 只有能从源格式读到这些字段的入口会填它(`.dot` 的自定义属性).
/// 一条边的两端强制端口(from, to);None = auto
pub type EdgePorts = (Option<crate::model::Side>, Option<crate::model::Side>);

#[derive(Clone, Copy, Debug, Default)]
pub struct NodeStyle {
    /// 框背景填充
    pub fill: Option<Rgb>,
    /// 文字水平对齐
    pub align: Align,
    /// 文字颜色
    pub text_color: Option<Rgb>,
    /// 文字底色
    pub text_bg: Option<Rgb>,
    /// 框尺寸 `"WxH"`;没写就按文字算
    pub size: Option<(u16, u16)>,
    /// 注释框:绑到第几条边(1 起,按文件里边的顺序)
    pub note: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct EdgeOverride {
    /// 拐角样式
    pub corner: Corner,
    /// 两端强制端口
    pub ports: EdgePorts,
    /// 箭头字形
    pub head: Option<crate::model::ArrowStyle>,
    /// 线的背景色
    pub bg: Option<Rgb>,
    /// 边上写的注释文字(没有注释框的 `.dot` 用它新建注释框)
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportStyles {
    /// 每个节点的样式(和 `Graph::nodes` 同下标)
    pub nodes: Vec<NodeStyle>,
    /// 每条边的样式(和 `Graph::edges` 同下标)
    pub edges: Vec<EdgeOverride>,
}

/// 投影:把输入坐标换成单元格.`.dot` 的米制坐标需要缩放,`.canvas` 已经是格子.
#[derive(Clone, Copy)]
enum Projection {
    Geo(Geo),
    Cells,
    /// 本项目的 `.dot` 导出:`pos` 是框中心的单元格,y 向上(翻转后即文档坐标)
    CentreUp,
}

/// `.dot` 坐标的线性投影参数:先把 bbox 的左上角平移到原点,
/// x 乘 s,y 乘 s/2(字符格一行等于两列高),再翻成屏幕方向(y 向下).
#[derive(Clone, Copy)]
struct Geo {
    x0: f64,
    /// bbox 的上沿(最大 y);翻 y 用
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

    /// 投影的浮点结果(取整前):CentreUp 是框中心的格子坐标
    fn point(&self, p: (f64, f64)) -> (f64, f64) {
        match self {
            Projection::Geo(g) => g.point(p),
            Projection::Cells => p,
            Projection::CentreUp => (p.0, -p.1),
        }
    }

    /// 投出来的格子是框的中心(而不是左上角)吗
    fn centre_anchored(&self) -> bool {
        matches!(self, Projection::CentreUp)
    }
}

/// 有限值取整;非有限值退成 0,不让 NaN 变成随机的 i32
fn round_i32(v: f64) -> i32 {
    if v.is_finite() {
        v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
    } else {
        0
    }
}

/// 按标称画布算一次线性投影.点集为空时给一个安全的退化参数
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
    // 纵向容量是 rows,压缩之后每米只占 s/2 行,所以高度方向允许 s 大到 rows*2/gh
    let s = (cols / gw).min(rows * 2.0 / gh);
    Geo {
        x0,
        y1,
        s,
        ox: (w - gw * s) / 2.0,
        oy: (h - gh * s * 0.5) / 2.0,
    }
}

/// 两个节点的中心落到同一格时,沿方形环往外找最近的空格.
/// 只要求错开至少一格,不负责解决框与框的尺寸重叠
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

/// `.dot` 导入:每个节点一个 box 加一条 text,坐标按 geo 投影到单元格.
pub fn from_graph(g: &Graph, view: ViewState, source: &str) -> Result<Document, String> {
    from_graph_styles(g, &ImportStyles::default(), view, source)
}

/// 同 `from_graph`,但带上源格式里读到的样式覆盖(框填充、文字对齐、边拐角).
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
    // 我们自己导出的 .dot 带 `pos_unit="cell"`:坐标已经是单元格,原样还原,
    // 既不缩放到标称画布,也不错开重叠的中心(重叠是文档本来的样子)
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
    // 没有坐标的 .dot 只能先摆一个默认样子:导入时跑一次布局,否则所有节点会叠在原点.
    // (native 文档打开时不重排;这里是一次性转换.)
    if !g.has_pos() {
        return laid_out(doc);
    }
    Ok(doc)
}

/// 导入时跑一次布局:选一套合适的算法,把整数单元格坐标写回.
fn laid_out(mut doc: Document) -> Result<Document, String> {
    let l = crate::layout::pick(&doc);
    crate::layout::apply(&mut doc, l)?;
    Ok(doc)
}

/// `.canvas` 导入:坐标已经是单元格,直接使用(只做取整).
pub fn from_graph_cells(g: &Graph, view: ViewState, source: &str) -> Result<Document, String> {
    let proj = Projection::Cells;
    let centers: Vec<(f64, f64)> = g
        .nodes
        .iter()
        .map(|n| proj.point(n.pos.unwrap_or((0.0, 0.0))))
        .collect();
    build_doc(g, view, source, &centers, proj, &ImportStyles::default())
}

/// 两条入口共用的转换.`centers` 是取整前的投影点(Geo/Cells 是左上角,CentreUp 是中心).
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

    // 分组:一条 Graph 分组对应一条文档分组.id 稳定用 gN,顺序照搬
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
    // 没有分组名的边(以及分组下标越界的边)落到默认分组
    let default_group = groups
        .iter()
        .position(|x| x.label.is_empty())
        .map(|i| groups[i].id.clone())
        .unwrap_or_else(|| groups[0].id.clone());

    // 图层:默认层必须存在且排第一,其余按首次出现
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
        // 让 `size` 这么大的框,其中心落在投影点上(取整在最后一步做,
        // 半格的宽框才不会因为先取整而偏一格)
        let centred = |size: (u16, u16)| {
            (
                round_i32(c.0 - (size.0 as f64 - 1.0) / 2.0),
                round_i32(c.1 - (size.1 as f64 - 1.0) / 2.0),
            )
        };
        let corner = (round_i32(c.0), round_i32(c.1));
        // 框尺寸:`.dot` 里显式写了 size 就用它(手工改过的框宽高),
        // 否则按文字算 —— 只有文字的框是"最长行 x 行数",带框的每边多 1 格
        let (lines, widest) = crate::document::text_metrics(&node.label);
        let derived = if matches!(node.box_style, BoxStyle::None) {
            (widest.max(1) as u16, lines as u16)
        } else if node.box_style.is_boxed() {
            ((widest.max(2) + 2).max(4) as u16, (lines + 2) as u16)
        } else {
            (1, 1)
        };
        let size = st.size.unwrap_or(derived);
        // 只有文字的点/框:左上角就落在投影格上(我方导出的 .dot 里投影格是中心,
        // 那就把中心反推成左上角);带框的框心落在投影点上
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
    // 边的 label 变成"绑在这条边上的文字框":先记下 (边下标, 文字, 颜色), 之后统一摆放
    let mut notes: Vec<(usize, String)> = Vec::new();
    // 节点自己声明是注释框时(`note="N"`),那条边不再由 label 新建一个,
    // 否则同一段注释会长出两个框
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

    // `note="N"`:把那个框绑到第 N 条边上(成员已在上面按文件顺序建好)
    for (i, st) in styles.nodes.iter().enumerate() {
        let Some(n) = st.note else { continue };
        let Some(idx) = n.checked_sub(1).filter(|k| *k < edges.len()) else {
            continue;
        };
        if let Some(bx) = boxes.get_mut(i) {
            bx.edge = Some(edges[idx].id.clone());
        }
    }

    // 注释框:放在两端中心的中点往右一格;位置一次性算好,之后由用户摆
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

/// native 文档 -> 中间图(导出用).失真的地方作为警告返回,不阻止导出.
pub fn to_graph(doc: &Document) -> Result<(Graph, Vec<String>), String> {
    if doc.boxes.is_empty() {
        return Err("cannot export a document with no boxes".to_string());
    }
    let mut b = Graph::builder();
    for (i, bx) in doc.boxes.iter().enumerate() {
        let label = doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
        // 节点坐标给框的中心(单元格);.dot 的 pos 只是尽力而为地表达它
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

    // 分组按文档顺序登记,记下每个文档分组在 Graph 里的下标.
    // 同名分组合并是 Graph 本身的限制,和 .dot 的语义一致
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
        // 同名分组会塌成同一个下标;取第一个登记的即可
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

/// 导出 `.dot` 一定存在的失真项.每次导出都逐条报出来,由调用方决定怎么显示
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
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
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
    fn dot_导入成框文字与边() {
        let d = sample();
        assert_eq!(d.boxes.len(), 3);
        assert_eq!(d.texts.len(), 3);
        assert_eq!(d.edges.len(), 2);

        // 有框节点:宽按文字显示宽 + 2 且至少 4,高固定 3;文字在框内左上 + (1,1)
        assert_eq!(d.boxes[0].style, BoxStyle::Single);
        let w0 = (plain_width("甲站").max(2) + 2).max(4) as u16;
        assert_eq!(d.boxes[0].size, (w0, 3));
        assert_eq!(d.texts[0].text, "甲站");
        assert_eq!(d.texts[0].at, (d.boxes[0].at.0 + 1, d.boxes[0].at.1 + 1));
        assert_eq!(d.boxes[0].color, Some((0x11, 0x22, 0x33)));
        assert!(d.boxes[0].terms.contains(&"jia".to_string()));
        assert!(d.boxes[0].terms.contains(&"甲".to_string()));
        assert_eq!(d.boxes[0].layer, "one");

        // 没写 box= 的节点:默认样式是 none(只有一行字,宽随文字)
        assert_eq!(d.boxes[2].style, BoxStyle::None);
        assert_eq!(d.boxes[2].size.1, 1);
        assert_eq!(d.texts[2].at, d.boxes[2].at);
        assert_eq!(d.boxes[2].layer, "two");

        // 边:有向/无向、样式、颜色、折点、图层
        assert_eq!(d.edges[0].dir, Dir::Forward);
        assert_eq!(d.edges[1].dir, Dir::None);
        assert_eq!(d.edges[0].style, EdgeStyle::Dashed);
        assert_eq!(d.edges[0].color, Some((0xdf, 0x46, 0x61)));
        assert_eq!(d.edges[0].via.len(), 2);
        assert_eq!(d.edges[0].layer, "one");
        assert!(d.edges[0].via.iter().all(|&(x, y)| x > 0 && y > 0));

        // 分组与图层
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
    fn 四态方向经转换原样进文档() {
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
        // 再导出回中间图,方向仍然直传
        let (back, _) = to_graph(&d).unwrap();
        let dirs: Vec<Dir> = back.edges.iter().map(|e| e.dir).collect();
        assert_eq!(dirs, vec![Dir::Forward, Dir::None, Dir::Back, Dir::Both]);
    }

    #[test]
    fn 导入的样式字段取默认值() {
        let d = sample();
        assert!(d.boxes.iter().all(|b| b.fill.is_none()));
        assert!(d.texts.iter().all(|t| t.align == Align::Left));
        assert!(d.edges.iter().all(|e| e.corner == Corner::Rounded));
    }

    #[test]
    fn 导入时可以用样式覆盖() {
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
        // 没给覆盖的项还是默认值
        assert_eq!(d.boxes[1].fill, None);
        assert_eq!(d.texts[1].align, Align::Left);
        assert_eq!(d.edges[1].corner, Corner::Rounded);
    }

    #[test]
    fn 几何投影后坐标仍是整数且同坐标节点错开() {
        let text = r#"graph {
            "a" [pos="0,0"];
            "b" [pos="0,0"];
            "c" [pos="100000,50000"];
            "a" -- "b";
            "b" -- "c";
        }"#;
        let g = Graph::from_dot(text).unwrap();
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        // 点节点都占一格,位置互不相同(至少错开一格)
        assert_ne!(d.boxes[0].at, d.boxes[1].at);
        assert_ne!(d.boxes[1].at, d.boxes[2].at);
        // 投影后落在一个合理的画布范围内,而不是把米直接当格
        for b in &d.boxes {
            assert!(b.at.0.abs() < 1000, "{:?}", b.at);
            assert!(b.at.1.abs() < 1000, "{:?}", b.at);
        }
    }

    #[test]
    fn 导入后能搜索文字与别名() {
        let d = sample();
        assert_eq!(d.search("甲站"), Hits::Boxes(vec![0]));
        assert_eq!(d.search("jia"), Hits::Boxes(vec![0]));
    }

    #[test]
    fn 导入后保存再加载关键字段保持() {
        let d = sample();
        let back = Document::load(&d.save()).unwrap();
        assert_eq!(back.boxes, d.boxes);
        assert_eq!(back.texts, d.texts);
        assert_eq!(back.edges, d.edges);
        assert_eq!(back.groups, d.groups);
        assert_eq!(back.layers, d.layers);
    }

    #[test]
    fn 自环也导进来() {
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

    /// 自己导出的 .dot 带 `pos_unit="cell"`:原样还原,负坐标、偶数宽、有框/点节点都一样
    #[test]
    fn 导出的_dot_能原样还原坐标() {
        let mut d = Document::scratch();
        // 负坐标、偶数宽(中心落在半格)、三种样式各来一个
        d.add_box("abcd", (-7, -3), BoxStyle::None).unwrap();
        d.add_box("框", (12, 4), BoxStyle::Single).unwrap();
        // 点节点是 1x1:`add_box` 建出来的是默认框,改成点样式后自己定位
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

    /// 多行文字:导出把换行写成 `\n`,导入按行数定框高,不再"放不下"
    #[test]
    fn 多行文字往返() {
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

    /// 同名/空文字的框不再合并:导出用框 id 当节点名,文字走 `label`
    #[test]
    fn 同名与空文字不合并() {
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
        // scratch 自带一个框 a,再加五个
        assert_eq!(back.boxes.len(), 6, "六框一个都不该少:{dot}");
        let got: Vec<String> = back.texts.iter().map(|t| t.text.clone()).collect();
        assert_eq!(got, vec!["a", "same", "same", "", "", ""], "{dot}");
        // 名字只是 id,不进搜索别名
        assert!(
            back.boxes
                .iter()
                .all(|b| !b.terms.iter().any(|t| t.starts_with('b'))),
            "{dot}"
        );
    }

    /// 手写的多行 `.dot`(没有 size=)按行数定高:纯文字 = 行数,带框 = 行数 + 2
    #[test]
    fn 没有尺寸的多行节点按行数长高() {
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

    /// 手工改过尺寸的框靠 `size=` 原样回来(纯文字框加宽、带框加高)
    #[test]
    fn 框尺寸往返() {
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

    /// 文字颜色、文字底色、箭头字形、线的底色都走自定义属性,能读回来
    #[test]
    fn 文字与箭头的样式往返() {
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

    /// 注释框不会被复制:它作为节点带 `note=N`,那条边不再从 label 新建一个
    #[test]
    fn 注释框往返不复制() {
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
        // 外来 `.dot`(只有 label)照旧新建注释框
        let foreign = "digraph { a [pos=\"0,0\"]; b [pos=\"10,0\"]; a -> b [label=\"hi\"]; }";
        let fd = crate::dot::parse_document(foreign, ViewState::default(), "dot").unwrap();
        assert_eq!(fd.boxes.len(), 3, "label 该变出一个注释框");
    }

    /// 分组配色:边没写自己的颜色时导出把分组色写实,导入再从"同组同色"反推回来
    #[test]
    fn 分组配色往返() {
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

    /// 区域的配色、边框、留白、折叠、图层都写进 cluster 属性
    #[test]
    fn 区域样式往返() {
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

    /// 没有 pos_unit 的外来 .dot 仍旧重投影(缩放到标称画布、翻 y),不会被当成单元格
    #[test]
    fn 外来_dot_坐标仍然重投影() {
        let text = "digraph { \"a\" [pos=\"0,0\"]; \"b\" [pos=\"40,20\"]; \"a\" -> \"b\"; }";
        let g = Graph::from_dot(text).unwrap();
        let d = from_graph(&g, ViewState::default(), "dot").unwrap();
        // 两点的中心相距很远(40 x 20 单位),说明确实乘了缩放;且 y 被翻过(上行的 b 在上)
        let dx = (d.boxes[1].at.0 - d.boxes[0].at.0).abs();
        assert!(dx > 10, "该被放大到画布:{dx}");
        assert!(d.boxes[0].at.1 > d.boxes[1].at.1, "y 向上的 b 该在 a 上面");
    }

    #[test]
    fn 导出中间图保留框样式与文字() {
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

        // 失真项必须逐条列出
        assert!(warnings.iter().any(|w| w.contains("pos")));
        assert!(warnings.iter().any(|w| w.contains("Graphviz 会忽略")));
        assert!(warnings.iter().any(|w| w.contains("缩放")));
        assert!(warnings.iter().any(|w| w.contains("同名分组会合并")));
    }

    #[test]
    fn 空文档导出报错() {
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
