//! .dot 子集解析(通用图,不再是地铁专用).
//!
//! 认这几种写法,其余一律忽略:
//!   strict? graph|digraph 名字 { ... }      图的类型(digraph -> 边默认有箭头)
//!   node [k=v, ...];  /  edge [k=v, ...];  默认属性(只对后面出现的元素生效)
//!   "甲" [pos="x,y", label="显示名", terms="a|b"];      节点
//!   "甲" [box="single", color="#rrggbb"];                 外框与节点自己的颜色
//!   "甲" [fill="#rrggbb", align="center"];                框填充与文字对齐(自定义属性)
//!   "甲" -- "乙" [color="#rrggbb", group="主线"];        无向边
//!   "甲" -> "乙" [color="#rrggbb", group="主线"];        有向边
//!   "甲" -> "乙" [style="dashed", via="1,2 3,4"];        线型与手工折点
//!   "甲" -> "乙" [dir="none|forward|back|both"];         四个方向覆盖默认值
//!   "甲" -> "乙" [corner="rounded|sharp"];                边拐角(自定义属性)
//!   "甲" <- "乙" [..];                                    有向边,交换端点后按 forward 处理
//!   "甲" "乙" [..];                                       没写箭头,方向由图类型定
//! 值都是 utf-8 字符串,引号里只认 `\"` 与 `\\` 两种转义(写回时也只产生这两种).
//! 子图(subgraph)当透明容器:里面的节点与边照收.
//! 边端点没声明过就隐式建一个节点(graphviz 的语义),所以 `"a" -> "b";` 一句就够;
//! 节点顺序永远是首次出现的顺序,名字大小写敏感,一个名字只对应一个节点.
//!
//! 分两趟:先把节点表建出来(边端点也算一次出现),再连边;边用名字写,
//! 靠这张表换成 Builder 的索引.

use std::collections::HashMap;

use crate::document::{Document, Rgb, ViewState};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Graph};

use crate::convert::ImportStyles;

/// 属性表:保持出现顺序,同名键以最后一次为准
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

    /// 只补自己缺的键:节点/边自己的属性优先于默认属性
    fn fill_from(&mut self, other: &Attrs) {
        for (k, v) in &other.kv {
            if self.get(k).is_none() {
                self.kv.push((k.clone(), v.clone()));
            }
        }
    }
}

/// 解析阶段的节点:label 缺省就拿名字当显示名
struct RawNode {
    name: String,
    label: Option<String>,
    pos: Option<(f64, f64)>,
    /// 搜索别名(不含 name)
    terms: Vec<String>,
    /// 外框;None = 没写这个属性(同名节点合并时还能被后面的定义补上)
    box_style: Option<BoxStyle>,
    color: Option<(u8, u8, u8)>,
    /// 框背景填充;None = 不填或没写
    fill: Option<Rgb>,
    /// 文字水平对齐;None = 没写这个属性
    align: Option<Align>,
    shift: Option<(f64, f64)>,
    layer: Option<String>,
}

/// 解析阶段的边:两端是节点下标
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
    /// 边标签(graphviz 的 label 属性)
    label: Option<String>,
}

pub fn parse(text: &str) -> Result<Graph, String> {
    parse_all(text).map(|(g, _)| g)
}

/// `.dot` 文本 -> native 文档,连同自定义样式属性一起导入.
pub fn parse_document(text: &str, view: ViewState, source: &str) -> Result<Document, String> {
    let (g, styles) = parse_all(text)?;
    crate::convert::from_graph_styles(&g, &styles, view, source)
}

/// 边在去重时的键:无向边按无序端点算,其余按有序端点算(和 model 的规则一致)
fn edge_key(a: usize, b: usize, dir: Dir, group: &str) -> (usize, usize, String) {
    let (a, b) = if dir != Dir::None || a < b {
        (a, b)
    } else {
        (b, a)
    };
    (a, b, group.to_string())
}

/// 解析出中间图和样式覆盖;`parse` 只用前者
fn parse_all(text: &str) -> Result<(Graph, ImportStyles), String> {
    let stmts = statements(text);
    let mut digraph = false;
    let mut node_def = Attrs::default();
    let mut edge_def = Attrs::default();
    let mut nodes: Vec<RawNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<RawEdge> = Vec::new();
    // 第一条非空语句里的 strict / graph / digraph 决定整张图的类型.
    // 开头可能有注释或空语句,所以不能拿循环下标当"第一条"
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
            // 子图是透明容器:只吃掉这一句,里面的节点与边由后面的语句照常收,
            // 所以不会凭空多出一个叫 subgraph 的节点
            "subgraph" | "strict" => continue,
            // 默认属性:后面出现的节点/边缺哪个键就从这里补
            "node" | "edge" => {
                let attrs = parse_attrs(&tok, 1);
                if tok[0] == "node" {
                    node_def = attrs;
                } else {
                    edge_def = attrs;
                }
                continue;
            }
            // graph [..] 是图级属性,图模型里没有对应字段,收下就丢
            _ if tok[0] == "graph" => continue,
            // 零碎的标点不是节点
            _ if matches!(tok[0].as_str(), "[" | "]" | "=" | "--" | "->" | "<-") => continue,
            // 图级属性赋值语句(rankdir=LR 之类)也不是节点
            _ if tok.get(1).map(String::as_str) == Some("=") => continue,
            _ => {}
        }
        // 边:中间那个词是箭头写法
        if let Some((a, b, dir, at)) = edge_parts(&tok, digraph)? {
            let mut attrs = parse_attrs(&tok, at);
            attrs.fill_from(&edge_def);
            // style 写坏了直接报错,免得静默画成实线
            let style = match attrs.get("style") {
                Some(v) => EdgeStyle::parse(v)
                    .ok_or_else(|| format!("bad edge style {v:?} between {a:?} and {b:?}"))?,
                None => EdgeStyle::Solid,
            };
            // dir 覆盖箭头/图类型定下的默认方向;认不出的值保留默认,不静默改语义
            let dir = match attrs.get("dir") {
                Some(v) => Dir::parse(v).unwrap_or(dir),
                None => dir,
            };
            let corner = attrs
                .get("corner")
                .and_then(Corner::parse)
                .unwrap_or_default();
            // group 与 line 都算分组名,两个都有就用 group
            let group = attrs
                .get("group")
                .filter(|s| !s.is_empty())
                .or_else(|| attrs.get("line").filter(|s| !s.is_empty()))
                .unwrap_or("");
            // 端点没声明过就按 `"名字";` 隐式建一个,顺序仍是首次出现的顺序
            let a = touch(&mut nodes, &mut index, &a, &node_def)?;
            let b = touch(&mut nodes, &mut index, &b, &node_def)?;
            let layer = attrs
                .get("layer")
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            // 边标签:多行的 `\n` 已经解成真换行,注释框放得下(框按行数长高);
            // 只剩 `\r`(回车)不收
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
                layer,
                label,
            });
            continue;
        }
        // 节点:名字 + 可选属性
        let mut attrs = parse_attrs(&tok, 1);
        attrs.fill_from(&node_def);
        merge_node(&mut nodes, &mut index, raw_of(&tok[0], &attrs)?);
    }

    // 节点按下标顺序进图,再连边
    let node_styles: Vec<(Option<Rgb>, Align)> = nodes
        .iter()
        .map(|n| (n.fill, n.align.unwrap_or_default()))
        .collect();
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
        // 显示名换成 label 之后,原来的节点名仍然能搜到
        if let Some(l) = &node.label {
            if l != &node.name {
                terms.push(&node.name);
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
        // 分组颜色一律交给 Builder 的调色板:dot 里的 color 现在只表示"这一条边"的颜色,
        // 不能再拿它决定分组颜色(否则第一条边的颜色会传染整个分组).
        // 这样同一分组的边在 dot 里都写同一个 color 时仍然同色.
        let g = b.group(&e.group, None);
        let idx = b.edge(e.a, e.b, g, e.dir);
        b.edge_style(idx, e.style);
        b.edge_color(idx, e.color);
        b.edge_via(idx, e.via.clone());
        if !e.layer.is_empty() {
            b.edge_layer(idx, e.layer.clone());
        }
    }
    let graph = b.build()?;

    // 边会被 Builder 去重,所以拐角按去重后的边重排;同一条边只留第一次出现的样式
    let mut corner_of: HashMap<(usize, usize, String), Corner> = HashMap::new();
    for e in &edges {
        corner_of
            .entry(edge_key(e.a, e.b, e.dir, &e.group))
            .or_insert(e.corner);
    }
    let corners: Vec<Corner> = graph
        .edges
        .iter()
        .map(|e| {
            let group = &graph.groups[e.group].label;
            corner_of
                .get(&edge_key(e.a, e.b, e.dir, group))
                .copied()
                .unwrap_or_default()
        })
        .collect();

    // 标签与拐角一样:去重后的每条边取第一次出现的那份
    let mut label_of: HashMap<(usize, usize, String), Option<String>> = HashMap::new();
    for e in &edges {
        label_of
            .entry(edge_key(e.a, e.b, e.dir, &e.group))
            .or_insert_with(|| e.label.clone());
    }
    let labels: Vec<Option<String>> = graph
        .edges
        .iter()
        .map(|e| {
            let group = &graph.groups[e.group].label;
            label_of
                .get(&edge_key(e.a, e.b, e.dir, group))
                .cloned()
                .flatten()
        })
        .collect();

    Ok((
        graph,
        ImportStyles {
            nodes: node_styles,
            corners,
            labels,
        },
    ))
}

/// 记下这个名字,返回节点下标:没声明过就按 `"名字";` 隐式建一个,
/// 所以隐式节点和显式声明一样吃当时的 node 默认属性
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

/// 同名节点只留一个:后一份定义只补前面缺的键
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

/// 从属性表里取出名字以外的东西.写坏的 box 直接报错,不静默降级
fn raw_of(name: &str, attrs: &Attrs) -> Result<RawNode, String> {
    let box_style = match attrs.get("box") {
        Some(v) => Some(
            BoxStyle::parse(v).ok_or_else(|| format!("bad box style {v:?} on node {name:?}"))?,
        ),
        // graphviz 的 shape=point / shape=none 也能认:很多 .dot 靠它说"这是站点/只是一行字"
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
        label: attrs
            .get("label")
            .filter(|s| !s.is_empty())
            .map(str::to_string),
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
    })
}

/// 边语句拆成 (起点, 终点, 默认方向, 属性表在分词里的下标);不是边就返回 Ok(None).
/// 箭头写法与图类型必须一致 —— Graphviz 本身就这么严格,混用别的工具会直接拒收.
/// `<-` 交换端点后按 forward 处理,和以前的行为一致.
fn edge_parts(
    tok: &[String],
    digraph: bool,
) -> Result<Option<(String, String, Dir, usize)>, String> {
    let (a, b, dir, at) = match tok.get(1).map(String::as_str) {
        // "--" 永远无向,"->" 永远有向,"<-" 是从右边指向左边
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
        // 只有两个名字,没写箭头:方向由图类型定
        Some(t) if t != "[" && t != "=" => {
            let dir = if digraph { Dir::Forward } else { Dir::None };
            (tok[0].clone(), tok[1].clone(), dir, 2)
        }
        _ => return Ok(None),
    };
    // 端点缺了(比如 `"甲" --;`)当这句没写全,忽略而不是造一个没名字的节点
    if a.is_empty() || b.is_empty() {
        return Ok(None);
    }
    Ok(Some((a, b, dir, at)))
}

/// 把一段文本切成"语句":去掉注释,按引号外的 ';' 切,花括号只当分隔符
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for line in text.lines() {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            // 引号里的 `\"` 与 `\\` 不是结束引号,原样留着交给切词去解转义
            if quoted && c == '\\' {
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
                continue;
            }
            // 行内注释:引号外的 "//" 之后到行尾都不要
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
        // 行与行之间总有空白,不然两行会粘成一个词
        cur.push(' ');
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// 切词:引号字符串去掉壳,'=' '[' ']' ',' 各自成词,
/// '--' '->' '<-' 各自成词(三者不能混为一谈,方向不一样)
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut quoted = false;
    // 引号里的反斜杠转义:只认 `\"` 与 `\\`,其它反斜杠原样保留
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if escaped {
                // graphviz 的转义:\n \l \r 是换行,\\ 是反斜杠,\" 是引号;
                // 其余 \x 原样保留反斜杠(以前是直接吞掉,`\n` 会变成 `n`)
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

/// 从 '[' 开始解析属性表;没有属性表就是空表
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

/// "x,y" -> 坐标;没写或者写坏了都当没有坐标
fn parse_pos(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
}

/// 手工折点:坐标两两成对,逗号或空白都能当分隔,空项丢掉.
/// 多出来落单的坐标没有意义,直接不要
fn parse_via(s: &str) -> Vec<(f64, f64)> {
    let nums: Vec<f64> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect();
    let (pairs, _) = nums.as_chunks::<2>();
    pairs.iter().map(|p| (p[0], p[1])).collect()
}

/// 别名属性按 '|' 拆开:前后空白与空项丢掉,重复的只留一份
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

/// "#rrggbb" -> 真彩色;'#' 可省,写得不完整就当没写
fn parse_color(v: &str) -> Option<(u8, u8, u8)> {
    let hex = v.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

/// color="#rrggbb" -> 真彩色;'#' 可省,写得不完整就当没写
fn color_of(attrs: &Attrs) -> Option<(u8, u8, u8)> {
    parse_color(attrs.get("color")?)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;
    use crate::model::{BoxStyle, EdgeStyle, PALETTE};

    #[test]
    fn 解析节点与边() {
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
        // 边自己的属性盖过 edge 默认属性
        assert_eq!(g.groups[g.edges[0].group].label, "1号线");
        assert_eq!(g.edge_color(&g.edges[0]), (0xdf, 0x46, 0x61));
        assert_eq!(g.groups[g.edges[1].group].label, "2号线");
        assert_eq!(g.edge_color(&g.edges[1]), (0x11, 0x22, 0x33));
        // 组内只有一条边、且它写了自己的颜色,那一组的颜色就采纳它(图例要用);
        // 组里颜色不一致时分组才回到调色板色
        assert_eq!(g.group_color(g.edges[0].group), (0xdf, 0x46, 0x61));
        assert_eq!(g.group_color(g.edges[1].group), (0x11, 0x22, 0x33));
    }

    #[test]
    fn 没有坐标的节点是_none() {
        let g = parse("graph g {\n \"甲\";\n}").unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn 边没写颜色就发调色板颜色() {
        let g = parse("graph g { \"甲\"; \"乙\"; \"甲\" -- \"乙\"; }").unwrap();
        assert_eq!(g.groups[g.edges[0].group].color, PALETTE[0]);
    }

    #[test]
    fn group_与_line_都算分组名且_group_优先() {
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
    fn digraph_的箭头边是_forward() {
        let g = parse("digraph deps { \"a\"; \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
    }

    #[test]
    fn 双横线解析成无方向() {
        let g = parse("graph g { \"a\" -- \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::None);
        // dir 属性盖过箭头/图类型定下的默认值
        let g = parse("graph g { \"a\" -- \"b\" [dir=\"forward\"]; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
    }

    #[test]
    fn 箭头写法必须与图类型一致() {
        // Graphviz 的硬规矩:digraph 只收 ->,graph 只收 --,混用是语法错误
        // (别的工具会直接拒收,所以这里也拒,免得生成的文件只能自己读)
        let e = parse("digraph g { \"a\" -- \"b\"; }").unwrap_err();
        assert!(e.contains("--"), "{e}");
        let e = parse("graph g { \"a\" -> \"b\"; }").unwrap_err();
        assert!(e.contains("->"), "{e}");
        assert!(parse("graph g { \"a\" -- \"b\"; }").is_ok());
        assert!(parse("digraph g { \"a\" -> \"b\"; }").is_ok());
    }

    #[test]
    fn 没写箭头的边按图类型定方向() {
        let g = parse("digraph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
        let g = parse("graph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::None);
    }

    #[test]
    fn 左箭头是从右边指向左边() {
        let g = parse("digraph g { \"甲\"; \"乙\"; \"甲\" <- \"乙\"; }").unwrap();
        let e = &g.edges[0];
        assert_eq!(g.nodes[e.a].label, "乙");
        assert_eq!(g.nodes[e.b].label, "甲");
        assert_eq!(e.dir, Dir::Forward);
    }

    #[test]
    fn subgraph_当透明容器不造节点() {
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
    fn 默认属性被自己的属性盖过() {
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
    fn label_当显示名且节点名进别名() {
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
    fn 同名节点合并() {
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
    fn 边端点没声明过就隐式建节点并按首次出现排序() {
        let g = parse("digraph g { \"a\" -> \"b\"; \"b\" -> \"c\"; }").unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].label, "a");
        assert_eq!(g.nodes[1].label, "b");
        assert_eq!(g.nodes[2].label, "c");
        assert_eq!(g.edges.len(), 2);
        // 隐式建的节点没有坐标
        assert!(!g.has_pos());
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn 声明写在边后面也生效() {
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
    fn 没写样式的节点默认只有文字() {
        // 默认样式是 none:一行字,宽随文字
        let d = parse_document(
            "graph g { \"甲\" [pos=\"0,0\"]; \"乙\" [pos=\"8,0\"]; \"甲\" -- \"乙\"; }",
            ViewState::default(),
            "dot",
        )
        .unwrap();
        assert_eq!(d.boxes[0].style, BoxStyle::None);
        assert_eq!(d.boxes[0].size.1, 1);
        assert_eq!(d.texts[0].at, d.boxes[0].at);

        // graphviz 的 shape=point / shape=none 也认
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
    fn 名字大小写敏感() {
        let g = parse("graph g { \"A\"; \"a\"; \"A\" -- \"a\"; }").unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 空图报错() {
        assert!(parse("graph g { }").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn strict_digraph_也认() {
        let g =
            parse("strict digraph g { \"a\"; \"b\"; \"a\" -> \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert_eq!(g.edges[0].dir, Dir::Forward);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 节点框与颜色() {
        let g = parse(
            "graph g {\n\
             node [box=\"single\"];\n\
             \"甲\";\n\
             \"乙\" [box=\"double\", color=\"#0a0b0c\"];\n\
             }",
        )
        .unwrap();
        // 默认属性生效,节点自己的属性盖过它
        assert_eq!(g.nodes[0].box_style, BoxStyle::Single);
        assert_eq!(g.nodes[0].color, None);
        assert_eq!(g.nodes[1].box_style, BoxStyle::Double);
        assert_eq!(g.nodes[1].color, Some((0x0a, 0x0b, 0x0c)));
    }

    #[test]
    fn 边标签导入与转义() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"收到 SYN\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .note_of_edge(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(note.as_deref(), Some("收到 SYN"), "label 变成注释框");
        // 引号与反斜杠按规则转义
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"say \\\"hi\\\" \\\\\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .note_of_edge(&d.edges[0].id)
            .and_then(|b| d.text(b).map(|t| t.text.clone()));
        assert_eq!(note.as_deref(), Some("say \"hi\" \\"));
    }

    /// `\n` / `\l` / `\r` 是换行,未知转义保留反斜杠(以前会被吞掉)
    #[test]
    fn 换行转义与未知转义() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\"];\n\
             \"b\" [box=\"single\"];\n\
             \"a\" -> \"b\" [label=\"一\\n二\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        let note = d
            .note_of_edge(&d.edges[0].id)
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
    fn 自定义样式属性随文档导入() {
        let text = "digraph g {\n\
             \"a\" [box=\"single\", fill=\"#102030\", align=\"center\"];\n\
             \"b\" [box=\"rounded\"];\n\
             \"a\" -> \"b\" [corner=\"sharp\"];\n\
             }";
        let d = parse_document(text, ViewState::default(), "dot").unwrap();
        assert_eq!(d.boxes[0].fill, Some((0x10, 0x20, 0x30)));
        assert_eq!(d.texts[0].align, Align::Center);
        assert_eq!(d.edges[0].corner, Corner::Sharp);
        // 没写的项取默认值
        assert_eq!(d.boxes[1].fill, None);
        assert_eq!(d.texts[1].align, Align::Left);
    }

    #[test]
    fn 边样式与颜色落在边上() {
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
    fn via_的逗号与空格两种写法() {
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
    fn dir_四个值都认() {
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
    fn 同组边写同一个_color_仍然同色() {
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
    fn 写坏的框样式报错() {
        assert!(parse("graph g { \"a\" [box=\"wavy\"]; }").is_err());
    }

    #[test]
    fn 写坏的线型报错() {
        assert!(parse("graph g { \"a\" -- \"b\" [style=\"wavy\"]; }").is_err());
    }
}
