//! `--dump` 与图片导出用的成帧:只画图,1:1,居中,四周留白.
//!
//! 为什么用"探测一次 + 裁剪复制",而不是算包围盒后重画:
//! 标签避让、走线、T/箭头都是 `render_map` 里算的,再算一遍容易漏;
//! 而按留白重新渲染一次又可能让自动标签换位。所以探测一帧、量出真实
//! 内容矩形、把那一块逐格搬到最终 Buffer,布局在搬运前后完全一致。

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::app::View;
use crate::document::Document;

/// 左右各留几列
pub const PAD_X: u16 = 2;
/// 上下各留几行
pub const PAD_Y: u16 = 1;
/// 探测缓冲比内容多开多少格(边、箭头、标签往外的余量)
const SLACK: i32 = 8;
/// 探测范围最多扩大一次
const MAX_RETRY: usize = 1;
/// 图例与地图之间留几列
const GAP_X: u16 = 2;

/// 一帧:最终缓冲 + 内容在探测缓冲里的原点 —— 另外带上探测那一帧的命中表
/// 与框矩形(坐标都是探测/画布坐标),json 输出要靠它换算
#[derive(Clone, Debug)]
pub struct Frame {
    pub buf: Buffer,
    pub clipped: bool,
    /// 内容矩形在探测缓冲里的左上角
    pub origin: (i32, i32),
    /// 探测坐标换算成最终坐标时的留白偏移(定尺寸时是 0)
    pub pad: (i32, i32),
    /// 图例在最终缓冲里的位置(没有图例就是 0 宽)
    pub legend: Rect,
    pub hit: crate::render::FrameHit,
    pub rects: Vec<(i32, i32, i32, i32)>,
}

impl Frame {
    /// 探测坐标 → 最终缓冲坐标
    pub fn map(&self, p: (i32, i32)) -> (i32, i32) {
        (
            p.0 - self.origin.0 + self.pad.0,
            p.1 - self.origin.1 + self.pad.1,
        )
    }
}

/// 出帧.`size` 为 None 表示按内容自动定尺寸.
/// `view` 只取 labels / legend 之类的开关,缩放永远按 1:1、中心按内容算
pub fn frame(doc: &Document, labels: bool, size: Option<(u16, u16)>, extra: &View) -> Frame {
    let (x0, y0, x1, y1) = doc.bbox_shown(extra.notes);
    let center = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
    let view = View {
        // 自动尺寸一律 1:1;给了尺寸才按调用方的缩放
        zoom: if size.is_none() {
            1.0
        } else if extra.zoom.is_finite() && extra.zoom > 0.0 {
            extra.zoom
        } else {
            1.0
        },
        // 自动尺寸永远按内容居中:调用方的中心/缩放只对"给了尺寸"的意义更大
        center: if size.is_none() {
            Some(center)
        } else {
            extra.center.or(Some(center))
        },
        labels,
        // 图例与边标签都是画面的一部分,跟着调用方的开关走
        legend: extra.legend,
        notes: extra.notes,
        lang: extra.lang,
        drawn: false,
        cursor: extra.cursor,
        focus: extra.focus.clone(),
        layer: extra.layer.clone(),
        arrow_style: extra.arrow_style,
        ..Default::default()
    };
    match size {
        Some((w, h)) => {
            let area = Rect::new(0, 0, w.max(1), h.max(1));
            let mut buf = Buffer::empty(area);
            // 图例先占宽度,剩下的给地图 —— 和 TUI 的 panels 同一套分法
            let (p, hit) = crate::render::render_picture_hit(&mut buf, area, doc, &view);
            // 放不放得下:量一次自动尺寸(只多画一帧,用来决定要不要提醒)
            let need = auto(doc, &view);
            let clipped = need.buf.area.width > w || need.buf.area.height > h;
            Frame {
                buf,
                clipped,
                origin: (0, 0),
                pad: (0, 0),
                legend: p.legend,
                hit,
                rects: crate::render::picture_box_rects(doc, area, &view),
            }
        }
        None => auto(doc, &view),
    }
}

/// 固定尺寸画一帧(只要缓冲时用)
pub fn render_at(doc: &Document, view: &View, w: u16, h: u16) -> Buffer {
    let area = Rect::new(0, 0, w.max(1), h.max(1));
    let mut buf = Buffer::empty(area);
    crate::render::render_map(&mut buf, area, doc, view, 1.0);
    buf
}

/// 自动尺寸:探测一帧,量出内容矩形,裁剪 + 留白.
/// 图例是独立的一块面板:先按"没有图例"把图画好,再把它贴到右侧(间隔 2 列),
/// 于是间隙是准确的两列,不用为了排版反复重画
fn auto(doc: &Document, view: &View) -> Frame {
    let mut plain = view.clone();
    plain.legend = false;
    let map = auto_map(doc, &plain);
    let (lw, lh) = crate::render::legend_extent(doc, view);
    if lw == 0 || lh == 0 {
        return map;
    }
    // 图例最多和地图一样高:超出部分从底部裁掉
    let lh = lh.min(map.buf.area.height.max(1));
    let w = map.buf.area.width.saturating_add(GAP_X).saturating_add(lw);
    let h = map.buf.area.height.max(lh);
    let mut out = Buffer::empty(Rect::new(0, 0, w, h));
    let (mw, mh) = (map.buf.area.width, map.buf.area.height);
    for y in 0..mh {
        for x in 0..mw {
            out[(x, y)] = map.buf[(x, y)].clone();
        }
    }
    // 图例竖直居中,离地图至少 GAP_X 列
    let lx = mw + GAP_X;
    let ly = h.saturating_sub(lh) / 2;
    // 图例先画在一块自己的小缓冲上(坐标从 0 起),再贴过去
    let mut lbuf = Buffer::empty(Rect::new(0, 0, lw, lh));
    crate::render::draw_legend(&mut lbuf, 0, 0, lw, doc, view);
    for y in 0..lh {
        for x in 0..lw {
            out[(lx + x, ly + y)] = lbuf[(x, y)].clone();
        }
    }
    Frame {
        buf: out,
        clipped: map.clipped,
        origin: map.origin,
        pad: map.pad,
        legend: Rect::new(lx, ly, lw, lh),
        hit: map.hit,
        rects: map.rects,
    }
}

/// 自动尺寸(不含图例)
fn auto_map(doc: &Document, view: &View) -> Frame {
    let (x0, y0, x1, y1) = doc.bbox_shown(view.notes);
    let bw = (x1 - x0 + 1).max(1);
    let bh = (y1 - y0 + 1).max(1);
    let label = max_label_width(doc) as i32;
    let mut w = (bw + 2 * SLACK).clamp(1, 2000) as u16;
    let mut h = (bh + 2 * SLACK).clamp(1, 2000) as u16;
    let mut probe = Buffer::empty(Rect::new(0, 0, w.max(1), h.max(1)));
    let (_, mut hit) = crate::render::render_map_hit(
        &mut probe,
        Rect::new(0, 0, w.max(1), h.max(1)),
        doc,
        view,
        1.0,
    );
    let mut content = content_rect(&probe);
    let mut retries = 0;
    // 内容贴到探测边:说明范围不够,扩一次再探;还贴就接受
    while let Some((cx0, cy0, cx1, cy1)) = content {
        let touches = cx0 == 0 || cy0 == 0 || cx1 == i32::from(w) - 1 || cy1 == i32::from(h) - 1;
        if !touches || retries >= MAX_RETRY {
            break;
        }
        retries += 1;
        w = (i32::from(w) + label.max(0) + SLACK * 2).clamp(1, 2000) as u16;
        h = (i32::from(h) + SLACK * 2).clamp(1, 2000) as u16;
        let area = Rect::new(0, 0, w, h);
        probe = Buffer::empty(area);
        let (_, h2) = crate::render::render_map_hit(&mut probe, area, doc, view, 1.0);
        hit = h2;
        content = content_rect(&probe);
    }
    let (cx0, cy0, cx1, cy1) = content.unwrap_or((0, 0, 0, 0));
    let cw = (cx1 - cx0 + 1).max(1) as u16;
    let chh = (cy1 - cy0 + 1).max(1) as u16;
    let fw = cw.saturating_add(PAD_X * 2).max(1);
    let fh = chh.saturating_add(PAD_Y * 2).max(1);
    let mut out = Buffer::empty(Rect::new(0, 0, fw, fh));
    // 逐格搬:字符、前景、背景、修饰符一起,不重新绘制
    for dy in 0..chh {
        for dx in 0..cw {
            let src = (cx0 + i32::from(dx), cy0 + i32::from(dy));
            if src.0 < 0 || src.1 < 0 || src.0 >= i32::from(w) || src.1 >= i32::from(h) {
                continue;
            }
            let cell = probe[(src.0 as u16, src.1 as u16)].clone();
            let d = (PAD_X + dx, PAD_Y + dy);
            if d.0 < fw && d.1 < fh {
                out[(d.0, d.1)] = cell;
            }
        }
    }
    Frame {
        buf: out,
        clipped: false,
        origin: (cx0, cy0),
        pad: (i32::from(PAD_X), i32::from(PAD_Y)),
        legend: Rect::new(0, 0, 0, 0),
        hit,
        rects: crate::render::box_rects(doc, Rect::new(0, 0, w, h), view),
    }
}

/// 缓冲里"有东西"的矩形:字形不是空格、或前景/背景/修饰符不是默认值
fn content_rect(buf: &Buffer) -> Option<(i32, i32, i32, i32)> {
    let (w, h) = (buf.area.width, buf.area.height);
    let (mut x0, mut y0) = (i32::MAX, i32::MAX);
    let (mut x1, mut y1) = (i32::MIN, i32::MIN);
    for y in 0..h {
        for x in 0..w {
            let c = &buf[(x, y)];
            let ink = c.symbol() != " "
                || c.fg != Color::Reset
                || c.bg != Color::Reset
                || !c.modifier.is_empty();
            if ink {
                x0 = x0.min(i32::from(x));
                y0 = y0.min(i32::from(y));
                x1 = x1.max(i32::from(x));
                y1 = y1.max(i32::from(y));
            }
        }
    }
    if x0 == i32::MAX {
        None
    } else {
        Some((x0, y0, x1, y1))
    }
}

/// 最长的点节点标签宽度(探测范围要放得下浮动标签)
fn max_label_width(doc: &Document) -> usize {
    doc.boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.style.is_boxed())
        .filter_map(|(i, _)| doc.text(i))
        .map(|t| crate::text::plain_width(&t.text))
        .max()
        .unwrap_or(0)
}
