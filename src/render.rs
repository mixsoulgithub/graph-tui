//! 投影与上屏:文档里的单元格坐标 -> 屏幕格 -> ratatui buffer.
//! 框、文字、边都从 Document 取;缩放和平移只是视图状态,不改几何.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::View;
use crate::canvas::{arrive_hor, Canvas, EndMark, DOWN, LEFT, RIGHT, UP};
use crate::document::{Box, Document, Hits};
use crate::lang::Lang;
use crate::model::Align;
use crate::model::{BoxStyle, EdgeStyle};
use crate::text;

/// 每行显示宽度(全角算两格).标签里的 `**粗**` / `/斜/` 标记不占宽度
pub fn width(s: &str) -> usize {
    text::plain_width(s)
}

/// 原样文本的显示宽度:不解释 `**` / `*` 标记。
/// 命令行、搜索词、提示行都是"原样文本",光标位置必须按这个算
pub fn raw_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// 单元格坐标 -> 屏幕格的投影.
/// 基础坐标就是单元格:1 格 = 1 列 x 1 行,x 向右、y 向下.
/// `scale` 一份单元格占几列(= 1 / zoom);窗口大小不参与.
#[derive(Clone, Copy, Debug)]
pub struct Proj {
    /// 每列多少单元格(= 1 / zoom)
    pub scale: f64,
    /// 视图中心(单元格坐标,已对齐到格)
    cx: f64,
    cy: f64,
    /// 视图中心落在哪一格(屏幕格)
    x0: i32,
    y0: i32,
}

impl Proj {
    /// 单元格坐标 -> 屏幕格
    pub fn cell(&self, p: (f64, f64)) -> (i32, i32) {
        (
            self.x0 + ((p.0 - self.cx) / self.scale).round() as i32,
            self.y0 + ((p.1 - self.cy) / self.scale).round() as i32,
        )
    }

    /// 屏幕格 -> 单元格坐标(1:1 时一一对应;缩放了就是取整回去)
    pub fn canvas(&self, col: i32, row: i32) -> (i32, i32) {
        let x = self.cx + (col - self.x0) as f64 * self.scale;
        let y = self.cy + (row - self.y0) as f64 * self.scale;
        (x.round() as i32, y.round() as i32)
    }

    /// 屏幕格 -> 画布上的连续坐标(不取整):锚点缩放要用它算"这一格下面是哪儿"
    pub fn canvas_f(&self, col: i32, row: i32) -> (f64, f64) {
        (
            self.cx + (col - self.x0) as f64 * self.scale,
            self.cy + (row - self.y0) as f64 * self.scale,
        )
    }

    /// 换缩放后,要让画布上的 `point` 仍落在同一屏幕格 (col, row),
    /// 视图中心该放在哪儿.缩放锚点的全部数学都在这里,便于单测
    pub fn center_for(&self, point: (f64, f64), col: i32, row: i32, zoom: f64) -> (f64, f64) {
        let zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom
        } else {
            1.0
        };
        let scale = 1.0 / zoom;
        (
            point.0 - (col - self.x0) as f64 * scale,
            point.1 - (row - self.y0) as f64 * scale,
        )
    }

    /// 把投影的屏幕原点定到某一格(区域中心)
    fn anchored(self, x0: i32, y0: i32) -> Proj {
        Proj { x0, y0, ..self }
    }
}

/// 这一帧怎么分:地图区(铺满)以及它的投影
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    pub map: Rect,
    pub proj: Proj,
}

/// 底部要留几行:够高就两行(状态 + 命令/提示),矮了退回一行
pub fn status_rows(area: Rect) -> u16 {
    if area.height >= 8 {
        2
    } else {
        1.min(area.height)
    }
}

/// 地图区:扣掉底部那几行
fn body_of(area: Rect) -> Rect {
    let rows = status_rows(area);
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(rows),
    }
}

/// 整幅文档的中心(单元格坐标);空文档给 (0, 0)
fn doc_center(doc: &Document, notes: bool) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox_shown(notes);
    ((x0 as f64 + x1 as f64) / 2.0, (y0 as f64 + y1 as f64) / 2.0)
}

/// 整幅文档的尺寸(单元格,至少 1)
fn doc_size(doc: &Document, notes: bool) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox_shown(notes);
    (
        ((x1 - x0 + 1) as f64).max(1.0),
        ((y1 - y0 + 1) as f64).max(1.0),
    )
}

/// 按给定缩放算这一帧的投影.视图中心对齐到字符格:相机的小数位置只让整幅图
/// 按整数格整体平移,不然有的框先跳一格、有的后跳,边在移动过程中一直变形.
/// 屏幕原点先给 (0, 0),由 panels / render_map 按区域中心定下来
pub fn proj_of(doc: &Document, view: &View, zoom: f64) -> Proj {
    // 缩放只看 zoom:一份单元格占几列.窗口大小只决定看得到多少,不改缩放
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    let scale = 1.0 / zoom;
    let fallback = doc_center(doc, view.notes);
    let (cx, cy) = match view.center {
        Some(c) if c.0.is_finite() && c.1.is_finite() => c,
        _ => fallback,
    };
    Proj {
        scale,
        cx: (cx / scale).round() * scale,
        cy: (cy / scale).round() * scale,
        x0: 0,
        y0: 0,
    }
}

/// 这一帧的区域划分:地图区(铺满整行)以及按视图中心/缩放算出来的投影
pub fn panels(doc: &Document, area: Rect, view: &View) -> Panels {
    layout(doc, body_of(area), view)
}

/// 不要状态行(整幅画)时的区域划分
fn picture_panels(doc: &Document, area: Rect, view: &View) -> Panels {
    layout(doc, area, view)
}

fn layout(doc: &Document, body: Rect, view: &View) -> Panels {
    let map = body;
    let proj = proj_of(doc, view, view.zoom).anchored(
        map.x as i32 + map.width as i32 / 2,
        map.y as i32 + map.height as i32 / 2,
    );
    Panels { map, proj }
}

/// 画布上的一个格子
pub type Cell = (i32, i32);

/// E 隔离视图:只显示中心节点、它的邻居,以及和中心相连的边.
/// 其余元素不是压暗而是**不画**(再按 E 或 Esc 退出)
pub struct Ego {
    /// 中心节点:光标落在可见邻居上时跟着换
    pub center: usize,
    /// 可见节点:中心与它的邻居
    nodes: HashSet<usize>,
    /// 可见边:中心连着的那几条
    edges: HashSet<usize>,
    /// 可见的注释框:绑在可见边上的那几条
    notes: HashSet<usize>,
}

impl Ego {
    pub fn has_node(&self, i: usize) -> bool {
        self.nodes.contains(&i) || self.notes.contains(&i)
    }

    pub fn has_edge(&self, e: usize) -> bool {
        self.edges.contains(&e)
    }
}

/// 从 `view.ego` 算出隔离集合;None = 没开 E
pub fn ego_visible(doc: &Document, view: &View) -> Option<Ego> {
    let n = view.ego.filter(|n| *n < doc.boxes.len())?;
    // 注释框当中心时,按它绑的那条边的源端算(和 toggle_ego 同一套规则)
    let n = match doc.edge_of_box(n) {
        Some(e) => doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
        None => n,
    };
    let mut nodes: HashSet<usize> = HashSet::from([n]);
    let mut edges: HashSet<usize> = HashSet::new();
    for &e in doc.adj(n) {
        edges.insert(e);
        if let Some((a, b)) = doc.edge_ends(e) {
            nodes.insert(if a == n { b } else { a });
        }
    }
    // 绑在可见边上的注释框跟着露出来,别的注释不画
    let notes: HashSet<usize> = (0..doc.boxes.len())
        .filter(|&i| {
            doc.edge_of_box(i)
                .and_then(|e| doc.edges.iter().position(|x| x.id == doc.edges[e].id))
                .is_some_and(|e| edges.contains(&e))
        })
        .collect();
    Some(Ego {
        center: n,
        nodes,
        edges,
        notes,
    })
}

/// 隔离时这个框看不见(既不是节点也不是可见注释)
pub fn ego_hides(ego: Option<&Ego>, i: usize) -> bool {
    ego.is_some_and(|e| !e.has_node(i))
}

/// 这一格最上面的**可见**框.隔离开着时,隐藏的框当它不存在
pub fn box_at(doc: &Document, cell: Cell, view: &View) -> Option<usize> {
    let ego = ego_visible(doc, view);
    doc.boxes_at_cell(cell)
        .into_iter()
        .find(|i| !ego_hides(ego.as_ref(), *i))
}

/// 光标格下面是什么
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pick {
    #[default]
    None,
    Box(usize),
    Edge(usize),
}

impl Pick {
    pub fn box_at(self) -> Option<usize> {
        match self {
            Pick::Box(i) => Some(i),
            _ => None,
        }
    }

    pub fn edge(self) -> Option<usize> {
        match self {
            Pick::Edge(e) => Some(e),
            _ => None,
        }
    }
}

/// 光标格下面是什么:box 优先,其次边,都没有就是空选.
/// `canvas` 是画布坐标(框按它查),`screen` 是屏幕格(命中表按它记)
pub fn pick_at(doc: &Document, hit: &FrameHit, canvas: Cell, screen: Cell, view: &View) -> Pick {
    // 隔离时看不见的框当空白:它既不画也不该被点中
    if let Some(i) = box_at(doc, canvas, view) {
        // 注释收起来时,注释框当它不存在(E 隔离时露出来的除外)
        if !view.notes && doc.is_note(i) && view.ego.is_none() {
            return Pick::None;
        }
        return Pick::Box(i);
    }
    match hit.edge_at(screen) {
        Some(e) => Pick::Edge(e),
        None => Pick::None,
    }
}

/// 菜单画出来的矩形:按最长的一项算宽,放不下就贴边
pub fn menu_rect(m: &crate::app::Menu, area: Rect) -> Rect {
    // 2 = 序号 + 当前值标记,色块再占 2,标题后面留 2 格给 " ›"
    let w = m
        .level()
        .iter()
        .map(|i| width(&i.title) + 2 + if i.swatch.is_some() { 2 } else { 0 } + 2)
        .max()
        .unwrap_or(8)
        .max(8) as u16
        + 2;
    let h = m.level().len().max(1) as u16 + 2;
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = m.at.0.min(area.x + area.width.saturating_sub(w));
    let y = m.at.1.min(area.y + area.height.saturating_sub(h));
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// 菜单上点到了第几项
pub fn menu_hit(m: &crate::app::Menu, area: Rect, col: u16, row: u16) -> Option<usize> {
    let r = menu_rect(m, area);
    let inside = col >= r.x && col < r.x + r.width && row > r.y && row + 1 < r.y + r.height;
    inside.then(|| (row - r.y - 1) as usize)
}

/// 右键菜单:就地一块带边框的列表,选中行反显
pub fn render_menu(buf: &mut Buffer, area: Rect, m: &crate::app::Menu) {
    let r = menu_rect(m, area);
    if r.width < 4 || r.height < 3 {
        return;
    }
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
                cell.set_style(Style::default().bg(Color::Rgb(24, 26, 38)));
            }
        }
    }
    // 边框
    for x in r.x..r.x + r.width {
        put(buf, x, r.y, "─", dark_style());
        put(buf, x, r.y + r.height - 1, "─", dark_style());
    }
    for y in r.y..r.y + r.height {
        put(buf, r.x, y, "│", dark_style());
        put(buf, r.x + r.width - 1, y, "│", dark_style());
    }
    put(buf, r.x, r.y, "╭", dark_style());
    put(buf, r.x + r.width - 1, r.y, "╮", dark_style());
    put(buf, r.x, r.y + r.height - 1, "╰", dark_style());
    put(
        buf,
        r.x + r.width - 1,
        r.y + r.height - 1,
        "╯",
        dark_style(),
    );
    for (i, item) in m.level().iter().enumerate() {
        let y = r.y + 1 + i as u16;
        if y + 1 >= r.y + r.height {
            break;
        }
        let arrow = matches!(item.action, crate::app::MenuAction::Sub(_));
        let style = if i == m.sel {
            Style::default()
                .fg(Color::Rgb(24, 26, 38))
                .bg(Color::Rgb(212, 218, 232))
        } else {
            bright_style()
        };
        if i == m.sel {
            for x in r.x + 1..r.x + r.width - 1 {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(style);
                }
            }
        }
        // 序号 + 当前值标记(`*` 表示这一项就是现在的值)
        let head = format!(
            "{}{}",
            if i < 9 {
                (i + 1).to_string()
            } else {
                " ".to_string()
            },
            if item.checked { "*" } else { " " }
        );
        put(buf, r.x + 2, y, &head, style);
        let mut x = r.x + 4;
        // 色块:两个空格的底色,不用宽字符
        if let Some(c) = item.swatch {
            for k in 0..2 {
                if let Some(cell) = buf.cell_mut((x + k, y)) {
                    cell.set_char(' ');
                    cell.set_style(Style::default().bg(Color::Rgb(c.0, c.1, c.2)));
                }
            }
            x += 2;
        }
        let text = format!("{}{}", item.title, if arrow { " ›" } else { "" });
        put(buf, x, y, &text, style);
    }
}

/// `:` 命令行的补全浮窗:贴在状态行上方,列出候选,当前那个反显
pub fn render_cmd_menu(buf: &mut Buffer, area: Rect, view: &View) {
    let Some((items, at)) = &view.cmd_menu else {
        return;
    };
    if items.is_empty() || area.height < 4 || area.width < 8 {
        return;
    }
    let shown: usize = items.len().min(8);
    let cur = if items.is_empty() {
        0
    } else {
        (at + items.len() - 1) % items.len()
    };
    let first = cur.saturating_sub(shown.saturating_sub(1));
    let w = items
        .iter()
        .take(shown)
        .map(|t| width(t))
        .max()
        .unwrap_or(8) as u16
        + 6;
    let w = w.min(area.width);
    let h = shown as u16 + 2;
    let x = area.x;
    let y = area.y + area.height - 1 - h;
    for yy in y..y + h {
        for xx in x..x + w {
            if let Some(cell) = buf.cell_mut((xx, yy)) {
                cell.reset();
                cell.set_style(Style::default().bg(Color::Rgb(24, 26, 38)));
            }
        }
    }
    for xx in x..x + w {
        put(buf, xx, y, "─", dark_style());
        put(buf, xx, y + h - 1, "─", dark_style());
    }
    for yy in y..y + h {
        put(buf, x, yy, "│", dark_style());
        put(buf, x + w - 1, yy, "│", dark_style());
    }
    put(buf, x, y, "╭", dark_style());
    put(buf, x + w - 1, y, "╮", dark_style());
    put(buf, x, y + h - 1, "╰", dark_style());
    put(buf, x + w - 1, y + h - 1, "╯", dark_style());
    for (row, i) in (first..first + shown).enumerate() {
        let yy = y + 1 + row as u16;
        let hl = i == cur;
        let style = if hl {
            Style::default()
                .fg(Color::Rgb(24, 26, 38))
                .bg(Color::Rgb(212, 218, 232))
        } else {
            bright_style()
        };
        if hl {
            for xx in x + 1..x + w - 1 {
                if let Some(cell) = buf.cell_mut((xx, yy)) {
                    cell.set_style(style);
                }
            }
        }
        put(buf, x + 2, yy, &items[i], style);
    }
}

/// 一帧的命中表:哪一格属于哪条边(鼠标点边、`E` 循环用).
/// 每帧重建,不落盘.
#[derive(Clone, Debug, Default)]
pub struct FrameHit {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// 每格最后显示的是哪条边(-1 = 没有)
    edge: Vec<i32>,
    /// 同一格上的边候选(按画的先后)
    pub candidates: std::collections::HashMap<(i32, i32), Vec<usize>>,
    /// 每条边最后可见的格子
    pub edge_cells: std::collections::HashMap<usize, Vec<(i32, i32)>>,
    /// 生成这张表时的文档修订号:对不上就说明下标已经失效
    pub rev: u64,
    /// 放不下的点节点名字(渲染时静默丢掉了什么,这里一五一十记着)
    pub labels_skipped: Vec<LabelSkip>,
    /// 每条边两端画在哪个格子(起点, 终点):拖端点重连要按它命中
    pub edge_ends: std::collections::HashMap<usize, (Cell, Cell)>,
    /// 画布格 -> 链接目标:文字里 `[x](y)` 盖住的每一格
    pub links: std::collections::HashMap<Cell, String>,
}

/// 一个没放下的点节点名字:四个候选位都被占 / 出画面
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSkip {
    pub box_id: String,
    pub text: String,
    /// 节点在屏幕上的那一格(方便调用方提示"哪里挤")
    pub cell: Cell,
}

impl FrameHit {
    /// 这一格显示的是哪条边
    pub fn edge_at(&self, cell: (i32, i32)) -> Option<usize> {
        let (x, y) = cell;
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        let i = ((y - self.y0) * self.w + (x - self.x0)) as usize;
        self.edge
            .get(i)
            .and_then(|&e| (e >= 0).then_some(e as usize))
    }

    /// 这一格上的边候选(有重叠时不止一条)
    pub fn candidates_at(&self, cell: (i32, i32)) -> &[usize] {
        self.candidates.get(&cell).map(Vec::as_slice).unwrap_or(&[])
    }
}

impl From<&Canvas> for FrameHit {
    fn from(cv: &Canvas) -> FrameHit {
        FrameHit {
            x0: cv.x0,
            y0: cv.y0,
            w: cv.w,
            h: cv.h,
            rev: 0,
            edge: cv.edge_of.clone(),
            candidates: cv.edge_cands.clone(),
            edge_cells: cv.edge_cells.clone(),
            labels_skipped: Vec::new(),
            edge_ends: std::collections::HashMap::new(),
            links: std::collections::HashMap::new(),
        }
    }
}

/// 铺满整个区域需要的缩放倍数(一份单元格占几列),给 f / F 键用
pub fn zoom_for_fit(doc: &Document, w: u16, h: u16, notes: bool) -> f64 {
    // 和 panels 一样先把状态行扣掉:
    // 否则"铺满整幅"铺的是整个窗口,而真正画图的区域比窗口矮,下面会被切掉
    let body = body_of(Rect::new(0, 0, w, h));
    let (wd, hd) = doc_size(doc, notes);
    let cols = (body.width as f64 - 4.0).max(8.0);
    let rows = (body.height as f64 - 2.0).max(4.0);
    let scale = (wd / cols).max(hd / rows);
    if scale.is_finite() && scale > 0.0 {
        // 铺满最多到 1:1 —— 字符格放大只是把格子拉开,字形不会变大,图会变成空框框
        (1.0 / scale).min(1.0)
    } else {
        1.0
    }
}

/// 把视图中心按**屏幕格**平移(dcols 列、drows 行,正数向右/向下),返回新的单元格坐标中心.
/// 滚轮平移就靠它:先换算中心的格坐标,挪几格,再换回单元格坐标
pub fn pan_center(proj: &Proj, from: (f64, f64), dcols: i32, drows: i32) -> (f64, f64) {
    let (x, y) = proj.cell(from);
    let (nx, ny) = (x + dcols, y + drows);
    (
        proj.cx + (nx - proj.x0) as f64 * proj.scale,
        proj.cy + (ny - proj.y0) as f64 * proj.scale,
    )
}

/// 按强度把颜色往红里拉:t = 0 是原色,t = 1 是纯红.
/// view.pulse 就是这里的 t(曲线由主循环按时间算好),所以这里只做插值
fn pulse_color(c: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::Rgb(
        mix(c.0, BLINK_RED.0),
        mix(c.1, BLINK_RED.1),
        mix(c.2, BLINK_RED.2),
    )
}

/// 压暗用的灰
const DIM_COLOR: Color = Color::Rgb(66, 74, 96);
/// 亮白:枢纽与光标节点平时用的那个
const BRIGHT: (u8, u8, u8) = (246, 248, 252);
/// 区块选择的灰边(底色,连续不断)
const SEL_GRAY: Color = Color::Rgb(84, 90, 110);
/// 连边模式里源框的稳定高亮色(不闪,和目标的红白呼吸区分开)
const LINK_SRC: (u8, u8, u8) = (250, 214, 96);
/// 搜索命中的框/点:和搜索提示同一个黄,一眼能对上
const SEARCH_HIT: (u8, u8, u8) = (238, 226, 104);
/// 连边预览线:暗蓝灰,一眼看出"还没建"
const PREVIEW: (u8, u8, u8) = (104, 118, 152);
/// 闪到最红的那一档
const BLINK_RED: (u8, u8, u8) = (255, 62, 56);
/// 普通节点名的灰
const LABEL: (u8, u8, u8) = (150, 158, 180);
/// 区域边框与标题的颜色(区域没给颜色时用)
const REGION_INK: (u8, u8, u8) = (108, 118, 148);

/// 浮层与提示用的底色
const PANEL_BG: (u8, u8, u8) = (24, 28, 44);

/// 亮色但不加粗:底部状态/提示行用(粗体会让整行显得比地图还重)
fn bright_plain() -> Style {
    Style::default().fg(Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2))
}

fn bright_style() -> Style {
    Style::default()
        .fg(Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2))
        .add_modifier(Modifier::BOLD)
}

fn dark_style() -> Style {
    Style::default().fg(Color::Rgb(70, 78, 100))
}

fn dim_style() -> Style {
    Style::default().fg(DIM_COLOR)
}

fn panel_bg() -> Style {
    Style::default().bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2))
}

/// 把一帧变成带 ANSI 颜色的文本(`--dump` 与 headless 的 ansi 输出共用)
pub fn ansi(buf: &Buffer) -> String {
    use ratatui::style::Modifier;
    let mut out = String::new();
    for y in 0..buf.area.height {
        // 全角字的第二格不用再输出(宽字本身已经占了两列)
        let mut skip = 0usize;
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if skip > 0 {
                skip -= 1;
                continue;
            }
            skip = unicode_width::UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
            let (r, g, b) = match cell.fg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (212, 218, 232),
            };
            let (br, bg, bb) = match cell.bg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (9, 10, 17),
            };
            out.push_str(&format!("\x1b[38;2;{r};{g};{b};48;2;{br};{bg};{bb}"));
            for (m, code) in [
                (Modifier::BOLD, ";1"),
                (Modifier::DIM, ";2"),
                (Modifier::ITALIC, ";3"),
                (Modifier::UNDERLINED, ";4"),
                (Modifier::REVERSED, ";7"),
            ] {
                if cell.modifier.contains(m) {
                    out.push_str(code);
                }
            }
            out.push('m');
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

/// 纯文本:每行正好 area.width 列宽.宽字占两列,第二列不重复输出
pub fn plain_lines(buf: &Buffer) -> Vec<String> {
    let mut out = Vec::with_capacity(buf.area.height as usize);
    for y in 0..buf.area.height {
        let mut line = String::new();
        let mut skip = 0usize;
        for x in 0..buf.area.width {
            let sym = buf[(x, y)].symbol();
            if skip > 0 {
                skip -= 1;
                continue;
            }
            skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
            line.push_str(sym);
        }
        out.push(line);
    }
    out
}

/// 每个框在屏幕上的矩形(左上角列、行、宽、高),顺序同 `doc.boxes`
pub fn box_rects(doc: &Document, area: Rect, view: &View) -> Vec<(i32, i32, i32, i32)> {
    let p = panels(doc, area, view);
    (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &p.proj))
        .collect()
}

/// 同 `box_rects`,但按"只有画面、没有状态行"的分区算(导出/headless 用)
pub fn picture_box_rects(doc: &Document, area: Rect, view: &View) -> Vec<(i32, i32, i32, i32)> {
    let p = picture_panels(doc, area, view);
    (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &p.proj))
        .collect()
}

/// 画一帧.浮层(候选列表)在最上面,画面本身在 render_frame 里
pub fn render(buf: &mut Buffer, area: Rect, doc: &Document, view: &View) -> Panels {
    render_hit(buf, area, doc, view, &FrameHit::default()).0
}

/// 同 `render`,外加这一帧的命中表(鼠标点边、选边高亮用)
pub fn render_hit(
    buf: &mut Buffer,
    area: Rect,
    doc: &Document,
    view: &View,
    prev_hit: &FrameHit,
) -> (Panels, FrameHit) {
    let (p, hit) = render_frame(buf, area, doc, view, prev_hit);
    if let Some(m) = &view.menu {
        render_menu(buf, area, m);
    }
    if view.cmd.is_some() {
        render_cmd_menu(buf, area, view);
    }
    (p, hit)
}

/// 走线缓存:同一份文档、同一个缩放、同一个画面尺寸下,平移/移动光标不需要重新规划.
/// 规划结果按画布坐标存,命中时整体平移;键里带上文档几何,所以改文档必然落空
mod route_cache {
    use crate::canvas::Stroke;
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub(super) struct Cached {
        pub key: u64,
        pub plans: Vec<Stroke>,
        pub plan_edges: Vec<usize>,
        pub usage: HashMap<(i32, i32), (u64, u8)>,
        pub usage_edges: HashMap<usize, Vec<(i32, i32)>>,
    }

    /// 留几个槽:切换 tab 时来回换着看,不至于每次都重新规划
    const SLOTS: usize = 4;

    thread_local! {
        static LAST: RefCell<Vec<Cached>> = const { RefCell::new(Vec::new()) };
        /// 测试用:命中次数
        static HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// 取匹配的规划结果
    pub(super) fn get(key: u64) -> Option<Cached> {
        LAST.with(|c| {
            let b = c.borrow();
            let hit = b.iter().find(|x| x.key == key)?;
            HITS.with(|h| h.set(h.get() + 1));
            Some(Cached {
                key: hit.key,
                plans: hit.plans.clone(),
                plan_edges: hit.plan_edges.clone(),
                usage: hit.usage.clone(),
                usage_edges: hit.usage_edges.clone(),
            })
        })
    }

    pub(super) fn put(c: Cached) {
        LAST.with(|last| {
            let mut v = last.borrow_mut();
            v.retain(|x| x.key != c.key);
            v.insert(0, c);
            v.truncate(SLOTS);
        });
    }

    #[cfg(test)]
    pub(super) fn clear() {
        LAST.with(|last| last.borrow_mut().clear());
        HITS.with(|h| h.set(0));
    }

    #[cfg(test)]
    pub(super) fn hits() -> usize {
        HITS.with(|h| h.get())
    }
}

/// 走线缓存的键:文档内容、缩放、画面尺寸、和"哪些边要画"有关的视图开关.
/// 故意不含视口原点 —— 平移只是整体挪一格,路线本身不变
fn route_key(doc: &Document, view: &View, proj: &Proj, map: Rect) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x1000_0000_01b3);
    };
    eat(doc.id());
    eat(doc.rev());
    eat(proj.scale.to_bits());
    eat((map.width as u64) << 32 | map.height as u64);
    // 视口也算键:路线按画布坐标算,挪过视图就得重来
    let (ox, oy) = proj.cell((0.0, 0.0));
    eat((ox as i64 as u64) << 32 | (oy as i64 as u64 & 0xffff_ffff));
    eat(u64::from(view.notes));
    eat(view.arrow_style as u64);
    eat(view.layer.as_deref().map_or(u64::MAX, |l| {
        doc.layers
            .iter()
            .position(|x| x.name == l)
            .unwrap_or(usize::MAX) as u64
    }));
    eat(view.ego.map_or(u64::MAX, |n| n as u64));
    eat(view.pick.edge().map_or(u64::MAX, |e| e as u64));
    for b in &doc.boxes {
        eat(b.at.0 as u64 ^ (b.at.1 as u64) << 20);
        eat((b.size.0 as u64) << 8 | b.size.1 as u64);
    }
    for e in &doc.edges {
        eat(e.from.len() as u64 ^ e.to.len() as u64);
        eat(e.via.len() as u64);
        eat(e.from_side.map_or(u64::MAX, |s| s.bit() as u64));
        eat(e.to_side.map_or(u64::MAX, |s| s.bit() as u64));
        eat(e.dir.heads().0 as u64 | (e.dir.heads().1 as u64) << 1);
    }
    h
}

/// E 模式:把高亮的边画到框、文字、标签之上,整条蓝色路径都能看见.
/// 只覆盖"这条边自己的格":线格要求 `shape_edge == e`,端口格要求 `edge_of == e`,
/// 交叉处别人的形状不会被打断
/// 共享箭头该用谁的颜色:选中的那条 > E 里唯一亮着的 owner > 第一条(可见的)owner.
/// 一个格只能一种颜色,多个 owner 同时亮时保持可见色,不随机跳
fn arrow_ink_for(
    cv: &Canvas,
    cell: (i32, i32),
    picked: Option<usize>,
    ego: Option<&Ego>,
) -> Option<(usize, (u8, u8, u8))> {
    let owners = cv.arrow_owners.get(&cell)?;
    let ink_of = |e: usize| -> Option<(u8, u8, u8)> {
        cv.arrow_owner_ink
            .get(&(cell, e))
            .copied()
            .or_else(|| cv.arrow_ink[cv.idx(cell.0, cell.1)?])
    };
    if let Some(p) = picked.filter(|p| owners.contains(p)) {
        if let Some(ink) = ink_of(p) {
            return Some((p, ink));
        }
    }
    if let Some(ego) = ego {
        let mut lit = owners.iter().copied().filter(|e| ego.edges.contains(e));
        if let (Some(first), None) = (lit.next(), lit.next()) {
            if let Some(ink) = ink_of(first) {
                return Some((first, ink));
            }
        }
    }
    let visible = owners.first().copied()?;
    Some((visible, ink_of(visible)?))
}

fn overlay_ego_edges(
    buf: &mut Buffer,
    cv: &Canvas,
    doc: &Document,
    ego: &Ego,
    picked: Option<usize>,
    pulse: f64,
) {
    // 选中的那条保持呼吸色,不然 E 里看不出选的是哪一条
    let ink_for = |e: usize, ink: (u8, u8, u8)| {
        if picked == Some(e) {
            Style::default()
                .fg(pulse_color(BRIGHT, pulse))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(ink.0, ink.1, ink.2))
        }
    };
    let write = |buf: &mut Buffer, x: i32, y: i32, ch: &str, style: Style| {
        if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
            // set_style 是合并而不是覆盖:先整体复位,免得底层画的 BOLD 留在这一格上
            cell.set_style(Style::reset());
            cell.set_symbol(ch);
            cell.set_style(style);
        }
    };
    // 1. 笔画.形状不属于这条边的格子(被别的线占住)不动
    for &e in &ego.edges {
        let Some(cells) = cv.stroke_cells.get(&e) else {
            continue;
        };
        for &(x, y) in cells {
            let Some(i) = cv.idx(x, y) else { continue };
            // 箭头格单独按 owner 处理(见下面),这里跳过
            if cv.arrow[i].is_some() {
                continue;
            }
            // 节点格最上层(点节点那一格既是端点也是字形),别被笔画盖掉
            if cv.node[i] != 0 {
                continue;
            }
            if cv.shape_edge[i] as usize != e {
                continue;
            }
            let mut ch = cv.glyphs.line(cv.mask[i], cv.corner[i]);
            if (ch == '─' || ch == '│')
                && cv.mask[i].count_ones() <= 2
                && !cv.solid[i]
                && !cv.style[i].keeps(x, y)
            {
                ch = ' ';
            }
            let ch = ch.to_string();
            write(buf, x, y, &ch, ink_for(e, cv.fg[i]));
        }
    }
    // 2. 箭头:只画属于中心的(共享箭头对每个 owner 都可见),路径中段别人的箭头原样留着
    for (&(x, y), owners) in &cv.arrow_owners {
        let Some(i) = cv.idx(x, y) else { continue };
        let Some(a) = cv.arrow[i] else { continue };
        // 颜色跟当前激活的 owner:选中的 > E 里唯一亮着的 > 可见的那条
        let Some((oe, ink)) = arrow_ink_for(cv, (x, y), picked, Some(ego)) else {
            continue;
        };
        if !ego.edges.contains(&oe) && picked != Some(oe) {
            continue;
        }
        // 选中边的端口外侧格上,别人的箭头在 blit 里已经藏起来了:这里不能再画回来
        if cv.picked_port_out.contains(&(x, y)) && !picked.is_some_and(|p| owners.contains(&p)) {
            continue;
        }
        let ch = a.to_string();
        write(buf, x, y, &ch, ink_for(oe, ink));
    }
    // 3. 边框上的端口(T / 嵌入的箭头):只有属于中心的边才重画
    for (&(x, y), owners) in &cv.port_owners {
        let Some(i) = cv.idx(x, y) else { continue };
        if cv.deco[i].is_none() {
            continue;
        }
        let Some(oe) = picked
            .filter(|p| owners.contains(p))
            .or_else(|| owners.iter().find(|e| ego.edges.contains(e)).copied())
        else {
            continue;
        };
        let Some(mark) = cv.border.get(&(x, y)) else {
            continue;
        };
        let glyph = match mark {
            // 共享箭头(边框上嵌入的那种)也用激活 owner 的颜色
            crate::canvas::BorderMark::Arrow(c, col) => (
                *c,
                arrow_ink_for(cv, (x, y), picked, Some(ego))
                    .map(|(_, ink)| ink)
                    .unwrap_or(*col),
            ),
            crate::canvas::BorderMark::Junction(m, col) => {
                let style = cv.deco[i]
                    .map(|d| doc.boxes[d.node as usize].style)
                    .unwrap_or(BoxStyle::Single);
                (crate::canvas::junction_glyph(style, *m), *col)
            }
            crate::canvas::BorderMark::Port => continue,
        };
        let ch = glyph.0.to_string();
        write(buf, x, y, &ch, ink_for(oe, glyph.1));
    }
}

/// 这一帧的分组聚焦掩码(最多 64 组)
fn focus_mask(view: &View) -> u64 {
    view.focus
        .iter()
        .filter(|gr| **gr < 64)
        .fold(0u64, |m, gr| m | (1 << gr))
}

/// 这一层是不是正在看的那一层(没按图层看时一律算)
fn lit_layer(view: &View, layer: &str) -> bool {
    match view.layer.as_deref() {
        None => true,
        Some(cur) => cur == layer,
    }
}

/// 图层叠放顺序:layers 的 order 先后,当前层挪到最后(画在最上面)
fn layer_order(doc: &Document, view: &View) -> Vec<String> {
    let mut out = doc.layer_names();
    if let Some(cur) = view.layer.as_deref() {
        if let Some(k) = out.iter().position(|l| l == cur) {
            let l = out.remove(k);
            out.push(l);
        }
    }
    out
}

/// 屏幕上这个框占的宽高:Point 永远一格;带框的按缩放放大,免得框与边脱节.
/// 上限只防极端缩放下算出天文数字(分配画布要按它开数组)
fn screen_size(b: &Box, scale: f64) -> (i32, i32) {
    if b.style.is_point() {
        return (1, 1);
    }
    let w = (b.size.0 as f64 / scale).round().clamp(1.0, 4096.0) as i32;
    let h = (b.size.1 as f64 / scale).round().clamp(1.0, 4096.0) as i32;
    (w, h)
}

/// 一条边在节点 i 这一端的端口:按两个节点的中心差选面对的边,
/// 再按宽高奇偶取连接格(点节点没有端口,返回 None)
fn end_mark(
    doc: &Document,
    i: usize,
    toward: usize,
    rects: &[(i32, i32, i32, i32)],
    forced: Option<crate::model::Side>,
) -> Option<EndMark> {
    if doc.boxes[i].style.is_point() {
        return None;
    }
    // 和画线用同一个选侧规则,否则命中表里的端点会跑到别的边上
    let side = end_side(doc, i, toward, rects, forced);
    let rect = rects[i];
    let b = &doc.boxes[i];
    Some(EndMark {
        rect,
        side,
        cell: Canvas::side_base(rect, side),
        // 没有边框就没地方画 T/箭头:标记落在区域外面那一格
        mark: b.style.is_boxed(),
        corners: b.style.is_boxed(),
        // 端口格由 PortPlan 补上;预览/自环这类没有计划的还得扫边框
        fixed: false,
    })
}

/// 这一端该从哪一面出去:显式端口优先,否则看另一头在哪边
/// 这一侧是不是背对着目标:目标整个在它后面
fn side_backwards(d: (f64, f64), side: u8) -> bool {
    match side {
        RIGHT => d.0 < 0.0,
        LEFT => d.0 > 0.0,
        DOWN => d.1 < 0.0,
        _ => d.1 > 0.0,
    }
}

fn end_side(
    doc: &Document,
    i: usize,
    toward: usize,
    rects: &[(i32, i32, i32, i32)],
    forced: Option<crate::model::Side>,
) -> u8 {
    let d = (
        doc.center(toward).0 - doc.center(i).0,
        doc.center(toward).1 - doc.center(i).1,
    );
    if let Some(s) = forced {
        // 钉住的侧如果正对"背后"(目标在对侧),照它画就得绕一圈:
        // 这种时候按自动来,只有方向不矛盾时才当真
        if !side_backwards(d, s.bit()) {
            return s.bit();
        }
    }
    // 先看投影:横向有交集、纵向没有,才谈得上"一上一下"
    let rect = rects[i];
    let other = rects[toward];
    let x_overlap = rect.0.min(other.0) < (rect.0 + rect.2).max(other.0 + other.2)
        && rect.0.max(other.0) < (rect.0 + rect.2).min(other.0 + other.2);
    let y_overlap = rect.1.min(other.1) < (rect.1 + rect.3).max(other.1 + other.3)
        && rect.1.max(other.1) < (rect.1 + rect.3).min(other.1 + other.3);
    // 分层布局:跨层的边一律从上/下走(下层从底下出去),这样一层的边挤在同一个通道里,
    // 端口也能按列对齐,而不是各走各的横线. 但两个框的矩形在纵向叠在一起时
    // (并排、只是 top y 不同),按层强制就会把左右出线硬掰成上下
    if !y_overlap {
        if let (Some(a), Some(b)) = (doc.rank(i), doc.rank(toward)) {
            // 只强制相邻层:跨好几层的长边让它自己找路,不然会绕出大圈
            if b == a + 1 {
                return DOWN;
            }
            if a == b + 1 {
                return UP;
            }
        }
    }
    // 先看两个框在屏幕上的投影:横向投影有交集、纵向没有,那就是"一上一下",
    // 该从上/下进出;反过来是左右.只有两个方向都错开(斜着)才按中心差挑主方向.
    // 只看中心差会这样翻车:一个很宽的框和它正下方的窄框,中心差是横向的,
    // 结果从上/下变成左/右,线要横着跨一大段再拐下来
    if x_overlap && !y_overlap {
        return if d.1 > 0.0 { DOWN } else { UP };
    }
    if y_overlap && !x_overlap {
        return if d.0 > 0.0 { RIGHT } else { LEFT };
    }
    if d.0.abs() >= d.1.abs() {
        if d.0 > 0.0 {
            RIGHT
        } else {
            LEFT
        }
    } else if d.1 > 0.0 {
        DOWN
    } else {
        UP
    }
}

/// 文字内容的中心(屏幕格).端口从这里量,而不是从框矩形量:
/// 换框型时文字不动、框围着文字长,内容中心是同一个点,端口就不会重排
fn content_center(doc: &Document, i: usize, proj: &Proj, rect: (i32, i32, i32, i32)) -> (i32, i32) {
    let Some(t) = doc.text(i) else {
        return (rect.0 + rect.2 / 2, rect.1 + rect.3 / 2);
    };
    let (lines, widest) = crate::document::text_metrics(&t.text);
    let (x0, y0) = proj.cell((t.at.0 as f64, t.at.1 as f64));
    let (x1, y1) = proj.cell((
        (t.at.0 + widest.max(1) as i32 - 1) as f64,
        (t.at.1 + lines.max(1) as i32 - 1) as f64,
    ));
    ((x0 + x1) / 2, (y0 + y1) / 2)
}

/// 内容在这一侧够不够宽/高到能放下两个端口
/// 一个图层里的连线:普通边按分组一段段走,自环另外处理
fn draw_edges(
    cv: &mut Canvas,
    doc: &Document,
    view: &View,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    layer: &str,
    ego: Option<&Ego>,
) {
    // 隔离时只落笔可见的边.规划那趟照旧画全量:可见边的端口和走线不随 E 跳变
    let drawing = !cv.planning;
    let hidden = |e: usize| drawing && ego.is_some_and(|g| !g.has_edge(e));
    // 隔离时不画的边也要把规划记账位吃掉,游标才不会错位
    let skip = |cv: &mut Canvas, e: usize| {
        if drawing {
            cv.skip_plan(e);
        }
    };
    // 端口先按"这一层要画的边"整体排好:同一组几条边决定用一个还是两个格,
    // 跟谁先画、边框被谁占了无关
    let plan = plan_ports(doc, proj, rects, layer);
    // 端口/箭头格先登记成"别人的软障碍":路由宁愿绕一格,也不从别人的箭头口上穿过去
    cv.port_guest.clear();
    // 按边下标排好再登记:两条边落在同一格时,"谁是这里的主人"必须稳定,
    // 否则每次运行的路由代价不同,同一份文档会画成两张图
    let mut guests: Vec<(usize, usize, u8, (i32, i32))> = plan.iter().collect();
    guests.sort_unstable_by_key(|(box_i, e, side, cell)| (*e, *box_i, *side, *cell));
    for (_, e, side, cell) in guests {
        let out = Canvas::outside(cell, side);
        cv.port_guest.entry(cell).or_insert(e);
        cv.port_guest.entry(out).or_insert(e);
    }
    // 这一层里"同一对节点上的边":按 (小下标, 大下标) 归堆,画一段时直接查.
    // 以前是每画一段就扫一遍全部边,大图上这是 O(段数 x 边数)
    let mut pairs: std::collections::HashMap<(usize, usize), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, e) in doc.edges.iter().enumerate() {
        if e.layer != layer {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        let k = if a < b { (a, b) } else { (b, a) };
        pairs.entry(k).or_default().push(i);
    }
    for (gr, path) in doc.series() {
        let gid = &doc.groups[*gr].id;
        for k in 0..path.len().saturating_sub(1) {
            // 有向边:箭头画在箭头那一端(series 里的先后顺序不一定就是箭头方向).
            // 同一个分组里两个方向都有时,这一段两端各画一个
            let (u, v) = (path[k], path[k + 1]);
            let key = if u < v { (u, v) } else { (v, u) };
            let pair = pairs.get(&key).map(Vec::as_slice).unwrap_or(&[]);
            // 这一段上的所有边:同一对节点可能连着好几条(a -> b、b -> a、同向重复).
            // `pair` 不分分组(点节点之间靠它的序号分道),这一段只画本分组的那几条
            let uid = &doc.boxes[u].id;
            if pair.is_empty() {
                continue;
            }
            for (lane, e) in pair.iter().copied().enumerate() {
                if doc.edges[e].group != *gid || hidden(e) {
                    skip(cv, e);
                    continue;
                }
                // 箭头只认这条边自己的方向:from 落在 u 就是 (u 端, v 端),否则对调.
                // 显式端口也按这条边自己取,不再拿同段第一条的凑合
                let (at_from, at_to) = doc.edges[e].dir.heads();
                let from_here = &doc.edges[e].from == uid;
                let heads = if from_here {
                    (at_from, at_to)
                } else {
                    (at_to, at_from)
                };
                let (forced_u, forced_v) = if from_here {
                    (doc.edges[e].from_side, doc.edges[e].to_side)
                } else {
                    (doc.edges[e].to_side, doc.edges[e].from_side)
                };
                // 两端的端口格:PortPlan 里查,查不到(点节点)就用中心格
                let mark_end = |box_i: usize, other: usize, arrow: bool, forced| {
                    let mut end = end_of(doc, rects, box_i, other, forced)?;
                    match plan.get(box_i, arrow, e) {
                        // 计划里带着实际的侧:一侧挤满时这一端被换到相邻侧了
                        Some((side, cell)) => {
                            end.side = side;
                            end.cell = cell;
                            end.fixed = true;
                        }
                        // 这一侧小到没有合法格(低倍率下框只剩一行):退回扫边框的老路
                        // 点节点 / 没有边框格:回到"按中心差选侧、取该侧中心格"的老路
                        None => {
                            end.side = end_side(doc, box_i, other, rects, forced);
                            end.cell = Canvas::side_base(end.rect, end.side);
                        }
                    }
                    Some(end)
                };
                let a_end = mark_end(u, v, heads.0, forced_u);
                let b_end = mark_end(v, u, heads.1, forced_v);
                let a_cell = a_end
                    .map(|x| x.cell)
                    .unwrap_or_else(|| center_cell(doc, proj, u));
                let b_cell = b_end
                    .map(|x| x.cell)
                    .unwrap_or_else(|| center_cell(doc, proj, v));
                // 下一个节点决定这一段该"竖着进"还是"横着进"
                let arrive = if k + 2 < path.len() {
                    let c = center_cell(doc, proj, path[k + 2]);
                    arrive_hor(a_cell, b_cell, c)
                } else {
                    None
                };
                // 命中表按边下标记格子:画这一段之前先说是哪条边(以及它的底色)
                cv.pen_edge = e;
                cv.pen_bg = doc.edges[e].bg;
                // 箭头字形:这条边自己写了就用它的,否则跟全局设置
                cv.pen_arrow = doc.edges[e].head.unwrap_or(view.arrow_style);
                let style = doc.edges[e].style;
                let fg = edge_ink(doc, view, e);
                cv.pen_corner = doc.edges[e].corner;
                // 两端都没有能错开的边框格(点节点、只有一格的文字节点):同一条段上的
                // 多条边靠 lane 偏移分开,不然它们会画成完全一样的一条线
                let spreadable = |end: Option<EndMark>| {
                    end.map(|e| e.rect.2 > 1 || e.rect.3 > 1).unwrap_or(false)
                };
                if pair.len() > 1 && !spreadable(a_end) && !spreadable(b_end) {
                    cv.edge_lane(a_cell, b_cell, lane_of(lane), *gr, fg, heads, style);
                    continue;
                }
                if !doc.edges[e].via.is_empty() {
                    // 手工折点:按 from -> via.. -> to 逐段走(这一段的方向决定折点顺序)
                    let mut pts = vec![a_cell];
                    let vs: Vec<(i32, i32)> = if &doc.edges[e].from == uid {
                        doc.edges[e].via.clone()
                    } else {
                        doc.edges[e].via.iter().rev().copied().collect()
                    };
                    pts.extend(vs.into_iter().map(|q| proj.cell((q.0 as f64, q.1 as f64))));
                    pts.push(b_cell);
                    cv.path(&pts, *gr, fg, style, heads, a_end, b_end);
                    continue;
                }
                // 文档写了 from_side 时,点节点也要按那个方向起步
                let depart = forced_u
                    .map(|sd| matches!(sd, crate::model::Side::Left | crate::model::Side::Right));
                cv.edge_full(
                    a_cell, b_cell, *gr, fg, arrive, heads, style, a_end, b_end, depart,
                );
            }
        }
    }
    // 1b. 自环:它不属于任何"线路",自己从节点一侧出去、拐到相邻一侧再回来
    for (i, e) in doc.edges.iter().enumerate() {
        if e.layer != layer || hidden(i) {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a != b {
            continue;
        }
        let r = rects[a];
        let end = end_of(doc, rects, a, a, e.from_side.or(e.to_side)).unwrap_or(EndMark {
            fixed: false,
            rect: r,
            side: RIGHT,
            cell: Canvas::side_base(r, RIGHT),
            mark: false,
            corners: false,
        });
        let Some(g) = doc.group_idx(&e.group) else {
            continue;
        };
        cv.pen_edge = i;
        cv.pen_bg = e.bg;
        cv.pen_corner = e.corner;
        cv.pen_arrow = e.head.unwrap_or(view.arrow_style);
        cv.edge_loop(end, e.dir.heads(), g, edge_ink(doc, view, i), e.style);
    }
}

/// 节点在屏幕上的中心格
fn center_cell(doc: &Document, proj: &Proj, i: usize) -> (i32, i32) {
    proj.cell(doc.center(i))
}

/// 一条边在节点 i 这一端的落点:带框的落边框连接格,点节点落中心格
fn end_of(
    doc: &Document,
    rects: &[(i32, i32, i32, i32)],
    i: usize,
    toward: usize,
    forced: Option<crate::model::Side>,
) -> Option<EndMark> {
    end_mark(doc, i, toward, rects, forced)
}

/// 某一侧的合法边框格,按离内容中心的远近排好
fn side_ports(
    doc: &Document,
    i: usize,
    rect: (i32, i32, i32, i32),
    side: u8,
    proj: &Proj,
) -> Vec<(i32, i32)> {
    let corners = doc.boxes[i].style.is_boxed();
    let cc = content_center(doc, i, proj, rect);
    let mut cells = Canvas::side_cells(rect, side, corners);
    cells.sort_by_key(|&(x, y)| {
        let (dx, dy) = (x - cc.0, y - cc.1);
        (dx * dx + dy * dy, x, y)
    });
    cells
}

/// 这一层里每条边两个端点的精确端口.
/// 键是 (框, 这一端有没有箭头, 边):同一对框之间可能有好几条边.
/// 值带着实际的侧 —— 一侧挤满时这一端会换到相邻侧,绘制必须照这个侧走
/// (框, 有没有箭头, 边) -> (实际的侧, 端口格)
type PortKey = (usize, bool, usize);
/// 一对端口的取舍:(总代价, 源格, 目标格)
type PortChoice = (i32, (i32, i32), (i32, i32));

#[derive(Default, PartialEq)]
struct PortPlan {
    cells: std::collections::HashMap<PortKey, (u8, (i32, i32))>,
}

impl PortPlan {
    fn get(&self, box_i: usize, arrow: bool, edge: usize) -> Option<(u8, (i32, i32))> {
        self.cells.get(&(box_i, arrow, edge)).copied()
    }

    /// 所有端点(框, 边, 侧, 端口格)
    fn iter(&self) -> impl Iterator<Item = (usize, usize, u8, (i32, i32))> + '_ {
        self.cells
            .iter()
            .map(|(&(box_i, _arrow, e), &(side, cell))| (box_i, e, side, cell))
    }
}

/// 一个端点:在哪一侧上挑格
#[derive(Clone)]
struct PortEnd {
    box_i: usize,
    side: u8,
    arrow: bool,
    /// 点节点只有一格,不参与选格,只当对齐目标
    point: bool,
    /// 显式写死的侧:整组换侧时不碰它
    forced: bool,
    /// 两端都没分配时用来判断"哪一侧朝着对方":对方框的内容中心(有 via 用 via)
    aim: (i32, i32),
    /// 自己这一端的框中心
    center: (i32, i32),
}

/// 侧边压成一维后的横向坐标:上下侧看列,左右侧看行
fn port_lat(cell: (i32, i32), side: u8) -> i32 {
    if side == UP || side == DOWN {
        cell.0
    } else {
        cell.1
    }
}

/// 某一侧的候选格,按离内容中心的远近排好;缓存起来,一条边只算一次
fn port_cands(
    doc: &Document,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    cache: &mut std::collections::HashMap<(usize, u8), Vec<(i32, i32)>>,
    box_i: usize,
    side: u8,
) -> Vec<(i32, i32)> {
    cache
        .entry((box_i, side))
        .or_insert_with(|| {
            let mut v = side_ports(doc, box_i, rects[box_i], side, proj);
            v.truncate(48);
            v
        })
        .clone()
}

/// 一侧挤满时的退路,顺序固定:
/// 本侧空格 -> 本侧已有的线(合并)-> 相邻侧空格 -> 相邻侧已有的线 -> 本侧中心格
#[allow(clippy::too_many_arguments)]
fn overflow_port(
    doc: &Document,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    blocked: &HashSet<(i32, i32)>,
    cands: &mut std::collections::HashMap<(usize, u8), Vec<(i32, i32)>>,
    end: &PortEnd,
    roles: &std::collections::HashMap<(i32, i32), crate::canvas::PortKind>,
) -> Option<(u8, (i32, i32))> {
    let free_on = |side: u8, list: &[(i32, i32)]| -> Option<(i32, i32)> {
        // 外面一格是框的话,线/箭头会被那个框盖住:这一格不算能用
        list.iter()
            .copied()
            .find(|&c| roles.get(&c).is_none() && !blocked.contains(&Canvas::outside(c, side)))
    };
    // 最近的已有线:普通连接可以跟它 T 合并
    let share_on = |list: &[(i32, i32)], side: u8, want: i32| -> Option<(i32, i32)> {
        let mut best: Option<(i32, (i32, i32))> = None;
        for c in list.iter().copied() {
            if roles.get(&c) != Some(&crate::canvas::PortKind::Line) {
                continue;
            }
            if blocked.contains(&Canvas::outside(c, side)) {
                continue;
            }
            let key = ((port_lat(c, side) - want).abs(), c);
            if best.is_none_or(|x| key < x) {
                best = Some(key);
            }
        }
        best.map(|(_, c)| c)
    };
    let cc = port_lat(
        content_center(doc, end.box_i, proj, rects[end.box_i]),
        end.side,
    );
    // 按"离目标多近"给四个侧打分,不再固定顺时针:
    // 目标在上方的边不该先溢出到下面去绕一圈
    let mut sides: Vec<(i32, u8)> = all_sides()
        .into_iter()
        .map(|side| {
            let list = port_cands(doc, proj, rects, cands, end.box_i, side);
            (side_score(end, side, &list), side)
        })
        .collect();
    // 本侧优先(它本来就在目标方向上),其余按分数
    sides.sort_by_key(|&(sc, side)| (side != end.side, sc, side));
    for &(_, side) in &sides {
        let list = port_cands(doc, proj, rects, cands, end.box_i, side);
        if let Some(c) = free_on(side, &list) {
            return Some((side, c));
        }
        if !end.arrow {
            if let Some(c) = share_on(&list, side, cc) {
                return Some((side, c));
            }
        }
    }
    // 所有侧都没有空格了,但同一侧已经有箭头格:并到那一个上.
    // 一个只有一行文字的目标(比如 systemd 里的 dbus.service)左右各只有一个箭头位,
    // 指进来的边却有四条,不共用就有一半的箭头根本画不出来.
    // 只共用一个"已经有箭头"的格:普通连接格不行,那会把 T 并到箭头上
    if end.arrow {
        for &(_, side) in &sides {
            let list = port_cands(doc, proj, rects, cands, end.box_i, side);
            let want = port_lat(end.aim, side);
            let mut best: Option<(i32, (i32, i32))> = None;
            for c in list {
                // 只共用一个"已经是箭头端"的格:普通连接格并上去会把 T 压到箭头上
                if roles.get(&c) != Some(&crate::canvas::PortKind::Arrow) {
                    continue;
                }
                if blocked.contains(&Canvas::outside(c, side)) {
                    continue;
                }
                let key = ((port_lat(c, side) - want).abs(), c);
                if best.is_none_or(|x| key < x) {
                    best = Some(key);
                }
            }
            if let Some((_, c)) = best {
                return Some((side, c));
            }
        }
    }
    // 所有侧都没有合法格(低倍率下框只剩一格宽):不留计划条目,
    // 绘制时回退到老的"扫边框找空位",至少不会把箭头压到角上
    None
}

/// 一侧的容量:边框上能接线的格子数(带框的排除四角)
fn side_capacity(
    doc: &Document,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    box_i: usize,
    side: u8,
) -> usize {
    side_ports(doc, box_i, rects[box_i], side, proj).len()
}

/// 溢出时给一侧打分:走这一侧的外侧格离目标越近越好,背对目标直接重罚
fn side_score(end: &PortEnd, side: u8, list: &[(i32, i32)]) -> i32 {
    let d = (
        end.aim.0 as f64 - end.center.0 as f64,
        end.aim.1 as f64 - end.center.1 as f64,
    );
    let back = if side_backwards(d, side) { 1000 } else { 0 };
    let best = list
        .iter()
        .map(|&c| {
            let o = Canvas::outside(c, side);
            (o.0 - end.aim.0).abs() + (o.1 - end.aim.1).abs()
        })
        .min()
        .unwrap_or(0);
    back + best
}

fn all_sides() -> [u8; 4] {
    [UP, RIGHT, DOWN, LEFT]
}

/// 短边判定:边长的曼哈顿距离不超过两端框里最大的那条边(比例,不是固定格数)
fn relative_short_edge(rects: &[(i32, i32, i32, i32)], a: usize, b: usize) -> bool {
    let (ra, rb) = (rects[a], rects[b]);
    let scale = ra.2.max(ra.3).max(rb.2).max(rb.3).max(1) as f64;
    let (ca, cb) = (
        (
            ra.0 as f64 + ra.2 as f64 / 2.0,
            ra.1 as f64 + ra.3 as f64 / 2.0,
        ),
        (
            rb.0 as f64 + rb.2 as f64 / 2.0,
            rb.1 as f64 + rb.3 as f64 / 2.0,
        ),
    );
    (ca.0 - cb.0).abs() + (ca.1 - cb.1).abs() <= scale
}

/// 短边选侧对:两端都没有显式端口时枚举 4x4,挑折弯最少、长度最短的一对;
/// 两侧在对应轴上有重叠时优先直轴. 相对短的边才用,长边不受影响
fn short_edge_sides(
    rects: &[(i32, i32, i32, i32)],
    blocked: &HashSet<(i32, i32)>,
    a: usize,
    b: usize,
) -> Option<(u8, u8)> {
    let (ra, rb) = (rects[a], rects[b]);
    let x_overlap = ra.0.min(rb.0) < (ra.0 + ra.2).max(rb.0 + rb.2)
        && ra.0.max(rb.0) < (ra.0 + ra.2).min(rb.0 + rb.2);
    let y_overlap = ra.1.min(rb.1) < (ra.1 + ra.3).max(rb.1 + rb.3)
        && ra.1.max(rb.1) < (ra.1 + ra.3).min(rb.1 + rb.3);
    let center_of =
        |r: (i32, i32, i32, i32)| (r.0 as f64 + r.2 as f64 / 2.0, r.1 as f64 + r.3 as f64 / 2.0);
    let (ca, cb) = (center_of(ra), center_of(rb));
    let d_a = (cb.0 - ca.0, cb.1 - ca.1);
    let d_b = (ca.0 - cb.0, ca.1 - cb.1);
    let (dx, dy) = ((cb.0 - ca.0).abs(), (cb.1 - ca.1).abs());
    let mut best: Option<((i32, u8, u8), u8, u8)> = None;
    for src_side in all_sides() {
        let sc = Canvas::side_base(ra, src_side);
        let s = Canvas::outside(sc, src_side);
        if blocked.contains(&s) || side_backwards(d_a, src_side) {
            continue;
        }
        for dst_side in all_sides() {
            let dc = Canvas::side_base(rb, dst_side);
            let t = Canvas::outside(dc, dst_side);
            if blocked.contains(&t) || side_backwards(d_b, dst_side) {
                continue;
            }
            let vertical_pair = matches!(src_side, UP | DOWN) && matches!(dst_side, UP | DOWN);
            let horizontal_pair =
                matches!(src_side, LEFT | RIGHT) && matches!(dst_side, LEFT | RIGHT);
            let straight = (vertical_pair && x_overlap && s.0 == t.0)
                || (horizontal_pair && y_overlap && s.1 == t.1);
            let same_line = s.0 == t.0 || s.1 == t.1;
            let mut bends = if straight || same_line { 0 } else { 2 };
            let len = (s.0 - t.0).abs() + (s.1 - t.1).abs();
            let mut bonus = 0;
            if vertical_pair && x_overlap && dy >= 2.0 {
                bends -= 1;
                bonus -= 20;
            }
            if horizontal_pair && y_overlap && dx >= 2.0 {
                bends -= 1;
                bonus -= 20;
            }
            let key = (bends * 1000 + len + bonus, src_side, dst_side);
            if best.is_none_or(|(bk, _, _)| key < bk) {
                best = Some((key, src_side, dst_side));
            }
        }
    }
    best.map(|(_, s, d)| (s, d))
}

fn plan_ports(
    doc: &Document,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    layer: &str,
) -> PortPlan {
    use std::collections::HashMap;
    // 被框占住的格子:外侧格落在这里,这条路第一步就出不去
    let mut blocked: HashSet<(i32, i32)> = HashSet::new();
    for (i, r) in rects.iter().enumerate() {
        if doc.boxes[i].style.is_point() {
            blocked.insert((r.0, r.1));
            continue;
        }
        for y in r.1..r.1 + r.3 {
            for x in r.0..r.0 + r.2 {
                blocked.insert((x, y));
            }
        }
    }
    // 1. 先把这一层每条边的两端收集起来:选侧、算目标,都在分配格子之前做完
    let mut items: Vec<(usize, [PortEnd; 2])> = Vec::new();
    for (e, ed) in doc.edges.iter().enumerate() {
        if ed.layer != layer {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&ed.from), doc.box_idx(&ed.to)) else {
            continue;
        };
        if a == b {
            continue; // 自环走它自己那套
        }
        let (at_from, at_to) = ed.dir.heads();
        let via: Vec<(i32, i32)> = ed
            .via
            .iter()
            .map(|&(x, y)| proj.cell((x as f64, y as f64)))
            .collect();
        // 两端的目标:普通边用对方框的中心,有 via 时按 via 首/末
        let aim_a = via
            .first()
            .copied()
            .unwrap_or_else(|| proj.cell(doc.center(b)));
        let aim_b = via
            .last()
            .copied()
            .unwrap_or_else(|| proj.cell(doc.center(a)));
        let ends = [
            PortEnd {
                box_i: a,
                side: end_side(doc, a, b, rects, ed.from_side),
                arrow: at_from,
                point: doc.boxes[a].style.is_point(),
                forced: ed.from_side.is_some(),
                aim: aim_a,
                center: proj.cell(doc.center(a)),
            },
            PortEnd {
                box_i: b,
                side: end_side(doc, b, a, rects, ed.to_side),
                arrow: at_to,
                point: doc.boxes[b].style.is_point(),
                forced: ed.to_side.is_some(),
                aim: aim_b,
                center: proj.cell(doc.center(b)),
            },
        ];
        if ends.iter().all(|x| x.point) {
            continue;
        }
        let mut ends = ends;
        // 相对短的边:两端都没钉住时重挑一对侧,优先直轴(按比例判断,不看图规模)
        if !ends[0].forced && !ends[1].forced && relative_short_edge(rects, a, b) {
            if let Some((s0, s1)) = short_edge_sides(rects, &blocked, a, b) {
                ends[0].side = s0;
                ends[1].side = s1;
            }
        }
        items.push((e, ends));
    }
    // 2. 侧容量.
    //    - 带框节点的"瘦侧"(容量不到 2)装不下时整组换到容量够、方向也对的一侧;
    //    - 普通侧装不下时只移出多余的端点,优先移普通连接(箭头是这条边的语义,尽量不动);
    //    - 一行 `none` 节点不整组换侧:它左右本来就各一格,多条普通连接共用是正常的.
    {
        let mut groups: HashMap<(usize, u8), Vec<(usize, usize)>> = HashMap::new();
        for (k, (_, ends)) in items.iter().enumerate() {
            for (j, end) in ends.iter().enumerate() {
                if end.point {
                    continue;
                }
                groups
                    .entry((end.box_i, end.side))
                    .or_default()
                    .push((k, j));
            }
        }
        // 每侧当前负载(含显式端口),移动端点时同步增减
        let mut load: HashMap<(usize, u8), usize> =
            groups.iter().map(|(k, v)| (*k, v.len())).collect();
        let mut keys: Vec<(usize, u8)> = groups.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let (box_i, side) = key;
            let cap = side_capacity(doc, proj, rects, box_i, side);
            let count = *load.get(&key).unwrap_or(&0);
            if count <= cap {
                continue;
            }
            let list = groups.get(&key).cloned().unwrap_or_default();
            let any_forced = list.iter().any(|(k, j)| items[*k].1[*j].forced);
            // 2a. 瘦侧整组换侧(只对带框节点)
            if cap < 2 && !any_forced && doc.boxes[box_i].style.is_boxed() {
                let (mut ax, mut ay) = (0i64, 0i64);
                for (k, j) in &list {
                    let e = &items[*k].1[*j];
                    ax += e.aim.0 as i64;
                    ay += e.aim.1 as i64;
                }
                let aim = (
                    (ax / list.len() as i64) as i32,
                    (ay / list.len() as i64) as i32,
                );
                let center = items[list[0].0].1[list[0].1].center;
                let d = (
                    aim.0 as f64 - center.0 as f64,
                    aim.1 as f64 - center.1 as f64,
                );
                let need = list.len();
                let mut best: Option<(i32, u8)> = None;
                for alt in all_sides() {
                    if alt == side {
                        continue;
                    }
                    let alt_cap = side_capacity(doc, proj, rects, box_i, alt);
                    if alt_cap < need || side_backwards(d, alt) {
                        continue;
                    }
                    let probe = PortEnd {
                        box_i,
                        side: alt,
                        arrow: true,
                        point: false,
                        forced: false,
                        aim,
                        center,
                    };
                    let list_c: Vec<(i32, i32)> = side_ports(doc, box_i, rects[box_i], alt, proj);
                    let sc = side_score(&probe, alt, &list_c);
                    if best.is_none_or(|(bs, _)| sc < bs) {
                        best = Some((sc, alt));
                    }
                }
                if let Some((_, alt)) = best {
                    for (k, j) in &list {
                        items[*k].1[*j].side = alt;
                    }
                    *load.entry((box_i, alt)).or_insert(0) += list.len();
                    *load.entry(key).or_insert(0) = 0;
                    continue;
                }
            }
            // 2b. 普通侧:算出该移走哪几个端点
            let excess = count.saturating_sub(cap);
            if excess == 0 {
                continue;
            }
            // (gain, line 优先, 边下标, 端点下标, 备选侧)
            let mut moves: Vec<(i32, bool, usize, usize, u8)> = Vec::new();
            for (k, j) in &list {
                let end = &items[*k].1[*j];
                if end.forced {
                    continue;
                }
                // 普通连接在"一格宽"的侧上本来就可以 T 合并(一行高的文字节点),
                // 挪走反而不如共享:留在原侧
                if !end.arrow && cap <= 1 {
                    continue;
                }
                let d = (
                    end.aim.0 as f64 - end.center.0 as f64,
                    end.aim.1 as f64 - end.center.1 as f64,
                );
                let stay = side_score(end, side, &side_ports(doc, box_i, rects[box_i], side, proj));
                let mut best: Option<(i32, u8)> = None;
                for alt in all_sides() {
                    if alt == side || side_backwards(d, alt) {
                        continue;
                    }
                    if *load.get(&(box_i, alt)).unwrap_or(&0)
                        >= side_capacity(doc, proj, rects, box_i, alt)
                    {
                        continue;
                    }
                    let sc = side_score(end, alt, &side_ports(doc, box_i, rects[box_i], alt, proj));
                    if best.is_none_or(|(bs, _)| sc < bs) {
                        best = Some((sc, alt));
                    }
                }
                let Some((mv, alt)) = best else { continue };
                let mut gain = stay - mv;
                if !end.arrow {
                    gain += 100; // 普通连接方向性弱,先动它
                }
                moves.push((gain, !end.arrow, *k, *j, alt));
            }
            moves.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then(b.1.cmp(&a.1))
                    .then(a.2.cmp(&b.2))
                    .then(a.3.cmp(&b.3))
            });
            let mut moved = 0;
            for (_, _, k, j, alt) in moves {
                if moved == excess {
                    break;
                }
                items[k].1[j].side = alt;
                *load.entry((box_i, alt)).or_insert(0) += 1;
                moved += 1;
            }
            if moved > 0 {
                *load.entry(key).or_insert(0) = count - moved;
            }
        }
    }

    let mut plan = PortPlan::default();
    // 端口角色表:按**格子**记,一个格只能是一种角色(相邻两个框的端口可能撞在同一格).
    // 箭头独占一格(共用是"所有侧都没空格"之后的兜底),普通连接可以在窄侧 T 合并
    let mut port_role: HashMap<(i32, i32), crate::canvas::PortKind> = HashMap::new();
    // 这一格第一个占住它的是哪一侧:同一侧要顺延到下一格,只有对面侧才共用一个格
    // (一行文字节点的上下两侧本来就是同一条物理边)
    let mut port_side: HashMap<(i32, i32), u8> = HashMap::new();
    // 这一格的端口会不会真画东西(带框的边框格画 T/箭头).
    // 文字节点的端口格只是占位,线端在那上面什么都不画,箭头可以接管
    let mut port_drawn: HashMap<(i32, i32), bool> = HashMap::new();
    let mut cands: HashMap<(usize, u8), Vec<(i32, i32)>> = HashMap::new();
    // 一侧只有一格:几条普通连接挤在它上面是正常的(不过是不是"一格"看容量,不看文字)
    let mut narrow: HashSet<(usize, u8)> = HashSet::new();
    // 每个 (框, 侧) 已经占了哪些横向位置:算拥挤度用
    let mut taken: HashMap<(usize, u8), Vec<i32>> = HashMap::new();
    let crowd = |taken: &HashMap<(usize, u8), Vec<i32>>, end: &PortEnd, c: (i32, i32)| -> i32 {
        let lat = port_lat(c, end.side);
        taken
            .get(&(end.box_i, end.side))
            .map(|v| {
                v.iter()
                    .filter(|&&x| {
                        let d = (x - lat).abs();
                        (1..=2).contains(&d)
                    })
                    .count() as i32
            })
            .unwrap_or(0)
    };
    // 3. 按边在数组里的顺序增量分配:新边总在末尾,已有边的候选和代价都不变
    for (e, ends) in items.iter_mut() {
        let e = *e;
        let ed = &doc.edges[e];
        let cells_of = |end: &PortEnd, cands: &mut HashMap<(usize, u8), Vec<(i32, i32)>>| {
            if end.point {
                vec![proj.cell(doc.center(end.box_i))]
            } else {
                port_cands(doc, proj, rects, cands, end.box_i, end.side)
            }
        };
        let via: Vec<(i32, i32)> = ed
            .via
            .iter()
            .map(|&(x, y)| proj.cell((x as f64, y as f64)))
            .collect();
        // 端口外面一格落在框里(两个框挨着/叠着)= 线和箭头都会被那个框盖住.
        // 这种侧先整个换掉(按方向打分挑另一侧),换不到再保留原侧
        for end_slot in ends.iter_mut() {
            let end = end_slot.clone();
            if end.point {
                continue;
            }
            let has_exit = |list: &[(i32, i32)], side: u8| -> bool {
                list.iter()
                    .any(|c| !blocked.contains(&Canvas::outside(*c, side)))
            };
            let here = port_cands(doc, proj, rects, &mut cands, end.box_i, end.side);
            if has_exit(&here, end.side) {
                continue;
            }
            let mut best: Option<(i32, u8)> = None;
            for alt in all_sides() {
                if alt == end.side {
                    continue;
                }
                let list = port_cands(doc, proj, rects, &mut cands, end.box_i, alt);
                if !list
                    .iter()
                    .any(|c| !blocked.contains(&Canvas::outside(*c, alt)))
                {
                    continue;
                }
                let probe = PortEnd {
                    side: alt,
                    ..end.clone()
                };
                let sc = side_score(&probe, alt, &list);
                if best.is_none_or(|(bs, _)| sc < bs) {
                    best = Some((sc, alt));
                }
            }
            if let Some((_, alt)) = best {
                end_slot.side = alt;
            }
        }
        // 这一侧能出去的格优先;全都出不去就保留原表,交给绘制兜底
        let usable = |list: Vec<(i32, i32)>, side: u8| -> Vec<(i32, i32)> {
            let keep: Vec<(i32, i32)> = list
                .iter()
                .copied()
                .filter(|c| !blocked.contains(&Canvas::outside(*c, side)))
                .collect();
            if keep.is_empty() {
                list
            } else {
                keep
            }
        };
        let cs = usable(cells_of(&ends[0], &mut cands), ends[0].side);
        let ct = usable(cells_of(&ends[1], &mut cands), ends[1].side);
        for (end, list) in [(&ends[0], &cs), (&ends[1], &ct)] {
            if !end.point && list.len() <= 1 {
                narrow.insert((end.box_i, end.side));
            }
        }
        // 端口往外一格,就是线真正起步的地方
        let target = |end: &PortEnd, c: (i32, i32)| {
            if end.point {
                c
            } else {
                Canvas::outside(c, end.side)
            }
        };
        let free = |end: &PortEnd, c: (i32, i32)| {
            if end.point {
                return true;
            }
            let role_cell = port_role.get(&c).copied();
            if end.arrow {
                // 箭头端:只看端口格的角色(32 的规则).外侧格的角色只约束线端,
                // 不然箭头会被挤到"外侧格埋在邻居框里"的地方,盖住别人的文字
                if role_cell == Some(crate::canvas::PortKind::Line) {
                    // 那一格是线端占的位,但什么都不画:箭头可以接管
                    return !port_drawn.get(&c).copied().unwrap_or(false);
                }
                if role_cell == Some(crate::canvas::PortKind::Arrow) {
                    // 对面侧可以共用(一行文字节点的上下两侧是同一格);
                    // 同一侧仍然顺延到下一格,挤不下了才在 overflow 兜底里共用
                    return port_side.get(&c).is_some_and(|s| *s != end.side);
                }
                return true;
            }
            // 线端:不能占别人的箭头格;外侧格由绘制层保证(Canvas::outside_arrow 不会
            // 在别人的线端外侧格上画箭头),规划层不因此挪端口,否则整条边会被挤歪
            if role_cell == Some(crate::canvas::PortKind::Arrow) {
                return false;
            }
            if role_cell == Some(crate::canvas::PortKind::Line) {
                return narrow.contains(&(end.box_i, end.side));
            }
            true
        };
        let mut best: Option<PortChoice> = None;
        for &c_s in &cs {
            if !free(&ends[0], c_s) {
                continue;
            }
            for &c_t in &ct {
                if !free(&ends[1], c_t) {
                    continue;
                }
                let (ts, tt) = (target(&ends[0], c_s), target(&ends[1], c_t));
                // 对面的外侧格决定这一端该横向落在哪:对齐了就没有多余的 jog
                let aim_s = via.first().copied().unwrap_or(tt);
                let aim_t = via.last().copied().unwrap_or(ts);
                let ideal_s = port_lat(aim_s, ends[0].side);
                let ideal_t = port_lat(aim_t, ends[1].side);
                let (ls, lt) = (port_lat(c_s, ends[0].side), port_lat(c_t, ends[1].side));
                // 主项是"对齐":离对面外侧格越远,jog 越长
                let mut cost = 10 * ((ls - ideal_s).abs() + (lt - ideal_t).abs());
                // 次项轻微偏好内容中心,免得端口全跑到边上
                let (r_s, r_t) = (rects[ends[0].box_i], rects[ends[1].box_i]);
                let cc_s = port_lat(content_center(doc, ends[0].box_i, proj, r_s), ends[0].side);
                let cc_t = port_lat(content_center(doc, ends[1].box_i, proj, r_t), ends[1].side);
                cost += (ls - cc_s).abs() + (lt - cc_t).abs();
                cost += 5 * (crowd(&taken, &ends[0], c_s) + crowd(&taken, &ends[1], c_t));
                if !ends[0].point && blocked.contains(&ts) {
                    cost += 1000;
                }
                if !ends[1].point && blocked.contains(&tt) {
                    cost += 1000;
                }
                let key = (cost, c_s, c_t);

                if best.is_none_or(|x| key < x) {
                    best = Some(key);
                }
            }
        }
        let mut chosen: [Option<(u8, (i32, i32))>; 2] = [None, None];
        match best {
            Some((_, c_s, c_t)) => {
                chosen[0] = Some((ends[0].side, c_s));
                chosen[1] = Some((ends[1].side, c_t));
            }
            // 一端或两端都没有空格:各自找溢出位置,已有端口不动
            None => {
                for (k, end) in ends.iter().enumerate() {
                    chosen[k] = if end.point {
                        Some((end.side, proj.cell(doc.center(end.box_i))))
                    } else {
                        overflow_port(doc, proj, rects, &blocked, &mut cands, end, &port_role)
                    };
                }
            }
        }
        for (k, end) in ends.iter().enumerate() {
            let Some((side, cell)) = chosen[k] else {
                continue;
            };
            plan.cells.insert((end.box_i, end.arrow, e), (side, cell));
            if end.point {
                continue;
            }
            let kind = if end.arrow {
                crate::canvas::PortKind::Arrow
            } else {
                crate::canvas::PortKind::Line
            };
            // 箭头赢过"什么都不画"的线端占位;其余的按先占的说了算
            let drawn = doc.boxes[end.box_i].style.is_boxed();
            match port_role.get(&cell) {
                None => {
                    port_role.insert(cell, kind);
                }
                Some(crate::canvas::PortKind::Line) if kind == crate::canvas::PortKind::Arrow => {
                    if !port_drawn.get(&cell).copied().unwrap_or(false) {
                        port_role.insert(cell, kind);
                    }
                }
                Some(_) => {}
            }
            port_side.entry(cell).or_insert(side);
            port_drawn
                .entry(cell)
                .and_modify(|d| *d |= drawn)
                .or_insert(drawn);
            taken
                .entry((end.box_i, side))
                .or_default()
                .push(port_lat(cell, side));
        }
    }
    plan
}

/// 框在屏幕上的矩形:(左上角列, 左上角行, 宽, 高)
pub fn rect_of(doc: &Document, i: usize, proj: &Proj) -> (i32, i32, i32, i32) {
    let b = &doc.boxes[i];
    let (x, y) = proj.cell((b.at.0 as f64, b.at.1 as f64));
    let (w, h) = screen_size(b, proj.scale);
    (x, y, w, h)
}

/// 一个框的文字(校验保证了每个框恰好一条)
/// 一个框的"名字"给状态行/标注用:多行文字只取第一行
fn label_of(doc: &Document, i: usize) -> &str {
    doc.text(i)
        .and_then(|t| t.text.split('\n').next())
        .unwrap_or("")
}

/// 矩形选中的 box:锚点格到光标格之间的闭矩形(屏幕格坐标).
/// view.sel 是 None 时返回空表
pub fn selected(doc: &Document, p: &Panels, view: &View) -> Vec<usize> {
    selected_all(doc, p, view, &FrameHit::default()).0
}

/// 选区的屏幕矩形(闭区间),没有选区就是 None
fn sel_rect(p: &Panels, view: &View) -> Option<(i32, i32, i32, i32)> {
    // 拖框期间不画选区矩形(框、文字、边照常)
    if view.drag_hide_sel {
        return None;
    }
    let sel = view.sel?;
    // 没确认时另一端跟着光标走;确认之后才固定在 end 上
    let end = if sel.confirmed { sel.end } else { view.cursor };
    let (ax, ay) = p.proj.cell((sel.anchor.0 as f64, sel.anchor.1 as f64));
    let (cx, cy) = p.proj.cell((end.0 as f64, end.1 as f64));
    Some((ax.min(cx), ay.min(cy), ax.max(cx), ay.max(cy)))
}

/// 选区里有哪些框和边:框看矩形相交,边看它的可见格有没有落在矩形里
pub fn selected_all(
    doc: &Document,
    p: &Panels,
    view: &View,
    hit: &FrameHit,
) -> (Vec<usize>, Vec<usize>) {
    let Some((x0, y0, x1, y1)) = sel_rect(p, view) else {
        return (Vec::new(), Vec::new());
    };
    let ego = ego_visible(doc, view);
    let boxes: Vec<usize> = (0..doc.boxes.len())
        .filter(|&i| {
            let (left, top, w, h) = rect_of(doc, i, &p.proj);
            !ego_hides(ego.as_ref(), i) && left <= x1 && left + w > x0 && top <= y1 && top + h > y0
        })
        .collect();
    let inside = |c: (i32, i32)| c.0 >= x0 && c.0 <= x1 && c.1 >= y0 && c.1 <= y1;
    let mut edges: Vec<usize> = hit
        .edge_cells
        .iter()
        .filter(|(_, cells)| cells.iter().any(|c| inside(*c)))
        .map(|(e, _)| *e)
        .collect();
    edges.sort_unstable();
    (boxes, edges)
}

/// 连边模式的目标集合:显式勾选(Shift+左键)优先,其次选区,最后光标下的框
pub fn link_targets(doc: &Document, p: &Panels, view: &View) -> Vec<usize> {
    let Some(link) = &view.link else {
        return Vec::new();
    };
    if !link.targets.is_empty() {
        return link.targets.clone();
    }
    let mut out = if view.sel.is_some() {
        selected_all(doc, p, view, &FrameHit::default()).0
    } else {
        match view.pick {
            Pick::Box(i) => vec![i],
            _ => Vec::new(),
        }
    };
    out.sort_unstable();
    out.dedup();
    out
}

/// 选中的节点要反显的格子:普通节点就自己在那一格,框节点是整个边框
/// (框里的文字不动,不然整块都翻过来太吵)
fn selected_cells(doc: &Document, p: &Panels, view: &View, hit: &FrameHit) -> HashSet<(i32, i32)> {
    let mut out = HashSet::new();
    let (boxes, edges) = selected_all(doc, p, view, hit);
    for e in edges {
        if let Some(cells) = hit.edge_cells.get(&e) {
            out.extend(cells.iter().copied());
        }
    }
    for i in boxes {
        // 注释收起来时,注释框不画选中框
        if !view.notes && doc.is_note(i) {
            continue;
        }
        let (left, top, w, h) = rect_of(doc, i, &p.proj);
        if doc.boxes[i].style.is_point() {
            out.insert((left, top));
            continue;
        }
        if matches!(doc.boxes[i].style, BoxStyle::None) {
            // 没有边框:整块文字区都算选中
            for y in top..top + h {
                for x in left..left + w {
                    out.insert((x, y));
                }
            }
            continue;
        }
        for y in top..top + h {
            for x in left..left + w {
                if x == left || x == left + w - 1 || y == top || y == top + h - 1 {
                    out.insert((x, y));
                }
            }
        }
    }
    out
}

/// 区块选择的框:虚线灰框,只画在地图区里,不压图例/状态行
fn draw_sel_box(buf: &mut Buffer, p: &Panels, view: &View) {
    let Some((x0, y0, x1, y1)) = sel_rect(p, view) else {
        return;
    };
    let map = p.map;
    let inside = |x: i32, y: i32| {
        x >= map.x as i32
            && x < (map.x + map.width) as i32
            && y >= map.y as i32
            && y < (map.y + map.height) as i32
    };
    // 只改底色,不动字形:框里的字还能看清;灰边是连续的,不留断口
    let put_mark = |buf: &mut Buffer, x: i32, y: i32| {
        if !inside(x, y) {
            return;
        }
        buf[(x as u16, y as u16)].set_bg(SEL_GRAY);
    };
    for x in x0..=x1 {
        put_mark(buf, x, y0);
        put_mark(buf, x, y1);
    }
    for y in y0..=y1 {
        put_mark(buf, x0, y);
        put_mark(buf, x1, y);
    }
}

/// 整幅图在屏幕上的范围:所有框的并集再留两格给绕线.走线按它算,
/// 不按视口算 —— 不然挪一下视图,同一张图的路线就换了一条
fn doc_frame(rects: &[(i32, i32, i32, i32)], map: Rect) -> (i32, i32, i32, i32) {
    let mut f: Option<(i32, i32, i32, i32)> = None;
    for r in rects {
        if r.2 <= 0 || r.3 <= 0 {
            continue;
        }
        let (x0, y0, x1, y1) = (r.0, r.1, r.0 + r.2 - 1, r.1 + r.3 - 1);
        f = Some(match f {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    match f {
        // 留的余量要盖住搜索窗口(两端外扩 6 格),不然绕行会被这一圈提前裁掉
        Some((x0, y0, x1, y1)) => (x0 - 8, y0 - 8, x1 + 8, y1 + 8),
        None => (
            map.x as i32,
            map.y as i32,
            (map.x + map.width) as i32 - 1,
            (map.y + map.height) as i32 - 1,
        ),
    }
}

/// 连边预览:源 -> 目标的临时线(虚线暗色,不落 T/箭头).最多画 64 条
pub(crate) fn draw_link_preview(
    buf: &mut Buffer,
    p: &Panels,
    doc: &Document,
    view: &View,
) -> usize {
    let Some(link) = &view.link else {
        return 0;
    };
    let targets = link_targets(doc, p, view);
    if p.map.width == 0 || doc.groups.is_empty() {
        return 0;
    }
    let map = p.map;
    let mut cv = Canvas::new(
        (
            map.x as i32,
            map.y as i32,
            map.width as i32,
            map.height as i32,
        ),
        vec![false; doc.groups.len()],
        0,
        view.glyphs,
    );
    cv.preview = true;
    let rects: Vec<(i32, i32, i32, i32)> = (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &p.proj))
        .collect();
    cv.frame = doc_frame(&rects, map);
    for (i, r) in rects.iter().copied().enumerate() {
        // 源框不挡自己那条预览线(自由预览就是从源框里画出来的)
        if link.sources.contains(&i) {
            continue;
        }
        if doc.boxes[i].style.is_point() {
            cv.station_cells.insert((r.0, r.1));
        } else {
            cv.reserve_box((r.0, r.1), (r.2 as usize, r.3 as usize));
        }
    }
    let mut n = 0;
    // 还没瞄到目标框:从每个源画一条虚线到光标,告诉用户"往这边走"
    if targets.is_empty() {
        let free = p.proj.cell((view.cursor.0 as f64, view.cursor.1 as f64));
        for &src in &link.sources {
            if n >= 64 {
                break;
            }
            if src >= doc.boxes.len() {
                continue;
            }
            let gr = doc
                .box_groups(src)
                .first()
                .copied()
                .filter(|g| *g < doc.groups.len())
                .unwrap_or(0);
            let a = p.proj.cell(doc.center(src));
            if a == free {
                continue;
            }
            cv.edge_full(
                a,
                free,
                gr,
                PREVIEW,
                None,
                (false, false),
                EdgeStyle::Dashed,
                None,
                None,
                None,
            );
            n += 1;
        }
    }
    for &src in &link.sources {
        if src >= doc.boxes.len() {
            continue;
        }
        let gr = doc
            .box_groups(src)
            .first()
            .copied()
            .filter(|g| *g < doc.groups.len())
            .unwrap_or(0);
        for &dst in &targets {
            if n >= 64 {
                break;
            }
            if src == dst || dst >= doc.boxes.len() {
                continue;
            }
            let lrects: Vec<(i32, i32, i32, i32)> = (0..doc.boxes.len())
                .map(|k| rect_of(doc, k, &p.proj))
                .collect();
            let a_end = end_mark(doc, src, dst, &lrects, None);
            let b_end = end_mark(doc, dst, src, &lrects, None);
            let a = p.proj.cell(doc.center(src));
            let b = p.proj.cell(doc.center(dst));
            cv.edge_full(
                a,
                b,
                gr,
                PREVIEW,
                None,
                (false, true),
                EdgeStyle::Dashed,
                a_end,
                b_end,
                None,
            );
            n += 1;
        }
    }
    // 只写在空白格上:预览永远不压框、不压文字
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.mask[i] == 0 || cv.deco[i].is_some() {
                continue;
            }
            let ch = cv.glyphs.line(cv.mask[i], cv.corner[i]);
            // 虚线只断纯直笔(和正式连线一条规则),拐角照样实画
            let breaks = (ch == '─' || ch == '│')
                && cv.mask[i].count_ones() <= 2
                && !cv.style[i].keeps(x, y);
            if breaks {
                continue;
            }
            let cell = &mut buf[(x as u16, y as u16)];
            if cell.symbol() != " " {
                continue;
            }
            cell.set_char(ch);
            cell.set_style(Style::default().fg(Color::Rgb(PREVIEW.0, PREVIEW.1, PREVIEW.2)));
        }
    }
    n
}

/// 只画地图(边、框、文字),不画图例、状态行和任何浮层.导出 SVG / 图片用这个.
/// 投影用传入的 zoom,地图区就是整个 area
pub fn render_map(buf: &mut Buffer, area: Rect, doc: &Document, view: &View, zoom: f64) -> Rect {
    render_map_hit(buf, area, doc, view, zoom).0
}

/// 同 `render_map`,外加这一帧的命中表(导出/headless 的 json 要用)
pub fn render_map_hit(
    buf: &mut Buffer,
    area: Rect,
    doc: &Document,
    view: &View,
    zoom: f64,
) -> (Rect, FrameHit) {
    let map = area;
    if map.width == 0 || map.height == 0 || doc.boxes.is_empty() {
        return (map, FrameHit::default());
    }
    let p = Panels {
        map,
        proj: proj_of(doc, view, zoom).anchored(
            map.x as i32 + map.width as i32 / 2,
            map.y as i32 + map.height as i32 / 2,
        ),
    };
    let hit = draw_map(buf, &p, doc, view, &FrameHit::default());
    (map, hit)
}

/// 地图本身:连边、节点框、节点、文字、点节点的名标注
fn draw_map(buf: &mut Buffer, p: &Panels, doc: &Document, view: &View, hit: &FrameHit) -> FrameHit {
    let (map, proj) = (p.map, p.proj);
    if map.width == 0 || map.height == 0 || doc.boxes.is_empty() {
        return FrameHit::default();
    }
    // 低倍率下的细节等级:文字 / 形状 / 实心块(见 Lod).导出关掉它,永远画全套
    let lod = if view.lod {
        lod_of(view.zoom)
    } else {
        Lod::Full
    };
    let show_text = lod == Lod::Full;
    // 颜色冲突时谁说了算:聚焦了某几个分组就一直归它们(光标挪到别的分组上也不变),
    // 没聚焦时才是光标格下那个节点的分组
    let mut priority = vec![false; doc.groups.len()];
    let who: Vec<usize> = if view.focus.is_empty() {
        view.pick
            .box_at()
            .map(|c| doc.box_groups(c).to_vec())
            .unwrap_or_default()
    } else {
        view.focus.clone()
    };
    for gr in who {
        if gr < priority.len() {
            priority[gr] = true;
        }
    }
    let focus = focus_mask(view);
    // E 隔离:可见集合整帧只算一次,下面的绘制/命中都用它
    let ego = ego_visible(doc, view);
    // 注释开关:关掉时注释框当"不存在"(不画、不算障碍、不进命中/选区);
    // E 隔离开着时,绑在可见边上的那几条露出来
    let note_visible = |i: usize| -> bool {
        if !doc.is_note(i) || view.notes {
            return true;
        }
        ego.as_ref().is_some_and(|e| e.notes.contains(&i))
    };
    // 文字里的链接:画布格 -> 目标.框内文字和点节点名字都往里记
    let mut links: std::collections::HashMap<(i32, i32), String> = std::collections::HashMap::new();
    let mut cv = Canvas::new(
        (
            map.x as i32,
            map.y as i32,
            map.width as i32,
            map.height as i32,
        ),
        priority,
        focus,
        view.glyphs,
    );
    cv.layer_filter = view.layer.is_some();
    // 选中的边盖在其它边上面(默认是先画的在上面);
    // E 模式下亮着的那几条边也一样,不然交叉处会被别的线断掉
    cv.raise(view.pick.edge());
    cv.picked = view.pick.edge();
    if let Some(n) = view.ego.filter(|n| *n < doc.boxes.len()) {
        for &e in doc.adj(n) {
            cv.raise(Some(e));
        }
    }
    cv.arrow_style = view.arrow_style;
    // 每个框在屏幕上的矩形,后面画边、命中、选择都用它
    let mut rects: Vec<(i32, i32, i32, i32)> = (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &proj))
        .collect();
    // 就地改字时,框按草稿的尺寸画(长出来的字要看得见)
    if let Some(edit) = &view.edit {
        if let Some(&(x, y, _, _)) = rects.get(edit.box_idx) {
            let (w, h) = edit.text_size(doc, edit.box_idx);
            rects[edit.box_idx] = (x, y, w, h);
        }
    }
    cv.frame = doc_frame(&rects, map);
    // 节点框占的格子先登记好:画边时要避开整个框
    for (i, r) in rects.iter().enumerate() {
        if !note_visible(i) {
            continue;
        }
        if doc.boxes[i].style.is_point() {
            cv.station_cells.insert((r.0, r.1));
        } else {
            // 带框的与"只有文字"的都占整块:线别从字上穿过去
            cv.reserve_box((r.0, r.1), (r.2 as usize, r.3 as usize));
        }
    }
    // 0. 区域(cluster):先铺在最下面,后画的边和框自然盖住它.
    //    深浅排序:外层先画,内层后画,内层边框盖在外层上面
    let mut order_r: Vec<usize> = (0..doc.regions.len()).collect();
    order_r.sort_by_key(|&i| (doc.region_depth(i), i));
    for i in &order_r {
        let r = &doc.regions[*i];
        // 隔离时区域全部收起来:它多半框着已经消失的节点,留着只是空框
        if ego.is_some() {
            continue;
        }
        // 只看/只动某一层时,别层的区域也压掉(和框、边一致)
        if !lit_layer(view, &r.layer) {
            continue;
        }
        let Some((rx, ry, rw, rh)) = doc.region_rect(*i) else {
            continue;
        };
        let fg = r.color.unwrap_or(REGION_INK);
        let (x, y) = proj.cell((rx as f64, ry as f64));
        let (x1, y1) = proj.cell(((rx + rw - 1) as f64, (ry + rh - 1) as f64));
        let (w, h) = ((x1 - x).max(0) as usize + 1, (y1 - y).max(0) as usize + 1);
        let title = r.label.trim();
        if r.collapsed {
            // 折叠:只剩一个标题条,成员框和它们的边都不画
            let tw = title.chars().count() + 4;
            let (cw, ch) = (tw, 3usize);
            cv.plate_at((x, y), (cw, ch), r.style, fg);
            if !title.is_empty() {
                cv.stamp_at(x + 2, y + 1, title, fg);
            }
            // 右上角一个小标记:说明这里是折叠的区域
            if cw >= 3 {
                let mark = if view.glyphs.ascii { '+' } else { '…' };
                cv.stamp_at(x + cw as i32 - 2, y + 1, &mark.to_string(), fg);
            }
        } else {
            cv.plate_at((x, y), (w, h), r.style, fg);
            if !title.is_empty() {
                // 标题骑在左上角那条边上:` label `
                let text = format!(" {} ", title);
                let avail = w.saturating_sub(2);
                let text: String = text.chars().take(avail).collect();
                cv.stamp_at(x + 1, y, &text, fg);
            }
        }
    }
    // 1. 连边:先按图层顺序规划一遍(每条边都看得见前面那些边的走向),
    //    再按同样的顺序落笔
    let layers = layer_order(doc, view);
    let key = route_key(doc, view, &proj, map);
    // 命中就直接用上一次的规划结果(同一个视口、同一份文档、同一个缩放)
    let cached = route_cache::get(key);
    match cached {
        Some(c) => {
            cv.plans = c.plans;
            cv.plan_edges = c.plan_edges;
            cv.usage = c.usage;
            cv.usage_edges = c.usage_edges;
        }
        None => {
            cv.planning = true;
            for layer in &layers {
                cv.layer_lit = lit_layer(view, layer);
                draw_edges(&mut cv, doc, view, &proj, &rects, layer, ego.as_ref());
            }
            cv.planning = false;
            route_cache::put(route_cache::Cached {
                key,
                plans: cv.plans.clone(),
                plan_edges: cv.plan_edges.clone(),
                usage: cv.usage.clone(),
                usage_edges: cv.usage_edges.clone(),
            });
        }
    }
    for layer in &layers {
        cv.layer_lit = lit_layer(view, layer);
        draw_edges(&mut cv, doc, view, &proj, &rects, layer, ego.as_ref());
        // 2. 节点框与节点:同一个图层里,框盖在这一层的连线上面
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer
                || !b.style.is_boxed()
                || !note_visible(i)
                || doc.box_hidden(i)
                || ego_hides(ego.as_ref(), i)
            {
                continue;
            }
            let mask = doc
                .box_groups(i)
                .iter()
                .filter(|gr| **gr < 64)
                .fold(0u64, |m, gr| m | (1 << gr));
            let fg = doc.box_color(i).unwrap_or(BRIGHT);
            let r = rects[i];
            // 再小一点就当成一块实心色块:边框在 0.3 以下只剩一条线,看不出是框
            let fill = if lod == Lod::Blocks {
                Some(fg)
            } else {
                doc.boxes[i].fill
            };
            cv.box_at(
                i,
                (r.0, r.1),
                (r.2 as usize, r.3 as usize),
                doc.boxes[i].style,
                mask,
                fg,
                fill,
            );
        }
        // 3. 点节点:一格也只有一个颜色,同样让光标那条分组先挑
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer
                || b.style.is_boxed()
                || !note_visible(i)
                || doc.box_hidden(i)
                || ego_hides(ego.as_ref(), i)
            {
                continue;
            }
            let r = rects[i];
            let groups = doc.box_groups(i);
            let gr = groups
                .iter()
                .copied()
                .find(|gr| *gr < cv.priority.len() && cv.priority[*gr])
                .or_else(|| groups.first().copied());
            let mask = groups
                .iter()
                .filter(|gr| **gr < 64)
                .fold(0u64, |m, gr| m | (1 << gr));
            let fg = gr.map(|gr| doc.groups[gr].color).unwrap_or(BRIGHT);
            // 文字节点的字形落在文字那一格(框可能比文字大:换样式留下的白边);
            // 点节点的名字是另挂的,字形还是节点自己那一格
            let (gx, gy) = if matches!(b.style, BoxStyle::None) {
                doc.text(i)
                    .map(|t| proj.cell((t.at.0 as f64, t.at.1 as f64)))
                    .unwrap_or((r.0, r.1))
            } else {
                (r.0, r.1)
            };
            cv.node_at(gx, gy, doc.hub(i), mask, gr, fg);
        }
    }
    // 画出来的光标标记:就在光标格上
    let cursor_cell = proj.cell((view.cursor.0 as f64, view.cursor.1 as f64));
    let cursor_box = view.pick.box_at();
    let marked = selected_cells(doc, p, view, hit);

    // E 选中一条边:另一头的邻居节点一起高亮,一眼能看出这条边通到哪
    let picked_nodes: HashSet<(i32, i32)> = view
        .pick
        .edge()
        .and_then(|e| {
            let (a, b) = doc.edge_ends(e)?;
            let cur = doc.box_at_cell(view.cursor);
            Some(if cur == Some(a) { b } else { a })
        })
        .map(|n| {
            let (l, t, w, h) = rects[n];
            (0..w)
                .flat_map(|dx| (0..h).map(move |dy| (l + dx, t + dy)))
                .collect()
        })
        .unwrap_or_default();
    // 搜索命中:框用框的边框黄,点节点只有一格,先算出它们在屏幕上的格子
    let hit_cells: HashSet<(i32, i32)> = view
        .hits
        .iter()
        .filter(|i| **i < rects.len() && !ego_hides(ego.as_ref(), **i))
        .flat_map(|&i| {
            let (l, t, w, h) = rects[i];
            (0..w).flat_map(move |dx| (0..h).map(move |dy| (l + dx, t + dy)))
        })
        .collect();
    // 选中那条边的端口外侧格:以**实际落笔**的 out 点为准(plan 可能没有这一端),
    // 别人的箭头落在这里会让它看着像 <->,blit/overlay 时藏掉
    cv.picked_port_out = view
        .pick
        .edge()
        .and_then(|pe| cv.edge_out_cells.get(&pe).cloned())
        .unwrap_or_default();
    blit(
        buf,
        &cv,
        bright_style(),
        Some(cursor_cell),
        cursor_box,
        view.pulse,
        view.drawn,
        &marked,
        view.pick,
        &picked_nodes,
        ego.as_ref(),
        view.link
            .as_ref()
            .map(|l| l.sources.as_slice())
            .unwrap_or(&[]),
        &view.hits,
        &hit_cells,
    );
    // 4. 框里的文字:框内区域起点是 (左+1, 中行),按 align 在可用宽度里偏移.
    //    文字节点(None)没有边框:区域就是整块,起始在左上角.
    //    缩得太小时文字只剩碎片:LOD 一挡就整块跳过(注释框是框,跟着一起跳)
    // 正在动的那一个先写:两个框叠在一起时,用户手底下这个的字最完整
    let mut text_order: Vec<usize> = (0..doc.boxes.len()).collect();
    if let Some(p) = view.pick.box_at().or_else(|| doc.box_at_cell(view.cursor)) {
        if p < text_order.len() {
            text_order.retain(|&k| k != p);
            text_order.insert(0, p);
        }
    }
    // 别的框先写过的格子:后面的框不再覆盖(否则宽字的续格空格会把别人的字擦掉)
    let mut kept: HashSet<(i32, i32)> = HashSet::new();
    for &i in &text_order {
        let b = &doc.boxes[i];
        if !show_text {
            break;
        }
        if b.style.is_point() || !note_visible(i) || ego_hides(ego.as_ref(), i) {
            continue;
        }
        let editing = view.edit.as_ref().filter(|e| e.box_idx == i);
        let Some(t) = doc.text(i) else { continue };
        let (left, top, w, h) = rects[i];
        let bare = matches!(b.style, BoxStyle::None);
        // 文字节点:起点就是文字自己那一格(换样式时框可能比文字大,文字居中留着白边),
        // 带边框的:框内左上角,一行一行往下画
        let (region_x, region_y, max_w) = if bare {
            let (tx, ty) = proj.cell((t.at.0 as f64, t.at.1 as f64));
            (tx, ty, ((left + w) - tx).max(1) as usize)
        } else {
            (left + 1, top + 1, (w - 2).max(1) as usize)
        };
        if !bare && h < 3 {
            continue; // 压得只剩一条线:没有能放字的地方
        }
        // 被画面边缘裁掉时只画剩下的部分(逐格裁剪),不再整块消失
        let (clip_lo, clip_hi) = (map.x as i32, (map.x + map.width) as i32);
        if region_x >= clip_hi || region_x + max_w as i32 <= clip_lo {
            continue;
        }
        let on_focus = focus == 0
            || doc
                .box_groups(i)
                .iter()
                .any(|gr| *gr < 64 && focus & (1 << gr) != 0);
        let (r, gg, b) = text_ink(doc, view, i).unwrap_or(BRIGHT);
        let live = on_focus && lit_layer(view, &doc.boxes[i].layer);
        let mut style = if live {
            Style::default().fg(Color::Rgb(r, gg, b))
        } else {
            dim_style()
        };
        // 搜索命中的框:文字也换成搜索黄(文字节点没有边框可染,就靠它)
        if live && view.hits.contains(&i) {
            style = style
                .fg(Color::Rgb(SEARCH_HIT.0, SEARCH_HIT.1, SEARCH_HIT.2))
                .add_modifier(Modifier::BOLD);
        }
        // 底色:文字自己的底色优先,其次框的填充(两者独立);压暗时不画
        if live {
            if let Some((r, gg, b)) = t.bg.or(doc.boxes[i].fill) {
                style = style.bg(Color::Rgb(r, gg, b));
            }
        }
        let bottom = map.y as i32 + map.height as i32;
        // 编辑态:按字面画草稿(不解析 markdown),光标那一格反显
        if let Some(edit) = editing {
            let mut row = 0usize;
            for line in edit.text.split('\n') {
                let ty = region_y + row as i32;
                if ty >= bottom {
                    break;
                }
                for (col, ch) in line.chars().enumerate() {
                    if col >= max_w {
                        break;
                    }
                    let cx = region_x + col as i32;
                    if cx < map.x as i32 || cx >= (map.x + map.width) as i32 {
                        continue;
                    }
                    if let Some(c) = buf.cell_mut((cx as u16, ty as u16)) {
                        c.set_symbol(&ch.to_string());
                        c.set_style(style);
                    }
                }
                row += 1;
            }
            // 光标不在这里画:真光标(view.cursor)已经落在草稿对应的格子上,
            // 画面上只该有一个光标
            continue;
        }
        // 这一段文字会不会压在已经写好的文字上(叠在一起的框):
        // 会的话整段让开,不要缺几个字,看着像"字丢了"
        let mut collides = false;
        for (k, line) in t.text.split('\n').enumerate() {
            let ty = region_y + k as i32;
            let spans = text::parse(line);
            let tw = text::width_of(&spans).min(max_w);
            let off = match t.align {
                Align::Left => 0,
                Align::Center => max_w.saturating_sub(tw) / 2,
                Align::Right => max_w.saturating_sub(tw),
            };
            let x0 = region_x + off as i32;
            if kept
                .iter()
                .any(|&(kx, ky)| ky == ty && kx >= x0 && kx < x0 + tw as i32)
            {
                collides = true;
                break;
            }
        }
        if collides {
            continue;
        }
        for (k, line) in t.text.split('\n').enumerate() {
            let ty = region_y + k as i32;
            if ty >= bottom {
                break; // 超出画面就别写了
            }
            if ty < map.y as i32 {
                continue; // 上半截在画面外
            }
            let spans = text::parse(line);
            let tw = text::width_of(&spans).min(max_w);
            let off = match t.align {
                Align::Left => 0,
                Align::Center => max_w.saturating_sub(tw) / 2,
                Align::Right => max_w.saturating_sub(tw),
            };
            text::put_link_clip_kept(
                buf,
                region_x + off as i32,
                ty,
                &spans,
                style,
                max_w,
                (clip_lo, clip_hi),
                &mut kept,
                |x, y, target| {
                    links.insert(proj.canvas(x as i32, y as i32), target.to_string());
                },
            );
        }
    }
    // 间距够宽(相邻节点拉开到 4 格以上)就自动把点节点的名字标出来
    let labels = show_text && (view.labels || median_edge_cells(doc, &proj) >= 4.5);
    // 5. 标注(画在最上层,只写在空位上),三拨共用一张占用表:
    //    端点/枢纽的点节点名 -> 边标签 -> 其余点节点名.带框的节点文字已经在框里了
    let mut used: HashSet<(i32, i32)> = HashSet::new();
    let mut order: Vec<usize> = (0..doc.boxes.len())
        .filter(|&i| !ego_hides(ego.as_ref(), i))
        .collect();
    order.sort_by_key(|&i| {
        let terminus = doc.adj(i).len() == 1;
        !(doc.hub(i) || terminus)
    });
    let head_of = |i: usize| doc.hub(i) || doc.adj(i).len() == 1;
    let mut skipped: Vec<LabelSkip> = Vec::new();
    let mut place = |i: usize, skipped: &mut Vec<LabelSkip>| {
        let placed = draw_point_label(
            buf, &cv, map, &proj, doc, view, focus, &mut used, i, labels, &mut links,
        );
        if !placed {
            // 放不下:记下来,状态栏 / headless 要靠它说"有 N 个名字没画"
            skipped.push(LabelSkip {
                box_id: doc.boxes[i].id.clone(),
                text: label_of(doc, i).to_string(),
                cell: proj.cell(doc.center(i)),
            });
        }
    };
    for &i in &order {
        if head_of(i) {
            place(i, &mut skipped);
        }
    }
    for &i in &order {
        if !head_of(i) {
            place(i, &mut skipped);
        }
    }
    // E 模式:高亮的边压过框和文字,蓝色路径完整可见
    if let Some(ego) = &ego {
        overlay_ego_edges(buf, &cv, doc, ego, view.pick.edge(), view.pulse);
    }
    let mut hit = FrameHit::from(&cv);
    hit.rev = doc.rev();
    hit.labels_skipped = skipped;
    hit.links = links;
    // 端点格:按渲染时同一套端口规则算一遍(拖端点重连靠它命中)
    for (e, edge) in doc.edges.iter().enumerate() {
        let (Some(a), Some(b)) = (doc.box_idx(&edge.from), doc.box_idx(&edge.to)) else {
            continue;
        };
        let ca = end_of(doc, &rects, a, b, edge.from_side)
            .map(|m| m.cell)
            .unwrap_or_else(|| center_cell(doc, &proj, a));
        let cb = end_of(doc, &rects, b, a, edge.to_side)
            .map(|m| m.cell)
            .unwrap_or_else(|| center_cell(doc, &proj, b));
        hit.edge_ends.insert(e, (ca, cb));
    }
    hit
}

/// 这个位置放得下一行宽 `w` 的字吗:在画面里,每一格都空、也没被别的标签占
fn label_fits(cv: &Canvas, used: &HashSet<(i32, i32)>, map: Rect, x: i32, y: i32, w: i32) -> bool {
    if x < map.x as i32
        || y < map.y as i32
        || x + w > (map.x + map.width) as i32
        || y >= (map.y + map.height) as i32
    {
        return false;
    }
    !(0..w).any(|k| cv.at(x + k, y) != 0 || used.contains(&(x + k, y)))
}

/// 占住这一行与上下各一行:标签之间自然留出空当
fn reserve_label(used: &mut HashSet<(i32, i32)>, x: i32, y: i32, w: i32) {
    for k in 0..w + 1 {
        used.insert((x + k, y));
        used.insert((x + k, y + 1));
        used.insert((x + k, y - 1));
    }
}

/// 给一个点节点挂名字:文字自己的位置优先,否则挑四个候选位(端点先背离邻居)
#[allow(clippy::too_many_arguments)]
fn draw_point_label(
    buf: &mut Buffer,
    cv: &Canvas,
    map: Rect,
    proj: &Proj,
    doc: &Document,
    view: &View,
    focus: u64,
    used: &mut HashSet<(i32, i32)>,
    i: usize,
    labels: bool,
    links: &mut std::collections::HashMap<(i32, i32), String>,
) -> bool {
    // 只有点节点需要外挂名字:带框的在框里,文字节点本身就在画字
    if !doc.boxes[i].style.is_point() {
        return true;
    }
    let terminus = doc.adj(i).len() == 1;
    if !labels && !terminus {
        return true;
    }
    // 点节点的字形那一格:按框左上角算(1x1,所以就是它).
    // 用中心点算会舍入到旁边一格,名字就有机会盖掉字形自己
    let (x, y) = proj.cell((doc.boxes[i].at.0 as f64, doc.boxes[i].at.1 as f64));
    let label = label_of(doc, i);
    // 名字里可能有 `**粗体**` 和 `[文字](目标)`:按标记走,宽度只算看得见的字
    let spans = text::parse(label);
    let w = text::width_of(&spans) as i32;
    if label.is_empty() || w == 0 {
        return true;
    }
    // stored_at 优先:文字自己的位置就是画在这里
    let stored = doc
        .text(i)
        .map(|t| proj.cell((t.at.0 as f64, t.at.1 as f64)));
    // 四个候选位:右边、左边、上面居中、下面居中
    let mut slots = [
        (x + 2, y),
        (x - w - 1, y),
        (x - w / 2, y - 1),
        (x - w / 2, y + 1),
    ];
    // 端点:先试"背着邻居"的那个方向,标签就像从边的末端伸出去
    if terminus {
        if let Some(&e) = doc.adj(i).first() {
            if let Some((a, b)) = doc.edge_ends(e) {
                let nbr = if a == i { b } else { a };
                let n = proj.cell(doc.center(nbr));
                let (dx, dy) = ((x - n.0).signum(), (y - n.1).signum());
                slots.sort_by_key(|(lx, ly)| {
                    let (ox, oy) = ((lx - x).signum(), (ly - y).signum());
                    // 与"背离邻居"方向一致的最靠前
                    (ox != dx || oy != dy) as u8
                });
            }
        }
    }
    // 四个标准位都被线或别的名字占着时,再试一圈"错开一格"的位置:
    // 节点正好压在一横一竖的交叉点上时(比如朝代图里的东周),上下两行常常
    // 靠边的位置其实是空的,只挤在正中反而放不下
    let mut extra: Vec<(i32, i32)> = Vec::new();
    for dy in [-1i32, 1] {
        for dx in [-w - 1, 2, -w / 2, -w / 2 - 1, -w / 2 + 1] {
            extra.push((x + dx, y + dy));
        }
    }
    for dx in [-w - 1, 2] {
        for dy in [-1i32, 1] {
            extra.push((x + dx, y + dy));
        }
    }
    let pick = stored
        .filter(|&q| q != (x, y) && label_fits(cv, used, map, q.0, q.1, w))
        .or_else(|| {
            slots
                .iter()
                .copied()
                .find(|&(lx, ly)| label_fits(cv, used, map, lx, ly, w))
        })
        .or_else(|| {
            extra
                .into_iter()
                .find(|&(lx, ly)| label_fits(cv, used, map, lx, ly, w))
        });
    let Some((lx, ly)) = pick else { return false };
    reserve_label(used, lx, ly, w);
    let on_focus = focus == 0
        || doc
            .box_groups(i)
            .iter()
            .any(|gr| *gr < 64 && focus & (1 << gr) != 0);
    let text_fg = doc.text(i).and_then(|t| t.color).or(view.text_ink);
    let picked_here = view.pick.box_at() == Some(i);
    let style = if view.hits.contains(&i) && !picked_here {
        // 搜索命中的点节点:名字也染成搜索黄,不然只有一个小标记变色看不出来
        Style::default()
            .fg(Color::Rgb(SEARCH_HIT.0, SEARCH_HIT.1, SEARCH_HIT.2))
            .add_modifier(Modifier::BOLD)
    } else if picked_here {
        // 选中的点节点:名字跟着呼吸(没有框可闪,闪字)
        bright_style()
            .fg(pulse_color(BRIGHT, view.pulse))
            .add_modifier(Modifier::BOLD)
    } else if !on_focus || !lit_layer(view, &doc.boxes[i].layer) {
        dim_style()
    } else if let Some((r, gg, b)) = text_fg {
        // 点节点没有边框,文字只认自己的 color
        Style::default().fg(Color::Rgb(r, gg, b))
    } else if doc.hub(i) || terminus {
        bright_style()
    } else {
        Style::default().fg(Color::Rgb(LABEL.0, LABEL.1, LABEL.2))
    };
    text::put_link(
        buf,
        lx as u16,
        ly as u16,
        &spans,
        style,
        usize::MAX,
        |x, y, target| {
            links.insert(proj.canvas(x as i32, y as i32), target.to_string());
        },
    );
    true
}

/// 输入行显示的文字:多行只显示第一行,后面缀 `[+N 行]`
pub fn prompt_display(text: &str, lang: Lang) -> String {
    let mut it = text.split('\n');
    let first = it.next().unwrap_or("");
    let more = it.count();
    if more == 0 {
        first.to_string()
    } else {
        format!(
            "{} {}",
            first,
            lang.pick(&format!("[+{more} 行]"), &format!("[+{more} lines]"))
        )
    }
}

/// 只画图(地图),不画状态行/覆盖层:`--dump` 与导出用
pub fn render_picture_hit(
    buf: &mut Buffer,
    area: Rect,
    doc: &Document,
    view: &View,
) -> (Panels, FrameHit) {
    let p = picture_panels(doc, area, view);
    if area.width == 0 || area.height == 0 || doc.boxes.is_empty() {
        return (p, FrameHit::default());
    }
    let hit = draw_map(buf, &p, doc, view, &FrameHit::default());
    (p, hit)
}

/// 一条边画什么颜色:自己写的颜色 > 配置里的 `edge_color` > 分组色
fn edge_ink(doc: &Document, view: &View, e: usize) -> (u8, u8, u8) {
    doc.edges[e]
        .color
        .or(view.edge_ink)
        .unwrap_or_else(|| doc.edge_color(e))
}

/// 一段文字画什么颜色:自己写的颜色 > 配置里的 `text_color` > 框/边色(None = 用默认亮色)
fn text_ink(doc: &Document, view: &View, i: usize) -> Option<(u8, u8, u8)> {
    doc.text(i)
        .and_then(|t| t.color)
        .or(view.text_ink)
        .or_else(|| doc.text_color(i))
}

/// 同一段上第几条边的偏移:0, -1, +1, -2, +2 ...
fn lane_of(k: usize) -> i32 {
    if k == 0 {
        return 0;
    }
    let n = (k as i32 + 1) / 2;
    if k % 2 == 1 {
        -n
    } else {
        n
    }
}

/// 画面本身:地图 + 图例 + 状态行
fn render_frame(
    buf: &mut Buffer,
    area: Rect,
    doc: &Document,
    view: &View,
    prev_hit: &FrameHit,
) -> (Panels, FrameHit) {
    let p = panels(doc, area, view);
    if view.help {
        render_help(buf, area, help_keys(view.lang), view.lang);
        return (p, FrameHit::default());
    }
    if area.width == 0 || area.height == 0 || doc.boxes.is_empty() {
        return (p, FrameHit::default());
    }
    let hit = draw_map(buf, &p, doc, view, prev_hit);
    // 连边预览画在地图上,再用连续的灰边标出选区
    draw_link_preview(buf, &p, doc, view);
    draw_sel_box(buf, &p, view);
    // 4. 图例:文档自带的条目,或者退回去的"分组 + 字形说明"
    // 5. 状态行(搜索中就是输入行)
    // 底部:够高就两行(状态 + 命令/提示),矮了退回一行
    let rows = status_rows(area);
    let bottom = area.y + area.height - 1;
    let top = bottom + 1 - rows;
    // 状态行(以及上下文)先算好:输入期间上行也要一直显示文件
    let pick = match view.pick {
        Pick::None => match doc.box_at_cell(view.cursor) {
            Some(i) => Pick::Box(i),
            None => Pick::None,
        },
        p => p,
    };
    let mut left = match pick {
        Pick::Box(c) => label_of(doc, c).to_string(),
        Pick::Edge(e) => {
            let (a, b) = doc
                .edge_ends(e)
                .map(|(a, b)| (label_of(doc, a).to_string(), label_of(doc, b).to_string()))
                .unwrap_or_default();
            view.lang
                .pick_owned(format!("边 {a} → {b}"), format!("edge {a} → {b}"))
                .to_string()
        }
        Pick::None => view.lang.pick("空选区", "nothing").to_string(),
    };
    // 选区与图层是全局状态:左边补选中数与确认状态,末尾补层名
    if let Some(sel) = view.sel {
        let (boxes, edges) = selected_all(doc, &p, view, &hit);
        let state = if sel.confirmed {
            view.lang.pick("已确认", "confirmed")
        } else {
            view.lang.pick("Enter 确认", "Enter to confirm")
        };
        left = format!(
            "{}   {left}",
            view.lang.pick_owned(
                format!("选区 {} 框 {} 边 · {state}", boxes.len(), edges.len()),
                format!("{} box(es) {} edge(s) · {state}", boxes.len(), edges.len()),
            )
        );
    }
    if let Some(layer) = &view.layer {
        left = format!("{left}   {}{layer}", view.lang.pick("图层 ", "layer "));
    }
    // 隔离开着时常驻一行:不然后面看到的是"图怎么不见了"
    if let Some(n) = view.ego.filter(|n| *n < doc.boxes.len()) {
        left = format!(
            "{left}   {}{}",
            view.lang.pick("隔离 ", "isolate "),
            label_of(doc, n)
        );
    }
    // 样式菜单:改几个元素 + 当前属性/值
    if let Some(menu) = &view.menu {
        let (boxes, edges) = if view.sel.is_some() {
            selected_all(doc, &p, view, &hit)
        } else {
            (Vec::new(), Vec::new())
        };
        let (n, zh, en) = match view.pick {
            Pick::Box(_) => (boxes.len().max(1), "个框", "box(es)"),
            Pick::Edge(_) => (edges.len().max(1), "条边", "edge(s)"),
            Pick::None => (0, "", ""),
        };
        if n > 0 {
            let cur = menu
                .level()
                .get(menu.sel)
                .map(|i| i.title.clone())
                .unwrap_or_default();
            let parent = if menu.levels.len() >= 2 {
                menu.levels[menu.levels.len() - 2]
                    .get(
                        menu.sel
                            .min(menu.levels[menu.levels.len() - 2].len().saturating_sub(1)),
                    )
                    .map(|i| i.title.clone())
            } else {
                None
            };
            let now = match (menu.title.as_deref(), parent) {
                (Some(t), _) if !cur.is_empty() => format!("{t} {cur}"),
                (Some(t), _) => t.to_string(),
                (None, Some(p)) if !cur.is_empty() => format!("{p} › {cur}"),
                _ => cur,
            };
            left = format!(
                "{}   {left}",
                view.lang
                    .pick_owned(format!("改 {n} {zh} · {now}"), format!("{n} {en} · {now}"))
            );
        }
    }
    // 连边模式:源数 / 目标数 / 将建数量,确认态再提示一次 Enter
    if let Some(link) = &view.link {
        let targets = link_targets(doc, &p, view);
        let total = link.sources.len() * targets.len();
        let text = if link.confirming {
            view.lang.pick_owned(
                format!("再按 Enter 创建 {total} 条"),
                format!("press Enter again to create {total} edges"),
            )
        } else {
            view.lang.pick_owned(
                format!(
                    "连边:源 {} -> 目标 {},将建 {} 条,Enter 确认,Esc 取消",
                    link.sources.len(),
                    targets.len(),
                    total
                ),
                format!(
                    "link: {} source(s) -> {} target(s), {} edge(s), Enter ok, Esc cancel",
                    link.sources.len(),
                    targets.len(),
                    total
                ),
            )
        };
        left = format!("{}   {left}", text);
    }

    if rows > 1 {
        // 上行常驻:文件 + 模式 + 上下文,cwd 与坐标靠右
        let head = status_left(view);
        let top_line = format!("{head}   {left}");
        draw_status_row(buf, area, view, &top_line, top);
    }
    if let Some(q) = &view.query {
        let prompt = format!("/{q}");
        put(
            buf,
            area.x,
            bottom,
            &prompt,
            Style::default().fg(Color::Rgb(238, 226, 104)),
        );
        let x = area.x + width(&prompt) as u16;
        let what = if q.trim().is_empty() {
            view.lang
                .pick(
                    "搜文字:节点文字 / 别名(比如 wlgc)/ 区域标题;命中的框高亮,或填分组号",
                    "search text: node labels / aliases (e.g. wlgc) / region titles; matches are highlighted, or type a group number",
                )
                .to_string()
        } else if q.trim().chars().all(|c| c.is_ascii_digit()) {
            let hit_groups = match doc.search(q.trim()) {
                Hits::Groups(hits) => hits,
                _ => Vec::new(),
            };
            if hit_groups.is_empty() {
                view.lang.pick("没有这个分组", "no such group").to_string()
            } else {
                let names = hit_groups
                    .iter()
                    .map(|gr| doc.groups[*gr].label.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let n = view.hits.len();
                view.lang.pick_owned(
                    format!("聚焦 {names}:{n} 个节点"),
                    format!("focus {names}: {n} nodes"),
                )
            }
        } else if view.hits.is_empty() {
            view.lang.pick("没找到", "no match").to_string()
        } else {
            let at = view.hit_at.min(view.hits.len() - 1);
            let cur = view.hits[at].min(doc.boxes.len() - 1);
            let more = if view.hits.len() > 1 {
                view.lang.pick(",n 下一个", ", n for next")
            } else {
                ""
            };
            let name = label_of(doc, cur);
            view.lang.pick_owned(
                format!("{} 处匹配:{}{more}", view.hits.len(), name),
                format!("{} match(es): {}{more}", view.hits.len(), name),
            )
        };
        put_plain(buf, x + 2, bottom, &what, dark_style());
        let tip = view
            .lang
            .pick("Enter 确定  Esc 取消", "Enter to confirm, Esc to cancel");
        let tw = width(tip) as u16;
        if area.width > tw + 4 {
            put_plain(buf, area.x + area.width - tw, bottom, tip, dark_style());
        }
        return (p, FrameHit::default());
    }
    // 5.3 底行单行输入(新建框 / 改文字 / 连边):和 `:` 一样独占整行
    if let Some((what, typed)) = &view.prompt {
        let prefix = match what {
            crate::app::Prompt::NewBox => view.lang.pick("框: ", "box: "),
            crate::app::Prompt::Text(_) => view.lang.pick("文字: ", "text: "),
            crate::app::Prompt::Link(_) => view.lang.pick("连到: ", "link: "),
        };
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, bottom)) {
                cell.reset();
            }
        }
        put_plain(buf, area.x, bottom, prefix, dark_style());
        let shown = prompt_display(typed, view.lang);
        put_plain(
            buf,
            area.x + width(prefix) as u16,
            bottom,
            &shown,
            bright_plain(),
        );
        return (p, FrameHit::default());
    }
    // 5.4 ":" 命令行独占整行:从最左边起,盖住模式标记与节点名,免得输入时还要往右找
    if let Some(c) = &view.cmd {
        let line = format!(":{c}");
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, bottom)) {
                cell.reset();
            }
        }
        // 命令行原样输出:路径里的 `/` 不能被当成斜体标记
        put_plain(buf, area.x, bottom, &line, bright_plain());
        // 右侧补一句"会写到哪 / 从哪里读";放不下就左侧截断,保住文件名,
        // 也别压到输入的内容上
        if let Some(hint) = &view.cmd_hint {
            let used = raw_width(&line) as u16;
            let room = area.width.saturating_sub(used + 1);
            if let Some(h) = fit_tail(hint, room as usize) {
                let x = area.x + area.width - raw_width(&h) as u16;
                put_plain(buf, x, bottom, &h, dim_style());
            }
        }
        return (p, FrameHit::default());
    }
    // 5.5 状态行:先标出 VIEW / VIEW x{zoom},空闲时报当前 box 的文字,
    // 计数、布局、聚焦、命中都不在这儿
    // pick 由调用方派生;没派生(比如直接 render 的测试)就按格子现算一次
    // 两行底部:上行画好了,下行放提示/错误;一行的时候仍然把提示挤在同一行右边
    if rows > 1 {
        if let Some(hint) = hint_text(view) {
            let room = if view.showcmd.is_empty() {
                area.width as usize
            } else {
                area.width.saturating_sub(width(&view.showcmd) as u16 + 2) as usize
            };
            put_n(buf, area.x, bottom, &hint, dark_style(), room);
        }
        if !view.showcmd.is_empty() {
            // 右下角:还没走完的按键序列(像 nvim 的 showcmd)
            let w = width(&view.showcmd) as u16;
            if area.width > w {
                put_plain(
                    buf,
                    area.x + area.width - w,
                    bottom,
                    &view.showcmd,
                    bright_plain(),
                );
            }
        }
        return (p, hit);
    }
    // 一行的时候:文件(有改动加 *) + 模式/缩放 + 上下文,提示挤在右边
    // (showcmd 最后画在最右边,和两行布局一致)
    left = format!("{}   {left}", status_left_tabbed(view));
    // 回显位放右边:":" 命令行 > 命令回显 / 提示;空闲时什么都不放.
    // 它优先级最高,左边那块放不下就截断
    if let Some(hint) = hint_text(view) {
        let right = area.x + area.width;
        let hw = width(&hint) as u16;
        if right > hw + 2 && area.width as usize > hw as usize + 8 {
            let max_left = right.saturating_sub(hw + 2).saturating_sub(area.x) as usize;
            put_n(buf, area.x, bottom, &left, bright_plain(), max_left);
            put_plain(buf, right - hw, bottom, &hint, dark_style());
        } else {
            // 右边放不下就接在左边那块后面,截到右边界
            put_n(
                buf,
                area.x,
                bottom,
                &left,
                bright_plain(),
                area.width as usize,
            );
            let start = area.x + width(&left) as u16 + 2;
            let room = right.saturating_sub(start) as usize;
            if room >= 8 {
                put_n(buf, start, bottom, &hint, dark_style(), room);
            }
        }
    } else {
        put(buf, area.x, bottom, &left, bright_plain());
    }
    if !view.showcmd.is_empty() {
        // 右下角:还没走完的按键序列(单行布局也盖在最右边)
        let w = width(&view.showcmd) as u16;
        if area.width > w {
            put_plain(
                buf,
                area.x + area.width - w,
                bottom,
                &view.showcmd,
                bright_plain(),
            );
        }
    }
    (p, hit)
}

/// 当前模式:两行底部的状态行要用
fn mode_state(view: &View) -> &'static str {
    if view.query.is_some() {
        "SEARCH"
    } else if view.cmd.is_some() {
        "COMMAND"
    } else if view.link.is_some() {
        "LINK"
    } else if view.sel.is_some() {
        "SELECT"
    } else {
        "NORMAL"
    }
}

/// 缩放写法:`100%` / `250%`,低倍率再缀 LOD
fn zoom_tag(view: &View) -> String {
    let zoom = if view.zoom.is_finite() && view.zoom > 0.0 {
        view.zoom
    } else {
        1.0
    };
    let pct = (zoom * 100.0).round() as i64;
    let lod = if !view.lod || lod_of(zoom) == Lod::Full {
        String::new()
    } else {
        " LOD".to_string()
    };
    format!("{pct}%{lod}")
}

/// 常驻状态行的左半边:`[2/5] ~/proj/x.gtui *   NORMAL   100%`
pub fn status_left(view: &View) -> String {
    let file = match &view.file_display {
        Some(p) => format!("{}{}", p, if view.tab_dirty { "*" } else { "" }),
        None => view.lang.pick("[空白]", "[scratch]").to_string(),
    };
    let tab = match view.tab {
        Some((at, total)) => format!("[{at}/{total}] "),
        None => String::new(),
    };
    format!("{tab}{file}   {}   {}", mode_state(view), zoom_tag(view))
}

/// 文件 + 脏标记 + 模式/缩放(单行底部用;两行时由 status_left 拼)
fn status_left_tabbed(view: &View) -> String {
    status_left(view)
}

/// 常驻状态行的右半边:`~/projects/graph-tui | 1,4`
pub fn status_right(view: &View) -> String {
    let coords = format!("{},{}", view.cursor.0, view.cursor.1);
    if view.cwd.is_empty() {
        coords
    } else {
        format!("{} | {coords}", view.cwd)
    }
}

/// 画常驻状态行:左边是文件 + 模式 + 上下文(左截断,保住文件名),
/// 右边工作目录与坐标;窄了先丢 cwd,只留坐标
fn draw_status_row(buf: &mut Buffer, area: Rect, view: &View, left: &str, y: u16) {
    let right = area.x + area.width;
    let coords = format!("{},{}", view.cursor.0, view.cursor.1);
    let cw = width(&coords) as u16;
    let full = status_right(view);
    let fw = width(&full) as u16;
    // 左边先扣掉坐标的位置,文件名从左边截断(留得住文件名)
    let room_left = area.width.saturating_sub(cw + 2);
    let shown = fit_tail(left, room_left as usize).unwrap_or_else(|| left.to_string());
    put(buf, area.x, y, &shown, bright_plain());
    let used = width(&shown) as u16;
    if used + fw + 2 <= area.width {
        put_plain(buf, right - fw, y, &full, dark_style());
    } else if used + cw + 2 <= area.width {
        put_plain(buf, right - cw, y, &coords, dark_style());
    }
}

/// 状态行最前面的模式标记:能一眼看出是 1:1 编辑,还是缩放后的查看
/// 低倍率下画到什么程度:文字在缩得太小时只会变成一堆碎片
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lod {
    /// 正常:文字、标注、注释都画
    Full,
    /// 只画形状:框、点、边,不画任何文字
    Shape,
    /// 连框也只画成一个实心块
    Blocks,
}

/// 倍率低于 0.6 就不画文字,低于 0.3 只画块
pub fn lod_of(zoom: f64) -> Lod {
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    if zoom >= 0.6 {
        Lod::Full
    } else if zoom >= 0.3 {
        Lod::Shape
    } else {
        Lod::Blocks
    }
}

/// 缩放倍数的显示写法:整数不带小数点,其余留两位再去掉多余的零
/// 回显位的内容:命令回显 / 提示(命令行自己独占一行,不走这里).空闲时没有
/// 源文件被外部改过时的那句提示(带两个出路)
pub fn stale_note(view: &View, gone: bool) -> String {
    if gone {
        return view.lang.pick_owned(
            "源文件不见了:R 重新载入会失败,:w 会重建".to_string(),
            "the source file is gone: R will fail, :w recreates it".to_string(),
        );
    }
    view.lang.pick_owned(
        "文件已在磁盘上改变:R 重新载入(丢弃改动),:w 覆盖".to_string(),
        "file changed on disk: R reloads (drops edits), :w overwrites".to_string(),
    )
}

fn hint_text(view: &View) -> Option<String> {
    // 回显优先;其次是"磁盘变了"的出路;再其次才是"有几个名字没画下"
    view.note
        .clone()
        .or_else(|| view.stale.then(|| stale_note(view, false)))
        .or_else(|| {
            (view.labels_hidden > 0).then(|| {
                view.lang.pick_owned(
                    format!("{} 个名字放不下(缩放后重排)", view.labels_hidden),
                    format!(
                        "{} label(s) did not fit (zoom re-places)",
                        view.labels_hidden
                    ),
                )
            })
        })
}

/// 屏幕格上的 box(点哪儿跳哪儿):按框在屏幕上的矩形命中.
/// 画面之外(状态行、图例)不算
pub fn node_at(doc: &Document, p: &Panels, area: Rect, col: u16, row: u16) -> Option<usize> {
    let (c, r) = (col as i32, row as i32);
    let inside = |a: Rect| {
        c >= a.x as i32
            && c < (a.x + a.width) as i32
            && r >= a.y as i32
            && r < (a.y + a.height) as i32
    };
    if !inside(area) || !inside(p.map) {
        return None;
    }
    (0..doc.boxes.len()).find(|&i| {
        let (x, y, w, h) = rect_of(doc, i, &p.proj);
        c >= x && c < x + w && r >= y && r < y + h
    })
}

/// 键位速查:中文
const HELP_KEYS_ZH: &[(&str, &str)] = &[
    ("h j k l / 方向键", "光标移动一格(5l = 走五格);光标下面是什么就选什么"),
    ("Ctrl-d / Ctrl-u", "半屏往下 / 往上跳一个节点(远距离旅行)"),
    ("Ctrl-o / Ctrl-i", "回到上一个 / 下一个跳过的节点"),
    ("n / N", "下一个 / 上一个搜索命中"),
    ("/", "搜节点名、别名、首字母;纯数字 = 聚焦那个分组"),
    ("L", "切换图层(只看/只动当前层)"),
    ("v", "区块选择:锚点落在光标格,挪光标扩大;Enter 确认后命令作用于整块"),
    ("Ctrl-hjkl", "挪动选中的节点(可带数字前缀,如 10 + Ctrl-l;光标跟着走)"),
    ("f", "把聚焦的分组(没聚焦就是当前节点的分组)铺满画面"),
    ("F", "铺满整幅图"),
    ("空格", "把当前节点挪到画面正中"),
    ("+ - / 0", "放大 / 缩小 / 回到 1:1"),
    ("B", "开关节点名标注"),
    ("w / b", "朝前 / 朝后射一条线,跳到第一个撞到的框或边"),
    (":rotate", "转方向:不带参数 +90 度,`:rotate 180` 绝对值,`:rotate auto` 交给布局按窗口选"),
    ("L", "循环图层"),
    ("i", "在框里直接改字(空处新建框;光标在边上建注释框;区块选择时只提示)"),
    ("S / M", "S:在光标行下面拆框;M:把选中的框并成一个"),
    ("y / d / p", "复制 / 剪切 / 粘贴图片段(框 + 文字 + 内部边;边界边不进剪贴板)"),
    (":notes", "注释开关:关掉后注释框不画、不算障碍、也点不到(边会重新布线)"),
    (":notes", "注释开关 on / off(和 a 一样);`:relabel [all]` 把注释框重新贴回边旁边"),
    ("i(选中边)", "建/改这条边的注释框(空框 Esc 或空回车就撤掉)"),
    ("s / c", "样式菜单(框型 point/none/single/… 、颜色、对齐)/ 颜色菜单(框=边框色,边=边颜色)"),
    ("d / x", "带边剪切 / 只剪框和文字(关联边都从文档里删掉,但只有 d 把边放进剪贴板)"),
    ("p", "把剪贴板里上次删掉的内容粘到光标处"),
    ("t", "改光标框的文字(底行提示)"),
    ("u / Ctrl-r", "撤销 / 重做"),
    ("右键", "同一个样式菜单:划过即预览,一次左键应用;键盘 j k 选,l/h 进出一层,Enter 应用"),
    ("e", "两段式连边:源(选区或光标框)-> 移动/选目标 -> Enter 建边;Esc 取消"),
    ("E", "隔离:只留光标节点、邻居与相连的边,别的元素直接消失(换节点自动跟着走;E 或 Esc 退出)"),
    ("Ctrl-e", "选边:在光标节点的相邻边之间轮换"),
    ("r", "循环切换布局"),
    ("鼠标左键", "点一下就落到那一格(重合的边连点会轮,点选区外面取消);空白处双击=新建节点;在节点上拖=挪框,空处拖=拉选区(松开即选完);Ctrl+左键/中键拖平移;滚轮缩放"),
    ("鼠标右键", "单击开菜单;空白处(或线上)拖动=平移;从框上拖动=从它连边,拖的时候一直有虚线预览跟着指针"),
    (":", "命令行:layout <名字> 换布局,cycle <倍数> 同组档位允许远多少,side <倍数> 横向偏斜算几倍距离,lang <zh|en> 界面语言"),
    (":", "建图:new <文字> 新建框、link <目标> 连边、del 删框/删选中的边"),
    (":", "编辑:box <point|none|single|rounded|double|heavy>、nodecolor <#rrggbb|auto>、fill <#rrggbb|none>、textcolor <#rrggbb|auto>、textbg <#rrggbb|none>、textalign <left|center|right>、edgebg <#rrggbb|none>、dir <none|forward|back|both>、corner <rounded|sharp>、via <x,y x,y|off>、text <余下整行>、save [路径]"),
    ("标签", "**粗体** 与 *斜体*"),
    ("?", "键位速查(任意键关掉)"),
    (":q / :wq", "退出 / 存盘后退出(q / esc 不退);命令行里 Tab / Shift-Tab 补全动词与路径"),
    (
        "R / :e!",
        "文件在磁盘上被改过时重新载入(有未保存改动要按两次 R);:w 覆盖磁盘上的版本",
    ),
];

/// 键位速查:英文
const HELP_KEYS_EN: &[(&str, &str)] = &[
    ("h j k l / arrows", "move the cursor one cell (5l = five cells); what it sits on is selected"),
    ("Ctrl-d / Ctrl-u", "jump half a screen down / up (long trips)"),
    ("Ctrl-o / Ctrl-i", "back to the previous / next node you jumped to"),
    ("n / N", "next / previous search hit"),
    ("/", "search name / alias / initials; a bare number focuses that group"),
    ("L", "cycle the layer (view and edit one layer at a time)"),
    ("v", "block select from the cursor cell; Enter confirms, then commands apply to the whole block"),
    ("Ctrl-hjkl", "nudge the selected boxes (count prefix works: 10 then Ctrl-l)"),
    ("f", "fit the focused group (or this node's group) to the screen"),
    ("F", "fit the whole document to the screen"),
    ("space", "centre this node"),
    ("+ - / 0", "zoom in / out / back to 1:1"),
    ("B", "toggle node labels"),
    ("w / b", "cast a ray forward / back and jump to the first box or edge it hits"),
    (":rotate", "rotate: bare +90deg, `:rotate 180` absolute, `:rotate auto` lets the layout pick"),
    ("Ctrl+G", "hand the prompt line to $VISUAL / $EDITOR (read back on exit; not for : or /)"),
    ("i", "edit the cursor box's text in place (empty space adds a box; on an edge it adds a note; a block selection only warns)"),
    ("S / M", "S: split the box below the cursor line; M: merge the selected boxes"),

    (":notes", "notes on / off; `:relabel [all]` puts note boxes back next to their edge"),
    ("y / d / p", "yank / cut / paste a picture fragment (boxes + texts + internal edges; boundary edges stay out of the clipboard)"),
    ("e", "two-step link: source (selection or cursor box) -> move/select target -> Enter; Esc cancels"),
    ("E", "isolate: keep only the cursor node, its neighbours and the edges between them; the rest is gone (follows the cursor; E/Esc exits)"),
    ("Ctrl-e", "pick an edge at the cursor node (cycles through its edges)"),
    ("r", "cycle the layout"),
    ("mouse left", "click to jump/select; drag on empty to block-select; drag a box to move it; Ctrl+drag or middle-drag pans; wheel zooms"),
    ("mouse right", "click for the menu; drag on empty (or an edge) to pan; drag from a box to link it — a dashed preview follows the pointer until you drop"),
    (":", "command line: layout <name> | side <x> | cycle <x> | lang <zh|en>"),
    (":", "build: new <text> | link <target> | del (box, or selected edge)"),
    (":", "edit: box <style> | nodecolor <#rrggbb|auto> | fill <#rrggbb|none> | textcolor <#rrggbb|auto> | textalign <left|center|right> | dir <none|forward|back|both> | corner <rounded|sharp> | via <x,y x,y|off> | text <rest of line> | save [path]"),
    ("labels", "**bold** and *italic*"),
    ("?", "key reference (any key closes it)"),
    (":q", "quit (only way; q / esc do not quit); Tab / Shift-Tab complete verbs and paths"),
    (
        "R / :e!",
        "reload the source when it changed on disk (twice with unsaved edits); :w overwrites",
    ),
];

/// 当前语言的键位表
pub fn help_keys(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::Zh => HELP_KEYS_ZH,
        Lang::En => HELP_KEYS_EN,
    }
}

/// 键位速查浮层
pub fn render_help(buf: &mut Buffer, area: Rect, lines: &[(&str, &str)], lang: Lang) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let title = lang.pick("键位速查", "key reference");
    let w = lines
        .iter()
        .map(|(k, v)| width(k) + width(v) + 4)
        .chain([width(title) + 6])
        .max()
        .unwrap_or(30)
        .min(area.width as usize) as u16;
    let h = (lines.len() as u16 + 4).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    // 铺底:把这块刷成面板色,免得和底下的图糊在一起
    for yy in y..(y + h).min(area.y + area.height) {
        // 左右各多刷一列:底下地图里若有全角字跨在面板边上,只清一半会留下残字
        let left = x.saturating_sub(1).max(area.x);
        let right = (x + w + 1).min(area.x + area.width);
        for xx in left..right {
            buf[(xx, yy)].set_char(' ').set_style(panel_bg());
        }
    }
    let bright = bright_style().patch(panel_bg());
    let dim = Style::default()
        .fg(Color::Rgb(170, 178, 200))
        .bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2));
    panel_frame(buf, x, y, w, h);
    put(buf, x + 2, y + 1, title, bright);
    for (i, (k, v)) in lines.iter().enumerate() {
        let yy = y + 2 + i as u16;
        if yy >= area.y + area.height {
            break;
        }
        put(buf, x + 2, yy, k, bright);
        put(buf, x + 2 + width(k).max(6) as u16 + 2, yy, v, dim);
    }
    put(
        buf,
        x + 2,
        y + h.saturating_sub(2),
        lang.pick("任意键关掉", "any key closes this"),
        dim,
    );
}

/// 打开文件的选择浮层:标题、过滤词、候选列表(当前项高亮)、按键提示
pub fn render_pick(buf: &mut Buffer, area: Rect, view: &View) {
    let Some(pick) = &view.file_pick else { return };
    if area.width < 24 || area.height < 6 {
        return;
    }
    let title = view.lang.pick("打开文件", "open a file");
    let hint = view.lang.pick(
        "输入过滤,↑↓ 选,回车打开,esc 取消",
        "type to filter, up/down, Enter to open, Esc to cancel",
    );
    let shown = pick.shown();
    let rows = shown
        .len()
        .min((area.height as usize).saturating_sub(4))
        .min(12);
    // 让当前项尽量留在可见范围里
    let first = (pick.at + 1).saturating_sub(rows.max(1));
    let cursor = pick.at.min(shown.len().saturating_sub(1));
    let texts: Vec<String> = (first..(first + rows).min(shown.len()))
        .map(|i| {
            let mark = if i == cursor { ">" } else { " " };
            format!("{mark} {}", pick.items[shown[i]])
        })
        .collect();
    let filter_line = format!("/{}", pick.filter);
    let w = texts
        .iter()
        .map(|t| width(t))
        .chain([width(title), width(hint), width(&filter_line)])
        .max()
        .unwrap_or(20)
        .saturating_add(4)
        .min(area.width as usize) as u16;
    let h = (texts.len() as u16 + 5).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    for yy in y..(y + h).min(area.y + area.height) {
        // 左右各多刷一列:底下地图里若有全角字跨在面板边上,只清一半会留下残字
        let left = x.saturating_sub(1).max(area.x);
        let right = (x + w + 1).min(area.x + area.width);
        for xx in left..right {
            buf[(xx, yy)].set_char(' ').set_style(panel_bg());
        }
    }
    let bright = bright_style().patch(panel_bg());
    let dim = Style::default()
        .fg(Color::Rgb(170, 178, 200))
        .bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2));
    panel_frame(buf, x, y, w, h);
    put(buf, x + 2, y + 1, title, bright);
    put(buf, x + 2, y + 2, &filter_line, bright);
    for (i, t) in texts.iter().enumerate() {
        let yy = y + 3 + i as u16;
        if yy >= y + h.saturating_sub(1) || yy >= area.y + area.height {
            break;
        }
        let style = if (first + i) == cursor { bright } else { dim };
        put_n(buf, x + 2, yy, t, style, w.saturating_sub(4) as usize);
    }
    // 提示行落在底边框上面一行(边框占 y + h - 1)
    put(buf, x + 2, y + h - 2, hint, dim);
}

/// 把画布扫一遍落到 ratatui buffer 上
#[allow(clippy::too_many_arguments)] // 一层 buffer 要写的东西本来就多,拆结构体只会多一层取值
fn blit(
    buf: &mut Buffer,
    cv: &Canvas,
    bright: Style,
    cursor: Option<(i32, i32)>,
    cursor_node: Option<usize>,
    pulse: f64,
    drawn: bool,
    marked: &HashSet<(i32, i32)>,
    picked: Pick,
    // E 选中一条边时,另一头的邻居节点占的格子(一起高亮,能看出这条边通到哪)
    picked_nodes: &HashSet<(i32, i32)>,
    // E 模式的高亮范围(None = 没开)
    ego: Option<&Ego>,
    sources: &[usize],
    // 搜索命中的框,以及它们在屏幕上占的格子(点节点只有一格)
    hits: &[usize],
    hit_cells: &HashSet<(i32, i32)>,
) {
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.node[i] == 0 && cv.mask[i] == 0 && cv.deco[i].is_none() {
                continue;
            }
            // 区块选择才用反显;当前选中的元素用红色呼吸(不铺底色)

            let selected = marked.contains(&(x, y));
            // 节点框盖在连线上面;光标节点的边框用呼吸色
            if let Some(d) = cv.deco[i] {
                let picked_box =
                    picked.box_at() == Some(d.node as usize) || picked_nodes.contains(&(x, y));
                let mut style = Style::default().fg(Color::Rgb(d.fg.0, d.fg.1, d.fg.2));
                // 边框上嵌入的箭头也可能是共享的:颜色同样跟激活的 owner 走
                if matches!(
                    cv.border.get(&(x, y)),
                    Some(crate::canvas::BorderMark::Arrow(..))
                ) {
                    if let Some((_, ink)) = arrow_ink_for(cv, (x, y), picked.edge(), ego) {
                        style = style.fg(Color::Rgb(ink.0, ink.1, ink.2));
                    }
                }
                if let Some((r, gg, b)) = d.bg {
                    style = style.bg(Color::Rgb(r, gg, b));
                }
                let dimmed =
                    (cv.focus != 0 && d.groups & cv.focus == 0) || (cv.layer_filter && !cv.lit[i]);
                if dimmed && !selected && !picked_box {
                    // 压暗时连填充一起丢掉,保持终端底色
                    style = dim_style();
                }
                // 选中的框:边框在白与红之间呼吸(只换前景,填充留着)
                // 连边的源框:稳定的亮色边框(目标那边才呼吸)
                if sources.contains(&(d.node as usize)) && !picked_box && !selected {
                    let mut st = Style::default()
                        .fg(Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2))
                        .add_modifier(Modifier::BOLD);
                    if let Some((r, gg, b)) = d.bg {
                        st = st.bg(Color::Rgb(r, gg, b));
                    }
                    style = st;
                }
                if hits.contains(&(d.node as usize)) && !picked_box && !selected {
                    let mut st = Style::default()
                        .fg(Color::Rgb(SEARCH_HIT.0, SEARCH_HIT.1, SEARCH_HIT.2))
                        .add_modifier(Modifier::BOLD);
                    if let Some((r, gg, b)) = d.bg {
                        st = st.bg(Color::Rgb(r, gg, b));
                    }
                    style = st;
                }
                if picked_box || (cursor_node == Some(d.node as usize) && drawn) {
                    let mut st = Style::default()
                        .fg(pulse_color(BRIGHT, pulse))
                        .add_modifier(Modifier::BOLD);
                    if let Some((r, gg, b)) = d.bg {
                        st = st.bg(Color::Rgb(r, gg, b));
                    }
                    style = st;
                }
                if selected {
                    // 选中框的边框:红白呼吸(不铺底色)
                    let mut st = style
                        .fg(pulse_color(BRIGHT, pulse))
                        .add_modifier(Modifier::BOLD);
                    if let Some((r, gg, b)) = d.bg {
                        st = st.bg(Color::Rgb(r, gg, b));
                    }
                    style = st;
                }
                let cell = &mut buf[(x as u16, y as u16)];
                cell.set_char(d.ch);
                cell.set_style(style);
                continue;
            }
            let (r, gg, b) = cv.fg[i];
            // 非节点处:这一格的形状只属于"盖在上面"的那条边.
            // 别的边从这儿过就断一格,不会把方向位并成 ┼ / ├
            // 箭头最上层:字形换成箭头,颜色也跟着箭头自己那条边走.
            // 但抬起来的边(选中的那条)压过别人的箭头:否则选一条从这点出发的边,
            // 屏幕上会看到别人指进来的箭头,像是两端都有箭头
            // 箭头格:谁的箭头由 owners 决定(共享箭头时一格可能属于几条边).
            // 抬起来的边压过别人的箭头,但抬起来的边自己拥有的箭头不会被压掉
            let shape = cv.shape_edge[i];
            let owners = cv.arrow_owners.get(&(x, y));
            // 选中那条边的端口外侧格上,别人的箭头要藏掉:不然选一条前向边,
            // 起点会顶着别人的箭头,看着像 <->
            let foreign_at_pick = cv.arrow[i].is_some()
                && cv.picked_port_out.contains(&(x, y))
                && match picked {
                    Pick::Edge(e) => !owners.is_some_and(|o| o.contains(&e)),
                    _ => false,
                };
            let foreign_arrow = foreign_at_pick
                || (cv.arrow[i].is_some()
                    && shape >= 0
                    && owners.is_some_and(|o| o.iter().all(|&e| e as i32 != shape))
                    && cv.raised.contains(&(shape as usize))
                    && !owners.is_some_and(|o| o.iter().any(|&e| cv.raised.contains(&e))));
            // 选中的边:真正画出来的箭头格看 owners(共享箭头一格里可能好几条边),
            // 其余格看命中归属 edge_of
            let on_picked_edge = match picked {
                Pick::Edge(e) if cv.arrow[i].is_some() && !foreign_arrow => {
                    cv.arrow_owners.get(&(x, y)).is_some_and(|o| o.contains(&e))
                }
                Pick::Edge(e) => cv.edge_of[i] == e as i32 || cv.shape_edge[i] as usize == e,
                _ => false,
            };
            // 共享箭头按"当前激活的 owner"取色(选中 > E 唯一亮着 > 可见的那条);
            // overlay 会再画一遍 E 模式下的箭头,两边用同一个函数,颜色才一致
            let arrow_ink = if cv.arrow[i].is_some() && !foreign_arrow {
                arrow_ink_for(cv, (x, y), picked.edge(), ego)
                    .map(|(_, ink)| ink)
                    .or(cv.arrow_ink[i])
            } else {
                None
            };
            let mut ch = match cv.node[i] {
                2 => cv.glyphs.node(true),
                1 => cv.glyphs.node(false),
                _ => cv.glyphs.line(cv.mask[i], cv.corner[i]),
            };
            if cv.node[i] == 0 {
                if let Some(a) = cv.arrow[i].filter(|_| !foreign_arrow) {
                    // 有向边的箭头盖在笔画上(最上层)
                    ch = a;
                } else if (ch == '─' || ch == '│')
                    && cv.mask[i].count_ones() <= 2
                    && !cv.solid[i]
                    && !cv.style[i].keeps(x, y)
                {
                    // 虚线/点线只断纯直笔:拐角、三通、交叉一律实画,不然看不出往哪拐
                    ch = ' ';
                }
            }
            let mut style = Style::default().fg(Color::Rgb(r, gg, b));
            if let Some((ar, ag, ab)) = arrow_ink {
                style = style.fg(Color::Rgb(ar, ag, ab));
            }
            // 枢纽平时是亮白的,一眼能认出来;但如果它也在光标那条分组上,
            // 就用那一组的颜色,这样顺着颜色能把整条路径连起来看
            if cv.node[i] == 2 && !cv.mine[i] {
                style = bright;
            }
            // 聚焦某几个分组时,别的分组(节点也好笔画也好)全压暗,
            // 只留聚焦那几个分组的颜色.按图层看时,别层的元素同样压暗
            let dimmed = (if cv.focus == 0 {
                false
            } else if cv.node[i] != 0 {
                // 节点格:看这一个节点自己在不在聚焦的分组上(别的分组只是从这儿路过)
                cv.node_groups[i] & cv.focus == 0
            } else {
                cv.groups_here[i] & cv.focus == 0
            }) || (cv.layer_filter && !cv.lit[i]);
            if dimmed && !selected && !on_picked_edge {
                style = dim_style();
            } else if let Some((br, bg_, bb)) = cv.edge_bg[i] {
                // 边自己的底色:压在终端底色上,不铺到框里(框是 deco 分支)
                style = style.bg(Color::Rgb(br, bg_, bb));
            }
            // 没有框的元素:字形与文字自己呼吸(实心态,不铺底色).
            // 只有"把光标画进画面"时才换实心字形(dump/导出不画光标)
            if drawn && cursor == Some((x, y)) {
                ch = match cv.node[i] {
                    2 => cv.glyphs.node_cursor(true),
                    1 => cv.glyphs.node_cursor(false),
                    _ => ch,
                };
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            } else if cv.node[i] != 0 && hit_cells.contains(&(x, y)) {
                // 搜索命中的点节点:字形换成搜索黄
                style = style
                    .fg(Color::Rgb(SEARCH_HIT.0, SEARCH_HIT.1, SEARCH_HIT.2))
                    .add_modifier(Modifier::BOLD);
            } else if picked_nodes.contains(&(x, y)) && cv.node[i] != 0 {
                // 邻居点节点:字形跟着呼吸(不换实心,光标那个才换)
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            } else if on_picked_edge {
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            }
            // 区块选区用同一套红白呼吸,不铺底色、不反显
            if selected {
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            }
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}

/// 相邻两个 box 在屏幕上大概隔几格(用来决定要不要自动标节点名)
fn median_edge_cells(doc: &Document, proj: &Proj) -> f64 {
    if doc.edges.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f64> = doc
        .edges
        .iter()
        .enumerate()
        .filter_map(|(k, _)| doc.edge_ends(k))
        .map(|(a, b)| {
            let a = proj.cell(doc.center(a));
            let b = proj.cell(doc.center(b));
            (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64).sqrt()
        })
        .collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// 给浮层画一圈边框:浮层只填了空格,没有框就跟底下的图糊在一起
fn panel_frame(buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    if w < 2 || h < 2 {
        return;
    }
    let style = Style::default()
        .fg(Color::Rgb(120, 132, 168))
        .bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2));
    let (x1, y1) = (x + w - 1, y + h - 1);
    for xx in x..=x1 {
        put(buf, xx, y, "─", style);
        put(buf, xx, y1, "─", style);
    }
    for yy in y..=y1 {
        put(buf, x, yy, "│", style);
        put(buf, x1, yy, "│", style);
    }
    for (cx, cy, ch) in [(x, y, "╭"), (x1, y, "╮"), (x, y1, "╰"), (x1, y1, "╯")] {
        put(buf, cx, cy, ch, style);
    }
}

/// 写一段文字,超出画面就丢掉(bounds 一律挡住,别让浮层把自己写出界)
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    put_n(buf, x, y, text, style, max);
}

/// 原样输出,不吃 `**粗**` / `/斜/` 这类标记:界面文案(命令行、路径、回显)都用它
/// 从左边截到 `room` 列以内,保留尾部(文件名在最后).
/// 截过就加 `...` 前缀;连 `...` 加一列都放不下就不画
fn fit_tail(text: &str, room: usize) -> Option<String> {
    if room == 0 {
        return None;
    }
    if raw_width(text) <= room {
        return Some(text.to_string());
    }
    if room <= 3 {
        return None;
    }
    let body = room - 3;
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut w = 0;
    for c in chars.into_iter().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > body {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.reverse();
    Some(format!("...{}", out.into_iter().collect::<String>()))
}

fn put_plain(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    let spans = [text::Span {
        link: None,
        text: text.to_string(),
        bold: false,
        italic: false,
    }];
    text::put(buf, x, y, &spans, style, max);
}

/// 同上,但最多占 max_w 列(标签里的 `**粗**` / `/斜/` 标记不占位)
fn put_n(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, max_w: usize) {
    if x >= buf.area.width || y >= buf.area.height || max_w == 0 {
        return;
    }
    let max = max_w.min((buf.area.width - x) as usize);
    let spans = text::parse(text);
    text::put(buf, x, y, &spans, style, max);
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests;
