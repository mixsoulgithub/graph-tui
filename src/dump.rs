//! One frame for the picture (`--export -`) and for image export: picture only, 1:1,
//! centred, padding on all sides.
//!
//! Why "probe once, then crop and copy" instead of computing a bbox and redrawing:
//! label avoidance, routing and T-junctions/arrowheads are all computed inside
//! `render_map`, and recomputing them here would easily miss cases; re-rendering with
//! padding could also move auto-placed labels. So probe one frame, measure the real
//! content rect and copy that block cell by cell into the final Buffer, which leaves
//! the layout identical.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::app::View;
use crate::document::Document;

/// Columns of padding on each side
pub const PAD_X: u16 = 2;
/// Rows of padding above and below
pub const PAD_Y: u16 = 1;
/// Extra cells the probe buffer gets beyond the content (room for edges, arrowheads and
/// labels)
const SLACK: i32 = 8;
/// The probe may be enlarged at most once
const MAX_RETRY: usize = 1;

/// One frame: the final buffer plus the content origin inside the probe buffer. Also
/// carries the probe frame's hit table and box rects (in probe/canvas coords), which the
/// JSON output needs for coordinate conversion.
#[derive(Clone, Debug)]
pub struct Frame {
    pub buf: Buffer,
    pub clipped: bool,
    /// Top-left corner of the content rect inside the probe buffer
    pub origin: (i32, i32),
    /// Padding offset from probe coords to final coords (0 for a fixed size)
    pub pad: (i32, i32),
    pub hit: crate::render::FrameHit,
    pub rects: Vec<(i32, i32, i32, i32)>,
}

impl Frame {
    /// Probe coords -> final buffer coords
    pub fn map(&self, p: (i32, i32)) -> (i32, i32) {
        (
            p.0 - self.origin.0 + self.pad.0,
            p.1 - self.origin.1 + self.pad.1,
        )
    }
}

/// Output rectangle: which canvas cell the origin lands on, plus the output size. Each
/// field can be auto on its own
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Area {
    /// Canvas cell at the top-left of the picture; None = centre this axis on the content
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Output size; None = content bbox plus padding along this axis
    pub w: Option<u16>,
    pub h: Option<u16>,
}

impl Area {
    /// All auto: centred on the content, sized by the content
    pub fn auto() -> Area {
        Area::default()
    }

    pub fn is_auto(&self) -> bool {
        self.x.is_none() && self.y.is_none() && self.w.is_none() && self.h.is_none()
    }
}

/// Produce a frame; an area with no width/height is sized from the content.
/// `view` contributes only switches such as labels / notes: zoom stays 1:1 and the centre
/// follows the content
pub fn frame(doc: &Document, node_labels: bool, area: Area, extra: &View) -> Frame {
    let (x0, y0, x1, y1) = doc.bbox_shown(extra.edge_labels);
    let center = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
    let view = View {
        // auto sizing is always 1:1; the caller's zoom applies only with an explicit size
        zoom: if area.w.is_none() && area.h.is_none() {
            1.0
        } else if extra.zoom.is_finite() && extra.zoom > 0.0 {
            extra.zoom
        } else {
            1.0
        },
        // overridden below when an origin is given: start from content-centred
        center: Some(center),
        node_labels,
        // edge labels are part of the picture, so follow the caller's switch
        edge_labels: extra.edge_labels,
        lang: extra.lang,
        drawn: false,
        cursor: extra.cursor,
        focus: extra.focus.clone(),
        layer: extra.layer.clone(),
        arrow_style: extra.arrow_style,
        // a frame must stay readable: draw text at any zoom, no LOD
        lod: false,
        // glyph set follows the caller (`--ascii`)
        glyphs: extra.glyphs,
        // default ink (edge_color / text_color from the config file)
        edge_ink: extra.edge_ink,
        text_ink: extra.text_ink,
        ..Default::default()
    };
    // auto parts follow the content: size is bbox + padding, origin centres the content
    if area.is_auto() {
        return auto(doc, &view);
    }
    let need = auto(doc, &view);
    let w = area.w.unwrap_or(need.buf.area.width).max(1);
    let h = area.h.unwrap_or(need.buf.area.height).max(1);
    // for an axis with an origin: the top-left lands on canvas (x,y), so the centre is
    // origin + half the size
    let view = View {
        center: Some((
            area.x.map_or(center.0, |x| x as f64 + w as f64 / 2.0),
            area.y.map_or(center.1, |y| y as f64 + h as f64 / 2.0),
        )),
        ..view
    };
    let out = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(out);
    let (_p, hit) = crate::render::render_picture_hit(&mut buf, out, doc, &view);
    // does it all fit? compare against the auto size just measured (with an origin the user
    // is resizing the window, so no extra warning)
    let clipped = need.buf.area.width > w || need.buf.area.height > h;
    Frame {
        buf,
        clipped,
        origin: (0, 0),
        pad: (0, 0),
        hit,
        rects: crate::render::picture_box_rects(doc, out, &view),
    }
}

/// Render one frame at a fixed size (for callers that only need the buffer)
pub fn render_at(doc: &Document, view: &View, w: u16, h: u16) -> Buffer {
    let area = Rect::new(0, 0, w.max(1), h.max(1));
    let mut buf = Buffer::empty(area);
    crate::render::render_map(&mut buf, area, doc, view, 1.0);
    buf
}

/// Auto size: probe one frame, measure the content rect, crop and add padding
fn auto(doc: &Document, view: &View) -> Frame {
    auto_map(doc, view)
}

/// Auto size for a picture: padding on all sides
fn auto_map(doc: &Document, view: &View) -> Frame {
    let (x0, y0, x1, y1) = doc.bbox_shown(view.edge_labels);
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
    // content touching the probe edge means the range is too small: enlarge once and probe
    // again; if it still touches, accept it
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
    // copy cell by cell (glyph, foreground, background, modifiers), never re-render
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
        hit,
        // a frame has no status line: rects are computed as if the whole area were the map
        rects: crate::render::picture_box_rects(doc, Rect::new(0, 0, w, h), view),
    }
}

/// Rect of the cells that hold something: glyph not a space, or fg/bg/modifier not default
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

/// Width of the widest point-node label (the probe must make room for floating labels)
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
