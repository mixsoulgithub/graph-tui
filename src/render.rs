//! Projection and blitting: cell coordinates in the document -> screen cells -> ratatui buffer.
//! Boxes, text and edges come from Document; zoom and pan are view state only, never geometry.

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
use crate::text::Span;

/// Display width of a line (wide glyphs count as two). `**bold**` / `/italic/` markers in labels
/// are zero-width
pub fn width(s: &str) -> usize {
    text::plain_width(s)
}

/// Display width of raw text: no `**` / `*` marker interpretation. Command line, search term and
/// prompt line are all raw text; cursor positions must use this width
pub fn raw_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Cell coordinate -> screen cell projection. Base coordinates are cells: 1 cell = 1 column x 1
/// row, x right, y down. `scale` is columns per cell (= 1 / zoom); window size does not
/// participate.
#[derive(Clone, Copy, Debug)]
pub struct Proj {
    /// Cells per column (= 1 / zoom)
    pub scale: f64,
    /// View centre (cell coordinates, snapped to the cell grid)
    cx: f64,
    cy: f64,
    /// Screen cell the view centre falls on
    x0: i32,
    y0: i32,
}

impl Proj {
    /// Cell coordinate -> screen cell
    pub fn cell(&self, p: (f64, f64)) -> (i32, i32) {
        (
            self.x0 + ((p.0 - self.cx) / self.scale).round() as i32,
            self.y0 + ((p.1 - self.cy) / self.scale).round() as i32,
        )
    }

    /// Screen cell -> cell coordinate (1:1 maps one-to-one; when zoomed, rounds back)
    pub fn canvas(&self, col: i32, row: i32) -> (i32, i32) {
        let x = self.cx + (col - self.x0) as f64 * self.scale;
        let y = self.cy + (row - self.y0) as f64 * self.scale;
        (x.round() as i32, y.round() as i32)
    }

    /// Screen cell -> continuous canvas coordinate (unrounded): anchor zoom uses it to find what
    /// lies under a cell
    pub fn canvas_f(&self, col: i32, row: i32) -> (f64, f64) {
        (
            self.cx + (col - self.x0) as f64 * self.scale,
            self.cy + (row - self.y0) as f64 * self.scale,
        )
    }

    /// After a zoom change, keep canvas `point` on the same screen cell (col, row): where must the
    /// view centre go? All anchor-zoom maths lives here so it can be unit-tested
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

    /// Anchor the projection's screen origin at a cell (region centre)
    fn anchored(self, x0: i32, y0: i32) -> Proj {
        Proj { x0, y0, ..self }
    }
}

/// How this frame is split: the map area (full-bleed) and its projection
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    pub map: Rect,
    pub proj: Proj,
}

/// Rows reserved at the bottom: two when tall enough (status + command/prompt), otherwise one
pub fn status_rows(area: Rect) -> u16 {
    if area.height >= 8 {
        2
    } else {
        1.min(area.height)
    }
}

/// Map area: bottom rows removed
fn body_of(area: Rect) -> Rect {
    let rows = status_rows(area);
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(rows),
    }
}

/// Centre of the whole document (cell coordinates); an empty document gives (0, 0)
fn doc_center(doc: &Document, edge_labels: bool) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox_shown(edge_labels);
    ((x0 as f64 + x1 as f64) / 2.0, (y0 as f64 + y1 as f64) / 2.0)
}

/// Size of the whole document (cells, at least 1)
fn doc_size(doc: &Document, edge_labels: bool) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox_shown(edge_labels);
    (
        ((x1 - x0 + 1) as f64).max(1.0),
        ((y1 - y0 + 1) as f64).max(1.0),
    )
}

/// Projection for this frame at the given zoom. The view centre snaps to the glyph grid so the
/// camera's fractional position only shifts the whole picture by whole cells; otherwise some boxes
/// jump a cell before others and edges deform while panning. The screen origin starts at (0, 0) and
/// is fixed by panels / render_map around the region centre
pub fn proj_of(doc: &Document, view: &View, zoom: f64) -> Proj {
    // Zoom only concerns zoom: columns per cell. Window size only decides how much is visible, not
    // the zoom
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    let scale = 1.0 / zoom;
    let fallback = doc_center(doc, view.edge_labels);
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

/// Frame regions: the map area (full width) and the projection derived from view centre / zoom
pub fn panels(doc: &Document, area: Rect, view: &View) -> Panels {
    layout(doc, body_of(area), view)
}

/// Frame regions without a status row (whole picture)
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

/// One cell on the canvas
pub type Cell = (i32, i32);

/// Ego isolated view: shows only the centre node, its neighbours, and the edges attached to the
/// centre. Other elements are not dimmed but **not drawn at all** (press E or Esc again to exit)
pub struct Ego {
    /// Centre node: follows the cursor when it lands on a visible neighbour
    pub center: usize,
    /// Visible nodes: the centre and its neighbours
    nodes: HashSet<usize>,
    /// Visible edges: those attached to the centre
    edges: HashSet<usize>,
    /// Visible edge labels: those bound to visible edges
    edge_labels: HashSet<usize>,
}

impl Ego {
    pub fn has_node(&self, i: usize) -> bool {
        self.nodes.contains(&i) || self.edge_labels.contains(&i)
    }

    pub fn has_edge(&self, e: usize) -> bool {
        self.edges.contains(&e)
    }
}

/// Compute the isolation set from `view.ego`; None = E is off
pub fn ego_visible(doc: &Document, view: &View) -> Option<Ego> {
    let n = view.ego.filter(|n| *n < doc.boxes.len())?;
    // When an edge label is the centre, use the source end of the edge it is bound to (same rule as
    // toggle_ego)
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
    // Edge labels bound to visible edges appear too; other labels are not drawn
    let edge_labels: HashSet<usize> = (0..doc.boxes.len())
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
        edge_labels,
    })
}

/// This box is hidden while isolated (neither a node nor a visible label)
pub fn ego_hides(ego: Option<&Ego>, i: usize) -> bool {
    ego.is_some_and(|e| !e.has_node(i))
}

/// Topmost **visible** box at this cell. While isolated, hidden boxes are treated as absent
pub fn box_at(doc: &Document, cell: Cell, view: &View) -> Option<usize> {
    let ego = ego_visible(doc, view);
    doc.boxes_at_cell(cell)
        .into_iter()
        .find(|i| !ego_hides(ego.as_ref(), *i))
}

/// What lies under the cursor cell
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

/// What lies under the cursor cell: box first, then edge, otherwise empty selection. `canvas` is
/// the canvas coordinate (boxes are looked up by it), `screen` is the screen cell (the hit table is
/// keyed by it)
pub fn pick_at(doc: &Document, hit: &FrameHit, canvas: Cell, screen: Cell, view: &View) -> Pick {
    // Boxes hidden by isolation are blank: they are neither drawn nor clickable
    if let Some(i) = box_at(doc, canvas, view) {
        // With labels collapsed, an edge label is treated as absent (except ones revealed by E
        // isolation)
        if !view.edge_labels && doc.is_edge_label(i) && view.ego.is_none() {
            return Pick::None;
        }
        return Pick::Box(i);
    }
    match hit.edge_at(screen) {
        Some(e) => Pick::Edge(e),
        None => Pick::None,
    }
}

/// Rectangle the menu paints: width from the longest item, pinned to the edge when it does not fit
pub fn menu_rect(m: &crate::app::Menu, area: Rect) -> Rect {
    // 2 = index + current-value marker, the swatch takes 2 more, 2 cells after the title for " ›"
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

/// Which item was clicked in the menu
pub fn menu_hit(m: &crate::app::Menu, area: Rect, col: u16, row: u16) -> Option<usize> {
    let r = menu_rect(m, area);
    let inside = col >= r.x && col < r.x + r.width && row > r.y && row + 1 < r.y + r.height;
    inside.then(|| (row - r.y - 1) as usize)
}

/// Right-click menu: an in-place bordered list with the selected row reversed
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
    // Border
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
        // Index + current-value marker (`*` marks the item that is the current value)
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
        // Swatch: two cells of background colour, no wide glyph
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

/// `:` command-line completion popup: sits above the status line, lists candidates, current one
/// reversed
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

/// Per-frame hit table: which cell belongs to which edge (used by mouse edge clicks and `E`
/// cycling). Rebuilt every frame, never persisted.
#[derive(Clone, Debug, Default)]
pub struct FrameHit {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// Last edge displayed in each cell (-1 = none)
    edge: Vec<i32>,
    /// Edge candidates per cell (in draw order)
    pub candidates: std::collections::HashMap<(i32, i32), Vec<usize>>,
    /// Last visible cells of each edge
    pub edge_cells: std::collections::HashMap<usize, Vec<(i32, i32)>>,
    /// Document revision when the table was built: a mismatch means indices are stale
    pub rev: u64,
    /// Point-node names that did not fit (anything silently dropped during rendering is recorded
    /// here)
    pub labels_skipped: Vec<LabelSkip>,
    /// Screen cell where each edge's two ends were drawn (start, end): reconnecting a dragged end
    /// hits by it
    pub edge_ends: std::collections::HashMap<usize, (Cell, Cell)>,
    /// Canvas cell -> link target: every cell covered by `[x](y)` in text
    pub links: std::collections::HashMap<Cell, String>,
}

/// A point-node name that did not fit: all four candidate slots taken / off screen
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSkip {
    pub box_id: String,
    pub text: String,
    /// Screen cell of the node (so callers can report where it is crowded)
    pub cell: Cell,
}

impl FrameHit {
    /// Which edge this cell shows
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

    /// Edge candidates for this cell (more than one when they overlap)
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

/// Zoom factor needed to fit the whole area (columns per cell), used by f / F
pub fn zoom_for_fit(doc: &Document, w: u16, h: u16, edge_labels: bool) -> f64 {
    // Reserve the status rows first, like panels: otherwise "fit whole picture" fits the whole
    // window, and the actual drawing area is shorter so it gets cut off
    let body = body_of(Rect::new(0, 0, w, h));
    let (wd, hd) = doc_size(doc, edge_labels);
    let cols = (body.width as f64 - 4.0).max(8.0);
    let rows = (body.height as f64 - 2.0).max(4.0);
    let scale = (wd / cols).max(hd / rows);
    if scale.is_finite() && scale > 0.0 {
        // Fit at most to 1:1 - zooming past that only spreads the cells apart, glyphs do not grow,
        // and the picture becomes empty frames
        (1.0 / scale).min(1.0)
    } else {
        1.0
    }
}

/// Pan the view centre by **screen cells** (dcols columns, drows rows, positive right/down) and
/// return the new cell-coordinate centre. Wheel panning uses it: convert the centre to cells, move
/// a few cells, convert back
pub fn pan_center(proj: &Proj, from: (f64, f64), dcols: i32, drows: i32) -> (f64, f64) {
    let (x, y) = proj.cell(from);
    let (nx, ny) = (x + dcols, y + drows);
    (
        proj.cx + (nx - proj.x0) as f64 * proj.scale,
        proj.cy + (ny - proj.y0) as f64 * proj.scale,
    )
}

/// Pull a colour toward red by strength: t = 0 is the original colour, t = 1 pure red. view.pulse
/// is this t (the curve is computed by the main loop over time), so this only interpolates
fn pulse_color(c: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::Rgb(
        mix(c.0, BLINK_RED.0),
        mix(c.1, BLINK_RED.1),
        mix(c.2, BLINK_RED.2),
    )
}

/// Search-hit background + ink; `cur` = this is the current one
fn search_style(cur: bool) -> Style {
    let (bg, ink) = if cur {
        (SEARCH_CUR_BG, SEARCH_CUR_INK)
    } else {
        (SEARCH_BG, SEARCH_BG_INK)
    };
    Style::default()
        .fg(Color::Rgb(ink.0, ink.1, ink.2))
        .bg(Color::Rgb(bg.0, bg.1, bg.2))
}

/// Paint hit ranges with the search background: split spans per character, recolour the hit runs
fn hit_spans(spans: &[Span], ranges: &[(usize, usize)], st: Style) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut idx = 0usize;
    for sp in spans {
        let mut buf = String::new();
        let mut cur: Option<bool> = None;
        for ch in sp.text.chars() {
            let hit = ranges.iter().any(|&(a, b)| idx >= a && idx < b);
            if cur != Some(hit) {
                if let Some(prev) = cur {
                    push_run(&mut out, sp, std::mem::take(&mut buf), prev, st);
                }
                cur = Some(hit);
            }
            buf.push(ch);
            idx += 1;
        }
        if let Some(prev) = cur {
            push_run(&mut out, sp, buf, prev, st);
        }
    }
    out
}

/// One run from `hit_spans`: hit runs use the search colour, the rest keep inline styles
fn push_run(out: &mut Vec<Span>, sp: &Span, text: String, hit: bool, st: Style) {
    if text.is_empty() {
        return;
    }
    let mut s = Span {
        text,
        fg: sp.fg,
        bg: sp.bg,
        modifiers: sp.modifiers,
        link: sp.link.clone(),
    };
    if hit {
        s.fg = st.fg.or(sp.fg);
        s.bg = st.bg.or(sp.bg);
    }
    out.push(s);
}

/// Dim grey
const DIM_COLOR: Color = Color::Rgb(66, 74, 96);
/// Bright white: the usual colour for hubs and the cursor node
const BRIGHT: (u8, u8, u8) = (246, 248, 252);
/// Grey edge for block selection (background, continuous)
const SEL_GRAY: Color = Color::Rgb(84, 90, 110);
/// Stable highlight for the source box in link mode (no pulse, distinct from the target's red/white
/// breathing)
const LINK_SRC: (u8, u8, u8) = (250, 214, 96);
/// Search hit: paint the background rather than tint the foreground (like nvim's Search /
/// CurSearch). Non-current hits use a blue background, the current hit (under the cursor) an orange
/// one
const SEARCH_BG: (u8, u8, u8) = (0x27, 0x44, 0x77);
/// Ink on the blue background: otherwise yellow-ish group colours are illegible on blue
const SEARCH_BG_INK: (u8, u8, u8) = (0xe9, 0xee, 0xfb);
/// Orange background and ink for the current hit
const SEARCH_CUR_BG: (u8, u8, u8) = (0xee, 0x88, 0x2a);
const SEARCH_CUR_INK: (u8, u8, u8) = (0x1b, 0x12, 0x06);
/// Link preview line: dark blue-grey so it clearly reads as "not created yet"
const PREVIEW: (u8, u8, u8) = (104, 118, 152);
/// The reddest pulse step
const BLINK_RED: (u8, u8, u8) = (255, 62, 56);
/// Grey for ordinary node names
const LABEL: (u8, u8, u8) = (150, 158, 180);
/// Colour for region borders and titles (used when a region has no colour)
const REGION_INK: (u8, u8, u8) = (108, 118, 148);

/// Background for overlays and hints
const PANEL_BG: (u8, u8, u8) = (24, 28, 44);

/// Bright but not bold: for the bottom status / prompt line (bold makes the whole row feel heavier
/// than the map)
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

/// Turn a frame into text with ANSI colours (shared by `--export -`)
pub fn ansi(buf: &Buffer) -> String {
    use ratatui::style::Modifier;
    let mut out = String::new();
    for y in 0..buf.area.height {
        // The second cell of a wide glyph is not emitted again (the glyph itself already spans two
        // columns)
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
                (Modifier::CROSSED_OUT, ";9"),
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

/// Plain text: each line is exactly area.width columns. Wide glyphs span two columns; the second is
/// not repeated
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

/// On-screen rectangle of each box (left column, row, width, height), in `doc.boxes` order
pub fn box_rects(doc: &Document, area: Rect, view: &View) -> Vec<(i32, i32, i32, i32)> {
    let p = panels(doc, area, view);
    (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &p.proj))
        .collect()
}

/// Same as `box_rects` but using the "picture only, no status row" split (for export)
pub fn picture_box_rects(doc: &Document, area: Rect, view: &View) -> Vec<(i32, i32, i32, i32)> {
    let p = picture_panels(doc, area, view);
    (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &p.proj))
        .collect()
}

/// Draw one frame. The overlay (candidate list) goes on top; the picture itself is drawn in
/// render_frame
pub fn render(buf: &mut Buffer, area: Rect, doc: &Document, view: &View) -> Panels {
    render_hit(buf, area, doc, view, &FrameHit::default()).0
}

/// Like `render`, plus this frame's hit table (mouse edge clicks, selected-edge highlight)
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

/// Routing cache: with the same document, zoom and picture size, panning/moving the cursor needs no
/// re-planning. Plans are stored in canvas coordinates and shifted as a whole on a hit; the key
/// includes document geometry, so any document change misses
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

    /// Keep a few slots: switching tabs back and forth should not re-plan every time
    const SLOTS: usize = 4;

    thread_local! {
        static LAST: RefCell<Vec<Cached>> = const { RefCell::new(Vec::new()) };
        /// Test-only: hit count
        static HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Fetch a matching plan
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

/// Routing cache key: document content, zoom, picture size, and the view switches that decide
/// "which edges to draw". Deliberately excludes the viewport origin - panning only shifts cells as
/// a whole, the routes do not change
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
    // The viewport is part of the key: routes are computed in canvas coordinates, so a moved view
    // must recompute
    let (ox, oy) = proj.cell((0.0, 0.0));
    eat((ox as i64 as u64) << 32 | (oy as i64 as u64 & 0xffff_ffff));
    eat(u64::from(view.edge_labels));
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

/// E mode: draw the highlighted edge over boxes, text and labels so the whole blue path is visible.
/// Only cells belonging to that edge are covered: line cells require `shape_edge == e`, port cells
/// require `edge_of == e`, so other shapes at crossings are not interrupted Which colour a shared
/// arrowhead uses: the selected edge > the only lit owner in E > the first (visible) owner. A cell
/// has one colour; with several owners lit, keep the visible colour instead of jumping at random
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
    // The selected edge keeps its breathing colour, otherwise E does not show which one is selected
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
            // set_style merges rather than replaces: reset first so a BOLD left by the layer below
            // does not stay on this cell
            cell.set_style(Style::reset());
            cell.set_symbol(ch);
            cell.set_style(style);
        }
    };
    // 1. Strokes. Cells whose shape belongs to another edge (occupied by another line) are left
    // alone
    for &e in &ego.edges {
        let Some(cells) = cv.stroke_cells.get(&e) else {
            continue;
        };
        for &(x, y) in cells {
            let Some(i) = cv.idx(x, y) else { continue };
            // Arrowhead cells are handled per owner (below); skip them here
            if cv.arrow[i].is_some() {
                continue;
            }
            // Topmost node cells (a point node cell is both an endpoint and a glyph), do not
            // overwrite with strokes
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
    // 2. Arrowheads: only draw those belonging to the centre (a shared arrowhead is visible to
    // every owner); other owners' arrowheads mid-path stay
    for (&(x, y), owners) in &cv.arrow_owners {
        let Some(i) = cv.idx(x, y) else { continue };
        let Some(a) = cv.arrow[i] else { continue };
        // Colour follows the active owner: selected > the only lit one in E > the visible one
        let Some((oe, ink)) = arrow_ink_for(cv, (x, y), picked, Some(ego)) else {
            continue;
        };
        if !ego.edges.contains(&oe) && picked != Some(oe) {
            continue;
        }
        // On cells just outside a selected edge's port, other owners' arrowheads are already hidden
        // by blit: do not redraw them here
        if cv.picked_port_out.contains(&(x, y)) && !picked.is_some_and(|p| owners.contains(&p)) {
            continue;
        }
        let ch = a.to_string();
        write(buf, x, y, &ch, ink_for(oe, ink));
    }
    // 3. Ports on borders (T / embedded arrowheads): redraw only for edges belonging to the centre
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
            // Shared arrowheads (the kind embedded in a border) also use the active owner's colour
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

/// Group focus mask for this frame (at most 64 groups)
fn focus_mask(view: &View) -> u64 {
    view.focus
        .iter()
        .filter(|gr| **gr < 64)
        .fold(0u64, |m, gr| m | (1 << gr))
}

/// Is this the layer currently being viewed (everything counts when not viewing by layer)
fn lit_layer(view: &View, layer: &str) -> bool {
    match view.layer.as_deref() {
        None => true,
        Some(cur) => cur == layer,
    }
}

/// Layer stacking order: layers order, current layer moved last (drawn on top)
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

/// On-screen width/height of this box: Point is always one cell; boxed nodes scale with zoom so
/// borders and edges stay attached. The cap only guards against absurd sizes at extreme zoom
/// (canvas allocation sizes arrays by it)
fn screen_size(b: &Box, scale: f64) -> (i32, i32) {
    if b.style.is_point() {
        return (1, 1);
    }
    let w = (b.size.0 as f64 / scale).round().clamp(1.0, 4096.0) as i32;
    let h = (b.size.1 as f64 / scale).round().clamp(1.0, 4096.0) as i32;
    (w, h)
}

/// Port for this end of an edge at node i: pick the facing side from the two node centres, then
/// pick a connection cell by width/height parity (point nodes have no port, returns None)
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
    // Same side-selection rule as line drawing, otherwise endpoints in the hit table drift onto
    // other edges
    let side = end_side(doc, i, toward, rects, forced);
    let rect = rects[i];
    let b = &doc.boxes[i];
    Some(EndMark {
        rect,
        side,
        cell: Canvas::side_base(rect, side),
        // No border means nowhere to draw T/arrowheads: the mark falls on the cell outside the area
        mark: b.style.is_boxed(),
        corners: b.style.is_boxed(),
        // Port cells come from PortPlan; previews/self-loops have no plan and scan the border
        fixed: false,
    })
}

/// Which side this end should leave from: explicit port first, otherwise where the other end is.
/// Is this side facing away from the target: the target is entirely behind it
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
        // If the pinned side faces "away" (target on the opposite side), drawing it would loop:
        // fall back to automatic in that case; honour it only when the direction is not
        // contradictory
        if !side_backwards(d, s.bit()) {
            return s.bit();
        }
    }
    // Look at the projection first: only when horizontally overlapping and vertically disjoint can
    // "one above the other" apply
    let rect = rects[i];
    let other = rects[toward];
    let x_overlap = rect.0.min(other.0) < (rect.0 + rect.2).max(other.0 + other.2)
        && rect.0.max(other.0) < (rect.0 + rect.2).min(other.0 + other.2);
    let y_overlap = rect.1.min(other.1) < (rect.1 + rect.3).max(other.1 + other.3)
        && rect.1.max(other.1) < (rect.1 + rect.3).min(other.1 + other.3);
    // Layered layout: edges crossing layers always run top/bottom (the lower layer leaves from the
    // bottom), so edges of a layer share one channel, and ports align by column instead of each
    // taking its own horizontal run. But when two box rectangles overlap vertically (side by side,
    // just different top y), forcing by layer would bend left/right exits into top/bottom
    if !y_overlap {
        if let (Some(a), Some(b)) = (doc.rank(i), doc.rank(toward)) {
            // Only force adjacent layers: let a long edge spanning several layers find its own
            // path, otherwise it takes a big detour
            if b == a + 1 {
                return DOWN;
            }
            if a == b + 1 {
                return UP;
            }
        }
    }
    // First look at the two boxes' screen projections: horizontal overlap without vertical overlap
    // means "one above the other", so enter/exit from top/bottom; the reverse means left/right.
    // Only when both axes are disjoint (diagonal) pick the dominant axis from the centre delta.
    // Using the centre delta alone fails like this: a wide box and a narrow box right below it have
    // a horizontal centre delta, so top/bottom becomes left/right and the line crosses a long
    // horizontal stretch before turning down
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

/// Centre of the text content (screen cell). Ports measure from here, not from the box rectangle:
/// changing box style leaves the text in place and grows the box around it, so the content centre
/// is the same point and ports do not move
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

/// Is the content on this side wide/tall enough to hold two ports Edges within a layer: ordinary
/// edges run group by group, self-loops are handled separately
// Many parameters: the geometry extracted for a frame (proj/rects), view state and this layer's
// port plan all come in, each from a different source; bundling them into a struct would just be
// unpacked at every call site
#[allow(clippy::too_many_arguments)]
fn draw_edges(
    cv: &mut Canvas,
    doc: &Document,
    view: &View,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    layer: &str,
    ego: Option<&Ego>,
    // Port plan for this layer: computed once per frame (shared by the planning pass and the
    // drawing pass)
    plan: &PortPlan,
) {
    // When isolated, only draw visible edges. The planning pass still plans everything: visible
    // edges' ports and routes must not jump with E
    let drawing = !cv.planning;
    let hidden = |e: usize| drawing && ego.is_some_and(|g| !g.has_edge(e));
    // Edges not drawn while isolated must still consume their planning bookkeeping slots so the
    // cursor does not shift
    let skip = |cv: &mut Canvas, e: usize| {
        if drawing {
            cv.skip_plan(e);
        }
    };
    // Ports are laid out for "the edges this layer draws" as a whole: several edges in one group
    // decide one or two cells, independent of draw order or who occupies the border; `plan` is
    // computed once per frame by the caller Port/arrowhead cells are registered first as "soft
    // obstacles for others": routing prefers to detour one cell rather than cross another's
    // arrowhead mouth
    cv.port_guest.clear();
    // Sort by edge index before registering: when two edges land on the same cell, "who owns it"
    // must be stable, otherwise routing costs differ run to run and the same document draws two
    // different pictures
    let mut guests: Vec<(usize, usize, u8, (i32, i32))> = plan.iter().collect();
    guests.sort_unstable_by_key(|(box_i, e, side, cell)| (*e, *box_i, *side, *cell));
    for (_, e, side, cell) in guests {
        let out = Canvas::outside(cell, side);
        cv.port_guest.entry(cell).or_insert(e);
        cv.port_guest.entry(out).or_insert(e);
    }
    // "Edges between the same node pair" in this layer: bucketed by (lower index, higher index) and
    // looked up directly when drawing a segment. Previously every segment scanned all edges, which
    // is O(segments x edges) on large graphs
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
            // Directed edge: the arrowhead is drawn at the head end (series order is not
            // necessarily the arrow direction). When a group has both directions, draw one at each
            // end of this segment
            let (u, v) = (path[k], path[k + 1]);
            let key = if u < v { (u, v) } else { (v, u) };
            let pair = pairs.get(&key).map(Vec::as_slice).unwrap_or(&[]);
            // All edges on this segment: the same node pair may have several (a -> b, b -> a,
            // duplicates). `pair` ignores groups (point-node lanes are separated by index); this
            // segment draws only this group's edges
            let uid = &doc.boxes[u].id;
            if pair.is_empty() {
                continue;
            }
            for (lane, e) in pair.iter().copied().enumerate() {
                if doc.edges[e].group != *gid || hidden(e) {
                    skip(cv, e);
                    continue;
                }
                // Arrowheads use this edge's own direction: if from is u the ends are (u, v),
                // otherwise swap. Explicit ports also come from this edge, not from the first edge
                // on the segment
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
                // Port cells at both ends: look up PortPlan, fall back to the centre cell when
                // absent (point nodes)
                let mark_end = |box_i: usize, other: usize, arrow: bool, forced| {
                    let mut end = end_of(doc, rects, box_i, other, forced)?;
                    match plan.get(box_i, arrow, e) {
                        // The plan carries the actual side: when one side is full, this end was
                        // moved to an adjacent side
                        Some((side, cell)) => {
                            end.side = side;
                            end.cell = cell;
                            end.fixed = true;
                        }
                        // This side is too small to have a legal cell (box down to one row at low
                        // zoom): fall back to scanning the border Point node / no border cell: fall
                        // back to "pick side from centre delta, take that side's centre cell"
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
                // The next node decides whether this segment should enter vertically or
                // horizontally
                let arrive = if k + 2 < path.len() {
                    let c = center_cell(doc, proj, path[k + 2]);
                    arrive_hor(a_cell, b_cell, c)
                } else {
                    None
                };
                // The hit table records cells by edge index: state which edge (and its background)
                // before drawing the segment
                cv.pen_edge = e;
                cv.pen_bg = doc.edges[e].bg;
                // Arrowhead glyph: this edge's own if set, otherwise the global setting
                cv.pen_arrow = doc.edges[e].head.unwrap_or_default();
                let style = doc.edges[e].style;
                let fg = edge_ink(doc, view, e);
                cv.pen_corner = doc.edges[e].corner;
                // Neither end has a border cell that can be spread (point node, one-cell text
                // node): edges on the same segment are separated by lane offset, otherwise they
                // would draw the exact same line
                let spreadable = |end: Option<EndMark>| {
                    end.map(|e| e.rect.2 > 1 || e.rect.3 > 1).unwrap_or(false)
                };
                if pair.len() > 1 && !spreadable(a_end) && !spreadable(b_end) {
                    cv.edge_lane(a_cell, b_cell, lane_of(lane), *gr, fg, heads, style);
                    continue;
                }
                if !doc.edges[e].via.is_empty() {
                    // Manual via points: walk from -> via.. -> to segment by segment (this
                    // segment's direction decides via order)
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
                // When the document sets from_side, point nodes must start in that direction too
                let depart = forced_u
                    .map(|sd| matches!(sd, crate::model::Side::Left | crate::model::Side::Right));
                cv.edge_full(
                    a_cell, b_cell, *gr, fg, arrive, heads, style, a_end, b_end, depart,
                );
            }
        }
    }
    // 1b. Self-loop: it belongs to no "route"; it leaves one side of the node, turns to an adjacent
    // side and comes back
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
        cv.pen_arrow = e.head.unwrap_or_default();
        cv.edge_loop(end, e.dir.heads(), g, edge_ink(doc, view, i), e.style);
    }
}

/// Screen centre cell of a node
fn center_cell(doc: &Document, proj: &Proj, i: usize) -> (i32, i32) {
    proj.cell(doc.center(i))
}

/// Landing point of an edge at node i: boxed nodes land on a border connection cell, point nodes on
/// the centre cell
fn end_of(
    doc: &Document,
    rects: &[(i32, i32, i32, i32)],
    i: usize,
    toward: usize,
    forced: Option<crate::model::Side>,
) -> Option<EndMark> {
    end_mark(doc, i, toward, rects, forced)
}

/// Legal border cells on one side, sorted by distance from the content centre
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

/// Exact port of both ends of every edge in this layer. Key is (box, whether this end has an
/// arrowhead, edge): a pair of boxes may have several edges. The value carries the actual side -
/// when one side is full this end moves to an adjacent side, and drawing must follow it (box, has
/// arrowhead, edge) -> (actual side, port cell)
type PortKey = (usize, bool, usize);
/// Choice for a pair of ports: (total cost, source cell, target cell)
type PortChoice = (i32, (i32, i32), (i32, i32));

#[derive(Default, PartialEq)]
struct PortPlan {
    cells: std::collections::HashMap<PortKey, (u8, (i32, i32))>,
}

impl PortPlan {
    fn get(&self, box_i: usize, arrow: bool, edge: usize) -> Option<(u8, (i32, i32))> {
        self.cells.get(&(box_i, arrow, edge)).copied()
    }

    /// All ends (box, edge, side, port cell)
    fn iter(&self) -> impl Iterator<Item = (usize, usize, u8, (i32, i32))> + '_ {
        self.cells
            .iter()
            .map(|(&(box_i, _arrow, e), &(side, cell))| (box_i, e, side, cell))
    }
}

/// One end: which side to pick a cell on
#[derive(Clone)]
struct PortEnd {
    box_i: usize,
    side: u8,
    arrow: bool,
    /// A point node has only one cell and does not pick one; it only serves as an alignment target
    point: bool,
    /// Explicitly pinned side: untouched when a whole group changes sides
    forced: bool,
    /// Used when neither end is assigned to decide "which side faces the other": the opposite box's
    /// content centre (via if present)
    aim: (i32, i32),
    /// Centre of this end's box
    center: (i32, i32),
}

/// Lateral coordinate after flattening a side: top/bottom use the column, left/right use the row
fn port_lat(cell: (i32, i32), side: u8) -> i32 {
    if side == UP || side == DOWN {
        cell.0
    } else {
        cell.1
    }
}

/// Candidate cells on one side, sorted by distance from the content centre; cached so each edge
/// computes it once
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

/// Fallback when one side is full, in fixed order: free cell on this side -> existing line on this
/// side (merge) -> free cell on an adjacent side -> existing line on an adjacent side -> centre
/// cell of this side
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
        // If the cell outside is a box, the line/arrowhead would be covered by it: this cell is
        // unusable
        list.iter()
            .copied()
            .find(|&c| roles.get(&c).is_none() && !blocked.contains(&Canvas::outside(c, side)))
    };
    // Nearest existing line: an ordinary connection can merge into it with a T
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
    // Score the four sides by "how close to the target" instead of always going clockwise: an edge
    // whose target is above should not overflow downwards first and loop around
    let mut sides: Vec<(i32, u8)> = all_sides()
        .into_iter()
        .map(|side| {
            let list = port_cands(doc, proj, rects, cands, end.box_i, side);
            (side_score(end, side, &list), side)
        })
        .collect();
    // This side first (it already faces the target), then the rest by score
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
    // No free cell on any side but this side already has an arrowhead cell: merge into it. A
    // one-line target (e.g. dbus.service in systemd) has only one arrowhead slot left and right,
    // yet four edges point into it; without sharing half the arrowheads cannot be drawn. Share only
    // a cell that already has an arrowhead: an ordinary connection cell would merge a T into the
    // arrowhead
    if end.arrow {
        for &(_, side) in &sides {
            let list = port_cands(doc, proj, rects, cands, end.box_i, side);
            let want = port_lat(end.aim, side);
            let mut best: Option<(i32, (i32, i32))> = None;
            for c in list {
                // Share only a cell that is already an arrowhead: merging an ordinary connection
                // would push the T onto the arrowhead
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
    // No legal cell on any side (box only one cell wide at low zoom): leave no plan entry, drawing
    // falls back to the old "scan the border for a free spot" so arrowheads at least are not pushed
    // into corners
    None
}

/// Capacity of one side: number of cells on the border that can take a line (boxed nodes exclude
/// the corners)
fn side_capacity(
    doc: &Document,
    proj: &Proj,
    rects: &[(i32, i32, i32, i32)],
    box_i: usize,
    side: u8,
) -> usize {
    side_ports(doc, box_i, rects[box_i], side, proj).len()
}

/// Score a side when overflowing: the closer its outside cell is to the target the better; facing
/// away is heavily penalised
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

/// Short-edge test: Manhattan length no greater than the largest side of either end box (a ratio,
/// not a fixed cell count)
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

/// Side pair for a short edge: with no explicit ports at either end, enumerate 4x4 and pick the
/// pair with fewest bends and shortest length; prefer the straight axis when the two sides overlap
/// on the corresponding axis. Used only for relatively short edges; long edges are unaffected
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
    // Cells occupied by boxes: an outside cell landing here blocks this route at its first step
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
    // 1. Collect both ends of every edge in this layer first: pick sides and compute aims before
    // allocating cells
    let mut items: Vec<(usize, [PortEnd; 2])> = Vec::new();
    for (e, ed) in doc.edges.iter().enumerate() {
        if ed.layer != layer {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&ed.from), doc.box_idx(&ed.to)) else {
            continue;
        };
        if a == b {
            continue; // self-loops use their own path
        }
        let (at_from, at_to) = ed.dir.heads();
        let via: Vec<(i32, i32)> = ed
            .via
            .iter()
            .map(|&(x, y)| proj.cell((x as f64, y as f64)))
            .collect();
        // Aims for both ends: the opposite box's centre for ordinary edges, or the first/last via
        // when present
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
        // Relatively short edge: re-pick a side pair when neither end is pinned, preferring the
        // straight axis (ratio-based, independent of graph size)
        if !ends[0].forced && !ends[1].forced && relative_short_edge(rects, a, b) {
            if let Some((s0, s1)) = short_edge_sides(rects, &blocked, a, b) {
                ends[0].side = s0;
                ends[1].side = s1;
            }
        }
        items.push((e, ends));
    }
    // 2. Side capacity.
    //    - When a boxed node's "thin side" (capacity below 2) overflows, move the whole group to a
    //    side with enough room and the right direction;
    //    - When an ordinary side overflows, move only the excess ends, preferring ordinary
    //    connections (an arrowhead is the edge's semantics, keep it if possible);
    //    - A one-row `none` node does not move as a group: it has one cell left and right by
    //    design, so several ordinary connections sharing is normal.
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
        // Current load per side (including explicit ports), updated as ends move
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
            // 2a. Move a thin side's whole group (boxed nodes only)
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
            // 2b. Ordinary side: work out which ends to move
            let excess = count.saturating_sub(cap);
            if excess == 0 {
                continue;
            }
            // (gain, line first, edge index, end index, alternative side)
            let mut moves: Vec<(i32, bool, usize, usize, u8)> = Vec::new();
            for (k, j) in &list {
                let end = &items[*k].1[*j];
                if end.forced {
                    continue;
                }
                // An ordinary connection on a "one-cell-wide" side may already merge with a T
                // (one-row text node), so moving it is worse than sharing: keep it on the original
                // side
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
                    gain += 100; // ordinary connections are weakly directional, move them first
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
    // Port role table, keyed by **cell**: one cell has one role (ports of two adjacent boxes may
    // collide on one cell). An arrowhead owns a cell exclusively (sharing is the last resort after
    // "no free cell on any side"); ordinary connections may merge with a T on a narrow side
    let mut port_role: HashMap<(i32, i32), crate::canvas::PortKind> = HashMap::new();
    // Which side first took this cell: the same side must shift to the next cell; only the opposite
    // side shares a cell (the top and bottom of a one-row text node are the same physical border)
    let mut port_side: HashMap<(i32, i32), u8> = HashMap::new();
    // Whether this cell's port actually draws anything (a boxed border cell draws T/arrowhead). A
    // text node's port cell is only a placeholder; the line end draws nothing there, so an
    // arrowhead may take it over
    let mut port_drawn: HashMap<(i32, i32), bool> = HashMap::new();
    let mut cands: HashMap<(usize, u8), Vec<(i32, i32)>> = HashMap::new();
    // A side with only one cell: several ordinary connections crowding on it is normal (whether it
    // is "one cell" depends on capacity, not text)
    let mut narrow: HashSet<(usize, u8)> = HashSet::new();
    // Lateral positions already taken per (box, side): used for crowding
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
    // Content centre depends only on box and text: compute once per box instead of inside the
    // candidate cross product (`text_metrics` splits + collects every time, and this document has
    // tens of thousands of candidate pairs per layer)
    let content_centers: Vec<(i32, i32)> = (0..rects.len())
        .map(|i| content_center(doc, i, proj, rects[i]))
        .collect();
    // 3. Allocate incrementally in edge array order: new edges are appended, existing edges'
    // candidates and costs do not change
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
        // If the cell outside the port falls inside a box (two boxes adjacent/overlapping), both
        // line and arrowhead are covered by it. Swap such a side entirely first (score the
        // alternatives by direction); keep it only when nothing else fits
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
        // Prefer cells that can exit this side; if none can, keep the original list and let drawing
        // fall back
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
        // The cell outside the port is where the line really starts
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
                // Arrowhead end: only the port cell's role matters (rule 32). The outside cell's
                // role only constrains line ends, otherwise the arrowhead gets pushed where the
                // outside cell is buried in a neighbour box and covers its text
                if role_cell == Some(crate::canvas::PortKind::Line) {
                    // That cell is a line-end placeholder that draws nothing: an arrowhead may take
                    // it over
                    return !port_drawn.get(&c).copied().unwrap_or(false);
                }
                if role_cell == Some(crate::canvas::PortKind::Arrow) {
                    // The opposite side may share (the top and bottom of a one-row text node are
                    // the same cell); the same side still shifts to the next cell and shares only
                    // as a last resort in overflow
                    return port_side.get(&c).is_some_and(|s| *s != end.side);
                }
                return true;
            }
            // Line end: may not take another's arrowhead cell; the outside cell is guaranteed by
            // drawing (Canvas::outside_arrow never draws an arrowhead outside another's line end),
            // and planning does not move ports for it, otherwise the whole edge bends
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
                // The opposite outside cell decides where this end should sit laterally: aligned
                // means no extra jog
                let aim_s = via.first().copied().unwrap_or(tt);
                let aim_t = via.last().copied().unwrap_or(ts);
                let ideal_s = port_lat(aim_s, ends[0].side);
                let ideal_t = port_lat(aim_t, ends[1].side);
                let (ls, lt) = (port_lat(c_s, ends[0].side), port_lat(c_t, ends[1].side));
                // Primary term is "alignment": the farther from the opposite outside cell, the
                // longer the jog
                let mut cost = 10 * ((ls - ideal_s).abs() + (lt - ideal_t).abs());
                // Secondary term mildly prefers the content centre so ports do not all drift to the
                // edges
                let cc_s = port_lat(content_centers[ends[0].box_i], ends[0].side);
                let cc_t = port_lat(content_centers[ends[1].box_i], ends[1].side);
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
            // One or both ends have no free cell: find overflow positions individually, leaving
            // existing ports in place
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
            // An arrowhead beats a "draws nothing" line-end placeholder; otherwise first come,
            // first served
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

/// On-screen rectangle of a box: (left column, top row, width, height)
pub fn rect_of(doc: &Document, i: usize, proj: &Proj) -> (i32, i32, i32, i32) {
    let b = &doc.boxes[i];
    let (x, y) = proj.cell((b.at.0 as f64, b.at.1 as f64));
    let (w, h) = screen_size(b, proj.scale);
    (x, y, w, h)
}

/// Text of a box (validation guarantees exactly one) A box's "name" for the status line / labels:
/// only the first line of multi-line text
fn label_of(doc: &Document, i: usize) -> &str {
    doc.text(i)
        .and_then(|t| t.text.split('\n').next())
        .unwrap_or("")
}

/// Boxes selected by the rectangle: the closed rectangle from the anchor cell to the cursor cell
/// (screen cell coordinates). Returns an empty list when view.sel is None
pub fn selected(doc: &Document, p: &Panels, view: &View) -> Vec<usize> {
    selected_all(doc, p, view, &FrameHit::default()).0
}

/// Screen rectangle of the selection (closed interval), None without a selection
fn sel_rect(p: &Panels, view: &View) -> Option<(i32, i32, i32, i32)> {
    // Do not draw the selection rectangle while dragging boxes (boxes, text and edges are drawn as
    // usual)
    if view.drag_hide_sel {
        return None;
    }
    let sel = view.sel?;
    // The other end follows the cursor until confirmed; only then is it pinned to end
    let end = if sel.confirmed { sel.end } else { view.cursor };
    let (ax, ay) = p.proj.cell((sel.anchor.0 as f64, sel.anchor.1 as f64));
    let (cx, cy) = p.proj.cell((end.0 as f64, end.1 as f64));
    Some((ax.min(cx), ay.min(cy), ax.max(cx), ay.max(cy)))
}

/// Boxes and edges in the selection: boxes by rectangle intersection, edges by whether any visible
/// cell falls inside
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

/// Link-mode target set: explicit ticks (Shift+left click) first, then the selection, then the box
/// under the cursor
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

/// Cells to reverse for selected nodes: a point node is just its own cell, a box node the whole
/// border (the text inside is left alone, reversing the whole block is too noisy)
fn selected_cells(doc: &Document, p: &Panels, view: &View, hit: &FrameHit) -> HashSet<(i32, i32)> {
    let mut out = HashSet::new();
    let (boxes, edges) = selected_all(doc, p, view, hit);
    for e in edges {
        if let Some(cells) = hit.edge_cells.get(&e) {
            out.extend(cells.iter().copied());
        }
    }
    for i in boxes {
        // With labels collapsed, do not draw a selection frame around edge labels
        if !view.edge_labels && doc.is_edge_label(i) {
            continue;
        }
        let (left, top, w, h) = rect_of(doc, i, &p.proj);
        if doc.boxes[i].style.is_point() {
            out.insert((left, top));
            continue;
        }
        if matches!(doc.boxes[i].style, BoxStyle::None) {
            // No border: the whole text area counts as selected
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

/// Block-selection frame: a dashed grey frame drawn only inside the map area, never over the
/// legend/status line
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
    // Change only the background, not the glyph: text in the frame stays readable; the grey edge is
    // continuous with no gaps
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

/// On-screen extent of the whole picture: union of all boxes plus two cells for routing. Routing
/// uses it, not the viewport - otherwise panning the view would change the route of the same
/// picture
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
        // The margin must cover the search window (6 cells each side) or the detour would be
        // clipped early by this boundary
        Some((x0, y0, x1, y1)) => (x0 - 8, y0 - 8, x1 + 8, y1 + 8),
        None => (
            map.x as i32,
            map.y as i32,
            (map.x + map.width) as i32 - 1,
            (map.y + map.height) as i32 - 1,
        ),
    }
}

/// Link preview: temporary source -> target lines (dashed dark, no T/arrowhead). At most 64
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
        // A source box does not block its own preview line (free preview is drawn from inside the
        // source box)
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
    // No target box aimed at yet: draw a dashed line from each source to the cursor, telling the
    // user "go this way"
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
    // Write only on blank cells: a preview never covers a box or text
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.mask[i] == 0 || cv.deco[i].is_some() {
                continue;
            }
            let ch = cv.glyphs.line(cv.mask[i], cv.corner[i]);
            // Dashes only break pure straight strokes (same rule as real edges); corners stay solid
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

/// Draw only the map (edges, boxes, text), no legend, status line or overlays. Used for SVG / image
/// export. Uses the given zoom for the projection; the map area is the whole area
pub fn render_map(buf: &mut Buffer, area: Rect, doc: &Document, view: &View, zoom: f64) -> Rect {
    render_map_hit(buf, area, doc, view, zoom).0
}

/// Like `render_map`, plus this frame's hit table (used by export)
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

/// The map itself: edges, node boxes, nodes, text, point-node name labels
fn draw_map(buf: &mut Buffer, p: &Panels, doc: &Document, view: &View, hit: &FrameHit) -> FrameHit {
    let (map, proj) = (p.map, p.proj);
    if map.width == 0 || map.height == 0 || doc.boxes.is_empty() {
        return FrameHit::default();
    }
    // Detail level at low zoom: text / shapes / solid blocks (see Lod). Export disables it and
    // always draws everything
    let lod = if view.lod {
        lod_of(view.zoom)
    } else {
        Lod::Full
    };
    let show_text = lod == Lod::Full;
    // Who wins colour conflicts: focused groups always own them (even when the cursor moves to
    // another group), unfocused means the group of the node under the cursor
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
    // E isolation: compute the visible set once per frame; drawing/hit testing below use it
    let ego = ego_visible(doc, view);
    // Edge-label switch: when off, label boxes are treated as absent (not drawn, not obstacles, not
    // in hits/selection); while E isolation is on, those bound to visible edges are revealed
    let edge_label_visible = |i: usize| -> bool {
        if !doc.is_edge_label(i) || view.edge_labels {
            return true;
        }
        ego.as_ref().is_some_and(|e| e.edge_labels.contains(&i))
    };
    // Links in text: canvas cell -> target. Text in boxes and point-node names both record here
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
    // The selected edge is raised above other edges (by default the latest drawn is on top); the
    // lit edges in E mode too, otherwise crossings would be cut by other lines
    cv.raise(view.pick.edge());
    cv.picked = view.pick.edge();
    if let Some(n) = view.ego.filter(|n| *n < doc.boxes.len()) {
        for &e in doc.adj(n) {
            cv.raise(Some(e));
        }
    }
    // On-screen rectangle of each box; drawing edges, hit testing and selection all use it later
    let mut rects: Vec<(i32, i32, i32, i32)> = (0..doc.boxes.len())
        .map(|i| rect_of(doc, i, &proj))
        .collect();
    // When editing in place, draw the box with the draft's size (grown text must be visible)
    if let Some(edit) = &view.edit {
        if let Some(&(x, y, _, _)) = rects.get(edit.box_idx) {
            let (w, h) = edit.text_size(doc, edit.box_idx);
            rects[edit.box_idx] = (x, y, w, h);
        }
    }
    cv.frame = doc_frame(&rects, map);
    // Register the cells occupied by node boxes first: edge drawing must avoid the whole box
    for (i, r) in rects.iter().enumerate() {
        if !edge_label_visible(i) {
            continue;
        }
        if doc.boxes[i].style.is_point() {
            cv.station_cells.insert((r.0, r.1));
        } else {
            // Boxed and "text-only" nodes both occupy the whole block: lines must not run through
            // the text
            cv.reserve_box((r.0, r.1), (r.2 as usize, r.3 as usize));
        }
    }
    // 0. Regions (clusters): painted underneath first, later edges and boxes naturally cover them.
    // Depth order: outer first, inner later, inner borders over outer ones
    let mut order_r: Vec<usize> = (0..doc.regions.len()).collect();
    order_r.sort_by_key(|&i| (doc.region_depth(i), i));
    for i in &order_r {
        let r = &doc.regions[*i];
        // While isolated all regions are hidden: they mostly frame nodes that are gone, leaving
        // only empty frames
        if ego.is_some() {
            continue;
        }
        // When viewing/editing one layer, suppress regions of other layers too (consistent with
        // boxes and edges)
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
            // Collapsed: only a title bar remains; member boxes and their edges are not drawn
            let tw = title.chars().count() + 4;
            let (cw, ch) = (tw, 3usize);
            cv.plate_at((x, y), (cw, ch), r.style, fg);
            if !title.is_empty() {
                cv.stamp_at(x + 2, y + 1, title, fg);
            }
            // A small mark in the top-right corner: signals a collapsed region
            if cw >= 3 {
                let mark = if view.glyphs.ascii { '+' } else { '…' };
                cv.stamp_at(x + cw as i32 - 2, y + 1, &mark.to_string(), fg);
            }
        } else {
            cv.plate_at((x, y), (w, h), r.style, fg);
            if !title.is_empty() {
                // The title rides on the top-left border: ` label `
                let text = format!(" {} ", title);
                let avail = w.saturating_sub(2);
                let text: String = text.chars().take(avail).collect();
                cv.stamp_at(x + 1, y, &text, fg);
            }
        }
    }
    // 1. Edges: plan first in layer order (each edge sees the routes of earlier ones), then draw in
    // the same order
    let layers = layer_order(doc, view);
    // Port planning depends only on doc / proj / rects / layer: computed once per layer per frame,
    // shared by both passes
    let port_plans: Vec<PortPlan> = layers
        .iter()
        .map(|layer| plan_ports(doc, &proj, &rects, layer))
        .collect();
    let key = route_key(doc, view, &proj, map);
    // On a hit, reuse the previous plan (same viewport, same document, same zoom)
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
            for (li, layer) in layers.iter().enumerate() {
                cv.layer_lit = lit_layer(view, layer);
                draw_edges(
                    &mut cv,
                    doc,
                    view,
                    &proj,
                    &rects,
                    layer,
                    ego.as_ref(),
                    &port_plans[li],
                );
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
    for (li, layer) in layers.iter().enumerate() {
        cv.layer_lit = lit_layer(view, layer);
        draw_edges(
            &mut cv,
            doc,
            view,
            &proj,
            &rects,
            layer,
            ego.as_ref(),
            &port_plans[li],
        );
        // 2. Node boxes and nodes: within a layer, boxes cover that layer's edges
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer
                || !b.style.is_boxed()
                || !edge_label_visible(i)
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
            // Any smaller becomes a solid block: below 0.3 a border is a single line and no longer
            // reads as a box
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
        // 3. Point nodes: one cell has one colour too; again let the cursor's group choose first
        for (i, b) in doc.boxes.iter().enumerate() {
            if b.layer != *layer
                || b.style.is_boxed()
                || !edge_label_visible(i)
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
            // A text node's glyph sits on its text cell (the box may be larger than the text:
            // whitespace left by style changes); a point node's name is attached separately, its
            // glyph stays on the node's own cell
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
    // Cursor marker to draw: on the cursor cell
    let cursor_cell = proj.cell((view.cursor.0 as f64, view.cursor.1 as f64));
    let cursor_box = view.pick.box_at();
    let marked = selected_cells(doc, p, view, hit);

    // When an edge is selected in E, highlight the neighbour node at its other end too, showing
    // where the edge leads
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
    // Search hits: paint the background (like nvim's Search / CurSearch); point nodes have one
    // cell, so compute their screen cells first Only point nodes show a hit by painting that cell's
    // background (they have no room for text); hits on boxed/text nodes colour the hit characters
    // in the text layer and must never paint the whole block here - once a background is set, later
    // text can only change the foreground (ratatui's set_style replaces per item), so it cannot be
    // erased
    let hit_cells: HashSet<(i32, i32)> = view
        .hits
        .iter()
        .filter(|i| **i < rects.len() && !ego_hides(ego.as_ref(), **i))
        .filter(|i| doc.boxes[**i].style.is_point())
        .flat_map(|&i| {
            let (l, t, w, h) = rects[i];
            (0..w).flat_map(move |dx| (0..h).map(move |dy| (l + dx, t + dy)))
        })
        .collect();
    // Current hit = the one most recently jumped to (the first after `/`, the one landed on after
    // `n`/`N`); same meaning as nvim's CurSearch: follows the hit sequence, not the mouse
    let cur_hit = view
        .hits
        .get(view.hit_at.min(view.hits.len().saturating_sub(1)))
        .copied()
        .filter(|i| *i < rects.len());
    let cur_cells: HashSet<(i32, i32)> = cur_hit
        .filter(|i| doc.boxes[*i].style.is_point())
        .map(|i| {
            let (l, t, w, h) = rects[i];
            (0..w)
                .flat_map(|dx| (0..h).map(move |dy| (l + dx, t + dy)))
                .collect()
        })
        .unwrap_or_default();
    // Cells just outside the selected edge's port: use the **actually drawn** out points (the plan
    // may lack this end); another's arrowhead landing there makes it look like <->, so hide it in
    // blit/overlay
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
        &hit_cells,
        &cur_cells,
    );
    // 4. Text inside boxes: the inner region starts at (left+1, middle row) and shifts within the
    // available width by align. A text node (None) has no border: the region is the whole block
    // starting at the top-left. At very small zoom the text is just fragments: LOD skips the whole
    // block (edge labels are boxes and skip too) Draw the moving one first: when two boxes overlap,
    // the text under the user's hand is the most complete
    let mut text_order: Vec<usize> = (0..doc.boxes.len()).collect();
    if let Some(p) = view.pick.box_at().or_else(|| doc.box_at_cell(view.cursor)) {
        if p < text_order.len() {
            text_order.retain(|&k| k != p);
            text_order.insert(0, p);
        }
    }
    // Cells already written by other boxes: later boxes do not overwrite them (otherwise the
    // continuation blank of a wide glyph would erase text)
    let mut kept: HashSet<(i32, i32)> = HashSet::new();
    for &i in &text_order {
        let b = &doc.boxes[i];
        if !show_text {
            break;
        }
        if b.style.is_point() || !edge_label_visible(i) || ego_hides(ego.as_ref(), i) {
            continue;
        }
        let editing = view.edit.as_ref().filter(|e| e.box_idx == i);
        let Some(t) = doc.text(i) else { continue };
        let (left, top, w, h) = rects[i];
        let bare = matches!(b.style, BoxStyle::None);
        // Text node: start at its own text cell (when the style changes the box may be larger than
        // the text, centred with whitespace), boxed: inside the top-left corner, row by row
        let (region_x, region_y, max_w) = if bare {
            let (tx, ty) = proj.cell((t.at.0 as f64, t.at.1 as f64));
            (tx, ty, ((left + w) - tx).max(1) as usize)
        } else {
            (left + 1, top + 1, (w - 2).max(1) as usize)
        };
        if !bare && h < 3 {
            continue; // squashed to a single line: no room for text
        }
        // When clipped by the picture edge, draw only the remaining part (cell-by-cell clipping)
        // instead of vanishing
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
        // Base: default foreground + background; inline `<fg>` / `<bg>` override it
        let mut style = Style::default().fg(Color::Rgb(r, gg, b));
        // Background: the text's own first, then the box's fill (independent); not drawn while
        // dimmed
        if live {
            if let Some((r, gg, b)) = t.bg.or(doc.boxes[i].fill) {
                style = style.bg(Color::Rgb(r, gg, b));
            }
        }
        // Temporary UI goes last: search hits colour only the hit characters, dimming only changes
        // the foreground
        let hit = view.hits.contains(&i);
        let query = view.hit_text.as_deref().filter(|_| hit);
        let cur_hit = cur_hit == Some(i);
        let mut overlay = text::TextOverlay::default();
        // Name/alias hits (the word is not in the text at all) have nothing to colour, so paint the
        // whole run's background
        let word_hit = query.is_some_and(|q| {
            t.text
                .split('\n')
                .any(|line| !text::find_ranges(line, q).is_empty())
        });
        if hit && !word_hit {
            let st = search_style(cur_hit);
            overlay.fg = st.fg;
            overlay.bg = st.bg;
        } else if !hit && !live {
            overlay.fg = Some(DIM_COLOR);
            overlay.bg = None;
        }
        let bottom = map.y as i32 + map.height as i32;
        // Editing: draw the draft literally (no markdown parsing), reverse the cursor cell
        if let Some(edit) = editing {
            let mut row = 0usize;
            for line in edit.text.split('\n') {
                let ty = region_y + row as i32;
                if ty >= bottom {
                    break;
                }
                // One cell per character would eat wide glyphs: a CJK glyph spans two cells and the
                // next character writes into its right half, so the screen shows "one character
                // missing". Go by display width and pad continuation cells with spaces (consistent
                // with normal layout)
                let mut col = 0usize;
                for ch in line.chars() {
                    let w = unicode_width::UnicodeWidthChar::width(ch)
                        .unwrap_or(1)
                        .max(1);
                    if col + w > max_w {
                        break;
                    }
                    let cx = region_x + col as i32;
                    col += w;
                    if cx < map.x as i32 || cx + w as i32 > (map.x + map.width) as i32 {
                        continue;
                    }
                    let mut one = [0u8; 4];
                    let sym = ch.encode_utf8(&mut one);
                    if let Some(c) = buf.cell_mut((cx as u16, ty as u16)) {
                        c.set_symbol(sym);
                        c.set_style(style);
                    }
                    for k in 1..w as i32 {
                        if let Some(c) = buf.cell_mut(((cx + k) as u16, ty as u16)) {
                            c.set_symbol(" ");
                            c.set_style(style);
                        }
                    }
                }
                row += 1;
            }
            // The cursor is not drawn here: the real cursor (view.cursor) already lands on the
            // draft's cell, the picture should have only one cursor
            continue;
        }
        // Would this run of text land on already-written text (overlapping boxes): if so skip the
        // whole run rather than lose a few characters, which looks like "text went missing"
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
                break; // beyond the picture, stop drawing
            }
            if ty < map.y as i32 {
                continue; // top half is off screen
            }
            let mut spans = text::parse(line);
            if let Some(q) = query {
                let ranges = text::find_ranges(line, q);
                if !ranges.is_empty() {
                    spans = hit_spans(&spans, &ranges, search_style(cur_hit));
                }
            }
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
                &overlay,
                max_w,
                (clip_lo, clip_hi),
                &mut kept,
                |x, y, target| {
                    links.insert(proj.canvas(x as i32, y as i32), target.to_string());
                },
            );
        }
    }
    // When spacing is wide enough (adjacent nodes at least 4 cells apart), label point-node names
    // automatically
    let labels = show_text && (view.node_labels || median_edge_cells(doc, &proj) >= 4.5);
    // 5. Labels (topmost, written only in free space); three batches share one occupancy table:
    // terminal/hub point-node names -> edge labels -> remaining point-node names. Boxed node text
    // is already inside the box
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
            // Did not fit: record it, the status line uses it to say "N names were not
            // drawn"
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
    // E mode: the highlighted edge overrides boxes and text, keeping the blue path complete
    if let Some(ego) = &ego {
        overlay_ego_edges(buf, &cv, doc, ego, view.pick.edge(), view.pulse);
    }
    let mut hit = FrameHit::from(&cv);
    hit.rev = doc.rev();
    hit.labels_skipped = skipped;
    hit.links = links;
    // End cells: recompute with the same port rules as rendering (reconnecting a dragged end hits
    // by it)
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

/// Does a line of width `w` fit here: inside the picture, every cell blank, and not taken by
/// another label
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

/// Reserve this row plus the rows above and below: labels get natural spacing
fn reserve_label(used: &mut HashSet<(i32, i32)>, x: i32, y: i32, w: i32) {
    for k in 0..w + 1 {
        used.insert((x + k, y));
        used.insert((x + k, y + 1));
        used.insert((x + k, y - 1));
    }
}

/// Attach a name to a point node: the text's own position first, otherwise pick among four
/// candidate slots (terminals first move away from their neighbour)
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
    // Only point nodes need an attached name: boxed nodes have it inside, text nodes draw their own
    // text
    if !doc.boxes[i].style.is_point() {
        return true;
    }
    let terminus = doc.adj(i).len() == 1;
    if !labels && !terminus {
        return true;
    }
    // The point node's glyph cell: from the box's top-left (1x1, so it is itself). Using the centre
    // would round to a neighbouring cell, letting the name cover the glyph itself
    let (x, y) = proj.cell((doc.boxes[i].at.0 as f64, doc.boxes[i].at.1 as f64));
    let label = label_of(doc, i);
    // A name may contain `**bold**` and `[text](target)`: go by the markers, count only visible
    // width
    let spans = text::parse(label);
    let w = text::width_of(&spans) as i32;
    if label.is_empty() || w == 0 {
        return true;
    }
    // stored_at first: the text's own position is where it is drawn
    let stored = doc
        .text(i)
        .map(|t| proj.cell((t.at.0 as f64, t.at.1 as f64)));
    // Four candidate slots: right, left, centred above, centred below
    let mut slots = [
        (x + 2, y),
        (x - w - 1, y),
        (x - w / 2, y - 1),
        (x - w / 2, y + 1),
    ];
    // Terminal: try the direction "away from the neighbour" first, so the label extends off the
    // edge's end
    if terminus {
        if let Some(&e) = doc.adj(i).first() {
            if let Some((a, b)) = doc.edge_ends(e) {
                let nbr = if a == i { b } else { a };
                let n = proj.cell(doc.center(nbr));
                let (dx, dy) = ((x - n.0).signum(), (y - n.1).signum());
                slots.sort_by_key(|(lx, ly)| {
                    let (ox, oy) = ((lx - x).signum(), (ly - y).signum());
                    // Prefer the one matching the "away from neighbour" direction
                    (ox != dx || oy != dy) as u8
                });
            }
        }
    }
    // When all four standard slots are taken by lines or other names, try a ring of "shifted one
    // cell" positions: when a node sits exactly on a horizontal/vertical crossing (e.g. Eastern
    // Zhou in a dynasty graph), the outer ends of the rows above and below are often free, and
    // crowding only the centre fails
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
    // Base: a point node has no border; text uses its own color, then hubs/terminals use bright
    let style = if let Some((r, gg, b)) = text_fg {
        Style::default().fg(Color::Rgb(r, gg, b))
    } else if doc.hub(i) || terminus {
        bright_style()
    } else {
        Style::default().fg(Color::Rgb(LABEL.0, LABEL.1, LABEL.2))
    };
    // Temporary UI goes last: search background / breathing / dimming. Hit characters get a
    // background (only those when the word is found in the name); the glyph cell's background is
    // painted in blit, this only handles the attached name
    let hit = view.hits.contains(&i) && !picked_here;
    let query = view.hit_text.as_deref().filter(|_| hit);
    let cur_hit = view
        .hits
        .get(view.hit_at.min(view.hits.len().saturating_sub(1)))
        == Some(&i);
    let mut spans = spans;
    let ranges = query
        .map(|q| text::find_ranges(label, q))
        .unwrap_or_default();
    if !ranges.is_empty() {
        spans = hit_spans(&spans, &ranges, search_style(cur_hit));
    }
    let mut overlay = text::TextOverlay::default();
    if hit && ranges.is_empty() {
        // Alias/initial hit: the word is not in the text, paint the whole run
        let st = search_style(cur_hit);
        overlay.fg = st.fg;
        overlay.bg = st.bg;
    } else if hit {
        // Characters already coloured: only skip dimming
    } else if picked_here {
        // Selected point node: the name breathes too (no box to pulse, pulse the text)
        overlay.fg = Some(pulse_color(BRIGHT, view.pulse));
        overlay.modifiers = Modifier::BOLD;
    } else if !on_focus || !lit_layer(view, &doc.boxes[i].layer) {
        overlay.fg = Some(DIM_COLOR);
    }
    text::put_link(
        buf,
        lx as u16,
        ly as u16,
        &spans,
        style,
        &overlay,
        usize::MAX,
        |x, y, target| {
            links.insert(proj.canvas(x as i32, y as i32), target.to_string());
        },
    );
    true
}

/// Label in front of the input line (`box: ` / `text: ` / `link: `). The renderer and the
/// real-cursor placement share it, so the cursor cannot drift by a cell in one language only.
pub fn prompt_prefix(lang: Lang, what: &crate::app::Prompt) -> &'static str {
    match what {
        crate::app::Prompt::NewBox => lang.pick("框: ", "box: "),
        crate::app::Prompt::Text(_) => lang.pick("文字: ", "text: "),
        crate::app::Prompt::Link(_) => lang.pick("连到: ", "link: "),
    }
}

/// Text shown on the input line: multi-line shows only the first line, suffixed with `[+N lines]`
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

/// Draw only the picture (map), no status line/overlays: for the screen and export
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

/// Colour of an edge: its own colour > configured `edge_color` > group colour
fn edge_ink(doc: &Document, view: &View, e: usize) -> (u8, u8, u8) {
    doc.edges[e]
        .color
        .or(view.edge_ink)
        .unwrap_or_else(|| doc.edge_color(e))
}

/// Colour of a text: its own colour > configured `text_color` > box/edge colour (None = default
/// bright)
fn text_ink(doc: &Document, view: &View, i: usize) -> Option<(u8, u8, u8)> {
    doc.text(i)
        .and_then(|t| t.color)
        .or(view.text_ink)
        .or_else(|| doc.text_color(i))
}

/// Offset of the k-th edge on the same segment: 0, -1, +1, -2, +2 ...
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

/// The picture itself: map + legend + status line
fn render_frame(
    buf: &mut Buffer,
    area: Rect,
    doc: &Document,
    view: &View,
    prev_hit: &FrameHit,
) -> (Panels, FrameHit) {
    let p = panels(doc, area, view);
    if view.help {
        render_help(
            buf,
            area,
            help_keys(view.lang),
            view.lang,
            view.tip.as_deref(),
        );
        return (p, FrameHit::default());
    }
    if area.width == 0 || area.height == 0 || doc.boxes.is_empty() {
        return (p, FrameHit::default());
    }
    let hit = draw_map(buf, &p, doc, view, prev_hit);
    // Draw the link preview on the map, then mark the selection with a continuous grey edge
    draw_link_preview(buf, &p, doc, view);
    draw_sel_box(buf, &p, view);
    // 4. Legend: entries carried by the document, or fall back to "groups + glyph legend"
    // 5. Status line (the input line while searching) Bottom: two rows when tall enough (status +
    // command/prompt), otherwise one
    let rows = status_rows(area);
    let bottom = area.y + area.height - 1;
    let top = bottom + 1 - rows;
    // Compute the status line (and context) first: the upper row must keep showing the file during
    // input
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
    // Selection and layer are global state: prefix the selection counts and confirm state, suffix
    // the layer name
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
    // While isolated, keep a persistent row: otherwise it later looks like "where did the picture
    // go"
    if let Some(n) = view.ego.filter(|n| *n < doc.boxes.len()) {
        left = format!(
            "{left}   {}{}",
            view.lang.pick("隔离 ", "isolate "),
            label_of(doc, n)
        );
    }
    // Style menu: which elements to change + current property/value
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
    // Link mode: source count / target count / how many will be created; in confirm state prompt
    // Enter again
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
        // Upper row is persistent: file + mode + context; cwd and coordinates on the right
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
    // 5.3 Single-line bottom input (new box / edit text / link): owns the whole row like `:`
    if let Some((what, typed)) = &view.prompt {
        let prefix = prompt_prefix(view.lang, what);
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
    // 5.4 The ":" command line owns the whole row: starts at the far left, covering the mode marker
    // and node name so input needs no right search
    if let Some(c) = &view.cmd {
        let line = format!(":{c}");
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, bottom)) {
                cell.reset();
            }
        }
        // Print the command line raw: `/` in a path must not be parsed as an italic marker
        put_plain(buf, area.x, bottom, &line, bright_plain());
        // On the right add a sentence "where it will write / where it reads from"; truncate on the
        // left if it does not fit, keeping the filename and never covering the input
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
    // 5.5 Status line: first mark VIEW / VIEW x{zoom}, when idle report the current box's text;
    // counts, layout, focus and hits are not here pick is derived by the caller; if not (e.g. a
    // direct render test) compute it once from the cell Two-row bottom: the upper row is drawn, the
    // lower row holds hints/errors; with one row the hint is still squeezed to its right
    if rows > 1 {
        // When there is nothing to say, show a session hint; not in one-row layout (it would
        // squeeze out the file/mode on the left)
        if let Some(hint) = hint_text(view).or_else(|| view.tip.clone()) {
            let room = if view.showcmd.is_empty() {
                area.width as usize
            } else {
                area.width.saturating_sub(width(&view.showcmd) as u16 + 2) as usize
            };
            put_n(buf, area.x, bottom, &hint, dark_style(), room);
        }
        if !view.showcmd.is_empty() {
            // Bottom-right: the pending key sequence (like nvim's showcmd)
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
    // One-row layout: file (with * when dirty) + mode/zoom + context, hint squeezed on the right
    // (showcmd is drawn last at the far right, consistent with two-row layout)
    left = format!("{}   {left}", status_left_tabbed(view));
    // Echo slot on the right: ":" command line > command echo / hint; nothing when idle. It has the
    // highest priority, so the left block is truncated if it does not fit
    if let Some(hint) = hint_text(view) {
        let right = area.x + area.width;
        let hw = width(&hint) as u16;
        if right > hw + 2 && area.width as usize > hw as usize + 8 {
            let max_left = right.saturating_sub(hw + 2).saturating_sub(area.x) as usize;
            put_n(buf, area.x, bottom, &left, bright_plain(), max_left);
            put_plain(buf, right - hw, bottom, &hint, dark_style());
        } else {
            // If it does not fit on the right, append it after the left block, clipped to the right
            // edge
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
        // Bottom-right: the pending key sequence (one-row layout also puts it at the far right)
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

/// Current mode: used by the two-row bottom status line
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

/// Zoom notation: `100%` / `250%`, suffixed with LOD at low zoom
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

/// Left half of the persistent status line: `[2/5] ~/proj/x.gtui *   NORMAL   100%`
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

/// File + dirty marker + mode/zoom (for the one-row bottom; the two-row layout assembles it via
/// status_left)
fn status_left_tabbed(view: &View) -> String {
    status_left(view)
}

/// Right half of the persistent status line: `~/projects/graph-tui | 1,4`
pub fn status_right(view: &View) -> String {
    let coords = format!("{},{}", view.cursor.0, view.cursor.1);
    if view.cwd.is_empty() {
        coords
    } else {
        format!("{} | {coords}", view.cwd)
    }
}

/// Draw the persistent status line: on the left the file + mode + context (left-truncated, keeping
/// the filename), on the right the working directory and coordinates; when narrow, drop cwd first,
/// keep coordinates
fn draw_status_row(buf: &mut Buffer, area: Rect, view: &View, left: &str, y: u16) {
    let right = area.x + area.width;
    let coords = format!("{},{}", view.cursor.0, view.cursor.1);
    let cw = width(&coords) as u16;
    let full = status_right(view);
    let fw = width(&full) as u16;
    // Reserve the coordinate space on the left first, truncate the filename from the left (keeping
    // the filename)
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

/// Mode marker at the front of the status line: shows at a glance whether this is 1:1 editing or a
/// zoomed view How much is drawn at low zoom: text becomes a pile of fragments when shrunk too far
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lod {
    /// Normal: text, labels and edge labels all drawn
    Full,
    /// Shapes only: boxes, points, edges; no text
    Shape,
    /// Even boxes are just solid blocks
    Blocks,
}

/// Below 0.6 zoom draw no text, below 0.3 draw blocks only
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

/// Display notation for a zoom factor: integers have no decimal point, others keep two places with
/// trailing zeros removed Echo-slot content: command echo / hint (the command line owns its own row
/// and does not go here). Nothing when idle The note shown when the source file was changed
/// externally (with two ways out)
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
    // Echo first; then the "disk changed" way out; then "some names did not fit"
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

/// Box at a screen cell (click to jump): hit by the box's on-screen rectangle. Cells outside the
/// picture (status line, legend) do not count
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

/// Key reference: Chinese
const HELP_KEYS_ZH: &[(&str, &str)] = &[
    ("h j k l / 方向键", "光标移动一格(5l = 走五格);光标下面是什么就选什么"),
    ("Ctrl-d / Ctrl-u", "半屏往下 / 往上跳一个节点(远距离旅行)"),
    ("Ctrl-o / Ctrl-i", "回到上一个 / 下一个跳过的节点"),
    ("n / N", "下一个 / 上一个搜索命中"),
    ("/", "搜节点名、别名、首字母;纯数字 = 聚焦那个分组"),
    ("L", "切换图层(只看/只动当前层)"),
    ("v", "区块选择:锚点落在光标格,挪光标扩大;Enter / 空格确认后命令作用于整块"),
    ("Ctrl-hjkl", "挪动选中的节点(可带数字前缀,如 10 + Ctrl-l;光标跟着走)"),
    ("f", "把聚焦的分组(没聚焦就是当前节点的分组)铺满画面"),
    ("F", "铺满整幅图"),
    ("z", "把当前节点挪到画面正中"),
    ("+ - / 0", "放大 / 缩小 / 回到 1:1"),
    ("B", "开关节点名(点节点的 label)"),
    ("w / b", "沿这一行往右 / 往左射一条线,跳到撞到的框或边(整幅图;这一行空了就取前 / 后半区里最近的框)"),
    (":rotate", "转方向:不带参数 +90 度,`:rotate 180` 绝对值,`:rotate auto` 交给布局按窗口选"),
    ("L", "循环图层"),
    ("i", "在框里直接改字(空处新建框;光标在边上建边标签;区块选择时只提示)"),
    ("S / M", "S:在光标行下面拆框;M:把选中的框并成一个"),
    ("y / d / p", "复制 / 剪切 / 粘贴图片段(框 + 文字 + 内部边;边界边不进剪贴板)"),
    (":edge-labels", "边标签开关:关掉后边标签不画、不算障碍、也点不到(边会重新布线)"),
    (":edge-labels", "边标签开关 on / off(和 a 一样);`:relabel [all]` 把边标签重新贴回边旁边"),
    ("i(选中边)", "建/改这条边的边标签(空框 Esc 或空回车就撤掉)"),
    ("s / c", "样式菜单(框型 point/none/single/… 、颜色、对齐)/ 颜色菜单(框=边框色,边=边颜色)"),
    ("d / x", "带边剪切 / 只剪框和文字(关联边都从文档里删掉,但只有 d 把边放进剪贴板)"),
    ("p", "把剪贴板里上次删掉的内容粘到光标处"),
    ("t", "改光标框的文字(底行提示)"),
    ("u / Ctrl-z / Ctrl-r", "撤销 / 重做"),
    ("右键", "同一个样式菜单:划过即预览,一次左键应用;键盘 j k 选,l/h 进出一层,Enter 应用"),
    ("e", "两段式连边:源(选区或光标框)-> 移动/选目标 -> Enter / 空格建边;Esc 取消"),
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

/// Key reference: English
const HELP_KEYS_EN: &[(&str, &str)] = &[
    ("h j k l / arrows", "move the cursor one cell (5l = five cells); what it sits on is selected"),
    ("Ctrl-d / Ctrl-u", "jump half a screen down / up (long trips)"),
    ("Ctrl-o / Ctrl-i", "back to the previous / next node you jumped to"),
    ("n / N", "next / previous search hit"),
    ("/", "search name / alias / initials; a bare number focuses that group"),
    ("L", "cycle the layer (view and edit one layer at a time)"),
    ("v", "block select from the cursor cell; Enter / space confirms, then commands apply to the whole block"),
    ("Ctrl-hjkl", "nudge the selected boxes (count prefix works: 10 then Ctrl-l)"),
    ("f", "fit the focused group (or this node's group) to the screen"),
    ("F", "fit the whole document to the screen"),
    ("z", "centre this node"),
    ("+ - / 0", "zoom in / out / back to 1:1"),
    ("B", "toggle node names (point-node labels)"),
    ("w / b", "cast a horizontal ray right / left to the first box or edge (whole picture; an empty row falls back to the nearest box ahead / behind)"),
    (":rotate", "rotate: bare +90deg, `:rotate 180` absolute, `:rotate auto` lets the layout pick"),
    ("Ctrl+G", "hand the prompt line to $VISUAL / $EDITOR (read back on exit; not for : or /)"),
    ("i", "edit the cursor box's text in place (empty space adds a box; on an edge it adds an edge label; a block selection only warns)"),
    ("S / M", "S: split the box below the cursor line; M: merge the selected boxes"),

    (":edge-labels", "edge labels on / off; `:relabel [all]` puts them back next to their edge"),
    (":node-labels", "node names (the label of a point node) on / off"),
    ("y / d / p", "yank / cut / paste a picture fragment (boxes + texts + internal edges; boundary edges stay out of the clipboard)"),
    ("u / Ctrl-z / Ctrl-r", "undo / redo"),
    ("e", "two-step link: source (selection or cursor box) -> move/select target -> Enter / space; Esc cancels"),
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

/// Key table for the current language
pub fn help_keys(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::Zh => HELP_KEYS_ZH,
        Lang::En => HELP_KEYS_EN,
    }
}

/// Key reference overlay
pub fn render_help(
    buf: &mut Buffer,
    area: Rect,
    lines: &[(&str, &str)],
    lang: Lang,
    tip: Option<&str>,
) {
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
    // A tip occupies one row: add it only if the key table itself fits, otherwise omit the row
    // entirely (in a small terminal the key table is already truncated, one more row would cover
    // the keys)
    let tip_row = tip.filter(|t| !t.is_empty());
    let show_tip = tip_row.is_some() && lines.len() as u16 + 5 <= area.height;
    let h = (lines.len() as u16 + 4 + u16::from(show_tip)).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    // Backdrop: paint this block in the panel colour so it does not blur into the picture below
    for yy in y..(y + h).min(area.y + area.height) {
        // Paint one extra column on each side: if a wide glyph in the map below straddles the panel
        // edge, clearing half would leave residue
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
        // Draw only inside the panel: when the panel is shortened by the window, do not write keys
        // onto the last two (hint) rows
        if yy + 2 >= y + h {
            break;
        }
        put(buf, x + 2, yy, k, bright);
        put(buf, x + 2 + width(k).max(6) as u16 + 2, yy, v, dim);
    }
    if show_tip {
        // The row below the key table and above "any key closes this"
        let label = lang.pick("提示 ", "TIP ");
        let mut row = label.to_string();
        row.push_str(tip_row.unwrap_or(""));
        put_n(
            buf,
            x + 2,
            y + h.saturating_sub(3),
            &row,
            dim,
            (w - 4) as usize,
        );
    }
    put(
        buf,
        x + 2,
        y + h.saturating_sub(2),
        lang.pick("任意键关掉", "any key closes this"),
        dim,
    );
}

/// File-open picker overlay: title, filter word, candidate list (current item highlighted), key
/// hints
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
    // Keep the current item within view as much as possible
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
        // Paint one extra column on each side: if a wide glyph in the map below straddles the panel
        // edge, clearing half would leave residue
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
    // The hint row goes just above the bottom border (the border occupies y + h - 1)
    put(buf, x + 2, y + h - 2, hint, dim);
}

/// Scan the canvas onto the ratatui buffer
#[allow(clippy::too_many_arguments)] // Many fields; a struct just adds a lookup
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
    // When an edge is selected in E, cells occupied by the neighbour node at the other end
    // (highlighted together to show where the edge leads)
    picked_nodes: &HashSet<(i32, i32)>,
    // E-mode highlight scope (None = off)
    ego: Option<&Ego>,
    sources: &[usize],
    // Point nodes cannot carry text: on a hit, paint that cell's background (other elements only
    // colour hit characters). `cur_cells` are the cells of the current one (where the cursor rests)
    hit_cells: &HashSet<(i32, i32)>,
    cur_cells: &HashSet<(i32, i32)>,
) {
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.node[i] == 0 && cv.mask[i] == 0 && cv.deco[i].is_none() {
                continue;
            }
            // Only block selection uses reverse video; the current selection breathes red (no
            // background)

            let selected = marked.contains(&(x, y));
            // Node boxes cover edges; the cursor node's border uses the breathing colour
            if let Some(d) = cv.deco[i] {
                let picked_box =
                    picked.box_at() == Some(d.node as usize) || picked_nodes.contains(&(x, y));
                let mut style = Style::default().fg(Color::Rgb(d.fg.0, d.fg.1, d.fg.2));
                // Arrowheads embedded in borders may also be shared: colour follows the active
                // owner too
                if matches!(
                    cv.border.get(&(x, y)),
                    Some(crate::canvas::BorderMark::Arrow(..))
                ) {
                    if let Some((_, ink)) = arrow_ink_for(cv, (x, y), picked.edge(), ego) {
                        style = style.fg(Color::Rgb(ink.0, ink.1, ink.2));
                    }
                }
                // Search hit background colours only the hit characters (see hit_spans); the border
                // keeps its own
                if let Some((r, gg, b)) = d.bg {
                    style = style.bg(Color::Rgb(r, gg, b));
                }
                let dimmed =
                    (cv.focus != 0 && d.groups & cv.focus == 0) || (cv.layer_filter && !cv.lit[i]);
                if dimmed && !selected && !picked_box {
                    // When dimmed, drop the fill too and keep the terminal background
                    style = dim_style();
                }
                // Selected box: the border breathes between white and red (foreground only;
                // fill/hit background stay) Link source box: a steady bright border (only the
                // target breathes)
                if sources.contains(&(d.node as usize)) && !picked_box && !selected {
                    style = style
                        .fg(Color::Rgb(LINK_SRC.0, LINK_SRC.1, LINK_SRC.2))
                        .add_modifier(Modifier::BOLD);
                }
                if picked_box || (cursor_node == Some(d.node as usize) && drawn) {
                    style = style
                        .fg(pulse_color(BRIGHT, pulse))
                        .add_modifier(Modifier::BOLD);
                }
                if selected {
                    style = style
                        .fg(pulse_color(BRIGHT, pulse))
                        .add_modifier(Modifier::BOLD);
                }
                let cell = &mut buf[(x as u16, y as u16)];
                cell.set_char(d.ch);
                cell.set_style(style);
                continue;
            }
            let (r, gg, b) = cv.fg[i];
            // Non-node cells: this cell's shape belongs only to the edge "on top". Another edge
            // crossing here breaks one cell instead of merging direction bits into ┼ / ├ Arrowheads
            // are topmost: the glyph becomes the arrowhead and its colour follows that arrowhead's
            // edge. But a raised edge (the selected one) overrides others' arrowheads; otherwise
            // selecting an edge leaving this point would show others' incoming arrowheads as if
            // both ends had one Arrowhead cells: owners decide whose arrowhead it is (a shared
            // arrowhead cell may belong to several edges). A raised edge overrides others'
            // arrowheads, but its own arrowheads are not overridden
            let shape = cv.shape_edge[i];
            let owners = cv.arrow_owners.get(&(x, y));
            // On cells just outside the selected edge's port, others' arrowheads are hidden:
            // otherwise selecting a forward edge shows another's arrowhead at the start, looking
            // like <->
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
            // Selected edge: arrowhead cells actually drawn follow owners (a shared cell may hold
            // several edges), other cells use the hit ownership edge_of
            let on_picked_edge = match picked {
                Pick::Edge(e) if cv.arrow[i].is_some() && !foreign_arrow => {
                    cv.arrow_owners.get(&(x, y)).is_some_and(|o| o.contains(&e))
                }
                Pick::Edge(e) => cv.edge_of[i] == e as i32 || cv.shape_edge[i] as usize == e,
                _ => false,
            };
            // Shared arrowheads take the "currently active owner" colour (selected > the only lit
            // in E > the visible one); overlay redraws E-mode arrowheads with the same function so
            // the colours match
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
                    // A directed edge's arrowhead covers the stroke (topmost)
                    ch = a;
                } else if (ch == '─' || ch == '│')
                    && cv.mask[i].count_ones() <= 2
                    && !cv.solid[i]
                    && !cv.style[i].keeps(x, y)
                {
                    // Dashes/dots break only pure straight strokes: corners, junctions and
                    // crossings stay solid, otherwise the turn direction is lost
                    ch = ' ';
                }
            }
            let mut style = Style::default().fg(Color::Rgb(r, gg, b));
            if let Some((ar, ag, ab)) = arrow_ink {
                style = style.fg(Color::Rgb(ar, ag, ab));
            }
            // Hubs are normally bright white so they stand out at a glance; but if the hub is also
            // in the cursor's group, use that group's colour so the whole path reads by colour
            if cv.node[i] == 2 && !cv.mine[i] {
                style = bright;
            }
            // When focusing some groups, all other groups (nodes and strokes alike) are dimmed,
            // leaving only the focused groups' colours. When viewing by layer, other layers are
            // dimmed similarly
            let dimmed = (if cv.focus == 0 {
                false
            } else if cv.node[i] != 0 {
                // Node cell: check whether this node itself is in a focused group (other groups
                // merely pass through)
                cv.node_groups[i] & cv.focus == 0
            } else {
                cv.groups_here[i] & cv.focus == 0
            }) || (cv.layer_filter && !cv.lit[i]);
            if dimmed && !selected && !on_picked_edge {
                style = dim_style();
            } else if let Some((br, bg_, bb)) = cv.edge_bg[i] {
                // The edge's own background: painted over the terminal background, not into boxes
                // (boxes are the deco branch)
                style = style.bg(Color::Rgb(br, bg_, bb));
            }
            // Elements without a box: glyph and text breathe themselves (solid feel, no
            // background). A solid glyph is used only when "drawing the cursor into the picture"
            // (dump/export do not draw the cursor)
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
                // Search-hit point node: paint one cell's background (orange for the current hit)
                style = search_style(cur_cells.contains(&(x, y)));
            } else if picked_nodes.contains(&(x, y)) && cv.node[i] != 0 {
                // Neighbour point node: the glyph breathes too (not solid; only the cursor one is
                // solid)
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            } else if on_picked_edge {
                style = style
                    .fg(pulse_color(BRIGHT, pulse))
                    .add_modifier(Modifier::BOLD);
            }
            // Block selection uses the same red/white breathing, no background and no reverse video
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

/// Roughly how many screen cells separate adjacent boxes (used to decide automatic node-name
/// labels)
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

/// Draw a border around an overlay: it only fills with spaces, so without a frame it blurs into the
/// picture
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

/// Write a run of text, dropping it beyond the picture (bounds always clip so an overlay cannot
/// write off-screen)
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    put_n(buf, x, y, text, style, max);
}

/// Raw output that does not interpret `**bold**` / `/italic/` markers: used for UI text (command
/// line, paths, echo) Truncate from the left to `room` columns, keeping the tail (the filename is
/// last). Prefix with `...` when truncated; if even `...` plus one column does not fit, draw
/// nothing
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
    let spans = [text::Span::plain(text)];
    text::put(buf, x, y, &spans, style, &text::TextOverlay::default(), max);
}

/// Same as above but at most max_w columns (`**bold**` / `/italic/` markers in labels are
/// zero-width)
fn put_n(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, max_w: usize) {
    if x >= buf.area.width || y >= buf.area.height || max_w == 0 {
        return;
    }
    let max = max_w.min((buf.area.width - x) as usize);
    let spans = text::parse(text);
    text::put(buf, x, y, &spans, style, &text::TextOverlay::default(), max);
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // Tests build a default, then tweak a few fields
mod tests;
