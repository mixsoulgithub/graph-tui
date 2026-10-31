//! 投影与上屏:文档里的单元格坐标 -> 屏幕格 -> ratatui buffer.
//! 框、文字、边都从 Document 取;缩放和平移只是视图状态,不改几何.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::View;
use crate::canvas::{arrive_hor, box_char, Canvas, EndMark, DOWN, LEFT, RIGHT, UP};
use crate::document::{Box, Document, Hits};
use crate::lang::Lang;
use crate::model::Align;
use crate::model::{BoxStyle, EdgeStyle};
use crate::text;

/// 每行显示宽度(全角算两格).标签里的 `**粗**` / `/斜/` 标记不占宽度
pub fn width(s: &str) -> usize {
    text::plain_width(s)
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

    /// 把投影的屏幕原点定到某一格(区域中心)
    fn anchored(self, x0: i32, y0: i32) -> Proj {
        Proj { x0, y0, ..self }
    }
}

/// 一块区域怎么分:地图区、图例区,以及这一帧的投影
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    pub map: Rect,
    pub legend: Rect,
    pub proj: Proj,
}

/// 画面高度只有 1 行时状态栏就没了,别把地图也吃掉
fn body_of(area: Rect) -> Rect {
    let status = 1.min(area.height);
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(status),
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

/// 这一帧的区域划分:地图区、图例区,以及按视图中心/缩放算出来的投影
pub fn panels(doc: &Document, area: Rect, view: &View) -> Panels {
    layout(doc, body_of(area), view)
}

/// 只要地图 + 图例(不要状态行)时的区域划分
fn picture_panels(doc: &Document, area: Rect, view: &View) -> Panels {
    layout(doc, area, view)
}

fn layout(doc: &Document, body: Rect, view: &View) -> Panels {
    let legend_w = legend_width(doc, view, body.width);
    let map = Rect {
        width: body.width.saturating_sub(legend_w),
        ..body
    };
    let legend = if legend_w > 2 {
        Rect::new(
            body.x + body.width - legend_w + 2,
            body.y,
            legend_w - 2,
            body.height,
        )
    } else {
        Rect::new(body.x + body.width, body.y, 0, 0)
    };
    let proj = proj_of(doc, view, view.zoom).anchored(
        map.x as i32 + map.width as i32 / 2,
        map.y as i32 + map.height as i32 / 2,
    );
    Panels { map, legend, proj }
}

/// 画布上的一个格子
pub type Cell = (i32, i32);

/// E 模式:只亮这个节点、它的邻居和相连的边,其余压暗(再按 E 或 Esc 退出)
#[derive(Default)]
struct Ego {
    /// 亮着的节点下标
    nodes: HashSet<usize>,
    /// 亮着的一切:这些节点的格子、这些边的笔画、相关注释的整块
    lit: HashSet<Cell>,
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
    if let Some(i) = doc.box_at_cell(canvas) {
        // 注释收起来时,注释框当它不存在(E 模式下临时露出来的除外)
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
        }
    }
}

/// 铺满整个区域需要的缩放倍数(一份单元格占几列),给 f / F 键用
pub fn zoom_for_fit(doc: &Document, w: u16, h: u16, legend: bool, notes: bool) -> f64 {
    // 和 panels 一样先把状态行与图例占的位置扣掉:
    // 否则"铺满整幅"铺的是整个窗口,而真正画图的区域比窗口窄,两边会被切掉
    let body = body_of(Rect::new(0, 0, w, h));
    let legend_w = legend_width(
        doc,
        &View {
            legend,
            notes,
            ..View::default()
        },
        body.width,
    );
    let (wd, hd) = doc_size(doc, notes);
    let cols = (body.width.saturating_sub(legend_w) as f64 - 4.0).max(8.0);
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

/// 图例一行用什么颜色
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    /// 标题与说明文字
    Dark,
    /// 枢纽那种亮白
    Bright,
    /// 直接给的颜色
    Rgb((u8, u8, u8)),
    /// 借分组色(聚焦时跟着压暗)
    Group(usize),
}

/// 图例的一行
#[derive(Clone, Debug)]
struct LegendRow {
    text: String,
    ink: Ink,
}

/// 图例要画哪些行.文档自带条目就用它;没有条目(v1/v2 老文件)就退回
/// "分组色块 + 三种字形说明"
fn legend_rows(doc: &Document, view: &View) -> Vec<LegendRow> {
    let mut rows: Vec<LegendRow> = Vec::new();
    if !doc.legend.entries.is_empty() {
        let title = doc
            .legend
            .title
            .clone()
            .unwrap_or_else(|| view.lang.pick("图例", "legend").to_string());
        rows.push(LegendRow {
            text: title,
            ink: Ink::Dark,
        });
        for e in &doc.legend.entries {
            let text = match e.glyph {
                Some(g) => format!("{} {}", g.sample(), e.text),
                None => e.text.clone(),
            };
            let ink = match (e.color, e.group.as_deref().and_then(|g| doc.group_idx(g))) {
                (Some(c), _) => Ink::Rgb(c),
                (None, Some(gr)) => Ink::Group(gr),
                (None, None) => Ink::Dark,
            };
            rows.push(LegendRow { text, ink });
        }
        return rows;
    }
    rows.push(LegendRow {
        text: view.lang.pick("分组", "groups").to_string(),
        ink: Ink::Dark,
    });
    let shown: Vec<usize> = (0..doc.groups.len())
        .filter(|&gr| !doc.groups[gr].label.trim().is_empty())
        .collect();
    for gr in shown {
        rows.push(LegendRow {
            text: format!("── {}", doc.groups[gr].label),
            ink: Ink::Group(gr),
        });
    }
    rows.push(LegendRow {
        text: String::new(),
        ink: Ink::Dark,
    });
    rows.push(LegendRow {
        text: view.lang.pick("○ 节点", "○ node").to_string(),
        ink: Ink::Dark,
    });
    rows.push(LegendRow {
        text: view.lang.pick("◎ 枢纽", "◎ hub").to_string(),
        ink: Ink::Bright,
    });
    rows.push(LegendRow {
        text: view.lang.pick("> 有向边", "> directed").to_string(),
        ink: Ink::Dark,
    });
    rows
}

/// 图例内容的宽高(关掉或没东西可画时是 0)
pub fn legend_extent(doc: &Document, view: &View) -> (u16, u16) {
    if !view.legend {
        return (0, 0);
    }
    let rows = legend_rows(doc, view);
    let w = rows.iter().map(|r| width(&r.text)).max().unwrap_or(0) as u16;
    (w, rows.len() as u16)
}

/// 图例占的宽度(放不下就是 0)
fn legend_width(doc: &Document, view: &View, area_w: u16) -> u16 {
    let (text, _) = legend_extent(doc, view);
    if text == 0 {
        return 0;
    }
    // 面板宽度 = 内容 + 与地图之间的 2 列 + 右边 2 列余量
    let need = text.saturating_add(8);
    // 地图至少留 20 列,否则整块收起
    if area_w < need + 20 {
        return 0;
    }
    need.min(area_w / 3).min(44)
}

/// 一行太长就截断并补 "..."
fn clip_row(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    if w <= 3 {
        return ".".repeat(w);
    }
    let mut out = String::new();
    for ch in s.chars() {
        if width(&out) + crate::text::plain_width(ch.encode_utf8(&mut [0u8; 4])) > w - 3 {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// 在 (x, y) 处往下画图例,超出 `w` 的截断.返回画了几行
pub fn draw_legend(buf: &mut Buffer, x: u16, y: u16, w: u16, doc: &Document, view: &View) -> u16 {
    if w == 0 {
        return 0;
    }
    let focus = focus_mask(view);
    let mut rows = 0u16;
    for row in legend_rows(doc, view) {
        let style = match row.ink {
            Ink::Dark => dark_style(),
            Ink::Bright => bright_style(),
            Ink::Rgb(c) => Style::default().fg(Color::Rgb(c.0, c.1, c.2)),
            Ink::Group(gr) => {
                if focus == 0 || (gr < 64 && focus & (1 << gr) != 0) {
                    let c = doc.groups[gr].color;
                    Style::default().fg(Color::Rgb(c.0, c.1, c.2))
                } else {
                    dim_style()
                }
            }
        };
        put(buf, x, y + rows, &clip_row(&row.text, w as usize), style);
        rows += 1;
    }
    rows
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
/// 连边预览线:暗蓝灰,一眼看出"还没建"
const PREVIEW: (u8, u8, u8) = (104, 118, 152);
/// 闪到最红的那一档
const BLINK_RED: (u8, u8, u8) = (255, 62, 56);
/// 普通节点名的灰
const LABEL: (u8, u8, u8) = (150, 158, 180);
/// 浮层与提示用的底色
const PANEL_BG: (u8, u8, u8) = (24, 28, 44);

/// 节点字形:枢纽是靶心,普通节点是空心圆
const GLYPH_HUB: char = '◎';
const GLYPH_NODE: char = '○';
/// 光标那一个画成实心的,一眼就能认出"我在这儿"
const GLYPH_HUB_CURSOR: char = '◉';
const GLYPH_NODE_CURSOR: char = '●';

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
    rect: (i32, i32, i32, i32),
) -> Option<EndMark> {
    if doc.boxes[i].style.is_point() {
        return None;
    }
    let d = (
        doc.center(toward).0 - doc.center(i).0,
        doc.center(toward).1 - doc.center(i).1,
    );
    let side = if d.0.abs() >= d.1.abs() {
        if d.0 > 0.0 {
            RIGHT
        } else {
            LEFT
        }
    } else if d.1 > 0.0 {
        DOWN
    } else {
        UP
    };
    Some(EndMark {
        rect,
        side,
        cell: Canvas::side_base(rect, side),
        // 没有边框就没地方画 T/箭头:标记落在区域外面那一格
        mark: doc.boxes[i].style.is_boxed(),
    })
}

/// 框在屏幕上的矩形:(左上角列, 左上角行, 宽, 高)
fn rect_of(doc: &Document, i: usize, proj: &Proj) -> (i32, i32, i32, i32) {
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
    let boxes: Vec<usize> = (0..doc.boxes.len())
        .filter(|&i| {
            let (left, top, w, h) = rect_of(doc, i, &p.proj);
            left <= x1 && left + w > x0 && top <= y1 && top + h > y0
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
    if targets.is_empty() || p.map.width == 0 || doc.groups.is_empty() {
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
    );
    cv.preview = true;
    for i in 0..doc.boxes.len() {
        let r = rect_of(doc, i, &p.proj);
        if doc.boxes[i].style.is_point() {
            if cv.idx(r.0, r.1).is_some() {
                cv.station_cells.insert((r.0, r.1));
            }
        } else {
            cv.reserve_box((r.0, r.1), (r.2 as usize, r.3 as usize));
        }
    }
    let mut n = 0;
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
            let a_end = end_mark(doc, src, dst, rect_of(doc, src, &p.proj));
            let b_end = end_mark(doc, dst, src, rect_of(doc, dst, &p.proj));
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
            let ch = box_char(cv.mask[i], cv.corner[i]);
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
        legend: Rect::new(area.x + area.width, area.y, 0, 0),
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
    // 注释开关:关掉时注释框当"不存在"(不画、不算障碍、不进命中/选区);
    // E 模式下和亮着的边相关的那几条临时露出来
    let ego_edges: Option<HashSet<usize>> = view
        .ego
        .filter(|n| *n < doc.boxes.len())
        .map(|n| doc.adj(n).iter().copied().collect());
    let note_visible = |i: usize| -> bool {
        if !doc.is_note(i) || view.notes {
            return true;
        }
        let Some(e) = doc.edge_of_box(i) else {
            return false;
        };
        ego_edges.as_ref().is_some_and(|s| s.contains(&e))
    };
    let mut cv = Canvas::new(
        (
            map.x as i32,
            map.y as i32,
            map.width as i32,
            map.height as i32,
        ),
        priority,
        focus,
    );
    cv.layer_filter = view.layer.is_some();
    cv.arrow_style = view.arrow_style;
    // 每个框在屏幕上的矩形,后面画边、命中、选择都用它
    let rects: Vec<(i32, i32, i32, i32)> = (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &proj))
        .collect();
    // 节点框占的格子先登记好:画边时要避开整个框
    for (i, r) in rects.iter().enumerate() {
        if !note_visible(i) {
            continue;
        }
        if doc.boxes[i].style.is_point() {
            if cv.idx(r.0, r.1).is_some() {
                cv.station_cells.insert((r.0, r.1));
            }
        } else {
            // 带框的与"只有文字"的都占整块:线别从字上穿过去
            cv.reserve_box((r.0, r.1), (r.2 as usize, r.3 as usize));
        }
    }
    let center_cell = |i: usize| proj.cell(doc.center(i));
    // 一条边在节点 i 这一端的落点:带框的落边框连接格,点节点落中心格
    let end_of =
        |i: usize, toward: usize| -> Option<EndMark> { end_mark(doc, i, toward, rects[i]) };
    // 1. 连边:按图层顺序一层层画(当前层最后),每层再按分组一条条走
    let layers = layer_order(doc, view);
    for layer in &layers {
        cv.layer_lit = lit_layer(view, layer);
        // 这一层里"同一对节点上的边":按 (小下标, 大下标) 归堆,画一段时直接查.
        // 以前是每画一段就扫一遍全部边,大图上这是 O(段数 x 边数)
        let mut pairs: std::collections::HashMap<(usize, usize), Vec<usize>> =
            std::collections::HashMap::new();
        for (i, e) in doc.edges.iter().enumerate() {
            if e.layer != *layer {
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
                let (a_end, b_end) = (end_of(u, v), end_of(v, u));
                let a_cell = a_end.map(|e| e.cell).unwrap_or_else(|| center_cell(u));
                let b_cell = b_end.map(|e| e.cell).unwrap_or_else(|| center_cell(v));
                // 下一个节点决定这一段该"竖着进"还是"横着进"
                let arrive = if k + 2 < path.len() {
                    let c = center_cell(path[k + 2]);
                    arrive_hor(a_cell, b_cell, c)
                } else {
                    None
                };
                // 这一段上的所有边:同一对节点可能连着好几条(a -> b、b -> a、同向重复).
                // 每条都自己画一次,端口被占就顺延到相邻边框格,于是两条线不会一条盖住另一条.
                // `pair` 不分分组(点节点之间靠它的序号分道),这一段只画本分组的那几条
                // 只用来判断这条边是从 u 还是从 v 出发(决定箭头方向和 via 的顺序)
                let uid = &doc.boxes[u].id;
                let pair = pairs
                    .get(&(u.min(v), u.max(v)))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if pair.is_empty() {
                    continue;
                }
                for (lane, e) in pair.iter().copied().enumerate() {
                    if doc.edges[e].group != *gid {
                        continue;
                    }
                    // 箭头只认这条边自己的方向:from 落在 u 就是 (u 端, v 端),否则对调
                    let (at_from, at_to) = doc.edges[e].dir.heads();
                    let heads = if &doc.edges[e].from == uid {
                        (at_from, at_to)
                    } else {
                        (at_to, at_from)
                    };
                    // 命中表按边下标记格子:画这一段之前先说是哪条边(以及它的底色)
                    cv.pen_edge = e;
                    cv.pen_bg = doc.edges[e].bg;
                    let style = doc.edges[e].style;
                    let fg = doc.edge_color(e);
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
                    cv.edge_full(a_cell, b_cell, *gr, fg, arrive, heads, style, a_end, b_end);
                }
            }
        }
        // 1b. 自环:它不属于任何"线路",自己从节点一侧出去、拐到相邻一侧再回来
        for (i, e) in doc.edges.iter().enumerate() {
            if e.layer != *layer {
                continue;
            }
            let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
                continue;
            };
            if a != b {
                continue;
            }
            let r = rects[a];
            let end = end_of(a, a).unwrap_or(EndMark {
                rect: r,
                side: RIGHT,
                cell: Canvas::side_base(r, RIGHT),
                mark: false,
            });
            let Some(g) = doc.group_idx(&e.group) else {
                continue;
            };
            cv.pen_edge = i;
            cv.pen_bg = e.bg;
            cv.pen_corner = e.corner;
            cv.edge_loop(end, e.dir.heads(), g, doc.edge_color(i), e.style);
        }
        // 2. 节点框与节点:同一个图层里,框盖在这一层的连线上面
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer || !b.style.is_boxed() || !note_visible(i) {
                continue;
            }
            let mask = doc
                .box_groups(i)
                .iter()
                .filter(|gr| **gr < 64)
                .fold(0u64, |m, gr| m | (1 << gr));
            let fg = doc.box_color(i).unwrap_or(BRIGHT);
            let r = rects[i];
            cv.box_at(
                i,
                (r.0, r.1),
                (r.2 as usize, r.3 as usize),
                doc.boxes[i].style,
                mask,
                fg,
                doc.boxes[i].fill,
            );
        }
        // 3. 点节点:一格也只有一个颜色,同样让光标那条分组先挑
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer || b.style.is_boxed() || !note_visible(i) {
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
            cv.node_at(r.0, r.1, doc.hub(i), mask, gr, fg);
        }
    }
    // 画出来的光标标记:就在光标格上
    let cursor_cell = proj.cell((view.cursor.0 as f64, view.cursor.1 as f64));
    let cursor_box = view.pick.box_at();
    let marked = selected_cells(doc, p, view, hit);
    // E 模式:只亮这个节点、它的邻居与相连的边
    let ego = view.ego.filter(|n| *n < doc.boxes.len()).map(|n| {
        let mut nodes: HashSet<usize> = HashSet::from([n]);
        let mut edges: HashSet<usize> = HashSet::new();
        for &e in doc.adj(n) {
            edges.insert(e);
            if let Some((a, b)) = doc.edge_ends(e) {
                nodes.insert(if a == n { b } else { a });
            }
        }
        let mut cells: HashSet<Cell> = HashSet::new();
        for &i in &nodes {
            let (l, t, w, h) = rects[i];
            for dx in 0..w.max(1) {
                for dy in 0..h.max(1) {
                    cells.insert((l + dx, t + dy));
                }
            }
        }
        let mut lit = cells.clone();
        for &e in &edges {
            if let Some(cs) = cv.edge_cells.get(&e) {
                lit.extend(cs.iter().copied());
            }
        }
        // 和高亮的边绑在一起的注释框整块点亮
        for (i, b) in doc.boxes.iter().enumerate() {
            let Some(eid) = b.edge.as_deref() else {
                continue;
            };
            let Some(ei) = doc.edges.iter().position(|x| x.id == eid) else {
                continue;
            };
            if !edges.contains(&ei) {
                continue;
            }
            let (l, t, w, h) = rects[i];
            for dx in 0..w.max(1) {
                for dy in 0..h.max(1) {
                    lit.insert((l + dx, t + dy));
                }
            }
        }
        Ego { nodes, lit }
    });
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
    );
    // 4. 框里的文字:框内区域起点是 (左+1, 中行),按 align 在可用宽度里偏移.
    //    文字节点(None)没有边框:区域就是整块,起始在左上角
    for (i, b) in doc.boxes.iter().enumerate() {
        if b.style.is_point() || !note_visible(i) {
            continue;
        }
        let Some(t) = doc.text(i) else { continue };
        let (left, top, w, h) = rects[i];
        let bare = matches!(b.style, BoxStyle::None);
        let max_w = if bare {
            w.max(1) as usize
        } else {
            (w - 2).max(1) as usize
        };
        let region_x = if bare { left } else { left + 1 };
        // 多行:第一行从框内左上角起,一行一行往下画
        let region_y = if bare { top } else { top + 1 };
        if (!bare && h < 3)
            || region_x < map.x as i32
            || region_y < map.y as i32
            || region_x + max_w as i32 > (map.x + map.width) as i32
            || region_y >= (map.y + map.height) as i32
        {
            continue; // 框被画面边缘裁掉时不往图例/状态行上写
        }
        let on_focus = focus == 0
            || doc
                .box_groups(i)
                .iter()
                .any(|gr| *gr < 64 && focus & (1 << gr) != 0);
        let (r, gg, b) = doc.text_color(i).unwrap_or(BRIGHT);
        let live = on_focus
            && lit_layer(view, &doc.boxes[i].layer)
            && ego.as_ref().is_none_or(|e| e.nodes.contains(&i));
        let mut style = if live {
            Style::default().fg(Color::Rgb(r, gg, b))
        } else {
            dim_style()
        };
        // 底色:文字自己的底色优先,其次框的填充(两者独立);压暗时不画
        if live {
            if let Some((r, gg, b)) = t.bg.or(doc.boxes[i].fill) {
                style = style.bg(Color::Rgb(r, gg, b));
            }
        }
        let bottom = map.y as i32 + map.height as i32;
        for (k, line) in t.text.split('\n').enumerate() {
            let ty = region_y + k as i32;
            if ty >= bottom {
                break; // 超出画面就别写了
            }
            let spans = text::parse(line);
            let tw = text::width_of(&spans).min(max_w);
            let off = match t.align {
                Align::Left => 0,
                Align::Center => max_w.saturating_sub(tw) / 2,
                Align::Right => max_w.saturating_sub(tw),
            };
            text::put(
                buf,
                (region_x + off as i32) as u16,
                ty as u16,
                &spans,
                style,
                max_w,
            );
        }
    }
    // 间距够宽(相邻节点拉开到 4 格以上)就自动把点节点的名字标出来
    let labels = view.labels || median_edge_cells(doc, &proj) >= 4.5;
    // 5. 标注(画在最上层,只写在空位上),三拨共用一张占用表:
    //    端点/枢纽的点节点名 -> 边标签 -> 其余点节点名.带框的节点文字已经在框里了
    let mut used: HashSet<(i32, i32)> = HashSet::new();
    let mut order: Vec<usize> = (0..doc.boxes.len()).collect();
    order.sort_by_key(|&i| {
        let terminus = doc.adj(i).len() == 1;
        !(doc.hub(i) || terminus)
    });
    let head_of = |i: usize| doc.hub(i) || doc.adj(i).len() == 1;
    for &i in &order {
        if !head_of(i) {
            continue;
        }
        draw_point_label(
            buf,
            &cv,
            map,
            &proj,
            doc,
            view,
            focus,
            &mut used,
            i,
            labels,
            ego.as_ref(),
        );
    }
    for &i in &order {
        if head_of(i) {
            continue;
        }
        draw_point_label(
            buf,
            &cv,
            map,
            &proj,
            doc,
            view,
            focus,
            &mut used,
            i,
            labels,
            ego.as_ref(),
        );
    }
    let mut hit = FrameHit::from(&cv);
    hit.rev = doc.rev();
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
    ego: Option<&Ego>,
) {
    // 只有点节点需要外挂名字:带框的在框里,文字节点本身就在画字
    if !doc.boxes[i].style.is_point() {
        return;
    }
    let terminus = doc.adj(i).len() == 1;
    if !labels && !terminus {
        return;
    }
    let (x, y) = proj.cell(doc.center(i));
    let label = label_of(doc, i);
    let w = width(label) as i32;
    if label.is_empty() || w == 0 {
        return;
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
    let pick = stored
        .filter(|&q| q != (x, y) && label_fits(cv, used, map, q.0, q.1, w))
        .or_else(|| {
            slots
                .iter()
                .copied()
                .find(|&(lx, ly)| label_fits(cv, used, map, lx, ly, w))
        });
    let Some((lx, ly)) = pick else { return };
    reserve_label(used, lx, ly, w);
    let on_focus = (focus == 0
        || doc
            .box_groups(i)
            .iter()
            .any(|gr| *gr < 64 && focus & (1 << gr) != 0))
        && ego.is_none_or(|e| e.nodes.contains(&i));
    let text_fg = doc.text(i).and_then(|t| t.color);
    let picked_here = view.pick.box_at() == Some(i);
    let style = if picked_here {
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
    put(buf, lx as u16, ly as u16, label, style);
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

/// 图例在面板里垂直居中
fn draw_legend_centered(buf: &mut Buffer, p: &Panels, doc: &Document, view: &View) {
    if p.legend.width == 0 {
        return;
    }
    let rows = legend_rows(doc, view).len() as u16;
    let ly = p.legend.y + p.legend.height.saturating_sub(rows) / 2;
    draw_legend(buf, p.legend.x, ly, p.legend.width, doc, view);
}

/// 只画图(地图 + 图例),不画状态行/覆盖层:`--dump` 与导出用
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
    draw_legend_centered(buf, &p, doc, view);
    (p, hit)
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
    draw_legend_centered(buf, &p, doc, view);
    // 5. 状态行(搜索中就是输入行)
    let status_y = area.y + area.height - 1;
    if let Some(q) = &view.query {
        let prompt = format!("/{q}");
        put(
            buf,
            area.x,
            status_y,
            &prompt,
            Style::default().fg(Color::Rgb(238, 226, 104)),
        );
        let x = area.x + width(&prompt) as u16;
        let what = if q.trim().is_empty() {
            view.lang
                .pick(
                    "节点名 / 别名(比如 wlgc),或分组号",
                    "node name / alias (e.g. wlgc), or a group number",
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
        put_plain(buf, x + 2, status_y, &what, dark_style());
        let tip = view
            .lang
            .pick("Enter 确定  Esc 取消", "Enter to confirm, Esc to cancel");
        let tw = width(tip) as u16;
        if area.width > tw + 4 {
            put_plain(buf, area.x + area.width - tw, status_y, tip, dark_style());
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
            if let Some(cell) = buf.cell_mut((x, status_y)) {
                cell.reset();
            }
        }
        put_plain(buf, area.x, status_y, prefix, dark_style());
        let shown = prompt_display(typed, view.lang);
        put_plain(
            buf,
            area.x + width(prefix) as u16,
            status_y,
            &shown,
            bright_style(),
        );
        return (p, FrameHit::default());
    }
    // 5.4 ":" 命令行独占整行:从最左边起,盖住模式标记与节点名,免得输入时还要往右找
    if let Some(c) = &view.cmd {
        let line = format!(":{c}");
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, status_y)) {
                cell.reset();
            }
        }
        // 命令行原样输出:路径里的 `/` 不能被当成斜体标记
        put_plain(buf, area.x, status_y, &line, bright_style());
        return (p, FrameHit::default());
    }
    // 5.5 状态行:先标出 VIEW / VIEW x{zoom},空闲时报当前 box 的文字,
    // 计数、布局、聚焦、命中都不在这儿
    // pick 由调用方派生;没派生(比如直接 render 的测试)就按格子现算一次
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
    left = format!("{}   {left}", mode_tag(view));
    // 回显位放右边:":" 命令行 > 命令回显 / 提示;空闲时什么都不放.
    // 它优先级最高,左边那块放不下就截断
    if let Some(hint) = hint_text(view) {
        let right = area.x + area.width;
        let hw = width(&hint) as u16;
        if right > hw + 2 && area.width as usize > hw as usize + 8 {
            let max_left = right.saturating_sub(hw + 2).saturating_sub(area.x) as usize;
            put_n(buf, area.x, status_y, &left, bright_style(), max_left);
            put_plain(buf, right - hw, status_y, &hint, dark_style());
        } else {
            // 右边放不下就接在左边那块后面,截到右边界
            put_n(
                buf,
                area.x,
                status_y,
                &left,
                bright_style(),
                area.width as usize,
            );
            let start = area.x + width(&left) as u16 + 2;
            let room = right.saturating_sub(start) as usize;
            if room >= 8 {
                put_n(buf, start, status_y, &hint, dark_style(), room);
            }
        }
    } else {
        put(buf, area.x, status_y, &left, bright_style());
    }
    (p, hit)
}

/// 状态行最前面的模式标记:能一眼看出是 1:1 编辑,还是缩放后的查看
fn mode_tag(view: &View) -> String {
    let zoom = if view.zoom.is_finite() && view.zoom > 0.0 {
        view.zoom
    } else {
        1.0
    };
    let one_to_one = (zoom - 1.0).abs() < 1e-9;
    if one_to_one {
        "VIEW".to_string()
    } else {
        format!("VIEW x{}", format_zoom(zoom))
    }
}

/// 缩放倍数的显示写法:整数不带小数点,其余留两位再去掉多余的零
fn format_zoom(z: f64) -> String {
    if (z - z.round()).abs() < 1e-9 {
        return format!("{}", z.round() as i64);
    }
    let s = format!("{z:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 回显位的内容:命令回显 / 提示(命令行自己独占一行,不走这里).空闲时没有
fn hint_text(view: &View) -> Option<String> {
    view.note.clone()
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
    ("V", "区块选择(和 v / Ctrl-q 一样),Enter 确认"),
    ("L", "切换图层(只看/只动当前层)"),
    ("Ctrl-q / v", "区块选择:锚点落在光标格,挪光标扩大;Enter 确认后命令作用于整块"),
    ("Ctrl-hjkl", "挪动选中的节点(可带数字前缀,如 10 + Ctrl-l;光标跟着走)"),
    ("f", "把聚焦的分组(没聚焦就是当前节点的分组)铺满画面"),
    ("F", "铺满整幅图"),
    ("z / 空格", "把当前节点挪到画面正中"),
    ("+ - / 0", "放大 / 缩小 / 回到 1:1"),
    ("b", "开关节点名标注"),
    (":legend", "开关右侧图例(默认开;文档的条目或分组颜色)"),
    (":rotate", "转方向:不带参数 +90 度,`:rotate 180` 绝对值,`:rotate auto` 交给布局按窗口选"),
    ("L / :legend", "循环图层 / 开关右侧图例(默认开)"),
    ("i / t", "改选中框的文字(空处新建框;选中边建注释框)/ 改光标框的文字"),
    ("a", "注释开关:关掉后注释框不画、不算障碍、也点不到(边会重新布线)"),
    (":notes", "注释开关 on / off(和 a 一样);`:relabel [all]` 把注释框重新贴回边旁边"),
    ("i(选中边)", "建/改这条边的注释框(空框 Esc 或空回车就撤掉)"),
    ("s / c", "样式菜单(框型 point/none/single/… 、颜色、对齐)/ 颜色菜单(框=边框色,边=边颜色)"),
    ("d / x", "带边剪切 / 只剪框和文字(关联边都从文档里删掉,但只有 d 把边放进剪贴板)"),
    ("p", "把剪贴板里上次删掉的内容粘到光标处"),
    ("t", "改光标框的文字(底行提示)"),
    ("u / Ctrl-r", "撤销 / 重做"),
    ("右键", "同一个样式菜单:划过即预览,一次左键应用;键盘 j k 选,l/h 进出一层,Enter 应用"),
    ("e", "两段式连边:源(选区或光标框)-> 移动/选目标 -> Enter 建边;Esc 取消"),
    ("E", "邻居高亮开关:只亮光标节点的邻居与相连的边(换节点自动跟着走;E 或 Esc 退出)"),
    ("Ctrl-e", "选边:在光标节点的相邻边之间轮换"),
    ("r", "循环切换布局"),
    ("鼠标左键", "点一下就落到那一格(重合的边连点会轮,点选区外面取消);空白处双击=新建节点;在节点上拖=挪框,空处拖=拉选区(松开即选完);Ctrl+左键/中键拖平移;滚轮缩放"),
    (":", "命令行:layout <名字> 换布局,cycle <倍数> 同组档位允许远多少,side <倍数> 横向偏斜算几倍距离,lang <zh|en> 界面语言"),
    (":", "建图:new <文字> 新建框、link <目标> 连边、del 删框/删选中的边"),
    (":", "编辑:box <point|none|single|rounded|double|heavy>、nodecolor <#rrggbb|auto>、fill <#rrggbb|none>、textcolor <#rrggbb|auto>、textbg <#rrggbb|none>、textalign <left|center|right>、edgebg <#rrggbb|none>、dir <none|forward|back|both>、corner <rounded|sharp>、via <x,y x,y|off>、text <余下整行>、save [路径]"),
    ("标签", "**粗体** 与 /斜体/"),
    ("?", "键位速查(任意键关掉)"),
    (":q / :wq", "退出 / 存盘后退出(q / esc 不退);命令行里 Tab / Shift-Tab 补全动词与取值"),
];

/// 键位速查:英文
const HELP_KEYS_EN: &[(&str, &str)] = &[
    ("h j k l / arrows", "move the cursor one cell (5l = five cells); what it sits on is selected"),
    ("Ctrl-d / Ctrl-u", "jump half a screen down / up (long trips)"),
    ("Ctrl-o / Ctrl-i", "back to the previous / next node you jumped to"),
    ("n / N", "next / previous search hit"),
    ("/", "search name / alias / initials; a bare number focuses that group"),
    ("V", "block select (same as v / Ctrl-q), Enter to confirm"),
    ("L", "cycle the layer (view and edit one layer at a time)"),
    ("Ctrl-q / v", "block select from the cursor cell; Enter confirms, then commands apply to the whole block"),
    ("Ctrl-hjkl", "nudge the selected boxes (count prefix works: 10 then Ctrl-l)"),
    ("f", "fit the focused group (or this node's group) to the screen"),
    ("F", "fit the whole document to the screen"),
    ("z / space", "centre this node"),
    ("+ - / 0", "zoom in / out / back to 1:1"),
    ("b", "toggle node labels"),
    (":legend", "toggle the legend panel (on by default)"),
    (":rotate", "rotate: bare +90deg, `:rotate 180` absolute, `:rotate auto` lets the layout pick"),
    ("Ctrl+G", "hand the prompt line to $VISUAL / $EDITOR (read back on exit; not for : or /)"),
    ("i / t", "edit the selected box text (i on empty space adds a box; i on an edge adds a note)"),
    ("a", "notes on/off: hidden notes are not drawn, not obstacles and not clickable"),
    (":notes", "notes on / off (same as a); `:relabel [all]` puts note boxes back next to their edge"),
    ("e", "two-step link: source (selection or cursor box) -> move/select target -> Enter; Esc cancels"),
    ("E", "neighbour view on/off: light up the cursor node's neighbours (follows the cursor; E/Esc exits)"),
    ("Ctrl-e", "pick an edge at the cursor node (cycles through its edges)"),
    ("r", "cycle the layout"),
    ("mouse left", "click to jump/select; drag on empty to block-select; drag a box to move it; Ctrl+drag or middle-drag pans; wheel zooms"),
    (":", "command line: layout <name> | side <x> | cycle <x> | lang <zh|en>"),
    (":", "build: new <text> | link <target> | del (box, or selected edge)"),
    (":", "edit: box <style> | nodecolor <#rrggbb|auto> | fill <#rrggbb|none> | textcolor <#rrggbb|auto> | textalign <left|center|right> | dir <none|forward|back|both> | corner <rounded|sharp> | via <x,y x,y|off> | text <rest of line> | save [path]"),
    ("labels", "**bold** and /italic/"),
    ("?", "key reference (any key closes it)"),
    (":q", "quit (only way; q / esc do not quit); Tab / Shift-Tab complete verbs and values"),
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
) {
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.node[i] == 0 && cv.mask[i] == 0 && cv.deco[i].is_none() {
                continue;
            }
            // 区块选择才用反显;当前选中的元素用红色呼吸(不铺底色)
            let on_picked_edge = match picked {
                Pick::Edge(e) => cv.edge_of[i] == e as i32,
                _ => false,
            };
            let selected = marked.contains(&(x, y));
            // 节点框盖在连线上面;光标节点的边框用呼吸色
            if let Some(d) = cv.deco[i] {
                let picked_box =
                    picked.box_at() == Some(d.node as usize) || picked_nodes.contains(&(x, y));
                let mut style = Style::default().fg(Color::Rgb(d.fg.0, d.fg.1, d.fg.2));
                if let Some((r, gg, b)) = d.bg {
                    style = style.bg(Color::Rgb(r, gg, b));
                }
                let dimmed = (cv.focus != 0 && d.groups & cv.focus == 0)
                    || (cv.layer_filter && !cv.lit[i])
                    || ego.is_some_and(|e| !e.nodes.contains(&(d.node as usize)));
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
            let mut ch = match cv.node[i] {
                2 => GLYPH_HUB,
                1 => GLYPH_NODE,
                // 非节点处:如果别的分组也打这儿过,就按"占住这一格的那一组"自己的形状画,
                // 后来的那一组断一格 —— 不然满屏 ┼ ├ ┬,看起来到处是枢纽,其实那儿什么都没有
                _ if cv.mask[i].count_ones() >= 3 && cv.owner_mask[i] != cv.mask[i] => {
                    box_char(cv.owner_mask[i], cv.corner[i])
                }
                _ => box_char(cv.mask[i], cv.corner[i]),
            };
            if cv.node[i] == 0 {
                if let Some(a) = cv.arrow[i] {
                    // 有向边的箭头盖在笔画上
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
            }) || (cv.layer_filter && !cv.lit[i])
                || ego.is_some_and(|e| !e.lit.contains(&(x, y)));
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
                    2 => GLYPH_HUB_CURSOR,
                    1 => GLYPH_NODE_CURSOR,
                    _ => ch,
                };
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
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
fn put_plain(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    let spans = [text::Span {
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
mod tests {
    use super::*;
    use crate::document::{DocEdge, Group, Layer, Text, ViewState};
    use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

    /// 把光标放到第 i 个框的文字起点(渲染测试用)
    fn set_at(d: &Document, view: &mut View, i: usize) {
        view.pick = Pick::Box(i);
        view.cursor = d
            .text(i)
            .map(|t| t.at)
            .unwrap_or((d.boxes[i].at.0 + 1, d.boxes[i].at.1 + 1));
    }

    fn grp(id: &str, label: &str, color: (u8, u8, u8)) -> Group {
        Group {
            id: id.into(),
            label: label.into(),
            color,
        }
    }

    fn lyr(name: &str, order: i32) -> Layer {
        Layer {
            name: name.into(),
            order,
        }
    }

    fn bx(id: &str, at: (i32, i32), size: (u16, u16), style: BoxStyle) -> Box {
        Box {
            id: id.into(),
            at,
            size,
            style,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        }
    }

    fn txt(id: &str, s: &str, at: (i32, i32), box_id: &str) -> Text {
        Text {
            id: id.into(),
            text: s.into(),
            at,
            box_id: box_id.into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        }
    }

    fn edg(id: &str, from: &str, to: &str, dir: Dir, group: &str) -> DocEdge {
        DocEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            dir,
            group: group.into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            corner: Corner::Rounded,
            layer: String::new(),
        }
    }

    fn doc_of(
        groups: Vec<Group>,
        layers: Vec<Layer>,
        boxes: Vec<Box>,
        texts: Vec<Text>,
        edges: Vec<DocEdge>,
    ) -> Document {
        let mut layers = layers;
        if !layers.iter().any(|l| l.name.is_empty()) {
            layers.insert(0, lyr("", 0));
        }
        Document::from_parts(
            "graph-tui test",
            groups,
            layers,
            boxes,
            texts,
            edges,
            ViewState::default(),
            crate::document::Legend::default(),
        )
        .expect("样例文档")
    }

    /// 一个带框的节点 + 一个点节点,一条带折点的有向边
    fn sample() -> Document {
        let mut e = edg("e1", "b1", "b2", Dir::Forward, "g0");
        e.via = vec![(4, 6)];
        doc_of(
            vec![grp("g0", "主线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("b1", (2, 2), (9, 3), BoxStyle::Rounded),
                bx("b2", (2, 8), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("t1", "中央", (3, 3), "b1"),
                txt("t2", "点", (2, 9), "b2"),
            ],
            vec![e],
        )
    }

    /// 三个点节点排成 L:甲的乙一根线,丙在下面
    fn points() -> Document {
        doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("pa", (0, 0), (1, 1), BoxStyle::Point),
                bx("pb", (10, 0), (1, 1), BoxStyle::Point),
                bx("pc", (20, 10), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("ta", "甲", (0, 1), "pa"),
                txt("tb", "乙", (10, 1), "pb"),
                txt("tc", "丙", (20, 11), "pc"),
            ],
            vec![edg("e1", "pa", "pb", Dir::Forward, "g0")],
        )
    }

    /// 把画面读成文本:全角字占的第二格要跨过去
    fn screen(buf: &Buffer) -> Vec<String> {
        let a = buf.area;
        (a.y..a.y + a.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = a.x;
                while x < a.x + a.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += width(sym).max(1) as u16;
                }
                line
            })
            .collect()
    }

    fn last_line(buf: &Buffer) -> String {
        screen(buf).pop().expect("有最后一行")
    }

    /// 框直接画在 at 上:左上角那一格就是左上角字形,不是从中心推出来的
    #[test]
    fn 框画在左上角_at() {
        let d = sample();
        let area = Rect::new(0, 0, 80, 30);
        let mut view = View::default();
        set_at(&d, &mut view, 1); // 光标挪到点节点上,好看清框本身
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let at = |dx: i32, dy: i32| buf[((x + dx) as u16, (y + dy) as u16)].symbol().to_string();
        assert_eq!(at(0, 0), "╭", "左上角就是 at");
        assert_eq!(at(8, 0), "╮");
        assert_eq!(at(0, 2), "╰");
        assert_eq!(at(8, 2), "╯");
        // 文字画在自己的 at 上(框内左上角 + (1, 1))
        let (tx, ty) = p.proj.cell((3.0, 3.0));
        assert_eq!((tx, ty), (x + 1, y + 1));
        assert_eq!(
            buf[(tx as u16, ty as u16)].symbol(),
            "中",
            "文字该在它自己的 at 上"
        );
        // 如果按中心推,框会被挪到 (6, 3) 那儿去
        let (cx, cy) = p.proj.cell((6.0, 3.0));
        assert_ne!((cx, cy), (x, y));
    }

    /// 点节点只占一格,画 ○;经过两个分组的画 ◎
    #[test]
    fn 点节点一格并画圆() {
        let d = doc_of(
            vec![grp("g0", "一", (255, 0, 0)), grp("g1", "二", (0, 255, 0))],
            vec![lyr("", 0)],
            vec![
                bx("p1", (5, 5), (1, 1), BoxStyle::Point),
                bx("p2", (15, 5), (1, 1), BoxStyle::Point),
                bx("p3", (5, 10), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("t1", "a", (5, 6), "p1"),
                txt("t2", "b", (15, 6), "p2"),
                txt("t3", "c", (5, 11), "p3"),
            ],
            vec![
                edg("e1", "p1", "p2", Dir::Forward, "g0"),
                edg("e2", "p3", "p2", Dir::Forward, "g1"),
            ],
        );
        assert!(d.hub(1), "p2 经过两个分组");
        let area = Rect::new(0, 0, 60, 24);
        let mut view = View::default();
        set_at(&d, &mut view, 2);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((5.0, 5.0));
        assert_eq!(buf[(x as u16, y as u16)].symbol(), "○");
        let (hx, hy) = p.proj.cell((15.0, 5.0));
        assert_eq!(buf[(hx as u16, hy as u16)].symbol(), "◎", "枢纽");
        // 只占一格:左右相邻格不是节点字形
        for dx in [-1i32, 1] {
            let sym = buf[((x + dx) as u16, y as u16)].symbol();
            assert!(!matches!(sym, "○" | "◎" | "●" | "◉"), "点节点只占一格");
        }
    }

    /// 有向边:箭头按 dir 画;目标带框时落在框外紧挨着的那一格
    #[test]
    fn 箭头按dir落在边框上() {
        let area = Rect::new(0, 0, 60, 20);
        // 点节点目标:箭头在终点前一格
        let plain_target = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 1), (1, 1), BoxStyle::Point),
                bx("b", (10, 1), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 2), "a"), txt("tb", "b", (10, 2), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &plain_target, &View::default());
        let (ex, ey) = p.proj.cell((10.0, 1.0));
        assert_eq!(buf[((ex - 1) as u16, ey as u16)].symbol(), ">");
        assert_eq!(buf[(ex as u16, ey as u16)].symbol(), "○", "终点本身是节点");
        // 目标带框:箭头嵌在左边界那一格
        let boxed_target = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 1), (1, 1), BoxStyle::Point),
                bx("b", (10, 0), (4, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (0, 2), "a"), txt("tb", "bb", (11, 1), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &boxed_target, &View::default());
        let (lx, ty) = p.proj.cell((10.0, 0.0));
        let (_, my) = p.proj.cell((11.0, 1.0));
        assert_eq!(buf[(lx as u16, ty as u16)].symbol(), "┌", "框的左上角");
        assert_eq!(
            buf[(lx as u16, my as u16)].symbol(),
            ">",
            "箭头该嵌在左边框上"
        );
        assert_eq!(
            buf[((lx - 1) as u16, my as u16)].symbol(),
            "─",
            "框外不再画箭头"
        );
        // 无向边不画箭头
        let plain_edge = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 1), (1, 1), BoxStyle::Point),
                bx("b", (10, 1), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 2), "a"), txt("tb", "b", (10, 2), "b")],
            vec![edg("e1", "a", "b", Dir::None, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &plain_edge, &View::default());
        let (bx, by) = p.proj.cell((10.0, 1.0));
        assert_eq!(buf[((bx - 1) as u16, by as u16)].symbol(), "─");
    }

    /// 连边模式:源框稳定高亮,目标框呼吸,状态栏报数量,预览线不落 T/箭头
    #[test]
    fn 连边模式的三种标记() {
        let area = Rect::new(0, 0, 80, 24);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 3), (6, 3), BoxStyle::Single),
                bx("b", (2, 12), (6, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (3, 4), "a"), txt("tb", "b", (3, 13), "b")],
            vec![],
        );
        let mut view = View::default();
        view.pick = Pick::Box(1);
        view.link = Some(crate::app::LinkState {
            sources: vec![0],
            targets: vec![1],
            confirming: false,
        });
        view.pulse = 0.0;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            (sx as u16, sy as u16)
        };
        // 源框:左上角是稳定高亮色
        let src = &buf[at(2, 3)];
        assert_eq!(src.symbol(), "┌");
        assert_eq!(src.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
        // 目标框:同一个格子是呼吸色(和源不同)
        let dst = &buf[at(2, 12)];
        assert_eq!(dst.symbol(), "┌");
        assert_ne!(dst.fg, Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2));
        // 预览线:暗蓝灰,而且目标边框没被 T/箭头改掉
        let mut preview = 0;
        for y in 0..area.height {
            for x in 0..area.width {
                if buf[(x, y)].fg == Color::Rgb(PREVIEW.0, PREVIEW.1, PREVIEW.2) {
                    preview += 1;
                }
            }
        }
        assert!(preview > 0, "该画出预览线");
        // 状态栏报数量
        // 宽字会占两格,第二条是空的:去掉空白再比
        let row: String = (0..area.width)
            .flat_map(|x| buf[(x, area.height - 1)].symbol().chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(row.contains("连边:源1->目标1"), "状态栏该有源和目标:{row}");
        assert!(row.contains("将建1条"), "状态栏该报数量:{row}");
    }

    /// 预览最多画 64 条:再多只报数量
    #[test]
    fn 连边预览最多六十四条() {
        let area = Rect::new(0, 0, 120, 40);
        let n: i32 = 18;
        let boxes: Vec<_> = (0..n)
            .map(|i| {
                bx(
                    &format!("b{i}"),
                    ((i % 9) * 12, (i / 9) * 8),
                    (6, 3),
                    BoxStyle::Single,
                )
            })
            .collect();
        let texts: Vec<_> = (0..n)
            .map(|i| {
                txt(
                    &format!("t{i}"),
                    "x",
                    ((i % 9) * 12 + 1, (i / 9) * 8 + 1),
                    &format!("b{i}"),
                )
            })
            .collect();
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            boxes,
            texts,
            vec![],
        );
        let mut view = View::default();
        view.link = Some(crate::app::LinkState {
            sources: (0..9).collect(),
            targets: (9..18).collect(),
            confirming: false,
        });
        let p = panels(&d, area, &view);
        let mut buf = Buffer::empty(area);
        let drawn = draw_link_preview(&mut buf, &p, &d, &view);
        assert_eq!(drawn, 64, "9x9 只画 64 条预览");
    }

    /// none:不画框,只有文字;边照样停在这块区域外,箭头落在外面那一格
    #[test]
    fn none_节点只有文字() {
        let area = Rect::new(0, 0, 40, 14);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (4, 3), BoxStyle::Single),
                bx("t", (10, 3), (4, 1), BoxStyle::None),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "文字", (10, 3), "t")],
            vec![edg("e1", "a", "t", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        // 文字就在区域左上角,没有边框(宽字占两格,第二格是空的)
        assert_eq!(at(10, 3), "文");
        assert_eq!(at(12, 3), "字");
        // 边从左边进来:箭头落在区域外面那一格
        assert_eq!(at(9, 3), ">", "箭头该在区域外");
        // 区域本身不该出现框线
        for x in 10..14 {
            let ch = at(x, 3);
            assert!(
                !"┌┐└┘├┤┬┴┼─│╭╮╯╰".contains(&ch),
                "({x},3) 出现了框线 {ch:?}"
            );
        }
    }

    /// 文字底色与边底色各自独立,互不覆盖
    #[test]
    fn 文字底色与边底色互不影响() {
        let area = Rect::new(0, 0, 40, 14);
        let mut a = bx("a", (0, 3), (4, 3), BoxStyle::Single);
        a.fill = Some((0x11, 0x11, 0x11));
        let t = bx("t", (10, 3), (4, 1), BoxStyle::None);
        let mut tb = txt("tb", "文字", (10, 3), "t");
        tb.bg = Some((0x33, 0x44, 0x55));
        let mut e = edg("e1", "a", "t", Dir::Forward, "g0");
        e.bg = Some((0x55, 0x22, 0x11));
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![a, t],
            vec![txt("ta", "a", (1, 4), "a"), tb],
            vec![e],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].clone()
        };
        // 文字:自己的底色
        assert_eq!(at(10, 3).symbol(), "文");
        assert_eq!(at(10, 3).bg, Color::Rgb(0x33, 0x44, 0x55), "文字底色");
        // 边经过的格子:边的底色
        let (ex, ey) = (9, 3);
        assert_eq!(at(ex, ey).bg, Color::Rgb(0x55, 0x22, 0x11), "边底色");
        // 框里:填充色不变,没被边的底色串到
        assert_eq!(at(1, 4).bg, Color::Rgb(0x11, 0x11, 0x11), "框的填充");
    }

    /// 端口走线:奇数宽上下对齐,0 折,T 与箭头同列
    #[test]
    fn 端口走线_奇数宽对齐() {
        let area = Rect::new(0, 0, 40, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (5, 3), BoxStyle::Single),
                bx("b", (0, 9), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (1, 10), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        // 宽 5:上下两侧都取正中列 2
        assert_eq!(at(2, 5), "┬", "起点 T");
        assert_eq!(at(2, 9), "v", "终点箭头");
        for y in 6..9 {
            assert_eq!(at(2, y), "│", "第 {y} 行该是直线");
        }
        assert_eq!(at(1, 5), "─", "底边其余是边框");
    }

    /// 端口走线:偶数宽上下差一格,2 折,jog 贴着目标端
    #[test]
    fn 端口走线_偶数宽错一格() {
        let area = Rect::new(0, 0, 40, 24);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (6, 3), BoxStyle::Single),
                bx("b", (0, 12), (6, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (1, 13), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        // 宽 6:a 下边取 left+2 = 2,b 上边取 left+3 = 3
        assert_eq!(at(2, 5), "┬", "起点 T 在左中列");
        assert_eq!(at(3, 12), "v", "箭头在右中列");
        // 长直线沿源端口列走,最后一格才错开
        for y in 6..10 {
            assert_eq!(at(2, y), "│", "第 {y} 行沿源列");
        }
        assert_eq!(at(2, 10), "╰", "jog 的起点");
        assert_eq!(at(3, 10), "╮", "jog 的终点");
        assert_eq!(at(3, 11), "│", "错一格后竖直进箭头");
    }

    /// 端口走线:水平错格,竖直 jog 贴着目标
    #[test]
    fn 端口走线_水平错格() {
        let area = Rect::new(0, 0, 40, 14);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 2), (6, 3), BoxStyle::Single),
                bx("b", (20, 5), (6, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (3, 3), "a"), txt("tb", "b", (21, 6), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        // a 右边取 top+h/2 = 3;b 左边取 top+(h-1)/2 = 6
        assert_eq!(at(7, 3), "├", "起点 T");
        assert_eq!(at(20, 6), ">", "箭头嵌在左边框");
        assert_eq!(at(18, 3), "╮", "竖直 jog 的起点");
        assert_eq!(at(18, 6), "╰", "竖直 jog 的终点");
        for x in 8..18 {
            assert_eq!(at(x, 3), "─", "第 {x} 列走在源行");
        }
    }

    /// 端口走线:左右互换,选侧跟着翻转
    #[test]
    fn 端口走线_左右互换() {
        let area = Rect::new(0, 0, 40, 14);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (20, 5), (6, 3), BoxStyle::Single),
                bx("b", (2, 2), (6, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (21, 6), "a"), txt("tb", "b", (3, 3), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        assert_eq!(at(20, 6), "┤", "a 改用左边");
        assert_eq!(at(7, 3), "<", "b 的右边框嵌入箭头");
    }

    /// 端口走线:上下互换,选侧翻到上边/下边
    #[test]
    fn 端口走线_上下互换() {
        let area = Rect::new(0, 0, 40, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 10), (5, 3), BoxStyle::Single),
                bx("b", (0, 3), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 11), "a"), txt("tb", "b", (1, 4), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        assert_eq!(at(2, 10), "┴", "a 改用上边");
        assert_eq!(at(2, 5), "^", "b 的底边嵌入朝上的箭头");
        for y in 6..10 {
            assert_eq!(at(2, y), "│", "第 {y} 行是直线");
        }
    }

    /// 端口走线:via 的首末段沿端口法线
    #[test]
    fn 端口走线_via() {
        let area = Rect::new(0, 0, 40, 26);
        let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
        e.via = vec![(12, 8)];
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (5, 3), BoxStyle::Single),
                bx("b", (10, 18), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (11, 19), "b")],
            vec![e],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let at = |x: i32, y: i32| {
            let (sx, sy) = p.proj.cell((x as f64, y as f64));
            buf[(sx as u16, sy as u16)].symbol().to_string()
        };
        // 起点先沿法线往下,再横到 via;最后一段仍然竖着进 b
        assert_eq!(at(2, 6), "│", "首段沿源法线");
        assert_eq!(at(2, 8), "╰", "转到 via 那一行");
        for x in 3..12 {
            assert_eq!(at(x, 8), "─", "第 {x} 列横着走");
        }
        assert_eq!(at(12, 8), "╮", "到 via 后转向下");
        assert_eq!(at(12, 17), "│", "末段竖着进目标");
        assert_eq!(at(12, 18), "v", "箭头");
    }

    /// 有向边:起点边框画 T,终点把箭头嵌进边框
    #[test]
    fn 边框上的连接点与嵌入箭头() {
        let area = Rect::new(0, 0, 60, 20);
        for (dir, want_a, want_b) in [
            (Dir::None, '├', '┤'),
            (Dir::Forward, '├', '>'),
            (Dir::Back, '<', '┤'),
            (Dir::Both, '<', '>'),
        ] {
            let d = doc_of(
                vec![grp("g0", "线", (76, 142, 248))],
                vec![lyr("", 0)],
                vec![
                    bx("a", (0, 3), (5, 3), BoxStyle::Single),
                    bx("b", (12, 3), (5, 3), BoxStyle::Single),
                ],
                vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
                vec![edg("e1", "a", "b", dir, "g0")],
            );
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &View::default());
            let (ax, ay) = p.proj.cell((4.0, 4.0));
            let (bx_, by_) = p.proj.cell((12.0, 4.0));
            assert_eq!(
                buf[(ax as u16, ay as u16)].symbol(),
                want_a.to_string(),
                "{dir:?} 的起点"
            );
            assert_eq!(
                buf[(bx_ as u16, by_ as u16)].symbol(),
                want_b.to_string(),
                "{dir:?} 的终点"
            );
        }
    }

    /// 粗线框的 T 用粗线字形
    #[test]
    fn 粗线框的连接点用粗线() {
        let area = Rect::new(0, 0, 60, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (5, 3), BoxStyle::Heavy),
                bx("b", (12, 3), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((4.0, 4.0));
        assert_eq!(buf[(ax as u16, ay as u16)].symbol(), "┣");
    }

    /// 颜色:连接点跟框的边框色,嵌入箭头跟边的颜色;两者都保留框的填充底色
    #[test]
    fn 连接点与箭头的颜色和填充() {
        let area = Rect::new(0, 0, 60, 20);
        let mut a = bx("a", (0, 3), (5, 3), BoxStyle::Single);
        a.color = Some((10, 20, 30));
        a.fill = Some((5, 6, 7));
        let mut b = bx("b", (12, 3), (5, 3), BoxStyle::Single);
        b.fill = Some((5, 6, 7));
        let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
        e.color = Some((200, 30, 40));
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![a, b],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
            vec![e],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((4.0, 4.0));
        let (bx_, by_) = p.proj.cell((12.0, 4.0));
        let ta = &buf[(ax as u16, ay as u16)];
        let ar = &buf[(bx_ as u16, by_ as u16)];
        assert_eq!(ta.symbol(), "├");
        assert_eq!(ta.fg, Color::Rgb(10, 20, 30), "T 用框的边框色");
        assert_eq!(ta.bg, Color::Rgb(5, 6, 7), "T 保留填充底色");
        assert_eq!(ar.symbol(), ">");
        assert_eq!(ar.fg, Color::Rgb(200, 30, 40), "箭头用边的颜色");
        assert_eq!(ar.bg, Color::Rgb(5, 6, 7), "箭头保留填充底色");
    }

    /// 同侧放不下第二个箭头时直接跳过,不报错也不画到别处
    #[test]
    fn 没空位就不画箭头() {
        let area = Rect::new(0, 0, 60, 20);
        // 框高固定 3,左右两侧各只有一格:第三个箭头无处可放
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("p", (0, 4), (1, 1), BoxStyle::Point),
                bx("q", (0, 6), (1, 1), BoxStyle::Point),
                bx("r", (0, 8), (1, 1), BoxStyle::Point),
                bx("b", (12, 5), (6, 3), BoxStyle::Single),
            ],
            vec![
                txt("tp", "p", (0, 5), "p"),
                txt("tq", "q", (0, 7), "q"),
                txt("tr", "r", (0, 9), "r"),
                txt("tb", "b", (13, 6), "b"),
            ],
            vec![
                edg("e1", "p", "b", Dir::Forward, "g0"),
                edg("e2", "q", "b", Dir::Forward, "g0"),
                edg("e3", "r", "b", Dir::Forward, "g0"),
            ],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        // 左边只有中间一行能放箭头,其余两个跳过;箭头数不超过格子数
        let mut n = 0;
        for y in 5..8 {
            let (sx, sy) = p.proj.cell((12.0, y as f64));
            if buf[(sx as u16, sy as u16)].symbol() == ">" {
                n += 1;
            }
        }
        assert!(n <= 1, "左边只有一格,画了 {n} 个箭头");
    }

    /// 框被推到画面外:T 在画面外不画,可见的那一侧照常
    #[test]
    fn 框出画面不崩() {
        let area = Rect::new(0, 0, 24, 10);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 2), (5, 3), BoxStyle::Single),
                bx("b", (30, 2), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 3), "a"), txt("tb", "b", (31, 3), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        // 把视图固定在 a 上:b 落在画面外,它的箭头格不该被画
        let view = View {
            center: Some((2.0, 3.5)),
            ..Default::default()
        };
        render(&mut buf, area, &d, &view);
        let t = (0..area.height).any(|y| (0..area.width).any(|x| buf[(x, y)].symbol() == "├"));
        assert!(t, "可见那侧照常画 T");
    }

    /// 同一侧多个箭头:首选格被占就顺时针找空位(框高固定 3,只有上下两侧放得下两个)
    #[test]
    fn 同侧多个箭头各占一格() {
        let area = Rect::new(0, 0, 60, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (12, 0), (1, 1), BoxStyle::Point),
                bx("c", (16, 0), (1, 1), BoxStyle::Point),
                bx("b", (12, 4), (6, 3), BoxStyle::Single),
            ],
            vec![
                txt("ta", "a", (12, 1), "a"),
                txt("tc", "c", (16, 1), "c"),
                txt("tb", "b", (13, 5), "b"),
            ],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "c", "b", Dir::Forward, "g0"),
            ],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        // b 的上边:13..16,首选 left + W/2 = 15,第二个顺时针落到 16
        let mut arrows = Vec::new();
        for x in 13..17 {
            let (sx, sy) = p.proj.cell((x as f64, 4.0));
            if buf[(sx as u16, sy as u16)].symbol() == "v" {
                arrows.push(x);
            }
        }
        assert_eq!(arrows.len(), 2, "两个箭头该各占一格: {arrows:?}");
        assert_eq!(arrows, vec![15, 16], "先首选格,再顺时针找空位");
    }

    /// via 折点:箭头按最后一段的方向落在对应的边上
    #[test]
    fn via_决定进入侧() {
        let area = Rect::new(0, 0, 60, 20);
        let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
        e.via = vec![(6, 6), (6, 8)];
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 2), (1, 1), BoxStyle::Point),
                bx("b", (10, 8), (5, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (2, 3), "a"), txt("tb", "b", (11, 9), "b")],
            vec![e],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        // 最后一段从 (6,8) 横着进 b 的左边
        let (lx, my) = p.proj.cell((10.0, 9.0));
        assert_eq!(buf[(lx as u16, my as u16)].symbol(), ">", "从左边进来");
    }

    /// 双线框的 T 用双线字形
    #[test]
    fn 双线框的连接点用双线() {
        let area = Rect::new(0, 0, 60, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 3), (5, 3), BoxStyle::Double),
                bx("b", (12, 3), (5, 3), BoxStyle::Double),
            ],
            vec![txt("ta", "a", (1, 4), "a"), txt("tb", "b", (13, 4), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((4.0, 4.0));
        assert_eq!(buf[(ax as u16, ay as u16)].symbol(), "╠", "双线框右边是 ╠");
    }

    /// 取格按宽高奇偶:宽 4 的框上边取右中列
    #[test]
    fn 连接格按宽高奇偶() {
        let area = Rect::new(0, 0, 60, 20);
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (11, 0), (1, 1), BoxStyle::Point),
                bx("b", (10, 6), (4, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (11, 1), "a"), txt("tb", "bb", (11, 7), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        // 上边:left + W/2 = 10 + 2 = 12
        let (x, y) = p.proj.cell((12.0, 6.0));
        assert_eq!(buf[(x as u16, y as u16)].symbol(), "v");
        let (x2, y2) = p.proj.cell((11.0, 6.0));
        assert_ne!(buf[(x2 as u16, y2 as u16)].symbol(), "v", "不是正中列");
    }

    /// 方向四态:0/1/1/2 个箭头,位置分别在 from 端、to 端
    #[test]
    fn 四态方向箭头() {
        let area = Rect::new(0, 0, 60, 20);
        for (dir, want) in [
            (Dir::None, (false, false)),
            (Dir::Forward, (false, true)),
            (Dir::Back, (true, false)),
            (Dir::Both, (true, true)),
        ] {
            let d = doc_of(
                vec![grp("g0", "线", (76, 142, 248))],
                vec![lyr("", 0)],
                vec![
                    bx("a", (2, 5), (1, 1), BoxStyle::Point),
                    bx("b", (12, 5), (1, 1), BoxStyle::Point),
                ],
                vec![txt("ta", "a", (2, 6), "a"), txt("tb", "b", (12, 6), "b")],
                vec![edg("e1", "a", "b", dir, "g0")],
            );
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &View::default());
            let (ax, ay) = p.proj.cell((2.0, 5.0));
            let (bx, by) = p.proj.cell((12.0, 5.0));
            assert_eq!(
                buf[((ax + 1) as u16, ay as u16)].symbol() == "<",
                want.0,
                "{} 的 from 端箭头",
                dir.name()
            );
            assert_eq!(
                buf[((bx - 1) as u16, by as u16)].symbol() == ">",
                want.1,
                "{} 的 to 端箭头",
                dir.name()
            );
        }
    }

    /// 框内填充:整个矩形每一格都有背景,框外没有;点节点忽略 fill
    #[test]
    fn 框内填充铺满矩形() {
        let area = Rect::new(0, 0, 80, 30);
        let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
        b1.fill = Some((1, 2, 3));
        let mut p1 = bx("p1", (20, 8), (1, 1), BoxStyle::Point);
        p1.fill = Some((1, 2, 3));
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![b1, p1],
            vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
            Vec::new(),
        );
        let mut view = View::default();
        set_at(&d, &mut view, 1);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let fill = Color::Rgb(1, 2, 3);
        for dy in 0..3 {
            for dx in 0..8 {
                let cell = &buf[((x + dx) as u16, (y + dy) as u16)];
                assert_eq!(cell.bg, fill, "框内 ({dx}, {dy}) 该有填充");
            }
        }
        assert_ne!(buf[((x - 1) as u16, y as u16)].bg, fill, "框左外边不填");
        assert_ne!(buf[(x as u16, (y - 1) as u16)].bg, fill, "框上外边不填");
        assert_ne!(buf[(x as u16, (y + 3) as u16)].bg, fill, "框下外边不填");
        // 文字格背景也在,字画在背景上面
        let cell = &buf[((x + 1) as u16, (y + 1) as u16)];
        assert_eq!(cell.symbol(), "a");
        assert_eq!(cell.bg, fill);
        // 点节点没有框,不铺填充
        let (px, py) = p.proj.cell((20.0, 8.0));
        assert_ne!(buf[(px as u16, py as u16)].bg, fill, "点节点忽略 fill");
    }

    /// 光标框只换前景(呼吸色),填充不能被冲掉
    #[test]
    fn 光标框保留填充() {
        let area = Rect::new(0, 0, 80, 30);
        let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
        b1.fill = Some((1, 2, 3));
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![b1],
            vec![txt("t1", "ab", (3, 3), "b1")],
            Vec::new(),
        );
        let mut view = View::default();
        set_at(&d, &mut view, 0);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        for dy in 0..3 {
            for dx in 0..8 {
                assert_eq!(
                    buf[((x + dx) as u16, (y + dy) as u16)].bg,
                    Color::Rgb(1, 2, 3),
                    "光标框 ({dx}, {dy}) 也要有填充"
                );
            }
        }
    }

    /// 宽字的续格也要有框内填充:文字只换前景,不能把底色冲掉
    #[test]
    fn 宽字续格保留填充() {
        let area = Rect::new(0, 0, 80, 30);
        let mut b1 = bx("b1", (2, 2), (10, 3), BoxStyle::Rounded);
        b1.fill = Some((1, 2, 3));
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![b1],
            vec![txt("t1", "中文", (3, 3), "b1")],
            Vec::new(),
        );
        let mut view = View::default();
        set_at(&d, &mut view, 0);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        // 内区 8 格:中 文 各占两列,续格不能是终端底色
        for dx in 1..9 {
            assert_eq!(
                buf[((x + dx) as u16, (y + 1) as u16)].bg,
                Color::Rgb(1, 2, 3),
                "内区第 {dx} 格该有填充"
            );
        }
    }

    /// 图例默认开:地图让出右边;`:legend off` 之后地图才占满整行
    #[test]
    fn 图例默认开着() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![bx("b1", (2, 2), (8, 3), BoxStyle::Rounded)],
            vec![txt("t1", "ab", (3, 3), "b1")],
            Vec::new(),
        );
        let area = Rect::new(0, 0, 120, 30);
        let mut view = View::default();
        let p = panels(&d, area, &view);
        assert!(p.legend.width > 0, "默认就该有图例");
        assert!(p.map.width < area.width, "地图要让出右边");

        view.legend = false;
        let p = panels(&d, area, &view);
        assert_eq!(p.legend.width, 0, "关掉之后不该占宽度");
        assert_eq!(p.map.width, area.width, "关掉之后地图占满宽度");
    }

    /// 选中元素用红白呼吸(不铺底色):框闪边框,没框的点节点闪字
    #[test]
    fn 选中元素红白呼吸() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("b1", (2, 2), (8, 3), BoxStyle::Rounded),
                bx("p1", (20, 8), (1, 1), BoxStyle::Point),
            ],
            vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
            Vec::new(),
        );
        let area = Rect::new(0, 0, 80, 30);
        let shot = |pick: Pick, cursor: (i32, i32), pulse: f64| {
            let mut view = View::default();
            view.pick = pick;
            view.cursor = cursor;
            view.pulse = pulse;
            view.labels = true;
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &view);
            let (x, y) = p.proj.cell((2.0, 2.0));
            let (px, py) = p.proj.cell((20.0, 9.0));
            (
                buf[(x as u16, y as u16)].clone(),
                buf[(px as u16, py as u16)].clone(),
            )
        };
        // 选中框:pulse=0 时白,=1 时红;底色不变(不铺高亮底)
        let (white, _) = shot(Pick::Box(0), (3, 3), 0.0);
        let (red, _) = shot(Pick::Box(0), (3, 3), 1.0);
        assert_eq!(
            white.fg,
            Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2),
            "呼吸到白"
        );
        assert_eq!(
            red.fg,
            Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
            "呼吸到红"
        );
        assert_eq!(white.bg, red.bg, "不铺底色");

        // 选中点节点:闪的是它的名字
        let (_, label_w) = shot(Pick::Box(1), (20, 8), 0.0);
        let (_, label_r) = shot(Pick::Box(1), (20, 8), 1.0);
        assert_eq!(
            label_w.fg,
            Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2),
            "名字呼吸到白"
        );
        assert_eq!(
            label_r.fg,
            Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
            "名字呼吸到红"
        );
    }

    /// 自环:自己绕节点画一圈,格子记进命中表,点得到
    #[test]
    fn 自环画得出来也点得到() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
                bx("b", (30, 5), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (3, 6), "a"), txt("tb", "b", (30, 5), "b")],
            vec![
                edg("e1", "a", "a", Dir::Forward, "g0"),
                edg("e2", "b", "b", Dir::Forward, "g0"),
            ],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let mut view = View::default();
        view.legend = false;
        let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        let screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
        for e in 0..2 {
            let cells = hit.edge_cells.get(&e).cloned().unwrap_or_default();
            assert!(!cells.is_empty(), "自环 e{e} 该画出格子");
            // 至少拐了两下,不是一根直线
            let xs: std::collections::HashSet<i32> = cells.iter().map(|c| c.0).collect();
            let ys: std::collections::HashSet<i32> = cells.iter().map(|c| c.1).collect();
            assert!(xs.len() > 1 && ys.len() > 1, "自环 e{e} 该是个环:{cells:?}");
            let c = cells[0];
            assert!(
                matches!(pick_at(&d, &hit, (0, 0), c, &view), Pick::Edge(k) if k == e),
                "自环 e{e} 的格子该点得到"
            );
        }
        assert!(screen((2, 5)) != (0, 0));
    }

    /// 多行文字:每行落在框内相邻的行上,对齐逐行生效
    #[test]
    fn 多行文字逐行画() {
        let mut d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![bx("a", (0, 0), (8, 3), BoxStyle::Rounded)],
            vec![txt("ta", "x", (1, 1), "a")],
            vec![],
        );
        d.set_text(0, "一\n二二\n三三三");
        assert_eq!(d.boxes[0].size.1, 5, "三行 + 上下边框");
        d.texts[0].align = Align::Center;
        let area = Rect::new(0, 0, 30, 12);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (x, y) = p.proj.cell((0.0, 0.0));
        let text = screen(&buf).join("\n");
        assert!(text.contains("一"), "{text}");
        // 三行都在框内、行距 1
        let rows: Vec<String> = (0..5)
            .map(|k| screen(&buf)[(y + 1 + k) as usize].clone())
            .collect();
        assert!(rows[0].contains('一'), "{rows:?}");
        assert!(rows[1].contains("二二"), "{rows:?}");
        assert!(rows[2].contains("三三三"), "{rows:?}");
        let _ = x;
    }

    /// 注释框就是普通 none 框:画在自己坐标上,关掉开关就整块消失(尺寸也变小)
    #[test]
    fn 注释框随开关显示() {
        let mut d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
                bx("b", (30, 5), (8, 3), BoxStyle::Rounded),
            ],
            vec![txt("ta", "甲", (3, 6), "a"), txt("tb", "乙", (31, 6), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let b = d.new_note(0).expect("该能建注释框");
        d.set_text(b, "收到 ACK");
        d.place_box(b, (18, 9));
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        view.legend = false;
        let mut buf = Buffer::empty(area);
        let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        let (x, y) = p.proj.cell((18.0, 9.0));
        let text = screen(&buf).join("\n");
        assert!(text.contains("收到 ACK"), "{text}");
        // 注释框和普通框一样能点
        assert!(
            matches!(pick_at(&d, &hit, (18, 9), (x, y), &view), Pick::Box(k) if k == b),
            "注释框该点得到"
        );
        let (_, y0, _, _) = d.bbox_shown(true);
        let (_, _, _, y1) = d.bbox_shown(false);
        assert!(
            y0 != y1 || d.bbox() != d.bbox_shown(false),
            "关掉后包围盒变小"
        );

        // 关掉:不画、也点不到
        view.notes = false;
        let mut buf = Buffer::empty(area);
        let (p2, hit2) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        let (x2, y2) = p2.proj.cell((18.0, 9.0));
        assert!(
            !screen(&buf).join("\n").contains("收到 ACK"),
            "关掉就不该画"
        );
        assert_eq!(
            pick_at(&d, &hit2, (18, 9), (x2, y2), &view),
            Pick::None,
            "关掉的注释框不该点得到"
        );
    }

    /// E 模式:亮着的边绑的注释一起亮;注释关着时它临时露出来
    #[test]
    fn 邻居高亮带上相关注释() {
        let mut d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (8, 3), BoxStyle::Rounded),
                bx("b", (30, 5), (8, 3), BoxStyle::Rounded),
                bx("c", (2, 20), (8, 3), BoxStyle::Rounded),
            ],
            vec![
                txt("ta", "甲", (3, 6), "a"),
                txt("tb", "乙", (31, 6), "b"),
                txt("tc", "丙", (3, 21), "c"),
            ],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "a", "c", Dir::Forward, "g0"),
            ],
        );
        let b = d.new_note(0).expect("该能建注释框");
        d.set_text(b, "收到 ACK");
        let area = Rect::new(0, 0, 60, 30);
        let mut view = View::default();
        view.legend = false;
        view.notes = false;
        view.ego = Some(0);
        let mut buf = Buffer::empty(area);
        let (p, _) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        let text = screen(&buf).join("\n");
        assert!(text.contains("收到 ACK"), "E 模式下临时露出来:{text}");
        let (x, y) = p
            .proj
            .cell((d.boxes[b].at.0 as f64, d.boxes[b].at.1 as f64));
        assert!(x >= 0 && y >= 0);
    }

    /// 命中优先级:框 > 边 > 空
    #[test]
    fn 命中优先级() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (6, 3), BoxStyle::Rounded),
                bx("b", (20, 5), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (3, 6), "a"), txt("tb", "", (20, 6), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let view = View::default();
        let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        // 框:画布格与屏幕格各算一份
        let screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
        assert!(matches!(
            pick_at(&d, &hit, (4, 6), screen((4, 6)), &view),
            Pick::Box(0)
        ));
        assert!(
            matches!(
                pick_at(&d, &hit, (2, 5), screen((2, 5)), &view),
                Pick::Box(0)
            ),
            "边框也算"
        );
        // 边:命中表记的是屏幕格,画布格随便给一个不在框里的
        let s = screen((11, 6));
        let got = pick_at(&d, &hit, (11, 6), s, &view);
        assert!(matches!(got, Pick::Edge(_)), "中间那格命中边:{got:?}");
        let s2 = screen((11, 14));
        assert_eq!(pick_at(&d, &hit, (11, 14), s2, &view), Pick::None);
    }

    /// 一帧的命中表:边笔画格能查到是哪条边,空白格查不到
    #[test]
    fn 命中表记得边和它的格子() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (2, 5), (1, 1), BoxStyle::Point),
                bx("b", (20, 5), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "", (2, 6), "a"), txt("tb", "", (20, 6), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let view = View::default();
        let (p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        // 两个点之间那一格是这条边画的
        let (x, y) = p.proj.cell((11.0, 5.0));
        assert_eq!(hit.edge_at((x, y)), Some(0), "中间那格该属于 e1");
        // 空白处没有边
        let (bx, by) = p.proj.cell((11.0, 12.0));
        assert_eq!(hit.edge_at((bx, by)), None);
        // 可见格子清单里有这条边,而且不止一格
        assert!(hit.edge_cells.get(&0).map(|v| v.len() > 5).unwrap_or(false));
        // 候选里也有它
        assert!(hit.candidates_at((x, y)).contains(&0));
    }

    /// 被聚焦过滤压暗的框不画填充,保持终端底色
    #[test]
    fn 压暗的框不画填充() {
        let area = Rect::new(0, 0, 80, 30);
        let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
        b1.fill = Some((1, 2, 3));
        let d = doc_of(
            vec![grp("g0", "一", (255, 0, 0)), grp("g1", "二", (0, 255, 0))],
            vec![lyr("", 0)],
            vec![b1, bx("p1", (20, 8), (1, 1), BoxStyle::Point)],
            vec![txt("t1", "ab", (3, 3), "b1"), txt("t2", "p", (20, 9), "p1")],
            Vec::new(),
        );
        let mut view = View::default();
        view.focus = vec![1];
        set_at(&d, &mut view, 1); // 光标别落在被压暗的框上,免得高亮盖掉判断
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        for dy in 0..3 {
            for dx in 0..8 {
                let bg = buf[((x + dx) as u16, (y + dy) as u16)].bg;
                assert_ne!(bg, Color::Rgb(1, 2, 3), "压暗时不该有填充");
                assert_eq!(bg, Color::Reset, "压暗时保持终端底色");
            }
        }
    }

    /// 边框色和文字色各用各的
    #[test]
    fn 边框与文字颜色分离() {
        let area = Rect::new(0, 0, 80, 30);
        let mut b1 = bx("b1", (2, 2), (8, 3), BoxStyle::Rounded);
        b1.color = Some((10, 20, 30));
        b1.fill = Some((5, 6, 7));
        let mut t1 = txt("t1", "ab", (3, 3), "b1");
        t1.color = Some((40, 50, 60));
        let mut t2 = txt("t2", "p", (20, 9), "p1");
        t2.color = Some((70, 80, 90));
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![b1, bx("p1", (20, 8), (1, 1), BoxStyle::Point)],
            vec![t1, t2],
            Vec::new(),
        );
        let mut view = View::default();
        // 光标放到空白处:这条测的是"颜色各自独立",不是选中时的呼吸色
        view.cursor = (-5, -5);
        view.pick = Pick::None;
        view.labels = true; // 点节点名平时未必画,这里强制标出来
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x, y) = p.proj.cell((2.0, 2.0));
        let border = &buf[(x as u16, y as u16)];
        assert_eq!(border.fg, Color::Rgb(10, 20, 30), "边框用自己的颜色");
        assert_eq!(border.bg, Color::Rgb(5, 6, 7), "边框格同样铺填充");
        let text = &buf[((x + 1) as u16, (y + 1) as u16)];
        assert_eq!(text.fg, Color::Rgb(40, 50, 60), "文字用自己的颜色");
        assert_eq!(text.bg, Color::Rgb(5, 6, 7), "文字格同样铺填充");
        let (px, py) = p.proj.cell((20.0, 9.0));
        assert_eq!(
            buf[(px as u16, py as u16)].fg,
            Color::Rgb(70, 80, 90),
            "点节点文字用自己的颜色,不取框色"
        );
    }

    /// 文字水平对齐:left 从左边 1 格,center 居中,right 靠右留 1 格
    #[test]
    fn 文字按align偏移() {
        let area = Rect::new(0, 0, 80, 30);
        // 框宽 12,可用宽度 10,文字 "abc" 宽 3
        for (align, off) in [(Align::Left, 0), (Align::Center, 3), (Align::Right, 7)] {
            let mut t = txt("t1", "abc", (3, 3), "b1");
            t.align = align;
            let d = doc_of(
                vec![grp("g0", "线", (255, 0, 0))],
                vec![lyr("", 0)],
                vec![bx("b1", (2, 2), (12, 3), BoxStyle::Rounded)],
                vec![t],
                Vec::new(),
            );
            let mut view = View::default();
            set_at(&d, &mut view, 0);
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &view);
            let (x, y) = p.proj.cell((2.0, 2.0));
            let tx = x + 1 + off;
            assert_eq!(
                buf[(tx as u16, (y + 1) as u16)].symbol(),
                "a",
                "{} 时文字起点",
                align.name()
            );
            if off > 0 {
                assert_ne!(buf[((tx - 1) as u16, (y + 1) as u16)].symbol(), "a");
            }
        }
    }

    /// 拐角样式:sharp 画 ┌┐└┘,rounded 画 ╭╮╯╰
    #[test]
    fn 边拐角按corner画() {
        let area = Rect::new(0, 0, 60, 20);
        for (corner, want) in [(Corner::Sharp, "┐"), (Corner::Rounded, "╮")] {
            let mut e = edg("e1", "a", "b", Dir::Forward, "g0");
            e.corner = corner;
            let d = doc_of(
                vec![grp("g0", "线", (76, 142, 248))],
                vec![lyr("", 0)],
                vec![
                    bx("a", (2, 2), (1, 1), BoxStyle::Point),
                    bx("b", (20, 10), (1, 1), BoxStyle::Point),
                ],
                vec![txt("ta", "a", (2, 3), "a"), txt("tb", "b", (20, 11), "b")],
                vec![e],
            );
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &View::default());
            let (cx, cy) = p.proj.cell((20.0, 2.0));
            assert_eq!(
                buf[(cx as u16, cy as u16)].symbol(),
                want,
                "{} 的拐角",
                corner.name()
            );
        }
    }

    /// 虚线:直笔按线型断开,拐角处仍是实画
    #[test]
    fn 虚线断直笔不断拐角() {
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (20, 0), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 1), "b")],
            vec![{
                let mut e = edg("e1", "a", "b", Dir::None, "g0");
                e.style = EdgeStyle::Dashed;
                e
            }],
        );
        let area = Rect::new(0, 0, 60, 12);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (x0, y) = p.proj.cell((0.0, 0.0));
        let (x1, _) = p.proj.cell((20.0, 0.0));
        let mut blank = false;
        let mut drawn = false;
        for x in x0 + 1..x1 {
            match buf[(x as u16, y as u16)].symbol() {
                " " => blank = true,
                "─" => drawn = true,
                _ => {}
            }
        }
        assert!(drawn, "虚线里也该有实笔");
        assert!(blank, "虚线里该有断开的空格");
        // 拐弯的虚线:拐角必须有字形
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (20, 10), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 11), "b")],
            vec![{
                let mut e = edg("e1", "a", "b", Dir::None, "g0");
                e.style = EdgeStyle::Dashed;
                e
            }],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &View::default());
        let text = screen(&buf).join("\n");
        assert!(
            text.chars().any(|c| matches!(c, '╭' | '╮' | '╰' | '╯')),
            "虚线拐角该有字形:\n{text}"
        );
    }

    /// 按图层看时:不在当前层的节点与边压暗,当前层的正常画
    #[test]
    fn 非当前层压暗() {
        let mut a = bx("a", (0, 0), (1, 1), BoxStyle::Point);
        a.layer = "底".into();
        let mut b = bx("b", (10, 0), (1, 1), BoxStyle::Point);
        b.layer = "顶".into();
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0), lyr("底", 1), lyr("顶", 2)],
            vec![a, b],
            vec![txt("ta", "下", (0, 1), "a"), txt("tb", "上", (10, 1), "b")],
            vec![edg("e1", "a", "b", Dir::None, "g0")],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        set_at(&d, &mut view, 1);
        view.layer = Some("顶".to_string());
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (ax, ay) = p.proj.cell((0.0, 0.0));
        assert_eq!(
            buf[(ax as u16, ay as u16)].fg,
            DIM_COLOR,
            "底层的节点该压暗"
        );
        let (cx, cy) = p.proj.cell((10.0, 0.0));
        assert_ne!(
            buf[(cx as u16, cy as u16)].fg,
            DIM_COLOR,
            "当前层的节点不该压暗"
        );
        let (mx, my) = p.proj.cell((5.0, 0.0));
        assert_eq!(
            buf[(mx as u16, my as u16)].fg,
            DIM_COLOR,
            "别层的边也该压暗"
        );
    }

    /// 区块选择画连续的灰边:只改底色,不盖字,整条边不留断口
    #[test]
    fn 区块选择画连续灰边() {
        let d = doc_of(
            vec![grp("g0", "线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![bx("a", (2, 2), (8, 3), BoxStyle::Rounded)],
            vec![txt("ta", "a", (3, 3), "a")],
            Vec::new(),
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        set_at(&d, &mut view, 0);
        view.sel = Some(crate::app::RectSel {
            anchor: (2, 2),
            end: view.cursor,
            confirmed: false,
        });
        view.cursor = (12, 7);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        let (x0, y0) = p.proj.cell((2.0, 2.0));
        let (x1, y1) = p.proj.cell((12.0, 7.0));
        let gray = |x: i32, y: i32| buf[(x as u16, y as u16)].bg == SEL_GRAY;
        assert!(gray(x0, y0), "左上角");
        assert!(gray(x1, y0), "右上角");
        assert!(gray(x0, y1), "左下角");
        assert!(gray(x1, y1), "右下角");
        // 框里的字还在:左上角那格本来就是框的角字
        assert_eq!(buf[(x0 as u16, y0 as u16)].symbol(), "╭");
        // 四条边都是连续的:中间每一格都盖着灰底,不留断口
        for y in y0..=y1 {
            assert!(gray(x0, y) && gray(x1, y), "竖边在 y={y} 处断了");
        }
        for x in x0..=x1 {
            assert!(gray(x, y0) && gray(x, y1), "横边在 x={x} 处断了");
        }
        // 框内不画
        assert!(buf[(x0 as u16 + 1, y0 as u16 + 1)].bg != SEL_GRAY);
    }

    /// 矩形选择:按格矩形收集 box,选中的框按 pulse 呼吸(不反显)
    #[test]
    fn 矩形选择按格子收集框() {
        let d = points();
        let area = Rect::new(0, 0, 60, 24);
        let mut view = View::default();
        let c0 = d.center(0);
        view.sel = Some(crate::app::RectSel {
            anchor: (c0.0 as i32, c0.1 as i32),
            end: view.cursor,
            confirmed: false,
        });
        set_at(&d, &mut view, 1);
        let p = panels(&d, area, &view);
        assert_eq!(
            selected(&d, &p, &view),
            vec![0, 1],
            "矩形里只有锚点与光标,下面那个不在里头"
        );
        let mut buf = Buffer::empty(area);
        view.pulse = 1.0; // 固定到红,便于断言
        render(&mut buf, area, &d, &view);
        for i in [0usize, 1] {
            let (x, y) = p
                .proj
                .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
            assert_eq!(
                buf[(x as u16, y as u16)].fg,
                Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
                "选中的 box {i} 该闪红"
            );
            assert!(
                !buf[(x as u16, y as u16)]
                    .modifier
                    .contains(Modifier::REVERSED),
                "选区不再用反显"
            );
        }
        let (x, y) = p.proj.cell((20.0, 10.0));
        assert_ne!(
            buf[(x as u16, y as u16)].fg,
            Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2),
            "矩形外的框不该闪"
        );
    }

    /// 只画图的入口:没有图例、没有状态行,zoom 参数生效
    #[test]
    fn 只画图不带图例状态行() {
        let d = doc_of(
            vec![grp("g0", "主线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (10, 0), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (10, 1), "b")],
            vec![edg("e1", "a", "b", Dir::None, "g0")],
        );
        let wide = Rect::new(0, 0, 200, 50);
        let mut view = View::default();
        view.note = Some("提示".to_string());
        let mut buf = Buffer::empty(wide);
        let map = render_map(&mut buf, wide, &d, &view, 1.0);
        assert_eq!(
            (map.x, map.y, map.width, map.height),
            (wide.x, wide.y, wide.width, wide.height),
            "没图例没状态行时地图区就是整块"
        );
        let text = screen(&buf).join("\n");
        for gone in ["分组", "有向边", "box=", "提示", "VIEW"] {
            assert!(!text.contains(gone), "导出地图不该有 {gone:?}:\n{text}");
        }
        // zoom 翻倍,两个节点在屏幕上离得也翻倍
        let gap = |zoom: f64| {
            let mut buf = Buffer::empty(wide);
            render_map(&mut buf, wide, &d, &view, zoom);
            let mut pts = Vec::new();
            for y in 0..wide.height {
                for x in 0..wide.width {
                    if matches!(buf[(x, y)].symbol(), "○" | "●") {
                        pts.push(x as i32);
                    }
                }
            }
            pts.sort_unstable();
            *pts.last().unwrap() - *pts.first().unwrap()
        };
        assert_eq!(gap(1.0), 10, "zoom 1 时按单元格一格一格");
        assert_eq!(gap(2.0), 20, "zoom 2 时距离翻倍");
    }

    /// 状态行只在 1:1 标 VIEW,缩放后标倍数;空闲时只报当前 box 的文字
    #[test]
    fn 状态行区分查看与缩放() {
        let d = sample();
        let area = Rect::new(0, 0, 200, 50);
        let line_of = |view: &View| {
            let mut buf = Buffer::empty(area);
            render(&mut buf, area, &d, view);
            last_line(&buf)
        };
        let mut idle_view = View::default();
        set_at(&d, &mut idle_view, 0);
        let idle = line_of(&idle_view);
        assert!(idle.starts_with("VIEW"), "空闲时该标 VIEW:{idle}");
        assert!(idle.contains("中央"), "空闲时该报当前 box 的文字:{idle}");
        let mut view = View::default();
        view.zoom = 2.0;
        let zoomed = line_of(&view);
        assert!(zoomed.starts_with("VIEW x2"), "缩放后该标倍数:{zoomed}");
        assert_ne!(idle, zoomed);
    }

    /// 缩放只跟 zoom 走:proj.scale = 1 / zoom,窗口大小不改变它
    #[test]
    fn 缩放不随窗口变() {
        let d = points();
        let a = Rect::new(0, 0, 100, 30);
        let b = Rect::new(0, 0, 200, 60);
        let v = View::default();
        let pa = panels(&d, a, &v);
        let pb = panels(&d, b, &v);
        assert_eq!(pa.proj.scale, pb.proj.scale, "缩放不随窗口变");
        assert_eq!(pa.proj.scale, 1.0, "默认一份单元格占一列");
        let q = (10.0, 0.0);
        let ca = pa.proj.cell(q);
        let cb = pb.proj.cell(q);
        let ma = (
            pa.map.x as i32 + pa.map.width as i32 / 2,
            pa.map.y as i32 + pa.map.height as i32 / 2,
        );
        let mb = (
            pb.map.x as i32 + pb.map.width as i32 / 2,
            pb.map.y as i32 + pb.map.height as i32 / 2,
        );
        assert_eq!(
            (ca.0 - ma.0, ca.1 - ma.1),
            (cb.0 - mb.0, cb.1 - mb.1),
            "同一坐标落在同量级的位置"
        );
        for (zoom, want) in [(2.0, 0.5), (0.5, 2.0)] {
            let mut vz = View::default();
            vz.zoom = zoom;
            let pz = panels(&d, a, &vz);
            assert_eq!(pz.proj.scale, want, "zoom {zoom}");
        }
    }

    /// 图例:有名字的分组画一行色块 + 名字,再说明三种字形;没名字的分组不画
    #[test]
    fn 图例画分组名与字形说明() {
        let d = doc_of(
            vec![grp("g0", "主线", (255, 0, 0)), grp("g1", "", (0, 255, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (10, 0), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "甲", (0, 1), "a"), txt("tb", "乙", (10, 1), "b")],
            vec![edg("e1", "a", "b", Dir::None, "g0")],
        );
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        let mut view = View::default();
        view.legend = true;
        let p = render(&mut buf, area, &d, &view);
        assert!(p.legend.width > 0, "这么宽该放得下图例");
        let legend = (p.legend.y..p.legend.y + p.legend.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = p.legend.x;
                while x < p.legend.x + p.legend.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += width(sym).max(1) as u16;
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(legend.contains("分组"), "图例该有标题:\n{legend}");
        assert!(legend.contains("主线"), "有名字的分组要画出来:\n{legend}");
        assert!(
            legend.contains("○ 节点") && legend.contains("◎ 枢纽"),
            "{legend}"
        );
        assert!(legend.contains("> 有向边"), "{legend}");
        let bars = (p.legend.y..p.legend.y + p.legend.height)
            .flat_map(|y| (p.legend.x..p.legend.x + p.legend.width).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(x, y)].symbol() == "─")
            .count();
        assert_eq!(bars, 2, "只有有名字的那一组才画色块");
    }

    /// 文档自带的图例:标题 + 逐行,颜色来自分组或显式色,`glyph` 画前缀示意
    #[test]
    fn 图例条目来自文档() {
        let mut d = doc_of(
            vec![grp("g0", "主线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (10, 0), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "甲", (0, 1), "a"), txt("tb", "乙", (10, 1), "b")],
            vec![edg("e1", "a", "b", Dir::None, "g0")],
        );
        d.legend = crate::document::Legend {
            title: Some("转移".to_string()),
            entries: vec![
                crate::document::LegendEntry {
                    text: "借分组色".to_string(),
                    group: Some("g0".to_string()),
                    color: None,
                    glyph: None,
                },
                crate::document::LegendEntry {
                    text: "显式色".to_string(),
                    group: Some("g0".to_string()),
                    color: Some((0x22, 0xb4, 0x6a)),
                    glyph: Some(crate::document::LegendGlyph::Dashed),
                },
                crate::document::LegendEntry {
                    text: String::new(),
                    group: None,
                    color: None,
                    glyph: None,
                },
            ],
        };
        let area = Rect::new(0, 0, 120, 24);
        let mut view = View::default();
        view.legend = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);

        assert!(p.legend.width > 0);
        let rows: Vec<String> = (p.legend.y..p.legend.y + p.legend.height)
            .map(|y| {
                (p.legend.x..p.legend.x + p.legend.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect();
        let text = rows.join("\n");
        // 宽字后面跟着续格,比对前先把空白去掉
        let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("转移"), "标题该来自文档:{text}");
        assert!(flat.contains("借分组色"), "{text}");
        assert!(flat.contains("╌╌显式色"), "glyph 前缀没画:{text}");
        assert!(
            !flat.contains("○节点") && !flat.contains("◎枢纽"),
            "有条目时不该再画派生的字形说明:{text}"
        );
        // 借分组色的那行用的是分组色,显式色那行用的是自己给的颜色
        let find = |needle: &str| -> (u16, u16) {
            for y in p.legend.y..p.legend.y + p.legend.height {
                for x in p.legend.x..p.legend.x + p.legend.width {
                    if buf[(x, y)].symbol() == needle {
                        return (x, y);
                    }
                }
            }
            panic!("图例里找不到 {needle:?}:\n{text}");
        };
        let (x, y) = find("借");
        assert_eq!(buf[(x, y)].fg, Color::Rgb(255, 0, 0), "该借分组色");
        let (x, y) = find("显");
        assert_eq!(
            buf[(x, y)].fg,
            Color::Rgb(0x22, 0xb4, 0x6a),
            "color 赢过 group"
        );
    }

    /// 图例太窄:整行截断补 `...`,不铺到地图上
    #[test]
    fn 图例太窄会截断() {
        let mut d = doc_of(
            vec![grp("g0", "主线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![bx("a", (0, 0), (1, 1), BoxStyle::Point)],
            vec![txt("ta", "甲", (0, 1), "a")],
            Vec::new(),
        );
        d.legend.entries = vec![crate::document::LegendEntry {
            text: "很长很长很长很长很长很长很长很长很长的说明".to_string(),
            group: None,
            color: None,
            glyph: None,
        }];
        let area = Rect::new(0, 0, 80, 24);
        let mut view = View::default();
        view.legend = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        assert!(p.legend.width > 0, "80 列还放得下");
        // 图例竖直居中:在所有行里找那一行
        let mut found = false;
        for y in p.legend.y..p.legend.y + p.legend.height {
            let row: String = (p.legend.x..p.legend.x + p.legend.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if row.contains("...") {
                // 宽字后面跟着续格:去掉空白再数,避免重复计宽
                let flat: String = row.chars().filter(|c| *c != ' ').collect();
                assert!(flat.ends_with("..."), "{row:?}");
                assert!(!flat.contains("说明"), "最后一截该被截掉:{row:?}");
                found = true;
            }
        }
        assert!(found, "超宽的行该截断补省略号");
    }

    /// 窗口太窄:图例整块收起,地图不被压到不可读
    #[test]
    fn 窄窗口图例收起() {
        let mut d = doc_of(
            vec![grp("g0", "主线", (255, 0, 0))],
            vec![lyr("", 0)],
            vec![bx("a", (0, 0), (1, 1), BoxStyle::Point)],
            vec![txt("ta", "甲", (0, 1), "a")],
            Vec::new(),
        );
        d.legend.entries = vec![crate::document::LegendEntry {
            text: "一个挺长的图例行".to_string(),
            group: None,
            color: None,
            glyph: None,
        }];
        let area = Rect::new(0, 0, 40, 20);
        let mut view = View::default();
        view.legend = true;
        let p = panels(&d, area, &view);
        assert_eq!(p.legend.width, 0, "地图连 20 列都留不下时图例要收起");
        assert_eq!(p.map.width, area.width);
    }

    /// 同一对节点上的两条边:命中表里各有自己的格子,点击轮换也认得出两条
    #[test]
    fn 互逆两条边各自记格子() {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (6, 3), BoxStyle::Single),
                bx("b", (14, 0), (6, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (1, 1), "a"), txt("tb", "b", (15, 1), "b")],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "b", "a", Dir::Forward, "g0"),
            ],
        );
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        let (_p, hit) = render_hit(&mut buf, area, &d, &View::default(), &FrameHit::default());
        let c1 = hit.edge_cells.get(&0).cloned().unwrap_or_default();
        let c2 = hit.edge_cells.get(&1).cloned().unwrap_or_default();
        assert!(!c1.is_empty() && !c2.is_empty(), "两条边都要有格子");
        assert_ne!(c1, c2, "两条边不能完全重合");
        // 同一格上有两条候选时,`E` 才有得轮换
        let shared = c1.iter().find(|c| c2.contains(c));
        if let Some(c) = shared {
            let list = &hit.candidates[c];
            assert!(
                list.contains(&0) && list.contains(&1),
                "候选该有两条:{list:?}"
            );
        }
    }

    /// 点节点之间的四条边(四个分组):各占一条平行线,箭头各指各的目标端
    #[test]
    fn 点节点之间多条边各占一条线() {
        let d = doc_of(
            vec![
                grp("g0", "一", (76, 142, 248)),
                grp("g1", "二", (239, 75, 75)),
                grp("g2", "三", (34, 180, 106)),
                grp("g3", "四", (245, 158, 11)),
            ],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 5), (1, 1), BoxStyle::Point),
                bx("b", (20, 5), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 6), "a"), txt("tb", "b", (20, 6), "b")],
            vec![
                edg("e1", "a", "b", Dir::None, "g0"),
                edg("e2", "a", "b", Dir::None, "g1"),
                edg("e3", "a", "b", Dir::None, "g2"),
                edg("e4", "a", "b", Dir::None, "g3"),
            ],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ax, ay) = p.proj.cell((0.0, 5.0));
        let (bx, _) = p.proj.cell((20.0, 5.0));
        let mid = (ax + bx) / 2;
        // 四条线分别落在四行上
        let rows: Vec<i32> = (-3..=3)
            .map(|dy| ay + dy)
            .filter(|y| (ax + 2..bx - 2).all(|x| buf[(x as u16, *y as u16)].symbol() == "─"))
            .collect();
        assert_eq!(rows.len(), 4, "四条边该占四条不同的行:{rows:?}");
        assert!(rows.contains(&ay), "其中一条走中间那条直线");
        assert_eq!(buf[(mid as u16, ay as u16)].symbol(), "─");
    }

    /// E 选中一条边:另一头的邻居节点也一起高亮(光标那一头本来就在闪)
    #[test]
    fn 选中边时邻居节点一起高亮() {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
                bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
                bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
            ],
            vec![
                txt("ta", "甲", (1, 1), "a"),
                txt("tb", "乙", (31, 1), "b"),
                txt("tc", "丙", (61, 1), "c"),
            ],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let area = Rect::new(0, 0, 90, 16);
        let mut view = View::default();
        view.legend = false;
        view.pulse = 0.0;
        view.pick = Pick::Edge(0);
        view.cursor = (1, 1); // 光标在 甲 上
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let p = panels(&d, area, &view);
        let border = |i: usize| {
            let (l, t, _, _) = rect_of(&d, i, &p.proj);
            buf[(l as u16, t as u16)].fg
        };
        // 换个呼吸相位再画一次:邻居跟着变,没连着的框不变
        let mut buf2 = Buffer::empty(area);
        view.pulse = 0.5;
        render(&mut buf2, area, &d, &view);
        let border2 = |i: usize| {
            let (l, t, _, _) = rect_of(&d, i, &p.proj);
            buf2[(l as u16, t as u16)].fg
        };
        assert_ne!(border(1), border2(1), "邻居该跟着呼吸");
        assert_eq!(border(2), border2(2), "没连着的框颜色不变");
    }

    /// 只有文字的节点:几条边各走各的进出点(不会全挤在同一格)
    #[test]
    fn 文字节点上的多条边各有端口() {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (10, 5), (8, 1), BoxStyle::None),
                bx("b", (10, 0), (1, 1), BoxStyle::Point),
                bx("c", (10, 10), (1, 1), BoxStyle::Point),
                bx("d", (25, 5), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("ta", "ABCDEFGH", (10, 5), "a"),
                txt("tb", "b", (10, 1), "b"),
                txt("tc", "c", (10, 11), "c"),
                txt("td", "d", (25, 6), "d"),
            ],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "a", "c", Dir::Forward, "g0"),
                edg("e3", "a", "d", Dir::Forward, "g0"),
            ],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        view.legend = false;
        let mut buf = Buffer::empty(area);
        let (_, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        // a 在屏幕上的矩形:三条边落在里面的那一格就是各自的端口
        let (ax, ay, aw, ah) = box_rects(&d, area, &view)[0];
        let inside = |c: (i32, i32)| c.0 >= ax && c.0 < ax + aw && c.1 >= ay && c.1 < ay + ah;
        let mut ports: Vec<(i32, i32)> = Vec::new();
        for e in 0..3 {
            let cells: Vec<(i32, i32)> = hit
                .edge_cells
                .get(&e)
                .map(|v| v.iter().copied().filter(|&c| inside(c)).collect())
                .unwrap_or_default();
            assert_eq!(cells.len(), 1, "e{e} 该正好占一个端口格:{cells:?}");
            ports.push(cells[0]);
        }
        assert_eq!(
            ports.iter().collect::<HashSet<_>>().len(),
            3,
            "三条边不能抢同一个进出点:{ports:?}"
        );
    }

    /// E 模式:邻居与相连的边正常画,其余压暗
    #[test]
    fn 邻居高亮时别的压暗() {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (8, 3), BoxStyle::Rounded),
                bx("b", (30, 0), (8, 3), BoxStyle::Rounded),
                bx("c", (60, 0), (8, 3), BoxStyle::Rounded),
            ],
            vec![
                txt("ta", "甲", (1, 1), "a"),
                txt("tb", "乙", (31, 1), "b"),
                txt("tc", "丙", (61, 1), "c"),
            ],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "b", "c", Dir::Forward, "g0"),
            ],
        );
        let area = Rect::new(0, 0, 90, 16);
        let mut view = View::default();
        view.legend = false;
        view.ego = Some(0); // 只亮 a 和它的邻居 b
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let p = panels(&d, area, &view);
        let border = |i: usize| {
            let (l, t, _, _) = rect_of(&d, i, &p.proj);
            buf[(l as u16, t as u16)].fg
        };
        let dim = dim_style().fg.unwrap();
        assert_ne!(border(0), dim, "自己不能被压暗");
        assert_ne!(border(1), dim, "邻居不能被压暗");
        assert_eq!(border(2), dim, "邻居之外的框该压暗");
        // 压暗的框里的字也一样
        let (cx, cy) = p.proj.cell((61.0, 1.0));
        assert_eq!(buf[(cx as u16, cy as u16)].fg, dim, "丙 该压暗");
    }

    /// 两端都只有一格(点节点 / 一格的文字节点)时,多条边不能叠成一条:
    /// 没有边框格可以顺延,就靠 lane 错开,每条都点得到
    #[test]
    fn 一格节点上多条边不重叠() {
        let d = doc_of(
            vec![grp("g0", "线", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 5), (1, 1), BoxStyle::None),
                bx("b", (20, 5), (1, 1), BoxStyle::None),
            ],
            vec![txt("ta", "A", (0, 5), "a"), txt("tb", "B", (20, 5), "b")],
            vec![
                edg("e1", "a", "b", Dir::Forward, "g0"),
                edg("e2", "b", "a", Dir::Forward, "g0"),
            ],
        );
        let area = Rect::new(0, 0, 40, 16);
        let mut view = View::default();
        view.legend = false;
        let mut buf = Buffer::empty(area);
        let (_p, hit) = render_hit(&mut buf, area, &d, &view, &FrameHit::default());
        for e in 0..2 {
            assert!(
                !hit.edge_cells
                    .get(&e)
                    .cloned()
                    .unwrap_or_default()
                    .is_empty(),
                "e{e} 该有自己的格子"
            );
        }
        let p = panels(&d, area, &view);
        let (mid, ay) = p.proj.cell((10.0, 5.0));
        // 两条线在中间列上各占一行,各点得到
        let lower = hit.edge_at((mid, ay));
        let upper = hit.edge_at((mid, ay - 1));
        assert!(lower.is_some() && upper.is_some(), "上下两行都该有线");
        assert_ne!(lower, upper, "两行不能是同一条边");
    }

    /// 搜索行与命令行共用底行
    #[test]
    fn 搜索行报命中数() {
        let d = points();
        let area = Rect::new(0, 0, 200, 50);
        let _ = &d;
        let mut view = View::default();
        view.query = Some("甲".to_string());
        view.hits = vec![0, 1, 2];
        view.hit_at = 1;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let last = last_line(&buf);
        assert!(last.contains("/甲"), "该显示搜索词:{last}");
        assert!(last.contains("3 处匹配"), "该显示命中数:{last}");
        assert!(last.contains("乙"), "该显示当前命中:{last}");
        // 纯数字:分组号
        let mut view = View::default();
        view.query = Some("1".to_string());
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let last = last_line(&buf);
        assert!(last.contains("没有这个分组"), "样例没有编号分组:{last}");
        // ":" 命令行独占状态行:从最左边起,盖掉模式标记与节点名
        let mut view = View::default();
        view.cmd = Some("lang en".to_string());
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let last = last_line(&buf);
        assert!(last.starts_with(":lang en"), "命令行该在行首:{last}");
        assert!(!last.contains("VIEW"), "命令行要盖住模式标记:{last}");
        let mut view = View::default();
        view.note = Some("换了布局".to_string());
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        assert!(last_line(&buf).contains("换了布局"));
    }

    /// 帮助浮层:画的是键位速查,不是地图;英文那份不许留中文
    #[test]
    fn 帮助浮层盖住地图() {
        let d = points();
        let area = Rect::new(0, 0, 90, 30);
        let mut view = View::default();
        view.help = true;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let text = screen(&buf).join("\n");
        assert!(text.contains("键位速查"), "该有标题:\n{text}");
        assert!(text.contains("任意键关掉"), "该提示怎么关:\n{text}");
        assert!(text.contains("半屏往下"), "该列出半屏跳:\n{text}");
        assert!(!text.contains('○'), "浮层打开时不该还在画地图:\n{text}");
        let mut view = View::default();
        view.help = true;
        view.lang = Lang::En;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &d, &view);
        let text = screen(&buf).join("\n");
        assert!(text.contains("key reference"), "{text}");
        // 浮层边框是制表符(非 ASCII 但不算界面文字),所以只查有没有 CJK
        let cjk = |c: char| {
            matches!(
                c as u32,
                0x3000..=0x303F | 0x4E00..=0x9FFF | 0xFF01..=0xFF5E
            )
        };
        assert!(!text.chars().any(cjk), "英文速查里不该有中文:\n{text}");
    }

    /// 点哪个框就选哪个;状态行和图例上的点击不算
    #[test]
    fn 点框矩形内命中() {
        let d = points();
        let area = Rect::new(0, 0, 60, 24);
        let p = panels(&d, area, &View::default());
        for i in 0..d.boxes.len() {
            let (x, y) = p
                .proj
                .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
            assert_eq!(
                node_at(&d, &p, area, x as u16, y as u16),
                Some(i),
                "点在框的格子上该选它"
            );
        }
        assert_eq!(
            node_at(&d, &p, area, 5, area.height - 1),
            None,
            "状态行上的点击该忽略"
        );
        // 空白处(不在任何框矩形里)没有命中
        let (x, y) = p.proj.cell((5.0, 5.0));
        assert_eq!(node_at(&d, &p, area, x as u16, y as u16), None);
        // 图例上的点击也不算
        let wide = Rect::new(0, 0, 200, 50);
        let mut lv = View::default();
        lv.legend = true;
        let pw = panels(&d, wide, &lv);
        assert!(pw.legend.width > 0);
        assert_eq!(
            node_at(&d, &pw, wide, pw.legend.x, pw.legend.y + 1),
            None,
            "图例上的点击该忽略"
        );
    }

    /// 滚轮平移按屏幕格算:挪几格,中心就落在原来那一格加几格上
    #[test]
    fn 平移按屏幕格算() {
        let d = points();
        let p = panels(&d, Rect::new(0, 0, 60, 24), &View::default());
        let from = (5.0, 3.0);
        let (c0, r0) = p.proj.cell(from);
        let to = pan_center(&p.proj, from, 3, -2);
        assert_eq!(p.proj.cell(to), (c0 + 3, r0 - 2));
    }

    /// 窗口小到几乎没有:照画不炸,状态行与浮层都不越界
    #[test]
    fn 极小画面也不炸() {
        let d = points();
        for (w, h) in [
            (0u16, 0u16),
            (1, 1),
            (1, 5),
            (5, 1),
            (3, 3),
            (8, 4),
            (12, 3),
        ] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            render(&mut buf, area, &d, &View::default());
            let mut view = View::default();
            view.query = Some("甲".to_string());
            view.hits = vec![0];
            render(&mut buf, area, &d, &view);
            let mut view = View::default();
            view.help = true;
            render(&mut buf, area, &d, &view);
            let p = panels(&d, area, &View::default());
            let hit = node_at(&d, &p, area, 0, 0);
            if w == 0 || h <= 1 {
                assert_eq!(hit, None, "{w}x{h} 上没有可点的框");
            }
        }
    }

    /// 铺满整个区域:缩放算出来之后整幅文档塞得下
    #[test]
    fn 铺满时刚好装下整幅文档() {
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (100, 40), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (100, 41), "b")],
            vec![],
        );
        let zoom = zoom_for_fit(&d, 60, 24, false, true);
        assert!(zoom > 0.0 && zoom.is_finite());
        // zoom 越小看得越全;区域越大一样大小的图能放得更大
        assert!(zoom_for_fit(&d, 200, 60, false, true) > zoom);
    }

    /// 手工折点直接是单元格:线段经过折点,并且离开直线
    #[test]
    fn 手工折点改变走线() {
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (20, 0), (1, 1), BoxStyle::Point),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "b", (20, 1), "b")],
            vec![{
                let mut e = edg("e1", "a", "b", Dir::None, "g0");
                e.via = vec![(10, 10)];
                e
            }],
        );
        let area = Rect::new(0, 0, 60, 30);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let v = p.proj.cell((10.0, 10.0));
        assert_ne!(
            buf[(v.0 as u16, v.1 as u16)].symbol(),
            " ",
            "线段该经过折点"
        );
        // 折点在直线之外:直线下面也得有笔画,说明不是直接横穿
        let direct = p.proj.cell((10.0, 0.0));
        let mut detour = false;
        for y in (direct.1 + 1)..(p.map.y + p.map.height) as i32 {
            for x in p.map.x as i32..(p.map.x + p.map.width) as i32 {
                let sym = buf[(x as u16, y as u16)].symbol();
                if matches!(sym, "─" | "│" | "╭" | "╮" | "╰" | "╯") {
                    detour = true;
                    break;
                }
            }
            if detour {
                break;
            }
        }
        assert!(detour, "折点那一段该离开直线");
    }

    /// 标签里的 `**粗**` 不占额外宽度
    #[test]
    fn 标签标记不占宽度() {
        assert_eq!(width("**粗**"), width("粗"));
        assert_eq!(width("/斜/"), width("斜"));
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![bx("a", (0, 0), (4, 3), BoxStyle::Single)],
            vec![txt("ta", "**ab**", (1, 1), "a")],
            vec![],
        );
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (x, y) = p.proj.cell((0.0, 0.0));
        assert_eq!(buf[(x as u16, y as u16)].symbol(), "┌");
        let row: String = (x..x + 4)
            .map(|dx| buf[(dx as u16, (y + 1) as u16)].symbol().to_string())
            .collect();
        assert!(row.contains("ab"), "标记不算宽度:{row}");
    }

    /// 同一个分组里两个方向都有边:两条各自占一条线,箭头各指各的目标端
    #[test]
    fn 双向边分成两条线() {
        let d = doc_of(
            vec![grp("g0", "依赖", (76, 142, 248))],
            vec![lyr("", 0)],
            vec![
                bx("u", (0, 1), (1, 1), BoxStyle::Point),
                bx("v", (10, 1), (1, 1), BoxStyle::Point),
            ],
            vec![txt("tu", "u", (0, 0), "u"), txt("tv", "v", (10, 0), "v")],
            vec![
                edg("e1", "u", "v", Dir::Forward, "g0"),
                edg("e2", "v", "u", Dir::Forward, "g0"),
            ],
        );
        assert_eq!(d.edges.len(), 2, "两个方向是两条独立的边");
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &View::default());
        let (ux, y) = p.proj.cell((0.0, 1.0));
        let (vx, _) = p.proj.cell((10.0, 1.0));
        assert_eq!(buf[((vx - 1) as u16, y as u16)].symbol(), ">", "去程指向东");
        // 回程走上面那条平行线,最后竖着进 u:箭头落在 u 头顶那一格
        assert_eq!(
            buf[(ux as u16, (y - 1) as u16)].symbol(),
            "v",
            "回程从上面指向 u"
        );
        // 两条边不能落在同一格上:去程那一行在 u 右边是横线
        assert_eq!(buf[((ux + 1) as u16, y as u16)].symbol(), "─");
    }

    /// 非节点处的交叉:交叉格照"先画的那一组"自己的形状画(直通的那一笔),
    /// 另一组在这里断一格 —— 不能画成 ┼,不然看起来像这里有枢纽.
    /// 这个形状**不跟光标走**:挪光标只换颜色
    #[test]
    fn 交叉格只画先到那条线的形状() {
        let d = doc_of(
            vec![
                grp("gx", "东西", (255, 0, 0)),
                grp("gy", "南北", (0, 255, 0)),
            ],
            vec![lyr("", 0)],
            vec![
                bx("a1", (0, 5), (1, 1), BoxStyle::Point),
                bx("a2", (20, 5), (1, 1), BoxStyle::Point),
                bx("b1", (10, 0), (1, 1), BoxStyle::Point),
                bx("b2", (10, 10), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("t1", "1", (0, 6), "a1"),
                txt("t2", "2", (20, 6), "a2"),
                txt("t3", "3", (11, 0), "b1"),
                txt("t4", "4", (11, 10), "b2"),
            ],
            vec![
                edg("ex", "a1", "a2", Dir::None, "gx"),
                edg("ey", "b1", "b2", Dir::None, "gy"),
            ],
        );
        let area = Rect::new(0, 0, 60, 24);
        let cross = |cursor: usize| {
            let mut view = View::default();
            set_at(&d, &mut view, cursor);
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &d, &view);
            let (x, y) = p.proj.cell((10.0, 5.0));
            let cell = &buf[(x as u16, y as u16)];
            (cell.symbol().to_string(), cell.fg)
        };
        let (s0, c0) = cross(0);
        let (s1, c1) = cross(2);
        assert_eq!(s0, "─", "交叉格该是先画那一组的直笔画");
        assert_eq!(s0, s1, "挪光标不该改交叉格的字形");
        assert_eq!(c0, Color::Rgb(255, 0, 0), "光标在东西组上时用它自己的颜色");
        assert_eq!(c1, Color::Rgb(0, 255, 0), "光标在南北组上时换成它的颜色");
    }

    /// 光标那个节点是实心的,并按 view.pulse 在亮白和红之间插值
    #[test]
    fn 光标节点实心并按脉冲闪红() {
        let d = points();
        let area = Rect::new(0, 0, 60, 24);
        let p = panels(&d, area, &View::default());
        let (x, y) = p.proj.cell((0.0, 0.0));
        let cell_of = |pulse: f64| {
            let mut view = View::default();
            view.pulse = pulse;
            let mut buf = Buffer::empty(area);
            render(&mut buf, area, &d, &view);
            let cell = &buf[(x as u16, y as u16)];
            (cell.fg, cell.symbol().to_string())
        };
        let (soft, ch) = cell_of(0.0);
        assert_eq!(ch, "●", "普通节点该画成实心");
        assert_eq!(soft, Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2));
        let (red, _) = cell_of(1.0);
        assert_eq!(red, Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2));
        let chan = |c: Color| match c {
            Color::Rgb(r, _, _) => r as i32,
            _ => -1,
        };
        let (half, _) = cell_of(0.5);
        assert!(
            chan(half) > chan(soft) && chan(half) < chan(red),
            "中间的档位该在两头之间"
        );
    }

    /// 点节点的名标注只写在空位上:标出来之后,节点自己那一格还是节点字形
    #[test]
    fn 点节点标注不压节点() {
        let d = doc_of(
            vec![grp("g0", "网", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (20, 0), (1, 1), BoxStyle::Point),
                bx("c", (0, 10), (1, 1), BoxStyle::Point),
                bx("d", (20, 10), (1, 1), BoxStyle::Point),
            ],
            vec![
                txt("ta", "很长的名字甲", (0, 0), "a"),
                txt("tb", "很长的名字乙", (20, 0), "b"),
                txt("tc", "很长的名字丙", (0, 10), "c"),
                txt("td", "很长的名字丁", (20, 10), "d"),
            ],
            vec![],
        );
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        view.labels = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        for i in 0..d.boxes.len() {
            let (x, y) = p
                .proj
                .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
            let sym = buf[(x as u16, y as u16)].symbol();
            assert!(
                matches!(sym, "○" | "◎" | "●" | "◉"),
                "节点 {i} 那一格被标成了 {sym:?}"
            );
        }
        let text = screen(&buf).join("\n");
        assert!(
            text.contains("很长的名字"),
            "地方够宽,总得标出名字来:\n{text}"
        );
    }

    /// 铺满整幅文档之后,每个框都在地图区里
    #[test]
    fn 铺满后每个框都在画面里() {
        let d = doc_of(
            vec![grp("g0", "线", (120, 200, 255))],
            vec![lyr("", 0)],
            vec![
                bx("a", (0, 0), (1, 1), BoxStyle::Point),
                bx("b", (100, 40), (4, 3), BoxStyle::Single),
            ],
            vec![txt("ta", "a", (0, 1), "a"), txt("tb", "bb", (101, 41), "b")],
            vec![edg("e1", "a", "b", Dir::Forward, "g0")],
        );
        let area = Rect::new(0, 0, 60, 24);
        let mut view = View::default();
        view.zoom = zoom_for_fit(&d, area.width, area.height, view.legend, view.notes);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &d, &view);
        for i in 0..d.boxes.len() {
            let (x, y) = p
                .proj
                .cell((d.boxes[i].at.0 as f64, d.boxes[i].at.1 as f64));
            assert!(
                x >= p.map.x as i32
                    && x < (p.map.x + p.map.width) as i32
                    && y >= p.map.y as i32
                    && y < (p.map.y + p.map.height) as i32,
                "框 {i} 出了画面:({x}, {y}) / {:?}",
                p.map
            );
        }
    }
}
