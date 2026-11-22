//! Layout: place the document's boxes on integer cells (character cells, x right, y down).
//!
//! Four algorithms with different input requirements:
//!   geo      coordinates are already cells; keep them as-is (only resolve overlaps)
//!   relax    start from the current positions and spring-relax: even out the pitch
//!           by box size, and snap edge directions to horizontal/vertical/45 degrees
//!   force    Fruchterman-Reingold: start from the current positions, or scatter
//!           deterministically when there are none
//!   layered  Sugiyama-lite, suited to directed dependency graphs / flowcharts
//!
//! Every algorithm works on box centres (f64); the finish converts them to top-left
//! integer cells:
//!   at = (round(cx) - (w - 1) / 2, round(cy) - (h - 1) / 2)
//! then writes them back through `Document::place_box` (which also syncs text
//! positions), and finally `rebuild`.
//!
//! Sizes affect spacing: the pitch comes from the widest box, and boxes keep at least
//! one empty cell between them. Every layout ends in `settle`, which puts each box in
//! the nearest free spot in a deterministic order, one cell apart, so no layout can
//! overlap boxes or leave two of them touching.
//!
//! Scale and complexity (n boxes, m edges, scale factor k = sqrt(n / 262)):
//!   relax    O(n + m + repel grid neighbourhood) per round,
//!           rounds = min(600 k, iteration budget), see Cfg::iters
//!   force    same round count; O(n + m + point pairs in the grid neighbourhood)
//!   layered  DFS cycle breaking + topological layering + a few barycentre sweeps,
//!           all O(n + m), and polish at the end
//!   polish   rounding O(n * rings); each round of separate / spread_all /
//!           octilinear scans the (2r+1)^2 neighbourhood, stopping early when idle
//!   settle   O(own area + bounded ring search on conflicts) per box, rings capped
//! Ring counts, search radii, iteration counts and post-processing rounds all have
//! caps (see Cfg and the constants below): when no free spot exists a box goes on a
//! new row under the existing bbox, so layout never hangs.
//!
//! Determinism: hash maps are only probed by key, never iterated; every sort breaks
//! ties on box index, and the no-position start uses the golden angle, not a random
//! number. The same document always lays out identically.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::document::Document;
use crate::lang::Lang;

/// Lower bound on the target pitch between adjacent boxes (cells)
const STEP: f64 = 2.6;
/// Baseline relax iteration count
const ITERS: usize = 600;
/// Cap on total work: rounds x boxes stays at this magnitude, so more boxes just get
/// fewer rounds. A round is one grid pass plus neighbourhood point pairs; with the
/// total capped here, a few thousand boxes still finish in seconds
const ITER_BUDGET: usize = 240_000;
/// Force layout work canvas: square, side = max(2 cells, FORCE_SPAN * sqrt(boxes)).
/// The square root shares the cells out evenly among boxes; going a bit above STEP
/// leaves the springs room (a tight canvas pushes boxes onto the border, and the
/// finish then has to move them one by one).
const FORCE_SPAN: f64 = STEP * 1.6;
/// Upper bound on the force layout target pitch (cells): a small graph must not
/// spread to dozens of cells and leave blanks in the middle
const FORCE_K_MAX: f64 = 4.0;
/// Force layout gravity: a gentle pull towards the canvas centre.
/// With repulsion and springs alone, boxes drift onto the border and bunch up there
/// (touching boxes all around the rim, which the finish has to move one by one,
/// growing the picture several times over); gravity spreads the density evenly.
const GRAVITY: f64 = 0.06;
/// Upper bound on post-processing rounds: each round scans every edge, so more boxes
/// settle for fewer rounds
const ROUND_CAP: usize = 64;
/// Upper bound on octilinear rounds: the most expensive step (every edge tries cells
/// in 8 directions)
/// Even so, more boxes do not get more rounds -- the later rounds mostly spin in
/// place, so that time is better spent elsewhere.
const OCTI_CAP: usize = 24;
/// Even the largest graph iterates at least this many rounds
const MIN_ITERS: usize = 40;
/// Straightening force: pull three-node runs into a line so corridors render straight
const K_STRAIGHT: f64 = 0.06;
/// Anchor strength: smaller ignores the input density more and evens out the pitch
const ANCHOR_W: f64 = 0.030;
/// Lower bound on the layered layer gap (cells): >= 3 keeps adjacent layers apart
const LAYER_GAP: f64 = 3.0;
/// Minimum cells between adjacent boxes in one layered lane
const LANE_GAP: f64 = 3.0;
/// Golden angle (radians): scatters points when there is no position, no RNG needed
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

/// Scale parameters: more boxes means larger search radii and more iterations --
/// otherwise post-processing finds no free spot in dense areas and boxes overlap
/// (a 262-box graph is the baseline, k = 1).
struct Cfg {
    n: usize,
    k: f64,
}

impl Cfg {
    fn of(n: usize) -> Cfg {
        Cfg {
            n,
            k: (n as f64 / 262.0).sqrt().max(1.0),
        }
    }
    /// Cap on rings searched for a free spot (baseline 64; large k stops at 96 -- past
    /// that, packing tighter beats searching further)
    fn rings(&self) -> i32 {
        ((64.0 * self.k) as i32).min(96)
    }
    /// best_cell search radius (baseline 3, i.e. 7x7; capped at 5)
    fn radius(&self) -> i32 {
        ((3.0 * self.k).round() as i32).clamp(2, 5)
    }
    /// Cells clean_cell may move a box (baseline 2; capped at 4)
    fn nudge(&self) -> i32 {
        ((2.0 * self.k).round() as i32).clamp(1, 4)
    }
    /// Post-processing rounds: baseline x k, tightened by graph size.
    /// The baseline was tuned on a 262-box graph, which small graphs follow exactly;
    /// large graphs do not converge in dense areas after dozens of rounds, so that time
    /// is better spent elsewhere (the final settle is the safety net)
    fn rounds(&self, base: usize) -> usize {
        let cap = if self.n > 2000 {
            6
        } else if self.n > 600 {
            16
        } else {
            ROUND_CAP
        };
        ((base as f64 * self.k).round() as usize).min(cap)
    }
    /// Main iteration count: baseline x k, capped by ITER_BUDGET but never below MIN_ITERS
    fn iters(&self, n: usize, base: usize) -> usize {
        let want = (base as f64 * self.k).round() as usize;
        want.min((ITER_BUDGET / n.max(1)).max(MIN_ITERS))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Geo,
    Relax,
    Force,
    Layered,
    /// Tree/forest: parents centred, siblings in original order
    Tree,
    /// Chain / pure cycle: the main line straight, branches hanging below
    Chain,
    /// Pick a concrete layout from structure (auto is only reached via `:layout auto` /
    /// `--layout auto`)
    Auto,
}

impl Layout {
    /// The six concrete layouts cycled by the `r` key (auto excluded)
    pub fn all() -> [Layout; 6] {
        [
            Layout::Geo,
            Layout::Relax,
            Layout::Force,
            Layout::Layered,
            Layout::Tree,
            Layout::Chain,
        ]
    }

    /// Names accepted on the command line / in ":" (case and surrounding spaces ignored)
    pub fn parse(s: &str) -> Option<Layout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "geo" | "real" | "geographic" => Some(Layout::Geo),
            "relax" | "spring" | "relaxed" => Some(Layout::Relax),
            "force" | "fr" => Some(Layout::Force),
            "layered" | "dag" | "sugiyama" => Some(Layout::Layered),
            "tree" | "forest" | "org" => Some(Layout::Tree),
            "chain" | "path" | "pipeline" | "line" => Some(Layout::Chain),
            "auto" => Some(Layout::Auto),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Layout::Geo => "geo",
            Layout::Relax => "relax",
            Layout::Force => "force",
            Layout::Layered => "layered",
            Layout::Tree => "tree",
            Layout::Chain => "chain",
            Layout::Auto => "auto",
        }
    }

    /// Chinese/English name for the status line
    pub fn label(&self, lang: Lang) -> &'static str {
        match self {
            Layout::Geo => lang.pick("真实坐标", "geographic"),
            Layout::Relax => lang.pick("松弛", "relaxed"),
            Layout::Force => lang.pick("力导", "force"),
            Layout::Layered => lang.pick("分层", "layered"),
            Layout::Tree => lang.pick("树", "tree"),
            Layout::Chain => lang.pick("链", "chain"),
            Layout::Auto => lang.pick("自动", "auto"),
        }
    }

    /// Next layout in the `r` key cycle
    pub fn next(&self) -> Layout {
        match self {
            Layout::Geo => Layout::Relax,
            Layout::Relax => Layout::Force,
            Layout::Force => Layout::Layered,
            Layout::Layered => Layout::Tree,
            Layout::Tree => Layout::Chain,
            Layout::Chain => Layout::Geo,
            // auto is not part of the cycle: pressing `r` enters geo
            Layout::Auto => Layout::Geo,
        }
    }
}

/// Run one layout and write the result back into the document's box positions (text
/// positions are synced by place_box). All coordinates are integers at the end, and
/// any two box rectangles are at least one cell apart.
/// Layout orientation: auto picks per window, the rest are fixed angles
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Orientation {
    #[default]
    Auto,
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Orientation {
    pub fn parse(s: &str) -> Option<Orientation> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Orientation::Auto),
            "0" => Some(Orientation::Deg0),
            "90" => Some(Orientation::Deg90),
            "180" => Some(Orientation::Deg180),
            "270" => Some(Orientation::Deg270),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Orientation::Auto => "auto",
            Orientation::Deg0 => "0",
            Orientation::Deg90 => "90",
            Orientation::Deg180 => "180",
            Orientation::Deg270 => "270",
        }
    }

    /// Fixed angle as quarter turns; auto has no fixed value
    pub fn quarters(&self) -> Option<i32> {
        match self {
            Orientation::Auto => None,
            Orientation::Deg0 => Some(0),
            Orientation::Deg90 => Some(1),
            Orientation::Deg180 => Some(2),
            Orientation::Deg270 => Some(3),
        }
    }

    pub fn degrees(&self) -> i32 {
        self.quarters().map(|k| k * 90).unwrap_or(0)
    }
}

/// Layout diagnostics: crossings, dropped back edges and structured warnings.
/// Callers (status line / headless) use it to say what this layout lost and why
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutReport {
    /// Edge crossings between adjacent layers (layered only, 0 elsewhere)
    pub crossings: usize,
    /// Directed edges dropped so a cyclic graph can be layered
    pub reversed: usize,
    pub warnings: Vec<LayoutWarning>,
    /// Layout actually used (the name `auto` resolved to: tree / chain / layered / relax / ...)
    pub profile: &'static str,
    /// Number of weakly connected components
    pub components: usize,
}

/// One warning: a stable code plus a human-readable message
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutWarning {
    pub code: &'static str,
    pub message: String,
    pub id: Option<String>,
}

impl LayoutReport {
    /// Join into one line for the status line / echo (at most `n` warnings)
    pub fn summary(&self, n: usize) -> String {
        self.warnings
            .iter()
            .take(n)
            .map(|w| w.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Layered spacing (cells): minimum lane gap, minimum layer gap, canvas margin
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub lane_gap: f64,
    pub layer_gap: f64,
    pub margin: f64,
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing {
            lane_gap: LANE_GAP,
            layer_gap: LAYER_GAP,
            margin: 0.0,
        }
    }
}

impl Spacing {
    /// Three optional values from config / the command line: defaults when absent,
    /// clamped to sane ranges
    pub fn of(lane: Option<f64>, layer: Option<f64>, margin: Option<f64>) -> Spacing {
        let d = Spacing::default();
        let pick = |v: Option<f64>, d: f64, lo: f64, hi: f64| match v {
            Some(x) if x.is_finite() => x.clamp(lo, hi),
            _ => d,
        };
        Spacing {
            lane_gap: pick(lane, d.lane_gap, 1.0, 64.0),
            layer_gap: pick(layer, d.layer_gap, 1.0, 64.0),
            margin: pick(margin, d.margin, 0.0, 256.0),
        }
    }
}

/// Layout with default spacing
pub fn apply(doc: &mut Document, l: Layout) -> Result<LayoutReport, String> {
    apply_with(doc, l, Spacing::default())
}

/// Layout with explicit spacing
pub fn apply_with(doc: &mut Document, l: Layout, spacing: Spacing) -> Result<LayoutReport, String> {
    // auto: pick a concrete layout from structure, then take the same path (the report
    // keeps auto's own warnings)
    if l == Layout::Auto {
        let (picked, mut report) = auto_layout(doc);
        let mut out = apply_with(doc, picked, spacing)?;
        report.crossings = out.crossings;
        report.reversed = out.reversed;
        report.profile = out.profile;
        report.components = out.components;
        report.warnings.append(&mut out.warnings);
        // Multiple components: lay out each block, then pack them; blocks share no ranks
        // and keep a gutter
        if report.components > 1 {
            pack_components(doc);
        }
        return Ok(report);
    }
    let n = doc.boxes.len();
    if n == 0 {
        return Ok(LayoutReport::default());
    }
    let sizes: Vec<(i32, i32)> = doc
        .boxes
        .iter()
        .map(|b| (b.size.0 as i32, b.size.1 as i32))
        .collect();
    if let Some(i) = sizes.iter().position(|s| s.0 <= 0 || s.1 <= 0) {
        return Err(format!("box {:?}: empty size", doc.boxes[i].id));
    }
    let cfg = Cfg::of(n);
    // geo positions are already cells: use at as-is, never derive them from centres (an
    // even width would be off by one)
    let mut report = LayoutReport {
        profile: l.name(),
        components: crate::profile::analyze(doc).components,
        ..LayoutReport::default()
    };
    let desired: Vec<(i32, i32)> = if l == Layout::Geo {
        doc.boxes.iter().map(|b| b.at).collect()
    } else {
        let centers = match l {
            Layout::Relax => relax(doc, &sizes),
            Layout::Force => force(doc, &sizes),
            Layout::Layered => {
                let (cands, r) = layered(doc, &sizes, spacing);
                report = r;
                // Both candidates (with and without dummy nodes) run through settle before the
                // comparison: ordering-stage numbers cannot predict the picture after boxes move
                let pinned: Vec<bool> = (0..n).map(|i| doc.is_edge_label(i)).collect();
                let orig: Vec<(i32, i32)> = doc.boxes.iter().map(|b| b.at).collect();
                let arcs = edge_pairs(doc);
                let mut best: Option<(usize, &Vec<(f64, f64)>)> = None;
                for cand in &cands {
                    let want: Vec<(i32, i32)> = cand
                        .iter()
                        .map(|&(x, y)| (x.round() as i32, y.round() as i32))
                        .collect();
                    // settle a copy here just for scoring; the real placement happens once in the
                    // common path below
                    let laid = settle(&want, &sizes, &cfg, &pinned, &orig);
                    let pos: Vec<(f64, f64)> =
                        laid.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
                    let rank = rank_from_rows(&pos);
                    let cross = crossings_final(&pos, &rank, &arcs);
                    if best.as_ref().is_none_or(|(bc, _)| cross < *bc) {
                        best = Some((cross, cand));
                    }
                }
                match best {
                    Some((_, cand)) => cand.clone(),
                    None => Vec::new(),
                }
            }
            Layout::Tree => {
                let (p, r) = tree_layout(doc, &sizes, spacing);
                report = r;
                p
            }
            Layout::Chain => {
                let (p, r) = chain_layout(doc, &sizes, spacing);
                report = r;
                p
            }
            // geo and auto are handled above; the empty table only completes the match
            Layout::Geo | Layout::Auto => Vec::new(),
        };
        to_top_left(&centers, &sizes)
    };
    // Edge labels are pinned: layout does not move them, other boxes route around them
    let pinned: Vec<bool> = (0..n).map(|i| doc.is_edge_label(i)).collect();
    let orig: Vec<(i32, i32)> = doc.boxes.iter().map(|b| b.at).collect();
    let placed = settle(&desired, &sizes, &cfg, &pinned, &orig);
    // Margin: shift all content to (margin, margin); geo stays as-is
    let placed = if l != Layout::Geo && spacing.margin > 0.0 {
        let m = spacing.margin.round() as i32;
        let x0 = placed.iter().map(|q| q.0).min().unwrap_or(0);
        let y0 = placed.iter().map(|q| q.1).min().unwrap_or(0);
        placed
            .into_iter()
            .map(|(x, y)| (x - x0 + m, y - y0 + m))
            .collect()
    } else {
        placed
    };
    if l == Layout::Layered {
        // Ranks come from the desired positions (where y is exactly rank * layer gap);
        // coordinates come from after settle, so the count matches the picture
        let want: Vec<(f64, f64)> = placed.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
        let rank = rank_from_rows(&want);
        let pos: Vec<(f64, f64)> = placed.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
        report.crossings = crossings_final(&pos, &rank, &edge_pairs(doc));
    }
    // Layered layouts record each row's rank: drawing picks the top/bottom exit by rank
    let ranks = if matches!(l, Layout::Layered | Layout::Tree | Layout::Chain) {
        let pos: Vec<(f64, f64)> = placed.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
        rank_from_rows(&pos)
    } else {
        Vec::new()
    };
    for (i, at) in placed.into_iter().enumerate() {
        doc.place_box(i, at);
    }
    doc.rebuild();
    doc.set_ranks(ranks);
    // Each branch fills only the fields it cares about; fill in the common ones here
    if report.profile.is_empty() {
        report.profile = l.name();
    }
    if report.components == 0 {
        report.components = crate::profile::analyze(doc).components;
    }
    Ok(report)
}

/// Without an explicit layout, pick one: `auto` decides by structure, whether the
/// edges are directed and whether a cycle exists. Coordinates are not considered.
pub fn pick(_doc: &Document) -> Layout {
    // Always hand off to auto: it picks among chain/tree/layered/relax by structure,
    // and breaks cycles with layered when there are any
    Layout::Auto
}

/// Whether the directed graph has a cycle (self loops and back edges count)
pub fn directed_cycle(doc: &Document) -> bool {
    let arcs = directed_arcs(doc);
    if arcs.is_empty() {
        return false;
    }
    if doc
        .edges
        .iter()
        .any(|e| e.dir.is_arrow() && e.from == e.to && doc.box_idx(&e.from).is_some())
    {
        return true;
    }
    has_cycle(doc.boxes.len(), &arcs)
}

/// Directed edges (by dir); endpoints are box indices
fn directed_arcs(doc: &Document) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for e in &doc.edges {
        if !e.dir.is_arrow() {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            // A self loop has no direction and takes no part in layering
            continue;
        }
        out.push((a, b));
    }
    out
}

/// All edges regardless of group: post-processing keeps both ends of every edge apart
fn edge_pairs(doc: &Document) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(doc.edges.len());
    for (i, _) in doc.edges.iter().enumerate() {
        if let Some((a, b)) = doc.edge_ends(i) {
            if a != b {
                // A self loop needs no separation
                out.push((a, b));
            }
        }
    }
    out
}

/// Widest and tallest box (at least 1x1)
fn max_size(sizes: &[(i32, i32)]) -> (i32, i32) {
    sizes
        .iter()
        .fold((1, 1), |(w, h), &(a, b)| (w.max(a), h.max(b)))
}

/// Lane / layer pitch: with boxes, leave one empty cell around the largest box;
/// without boxes, use the configured layered gaps. Returns (lane, layer).
/// (lane, layer)
fn pitch(sizes: &[(i32, i32)], sp: Spacing) -> (f64, f64) {
    let (w, h) = max_size(sizes);
    (
        ((w + 1) as f64).max(sp.lane_gap),
        ((h + 1) as f64).max(sp.layer_gap),
    )
}

/// Relax / force pitch, one value per axis: horizontal leaves one cell around the
/// widest box, vertical around the tallest.
/// A single (larger) value would push one-row-high boxes apart vertically by the
/// same amount
fn unit_axes(sizes: &[(i32, i32)]) -> (f64, f64) {
    let (w, h) = max_size(sizes);
    (((w + 1) as f64).max(STEP), ((h + 1) as f64).max(STEP))
}

/// Isotropic layout pitch: one empty cell around the largest box
fn unit(sizes: &[(i32, i32)]) -> f64 {
    let (w, h) = max_size(sizes);
    ((w + 1).max(h + 1) as f64).max(STEP)
}

/// Centres -> top-left integer cells
fn to_top_left(centers: &[(f64, f64)], sizes: &[(i32, i32)]) -> Vec<(i32, i32)> {
    centers
        .iter()
        .zip(sizes)
        .map(|(&(cx, cy), &(w, h))| (round_i32(cx) - (w - 1) / 2, round_i32(cy) - (h - 1) / 2))
        .collect()
}

/// Round and clamp to a safe range so extreme values cannot overflow later sums
fn round_i32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    v.round()
        .clamp(i32::MIN as f64 / 4.0, i32::MAX as f64 / 4.0) as i32
}

/// Whether the positions collapse to a blob (all boxes near one point): such input
/// counts as no position
fn flat(p: &[(f64, f64)]) -> bool {
    if p.len() < 2 {
        return true;
    }
    let (x0, y0, x1, y1) = bbox(p);
    (x1 - x0) < 0.5 && (y1 - y0) < 0.5
}

/// Scale centres uniformly into a w x h box: shape only, y is not flipped (cell y
/// points down)
fn fit_centers(p: &[(f64, f64)], w: f64, h: f64) -> Vec<(f64, f64)> {
    let (x0, y0, x1, y1) = bbox(p);
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    let s = ((w - 6.0).max(1.0) / gw).min((h - 4.0).max(1.0) / gh);
    let ox = (w - gw * s) / 2.0;
    let oy = (h - gh * s) / 2.0;
    p.iter()
        .map(|q| ((q.0 - x0) * s + ox, (q.1 - y0) * s + oy))
        .collect()
}

fn bbox(p: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in p {
        x0 = x0.min(q.0);
        y0 = y0.min(q.1);
        x1 = x1.max(q.0);
        y1 = y1.max(q.1);
    }
    (x0, y0, x1, y1)
}

/// Target edge direction as a unit vector in cell coordinates: horizontal, vertical
/// or 45 degrees
fn snap_dir(dx: f64, dy: f64) -> (f64, f64) {
    let (ax, ay) = (dx.abs(), dy.abs());
    if ay < ax * 0.4 {
        (dx.signum(), 0.0)
    } else if ax < ay * 0.4 {
        (0.0, dy.signum())
    } else {
        // 45 degrees: this slope in cell coordinates renders as a single right-angle bend
        let k = std::f64::consts::FRAC_1_SQRT_2;
        (dx.signum() * k, dy.signum() * k)
    }
}

// ---- finish: keep boxes apart ----

/// Whether a candidate rectangle (top-left at, size w x h) fits: neither its own
/// cells nor the ring around them may be taken, which keeps two boxes one cell apart
fn fits(at: (i32, i32), (w, h): (i32, i32), taken: &HashSet<(i32, i32)>) -> bool {
    for dy in -1..=h {
        for dx in -1..=w {
            if taken.contains(&(at.0 + dx, at.1 + dy)) {
                return false;
            }
        }
    }
    true
}

/// Record the cells the rectangle occupies (not the ring; fits expands by one)
fn mark(at: (i32, i32), (w, h): (i32, i32), taken: &mut HashSet<(i32, i32)>, bottom: &mut i32) {
    for dy in 0..h {
        for dx in 0..w {
            taken.insert((at.0 + dx, at.1 + dy));
        }
    }
    *bottom = (*bottom).max(at.1 + h - 1);
}

/// Spread boxes over their target positions, keeping any two rectangles one cell
/// apart.
///
/// Order is (target y, target x, index), so earlier boxes keep their spot and the
/// layout's original shape is not pushed around by later ones. When a target is
/// taken, the nearest free spot is searched ring by ring (capped);
/// if none is found the box goes on a new row below existing content, which is
/// always free.
fn settle(
    desired: &[(i32, i32)],
    sizes: &[(i32, i32)],
    cfg: &Cfg,
    pinned: &[bool],
    orig: &[(i32, i32)],
) -> Vec<(i32, i32)> {
    let n = desired.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        desired[a]
            .1
            .cmp(&desired[b].1)
            .then(desired[a].0.cmp(&desired[b].0))
            .then(a.cmp(&b))
    });
    let mut taken: HashSet<(i32, i32)> = HashSet::with_capacity(n.saturating_mul(4));
    let mut out = vec![(0, 0); n];
    let mut bottom = i32::MIN / 4;
    // Pinned boxes claim their cells first: they never move, others treat them as
    // obstacles
    for i in 0..n {
        if pinned.get(i).copied().unwrap_or(false) {
            mark(orig[i], sizes[i], &mut taken, &mut bottom);
        }
    }
    for i in order {
        let size = sizes[i];
        if pinned.get(i).copied().unwrap_or(false) {
            out[i] = orig[i];
            continue;
        }
        let (x0, y0) = desired[i];
        let mut spot = if fits((x0, y0), size, &taken) {
            Some((x0, y0))
        } else {
            None
        };
        if spot.is_none() {
            let cap = cfg.rings();
            // Walk only the ring cells, not the whole square: scanning the block would cost
            // O(r^2) per ring in dense areas
            'rings: for r in 1..=cap {
                for dy in -r..=r {
                    if dy == -r || dy == r {
                        for dx in -r..=r {
                            let c = (x0 + dx, y0 + dy);
                            if fits(c, size, &taken) {
                                spot = Some(c);
                                break 'rings;
                            }
                        }
                    } else {
                        for dx in [-r, r] {
                            let c = (x0 + dx, y0 + dy);
                            if fits(c, size, &taken) {
                                spot = Some(c);
                                break 'rings;
                            }
                        }
                    }
                }
            }
        }
        let at = spot.unwrap_or((x0, bottom + 2));
        mark(at, size, &mut taken, &mut bottom);
        out[i] = at;
    }
    out
}

// ---- relax ----

/// Repulsion: push boxes apart when they get too close (a grid finds candidates, so
/// not O(n^2)).
/// gap is the target pitch, already scaled by box size
fn repel(p: &[(f64, f64)], f: &mut [(f64, f64)], k: f64, gap: f64) {
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / gap).floor() as i32, (q.1 / gap).floor() as i32);
        grid.entry(cell).or_default().push(i);
    }
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / gap).floor() as i32, (q.1 / gap).floor() as i32);
        for di in -1..=1 {
            for dj in -1..=1 {
                for &j in grid
                    .get(&(cell.0 + di, cell.1 + dj))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if j <= i {
                        continue;
                    }
                    let (dx, dy) = (p[j].0 - q.0, p[j].1 - q.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    if len >= gap {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 {
                        (0.0, 1.0)
                    } else {
                        (dx / len, dy / len)
                    };
                    let push = (gap - len) * k;
                    f[i].0 -= ux * push;
                    f[i].1 -= uy * push;
                    f[j].0 += ux * push;
                    f[j].1 += uy * push;
                }
            }
        }
    }
}

/// Round and de-overlap: one box per cell; if taken, search ring by ring for the
/// nearest free cell. Coordinates here are cells; callers already divided by cell
/// (see polish).
fn snap_to_grid(p: &[(f64, f64)], cfg: &Cfg) -> Vec<(f64, f64)> {
    let mut taken: HashSet<(i32, i32)> = HashSet::with_capacity(p.len());
    let mut out = vec![(0.0, 0.0); p.len()];
    let mut order: Vec<usize> = (0..p.len()).collect();
    order.sort_by(|&a, &b| {
        p[a].1
            .total_cmp(&p[b].1)
            .then(p[a].0.total_cmp(&p[b].0))
            .then(a.cmp(&b))
    });
    for i in order {
        let (x0, y0) = (round_i32(p[i].0), round_i32(p[i].1));
        let mut spot = (x0, y0);
        // Walk only the ring cells, not the whole square: scanning the block would cost
        // O(r^2) per ring in dense areas
        'grow: for r in 0..cfg.rings() {
            for dy in -r..=r {
                if dy == -r || dy == r {
                    for dx in -r..=r {
                        if !taken.contains(&(x0 + dx, y0 + dy)) {
                            spot = (x0 + dx, y0 + dy);
                            break 'grow;
                        }
                    }
                } else {
                    for dx in [-r, r] {
                        if !taken.contains(&(x0 + dx, y0 + dy)) {
                            spot = (x0 + dx, y0 + dy);
                            break 'grow;
                        }
                    }
                }
            }
        }
        taken.insert(spot);
        out[i] = (spot.0 as f64, spot.1 as f64);
    }
    out
}

/// Eight directions: axes and both diagonals
const DIRS8: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Whether a candidate cell stays inside its box's leash (the finish only nudges
/// locally; see polish)
fn within(leash: (&[(i32, i32)], i32), who: usize, c: (i32, i32)) -> bool {
    let (origin, drift) = leash;
    let o = origin[who];
    (c.0 - o.0).abs().max((c.1 - o.1).abs()) <= drift
}

/// Snap edge directions to horizontal, vertical or 45 degrees by moving an endpoint
/// to the nearest free cell along that direction, so edges render as one clean
/// stroke instead of a staircase.
fn octilinear(
    p: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    cfg: &Cfg,
    leash: (&[(i32, i32)], i32),
) {
    for _ in 0..cfg.rounds(24).min(OCTI_CAP) {
        let mut taken: HashSet<(i32, i32)> = p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (ax, ay) = (p[a].0 as i32, p[a].1 as i32);
            let (bx, by) = (p[b].0 as i32, p[b].1 as i32);
            let (dx, dy) = (bx - ax, by - ay);
            if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
                continue;
            }
            // Try moving b first; when b is crowded, move a onto the same-direction cell
            // centred on b
            let spot = clean_cell((ax, ay), (bx, by), &taken, cfg.nudge(), leash, b)
                .map(|c| (b, c))
                .or_else(|| {
                    clean_cell((bx, by), (ax, ay), &taken, cfg.nudge(), leash, a).map(|c| (a, c))
                });
            let Some((who, c)) = spot else { continue };
            let old = (p[who].0 as i32, p[who].1 as i32);
            taken.remove(&old);
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// Around base, look in the 8 directions for the free cell closest to want (used to
/// straighten edges). Cells outside who's leash are filtered first (see best_cell).
fn clean_cell(
    base: (i32, i32),
    want: (i32, i32),
    taken: &HashSet<(i32, i32)>,
    nudge: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let l = (want.0 - base.0).abs().max((want.1 - base.1).abs());
    let mut best: Option<(i64, (i32, i32))> = None;
    for len in [l - 1, l, l + 1, l + 2] {
        if len < 1 {
            continue;
        }
        for (ux, uy) in DIRS8 {
            let c = (base.0 + ux * len, base.1 + uy * len);
            // Only allow a few cells of nudge: otherwise boxes drift further and further and
            // the picture explodes
            if (c.0 - want.0).abs().max((c.1 - want.1).abs()) > nudge {
                continue;
            }
            if !within(leash, who, c) {
                continue;
            }
            if !room_at(c, taken, want) {
                continue;
            }
            let d = ((c.0 - want.0) as i64).pow(2) + ((c.1 - want.1) as i64).pow(2);
            if best.map(|(s, _)| d < s).unwrap_or(true) {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// No two boxes may touch, not just edge-neighbours:
/// touching boxes render as one blob, which happens most in dense areas
fn spread_all(p: &mut [(f64, f64)], cfg: &Cfg, leash: (&[(i32, i32)], i32)) {
    for _ in 0..cfg.rounds(40) {
        // A cell can hold more than one box (earlier passes occasionally collide), so this
        // is a multimap
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, q) in p.iter().enumerate() {
            grid.entry((q.0 as i32, q.1 as i32)).or_default().push(i);
        }
        let mut taken: HashSet<(i32, i32)> = grid.keys().copied().collect();
        let mut moved = false;
        for i in 0..p.len() {
            let (x, y) = (p[i].0 as i32, p[i].1 as i32);
            // Only stacked boxes conflict: adjacent cells are already cell apart (largest box
            // + 1), enough room for a line; splitting those too would double the picture's size
            let mut close: Option<usize> = None;
            if let Some(cell) = grid.get(&(x, y)) {
                if let Some(&j) = cell.iter().find(|&&j| j > i) {
                    close = Some(j);
                }
            }
            let Some(j) = close else { continue };
            // Either box may move: try i, then j; each first looks for a cell whose eight
            // neighbours are free too, and only falls back to requiring just the cell itself
            let r = cfg.radius();
            let spot = best_cell(p[j], p[i], &taken, true, r, leash, i)
                .map(|c| (i, c))
                .or_else(|| best_cell(p[i], p[j], &taken, true, r, leash, j).map(|c| (j, c)))
                .or_else(|| best_cell(p[j], p[i], &taken, false, r, leash, i).map(|c| (i, c)))
                .or_else(|| best_cell(p[i], p[j], &taken, false, r, leash, j).map(|c| (j, c)));
            if let Some((who, c)) = spot {
                let old = (p[who].0 as i32, p[who].1 as i32);
                if let Some(cell) = grid.get_mut(&old) {
                    cell.retain(|&k| k != who);
                }
                grid.entry(c).or_default().push(who);
                // Update taken as well: otherwise it still holds the old cell and frees the new
                // one, and later boxes move into the cell just claimed (guaranteed collisions when
                // cells are scarce)
                taken.remove(&old);
                taken.insert(c);
                p[who] = (c.0 as f64, c.1 as f64);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Edge-adjacent boxes must be at least one cell apart: otherwise the segment
/// between them is eaten by the boxes and the line looks broken
fn separate(
    p: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    cfg: &Cfg,
    leash: (&[(i32, i32)], i32),
) {
    for _ in 0..cfg.rounds(60) {
        let mut taken: HashSet<(i32, i32)> = p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            if dx.abs().max(dy.abs()) >= 2.0 {
                continue;
            }
            // Move b first: find a nearby free cell further from a; if that fails, move a.
            // Prefer a cell whose surroundings are free too, and accept just the cell otherwise
            let r = cfg.radius();
            let spot = best_cell(p[a], p[b], &taken, true, r, leash, b)
                .map(|c| (b, c))
                .or_else(|| best_cell(p[b], p[a], &taken, true, r, leash, a).map(|c| (a, c)))
                .or_else(|| best_cell(p[a], p[b], &taken, false, r, leash, b).map(|c| (b, c)))
                .or_else(|| best_cell(p[b], p[a], &taken, false, r, leash, a).map(|c| (a, c)));
            let Some((who, c)) = spot else { continue };
            taken.remove(&(p[who].0 as i32, p[who].1 as i32));
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// Whether a candidate cell is usable: it and its 8 neighbours must be free.
/// The cell currently held by the box being moved does not count -- it frees up at
/// once.
/// (Cells held by other boxes always count, or two boxes would stack.)
fn room_at(c: (i32, i32), taken: &HashSet<(i32, i32)>, own: (i32, i32)) -> bool {
    let blocked = |q: (i32, i32)| taken.contains(&q) && q != own;
    if blocked(c) {
        return false;
    }
    !DIRS8.iter().any(|(ux, uy)| blocked((c.0 + ux, c.1 + uy)))
}

/// Pick a free cell in the (2r+1)^2 area around keep: as far from `from` as possible
/// without straying too far from keep. Cells outside the leash are filtered first --
/// they are useless however many there are, while the usability check costs 9 hash
/// lookups, so comparing coordinates first is much cheaper.
fn best_cell(
    from: (f64, f64),
    keep: (f64, f64),
    taken: &HashSet<(i32, i32)>,
    strict: bool,
    radius: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let (kx, ky) = (keep.0 as i32, keep.1 as i32);
    let own = (kx, ky);
    let mut best: Option<(f64, (i32, i32))> = None;
    // Baseline is 7x7: in crowded places (clustered hubs) there is often no free cell
    // nearby, and larger graphs need more
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let c = (kx + dx, ky + dy);
            if !within(leash, who, c) {
                continue;
            }
            if strict && !room_at(c, taken, own) {
                continue;
            }
            if !strict && taken.contains(&c) {
                continue;
            }
            let d_from = ((c.0 as f64 - from.0).powi(2) + (c.1 as f64 - from.1).powi(2)).sqrt();
            let score = d_from * 3.0 - (dx * dx + dy * dy) as f64;
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Post-processing pipeline: convert centres to cells by dividing by cell, snap them
/// onto integer cells one per cell, then alternate separate / spread_all / octilinear
/// for several rounds -- they interrupt each other, so each prefers a cell with free
/// surroundings until they settle; a final separate catches the edge-neighbours.
/// Force and layered run this same pipeline.
///
/// cell is the pitch derived from the largest box: two boxes on different cells are
/// at least cell apart on the left/top, i.e. one empty cell between them. With no
/// boxes (points only) cell = 1.
///
/// Each box may only move within drift (3x the search radius) cells of its rounded
/// original position.
/// Without that limit dense areas cascade -- one round moves a box away, the next
/// round it touches another neighbour, and after dozens of rounds it drifts right
/// out and the picture grows to several times the canvas.
/// With the leash the finish expands the bbox by at most 2 * drift cells; the price
/// is that very dense areas may keep a touching pair (the final settle fixes that).
fn polish(
    p: Vec<(f64, f64)>,
    edges: &[(usize, usize)],
    cfg: &Cfg,
    cell: (f64, f64),
) -> Vec<(f64, f64)> {
    let ok = |v: f64| if v.is_finite() && v > 1.0 { v } else { 1.0 };
    let (cx, cy) = (ok(cell.0), ok(cell.1));
    let scaled: Vec<(f64, f64)> = p.iter().map(|q| (q.0 / cx, q.1 / cy)).collect();
    let mut out = snap_to_grid(&scaled, cfg);
    let origin: Vec<(i32, i32)> = out.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
    let leash = (origin.as_slice(), cfg.radius() * 3);
    // Fewer passes for large graphs: shuffling in dense areas barely converges, and
    // the final settle always separates
    let passes = if out.len() > 2000 {
        1
    } else if out.len() > 600 {
        3
    } else {
        6
    };
    for _ in 0..passes {
        separate(&mut out, edges, cfg, leash);
        spread_all(&mut out, cfg, leash);
        octilinear(&mut out, edges, cfg, leash);
    }
    separate(&mut out, edges, cfg, leash);
    out.iter().map(|q| (q.0 * cx, q.1 * cy)).collect()
}

/// Boxes sitting between two others within a group: (previous, middle, next).
/// Degrees are per group, not whole-graph -- a hub is also a middle box on some
/// group path and gets straightened too.
fn bends(doc: &Document) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for gi in 0..doc.groups.len() {
        let mut nb: HashMap<usize, Vec<usize>> = HashMap::new();
        for e in doc
            .edges
            .iter()
            .filter(|e| doc.group_idx(&e.group) == Some(gi))
        {
            let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
                continue;
            };
            if a == b {
                // A self loop cannot sit between two boxes; skip it
                continue;
            }
            nb.entry(a).or_default().push(b);
            nb.entry(b).or_default().push(a);
        }
        // Walk boxes by ascending index so the result does not depend on hash order
        let mut nodes: Vec<usize> = nb.keys().copied().collect();
        nodes.sort_unstable();
        for i in nodes {
            let v = &nb[&i];
            // Only exactly two neighbours make a middle box: ends and forks have no fixed
            // direction
            if v.len() == 2 {
                out.push((v[0], i, v[1]));
            }
        }
    }
    out
}

/// Relax: spring-relax from the current positions, then run polish.
///
/// A length spring keeps edge-adjacent boxes at the pitch derived from their sizes
/// (per axis: horizontal moves need width, vertical ones height), direction snapping
/// pulls edges to horizontal/vertical/45 degrees, a three-box straightening force
/// smooths staircase bends, and a weak anchor holds the picture near a sqrt(n)-cell
/// square.
fn relax(doc: &Document, sizes: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let (gw, gh) = unit_axes(sizes);
    let mut p: Vec<(f64, f64)> = (0..n).map(|i| doc.center(i)).collect();
    if flat(&p) {
        // All boxes at one spot: no usable positions, scatter on a golden-angle spiral
        let side = (n as f64).sqrt();
        p = spiral(n, (side * gw).max(gw * 2.0), (side * gh).max(gh * 2.0));
    }
    let edges = edge_pairs(doc);
    let bends = bends(doc);
    // The anchor is fixed once: normalise input positions by the per-axis pitch, scale
    // the longest axis to sqrt(n) cells and keep only relative bearings. Scaling with
    // the current bbox would ratchet -- push out a little, anchor follows -- and the
    // picture would drift wider forever.
    let anchor_raw = p.clone();
    let norm: Vec<(f64, f64)> = anchor_raw.iter().map(|q| (q.0 / gw, q.1 / gh)).collect();
    let nb = bbox(&norm);
    let nc = ((nb.0 + nb.2) / 2.0, (nb.1 + nb.3) / 2.0);
    let nsize = (((nb.2 - nb.0) + (nb.3 - nb.1)) / 2.0).max(1.0);
    let s0 = (n as f64).sqrt() / nsize;
    let anchor: Vec<(f64, f64)> = norm
        .iter()
        .map(|q| {
            (
                (nc.0 + (q.0 - nc.0) * s0) * gw,
                (nc.1 + (q.1 - nc.1) * s0) * gh,
            )
        })
        .collect();

    let iters = cfg.iters(n, ITERS);
    let mut norm: Vec<(f64, f64)> = vec![(0.0, 0.0); n];
    let mut fnorm: Vec<(f64, f64)> = vec![(0.0, 0.0); n];
    for _ in 0..iters {
        let mut f = vec![(0.0f64, 0.0f64); n];
        for &(a, b) in &edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let (ux, uy) = (dx / len, dy / len);
            // Length spring: keep edge-adjacent boxes near the pitch along this direction
            let (wa, ha) = sizes[a];
            let (wb, hb) = sizes[b];
            let rest = ((wa + wb) as f64 / 2.0 + 2.0) * ux.abs()
                + ((ha + hb) as f64 / 2.0 + 2.0) * uy.abs();
            let fl = (len - rest) * 0.30;
            f[a].0 += ux * fl;
            f[a].1 += uy * fl;
            f[b].0 -= ux * fl;
            f[b].1 -= uy * fl;
            // Direction snapping: pull the edge towards horizontal / vertical / 45 degrees
            let (tx, ty) = snap_dir(dx, dy);
            let (px, py) = (-ty, tx);
            let c = (dx * px + dy * py) * 0.30;
            f[a].0 += px * c;
            f[a].1 += py * c;
            f[b].0 -= px * c;
            f[b].1 -= py * c;
        }
        for &(a, b, c) in &bends {
            // Pull the middle box towards the midpoint of its neighbours to smooth the bend
            f[b].0 += ((p[a].0 + p[c].0) / 2.0 - p[b].0) * K_STRAIGHT;
            f[b].1 += ((p[a].1 + p[c].1) / 2.0 - p[b].1) * K_STRAIGHT;
        }
        // Repulsion in normalised coordinates: with unequal axis pitches a circle would
        // become an ellipse
        for i in 0..n {
            norm[i] = (p[i].0 / gw, p[i].1 / gh);
            fnorm[i] = (0.0, 0.0);
        }
        repel(&norm, &mut fnorm, 0.7, 1.0);
        for i in 0..n {
            f[i].0 += fnorm[i].0 * gw;
            f[i].1 += fnorm[i].1 * gh;
        }
        for i in 0..n {
            // Anchor: keep the whole picture from drifting
            f[i].0 += (anchor[i].0 - p[i].0) * ANCHOR_W;
            f[i].1 += (anchor[i].1 - p[i].1) * ANCHOR_W;
            p[i].0 += f[i].0.clamp(-1.2, 1.2);
            p[i].1 += f[i].1.clamp(-1.2, 1.2);
        }
    }
    polish(p, &edges, &cfg, (gw, gh))
}

/// Start when there is no position: scatter on a golden-angle (sunflower) spiral --
/// even area coverage, no RNG, identical on every run, elliptical to match the
/// canvas so a square graph does not bunch up in the middle
fn spiral(n: usize, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = ((w - 6.0).max(1.0) / 2.0, (h - 4.0).max(1.0) / 2.0);
    (0..n)
        .map(|i| {
            let r = ((i as f64 + 0.5) / n as f64).sqrt();
            let a = i as f64 * GOLDEN_ANGLE;
            (cx + rx * r * a.cos(), cy + ry * r * a.sin())
        })
        .collect()
}

/// Force layout: works without positions too.
///
/// Fruchterman-Reingold -- repulsion spreads boxes, springs on edges pull neighbours
/// to the ideal pitch k.
/// A weak gravity then pulls every box towards the canvas centre (with only the first
/// two forces the picture drifts onto the border and bunches up there, and the finish
/// has to move those boxes one by one, growing the picture several times over).
/// Each round's displacement is capped by a linearly annealed temperature
/// (large early, fine later, avoiding oscillation) and clamped to the canvas.
/// With positions, start from them (scaled to the canvas); without, scatter on a
/// golden-angle spiral.
/// Repulsion only considers one grid neighbourhood (the grid pitch is the repulsion
/// cutoff), so a round is O(n + m).
///
/// The work canvas follows the graph (see FORCE_SPAN): the square root of the box
/// count gives each box roughly the same number of cells.
/// Large boxes keep a canvas of at least twice the pitch so nothing starts squeezed
fn force(doc: &Document, sizes: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let gap = unit(sizes);
    let span = (FORCE_SPAN * (n as f64).sqrt()).max(gap * 2.0);
    let (w, h) = (span, span);
    let cur: Vec<(f64, f64)> = (0..n).map(|i| doc.center(i)).collect();
    let mut p = if flat(&cur) {
        spiral(n, w, h)
    } else {
        fit_centers(&cur, w, h)
    };
    let edges = edge_pairs(doc);
    // Target pitch: the per-box edge length when the canvas is shared out, never below
    // the box pitch
    let k = (w * h / n as f64)
        .sqrt()
        .clamp(gap, (gap * 2.0).max(FORCE_K_MAX));
    // Repulsion stops at this distance, which is also the grid pitch (a 3x3
    // neighbourhood covers it)
    let cutoff = k * 2.5;
    let iters = cfg.iters(n, ITERS);
    let t0 = (w / 10.0).min(k * 2.0);
    let center = (w / 2.0, h / 2.0);
    let mut disp = vec![(0.0f64, 0.0f64); n];
    for it in 0..iters {
        // Linear annealing: the last rounds barely move and only fine-tune
        let t = t0 * (1.0 - it as f64 / iters as f64);
        fr_forces(&p, &mut disp, &edges, k, cutoff, center, GRAVITY);
        for i in 0..n {
            let (dx, dy) = disp[i];
            let l = (dx * dx + dy * dy).sqrt();
            if l <= 1e-9 {
                continue;
            }
            let s = l.min(t) / l;
            p[i].0 = (p[i].0 + dx * s).clamp(1.0, w - 1.0);
            p[i].1 = (p[i].1 + dy * s).clamp(1.0, h - 1.0);
        }
    }
    // Force density is relative, so absolute scale is meaningless: scale the result to
    // fill the work canvas. The equilibrium keeps edge-adjacent boxes about k apart,
    // but the overall extent is free; without filling, boxes shrink towards the middle
    // or hug the border, each gets fewer cells than planned, and the finish finds no
    // free spot in dense areas. Scaling preserves shape and only aligns the cell budget
    // with the canvas.
    let b = bbox(&p);
    // Degenerate to 1D (a horizontal/vertical line) or a single point: skip scaling --
    // the huge factor from a near-zero divisor only pushes coordinates towards the i32
    // edge and changes no shape
    let s = if b.2 - b.0 < 1e-6 || b.3 - b.1 < 1e-6 {
        1.0
    } else {
        ((w - 6.0) / (b.2 - b.0)).min((h - 4.0) / (b.3 - b.1))
    };
    let (bcx, bcy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    for q in p.iter_mut() {
        *q = (w / 2.0 + (q.0 - bcx) * s, h / 2.0 + (q.1 - bcy) * s);
    }
    polish(p, &edges, &cfg, (gap, gap))
}

/// Which grid cell a point falls in (grid pitch cell)
fn cell_of(p: (f64, f64), cell: f64) -> (i32, i32) {
    ((p.0 / cell).floor() as i32, (p.1 / cell).floor() as i32)
}

/// One force round: repulsion (grid candidates, only within cutoff) + edge springs +
/// gravity towards the canvas centre, all accumulated into disp. Each pair is
/// computed once (when i < j, both sides receive force),
/// so a round is O(n + m + neighbourhood pairs)
fn fr_forces(
    p: &[(f64, f64)],
    disp: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    k: f64,
    cutoff: f64,
    center: (f64, f64),
    gravity: f64,
) {
    for d in disp.iter_mut() {
        *d = (0.0, 0.0);
    }
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        grid.entry(cell_of(*q, cutoff)).or_default().push(i);
    }
    for i in 0..p.len() {
        let (cx, cy) = cell_of(p[i], cutoff);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &j in bucket {
                    if j <= i {
                        continue;
                    }
                    let (ex, ey) = (p[j].0 - p[i].0, p[j].1 - p[i].1);
                    let len = (ex * ex + ey * ey).sqrt();
                    if len >= cutoff {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 {
                        (0.0, 1.0)
                    } else {
                        (ex / len, ey / len)
                    };
                    // Repulsion k^2 / d: closer boxes push harder
                    let f = k * k / len.max(0.01);
                    disp[i].0 -= ux * f;
                    disp[i].1 -= uy * f;
                    disp[j].0 += ux * f;
                    disp[j].1 += uy * f;
                }
            }
        }
    }
    for &(a, b) in edges {
        let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len, dy / len);
        // Spring force d^2 / k: farther pairs pull harder
        let f = len * len / k;
        disp[a].0 += ux * f;
        disp[a].1 += uy * f;
        disp[b].0 -= ux * f;
        disp[b].1 -= uy * f;
    }
    for i in 0..p.len() {
        disp[i].0 += (center.0 - p[i].0) * gravity;
        disp[i].1 += (center.1 - p[i].1) * gravity;
    }
}

// ---- layered ----

/// Layering: orient the edges, break cycles, then assign ranks
fn ranks(doc: &Document) -> (Vec<usize>, Vec<(usize, usize)>) {
    let n = doc.boxes.len();
    let dirs = directed_arcs(doc);
    if dirs.is_empty() {
        // No directed edges at all: treat as undirected; BFS depth in a component is the
        // rank
        let r = ranks_bfs(doc);
        // Orient every edge downwards (lower rank on top) for the barycentre sweeps
        let arcs = edge_pairs(doc)
            .into_iter()
            .map(|(a, b)| if r[a] <= r[b] { (a, b) } else { (b, a) })
            .collect();
        (r, arcs)
    } else {
        let (kept, _) = break_cycles(n, &dirs);
        (longest_path(n, &kept), kept)
    }
}

/// Find cycles in the directed graph: DFS with three colours; an edge to a node on
/// the current path (grey) is a back edge, and dropping it breaks the cycle.
/// Returns the remaining arcs plus whether anything was dropped.
fn break_cycles(n: usize, arcs: &[(usize, usize)]) -> (Vec<(usize, usize)>, bool) {
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &(a, _)) in arcs.iter().enumerate() {
        out_of[a].push(i);
    }
    let mut color = vec![0u8; n]; // 0 unseen, 1 on the current path, 2 done
    let mut cursor = vec![0usize; n];
    let mut kept = vec![true; arcs.len()];
    let mut dropped = false;
    for s in 0..n {
        if color[s] != 0 {
            continue;
        }
        color[s] = 1;
        let mut stack = vec![s];
        while let Some(&v) = stack.last() {
            if cursor[v] < out_of[v].len() {
                let ei = out_of[v][cursor[v]];
                cursor[v] += 1;
                let w = arcs[ei].1;
                match color[w] {
                    0 => {
                        color[w] = 1;
                        stack.push(w);
                    }
                    1 => {
                        kept[ei] = false;
                        dropped = true;
                    }
                    _ => {}
                }
            } else {
                color[v] = 2;
                stack.pop();
            }
        }
    }
    let kept_arcs: Vec<(usize, usize)> = arcs
        .iter()
        .zip(kept.iter())
        .filter(|(_, &k)| k)
        .map(|(&e, _)| e)
        .collect();
    (kept_arcs, dropped)
}

/// Longest path as rank: enqueue zero in-degree nodes first and relax out-edges. The
/// queue starts in ascending index order, so the topological order is deterministic
fn longest_path(n: usize, arcs: &[(usize, usize)]) -> Vec<usize> {
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for &(a, b) in arcs {
        succ[a].push(b);
        indeg[b] += 1;
    }
    let mut rank = vec![0usize; n];
    let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    while let Some(u) = q.pop_front() {
        for &v in &succ[u] {
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    rank
}

/// Undirected ranks: BFS each component from its lowest-index box; depth is the rank
fn ranks_bfs(doc: &Document) -> Vec<usize> {
    let n = doc.boxes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, b) in edge_pairs(doc) {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut rank = vec![usize::MAX; n];
    let mut q: VecDeque<usize> = VecDeque::new();
    for s in 0..n {
        if rank[s] != usize::MAX {
            continue;
        }
        rank[s] = 0;
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if rank[v] == usize::MAX {
                    rank[v] = rank[u] + 1;
                    q.push_back(v);
                }
            }
        }
    }
    rank
}

/// After polish, pull boxes back into their layer bands: the rank picks the row
/// (y = rank * layer gap) and a layer is sorted by current x with at least lane_gap
/// between neighbours.
/// Different cells, no crowding within a layer, and the top-to-bottom order of
/// directed edges (higher rank below) is preserved.
fn relayer(p: &mut [(f64, f64)], rank: &[usize], (lane_gap, layer_gap): (f64, f64)) {
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..p.len() {
        layers[rank[i]].push(i);
    }
    for (r, layer) in layers.iter_mut().enumerate() {
        layer.sort_by(|&a, &b| p[a].0.total_cmp(&p[b].0).then(a.cmp(&b)));
        let y = r as f64 * layer_gap;
        let mut prev = f64::NEG_INFINITY;
        for &i in layer.iter() {
            let want = p[i].0.round();
            let x = if prev.is_finite() {
                want.max(prev + lane_gap)
            } else {
                want
            };
            p[i] = (x, y);
            prev = x;
        }
    }
}

/// Layered layout (Sugiyama-lite, small and explicit):
///   1. Directed edges are constraints; DFS drops back edges first, so cyclic graphs
///      still lay out
///   2. Ranks come from the longest path (lower rank on top); with no directed edges,
///      treat the graph as undirected and use the component's BFS depth as the rank
///   3. Order within a layer by a few barycentre sweeps, ties broken on box index, so
///      the result is stable
///   4. y = rank * layer gap, x = position in layer * lane pitch, as the first pass
///   5. Run the same polish to tidy the edges, then pull boxes back to their layers
///      (see relayer), so directed edges always run top-down and boxes keep room for
///      their size
fn layered(
    doc: &Document,
    sizes: &[(i32, i32)],
    sp: Spacing,
) -> (Vec<Vec<(f64, f64)>>, LayoutReport) {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let (lane, layer) = pitch(sizes, sp);
    let (rank, arcs) = ranks(doc);
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..n {
        layers[rank[i]].push(i);
    }
    let reversed = directed_arcs(doc).len().saturating_sub(arcs.len());
    let mut report = LayoutReport {
        reversed,
        ..LayoutReport::default()
    };
    if reversed > 0 {
        report.warnings.push(LayoutWarning {
            code: "edges_reversed",
            message: if reversed == 1 {
                "1 edge points back and is layered as if reversed".to_string()
            } else {
                format!("{reversed} edges point back and are layered as if reversed")
            },
            id: None,
        });
    }

    // Edges spanning several layers get a dummy node in each intermediate layer, so
    // ordering knows where the edge runs.
    // The total is capped (4n); past that, fall back to no dummy nodes and warn.
    let budget = 4 * (n + arcs.len());
    let mut total = n;
    let mut rank_ext = rank.clone();
    let mut layers_ext = layers;
    let mut arcs_ext: Vec<(usize, usize)> = Vec::new();
    let mut capped = false;
    for &(a, b) in &arcs {
        let (ra, rb) = (rank[a], rank[b]);
        if rb <= ra {
            continue;
        }
        if rb == ra + 1 {
            arcs_ext.push((a, b));
            continue;
        }
        let k = rb - ra - 1;
        if total + k > budget {
            capped = true;
            break;
        }
        let mut prev = a;
        for d in 0..k {
            let id = total;
            total += 1;
            let r = ra + 1 + d;
            rank_ext.push(r);
            layers_ext[r].push(id);
            arcs_ext.push((prev, id));
            prev = id;
        }
        arcs_ext.push((prev, b));
    }
    if capped {
        layers_ext = vec![Vec::new(); max_rank + 1];
        for i in 0..n {
            layers_ext[rank[i]].push(i);
        }
        rank_ext = rank.clone();
        arcs_ext = arcs.clone();
        report.warnings.push(LayoutWarning {
            code: "dummy_cap",
            message: format!(
                "too many long edges for dummy nodes (cap {budget}); ordering is coarser"
            ),
            id: None,
        });
    }

    // Only top-down edges take part in ordering
    let order_arcs: Vec<(usize, usize)> = arcs_ext
        .iter()
        .copied()
        .filter(|&(a, b)| rank_ext[a] < rank_ext[b])
        .collect();
    order_layers(&mut layers_ext, &order_arcs, &rank_ext, cfg.rounds(8), true);
    let full_order = layers_ext.clone();

    // Alternative without dummy nodes: sweep barycentres over real nodes only. Dummy
    // nodes and transpose do not pay off on every graph, so run both paths in full and
    // pick by final crossing count.
    let mut plain = layering_snapshot(&rank, n);
    order_layers(&mut plain, &arcs, &rank, 4, false);

    let place = |layers: &[Vec<usize>], total: usize| -> Vec<(f64, f64)> {
        let mut p = vec![(0.0, 0.0); total.max(n)];
        for (r, row) in layers.iter().enumerate() {
            for (j, &i) in row.iter().enumerate() {
                p[i] = (j as f64 * lane, r as f64 * layer);
            }
        }
        p.truncate(n);
        p
    };
    let edges = edge_pairs(doc);
    let finish = |p: Vec<(f64, f64)>| -> Vec<(f64, f64)> {
        let mut out = polish(p, &edges, &cfg, (1.0, 1.0));
        relayer(&mut out, &rank, (lane, layer));
        out
    };
    let a = finish(place(&full_order, total));
    let b = finish(place(&plain, n));
    let ca = crossings_between(&full_order, &order_arcs, &rank_ext);
    let _ = ca;
    (vec![a, b], report)
}

/// Build the initial layer array from ranks (within a layer, ascending box index)
fn layering_snapshot(rank: &[usize], n: usize) -> Vec<Vec<usize>> {
    let depth = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut layers = vec![Vec::new(); depth];
    for i in 0..n {
        layers[rank[i]].push(i);
    }
    layers
}

/// Inversions (merge sort); `tmp` is a reused buffer so each layer need not allocate
fn inversions(a: &mut [f64], tmp: &mut Vec<f64>) -> usize {
    let n = a.len();
    if n < 2 {
        return 0;
    }
    let mid = n / 2;
    let mut out = inversions(&mut a[..mid], tmp) + inversions(&mut a[mid..], tmp);
    tmp.clear();
    let (mut i, mut j) = (0usize, mid);
    while i < mid && j < n {
        if a[i] <= a[j] {
            tmp.push(a[i]);
            i += 1;
        } else {
            // a[i] exceeds everything left in the right half: this element crosses mid - i
            tmp.push(a[j]);
            out += mid - i;
            j += 1;
        }
    }
    tmp.extend_from_slice(&a[i..mid]);
    tmp.extend_from_slice(&a[j..n]);
    a.copy_from_slice(tmp);
    out
}

/// Derive ranks from placed positions: one row per distinct y, numbered ascending y
fn rank_from_rows(p: &[(f64, f64)]) -> Vec<usize> {
    let mut ys: Vec<i32> = p.iter().map(|q| q.1.round() as i32).collect();
    ys.sort_unstable();
    ys.dedup();
    p.iter()
        .map(|q| {
            let y = q.1.round() as i32;
            ys.binary_search(&y).unwrap_or(0)
        })
        .collect()
}

/// Crossings in the final geometry: long edges are split into one segment per layer
/// by linear interpolation between their endpoints, then each adjacent layer pair
/// is counted. Matches what gets rendered.
fn crossings_final(p: &[(f64, f64)], rank: &[usize], arcs: &[(usize, usize)]) -> usize {
    let n = p.len();
    if n < 2 || arcs.is_empty() {
        return 0;
    }
    // Sort each layer by x and record the slot index
    let depth = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for i in 0..n {
        rows[rank[i]].push(i);
    }
    let mut slot = vec![0usize; n];
    for row in rows.iter_mut() {
        row.sort_by(|&a, &b| p[a].0.total_cmp(&p[b].0).then(a.cmp(&b)));
        for (j, &i) in row.iter().enumerate() {
            slot[i] = j;
        }
    }
    let mut segs: Vec<Vec<(f64, f64)>> = vec![Vec::new(); depth.saturating_sub(1)];
    for &(a, b) in arcs {
        let (mut ra, mut rb) = (rank[a], rank[b]);
        let (mut pa, mut pb) = (slot[a] as f64, slot[b] as f64);
        if ra == rb {
            continue;
        }
        if ra > rb {
            std::mem::swap(&mut ra, &mut rb);
            std::mem::swap(&mut pa, &mut pb);
        }
        let span = (rb - ra) as f64;
        for (r, seg) in segs.iter_mut().enumerate().take(rb).skip(ra) {
            let t1 = (r - ra) as f64 / span;
            let t2 = (r + 1 - ra) as f64 / span;
            seg.push((pa + (pb - pa) * t1, pa + (pb - pa) * t2));
        }
    }
    let mut total = 0usize;
    for layer in segs.iter_mut() {
        if layer.len() < 2 {
            continue;
        }
        // Strict crossings: both ends must differ. Edges fanning out of one box share a
        // start and do not cross
        if layer.len() <= 512 {
            for i in 0..layer.len() {
                for j in i + 1..layer.len() {
                    let (a, b) = (layer[i], layer[j]);
                    if (a.0 - b.0) * (a.1 - b.1) < 0.0 {
                        total += 1;
                    }
                }
            }
        } else {
            // Too many segments: fall back to inversions (counts shared-start fans too, so it
            // is approximate)
            layer.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut ys: Vec<f64> = layer.iter().map(|s| s.1).collect();
            let mut tmp: Vec<f64> = Vec::with_capacity(ys.len());
            total += inversions(&mut ys, &mut tmp);
        }
    }
    total
}

/// Order within layers: alternate downward sweeps (looking at the previous layer)
/// and upward sweeps (looking at the next), transpose after each round,
/// and keep the order with the fewest crossings. Ties break on node index, so the
/// result is deterministic.
fn order_layers(
    layers: &mut [Vec<usize>],
    arcs: &[(usize, usize)],
    rank: &[usize],
    rounds: usize,
    transpose_passes: bool,
) {
    let total = rank.len();
    let depth = layers.len();
    if depth < 2 {
        return;
    }
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); total];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); total];
    for &(a, b) in arcs {
        pred[b].push(a);
        succ[a].push(b);
    }
    let mut best = layers.to_vec();
    let mut best_cross = crossings_between(layers, arcs, rank);
    let mut slot = vec![0.0f64; total];
    for round in 0..rounds {
        let down = round % 2 == 0;
        let order: Vec<usize> = if down {
            (1..depth).collect()
        } else {
            (0..depth.saturating_sub(1)).rev().collect()
        };
        for r in order {
            let adjacent = if down { r - 1 } else { r + 1 };
            let nb = if down { &pred } else { &succ };
            for (j, &v) in layers[adjacent].iter().enumerate() {
                slot[v] = j as f64;
            }
            let mut key: Vec<(f64, usize)> = Vec::with_capacity(layers[r].len());
            for (j, &v) in layers[r].iter().enumerate() {
                let mut sum = 0.0;
                let mut cnt = 0usize;
                for &u in &nb[v] {
                    if rank[u] == adjacent {
                        sum += slot[u];
                        cnt += 1;
                    }
                }
                // No neighbours in the adjacent layer: stay in place
                let bary = if cnt == 0 { j as f64 } else { sum / cnt as f64 };
                key.push((bary, v));
            }
            key.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            layers[r] = key.into_iter().map(|(_, v)| v).collect();
        }
        if transpose_passes {
            transpose(layers, arcs, rank);
        }
        let c = crossings_between(layers, arcs, rank);
        if c < best_cross {
            best_cross = c;
            best = layers.to_vec();
        }
    }
    for (dst, src) in layers.iter_mut().zip(best) {
        *dst = src;
    }
}

/// transpose: swap two neighbours in a layer and keep the swap when crossings drop.
/// On a large graph each swap needs a full crossing recount, which does not pay off,
/// so skip it (the barycentre sweeps already do most of the work).
fn transpose(layers: &mut [Vec<usize>], arcs: &[(usize, usize)], rank: &[usize]) {
    if rank.len() > 400 {
        return;
    }
    let mut improved = true;
    let mut guard = 0;
    while improved && guard < 4 {
        improved = false;
        guard += 1;
        for r in 0..layers.len() {
            let mut i = 0;
            while i + 1 < layers[r].len() {
                let before = crossings_between(layers, arcs, rank);
                layers[r].swap(i, i + 1);
                let after = crossings_between(layers, arcs, rank);
                if after < before {
                    improved = true;
                } else {
                    layers[r].swap(i, i + 1);
                }
                i += 1;
            }
        }
    }
}

/// Pack multiple components: take each block's bbox and shelf-pack them by
/// descending area, keeping 2 cells between blocks. Only translation happens, so
/// each block keeps its own layout untouched.
fn pack_components(doc: &mut Document) {
    let n = doc.boxes.len();
    if n == 0 {
        return;
    }
    // Weakly connected components
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            continue;
        }
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut comp = vec![usize::MAX; n];
    let mut blocks: Vec<Vec<usize>> = Vec::new();
    for s in 0..n {
        if comp[s] != usize::MAX {
            continue;
        }
        let id = blocks.len();
        let mut stack = vec![s];
        comp[s] = id;
        let mut members = Vec::new();
        while let Some(v) = stack.pop() {
            members.push(v);
            for &w in &adj[v] {
                if comp[w] == usize::MAX {
                    comp[w] = id;
                    stack.push(w);
                }
            }
        }
        members.sort_unstable();
        blocks.push(members);
    }
    if blocks.len() < 2 {
        return;
    }
    // Per-block bbox at current coordinates
    let bbox_of = |doc: &Document, members: &[usize]| -> (i32, i32, i32, i32) {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &i in members {
            let b = &doc.boxes[i];
            x0 = x0.min(b.at.0);
            y0 = y0.min(b.at.1);
            x1 = x1.max(b.at.0 + b.size.0 as i32 - 1);
            y1 = y1.max(b.at.1 + b.size.1 as i32 - 1);
        }
        (x0, y0, x1, y1)
    };
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&b| {
        let (x0, y0, x1, y1) = bbox_of(doc, &blocks[b]);
        let area = (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64;
        // Descending area; ties on block index keep it deterministic
        (-area, b)
    });
    // Shelf: the shelf width is the square root of the total block area
    let total_area: i64 = blocks
        .iter()
        .map(|m| {
            let (x0, y0, x1, y1) = bbox_of(doc, m);
            (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64
        })
        .sum();
    let shelf_w = ((total_area as f64).sqrt() as i32).max(8) * 2;
    let gutter = 2;
    let (mut cx, mut cy, mut row_h) = (0, 0, 0);
    for b in order {
        let (x0, y0, x1, y1) = bbox_of(doc, &blocks[b]);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        if cx > 0 && cx + w > shelf_w {
            cx = 0;
            cy += row_h + gutter;
            row_h = 0;
        }
        let (dx, dy) = (cx - x0, cy - y0);
        for &i in &blocks[b] {
            let at = doc.boxes[i].at;
            doc.move_box(i, at.0 + dx, at.1 + dy);
        }
        cx += w + gutter;
        row_h = row_h.max(h);
    }
    doc.rebuild();
}

/// `auto`: pick a concrete layout from structure and record why in the report.
/// Order: dense -> relax; pure chain/cycle -> chain; pure tree/forest -> tree;
/// mostly tree/chain -> tree or chain (with a warning); everything else, including
/// cyclic graphs, -> layered.
fn auto_layout(doc: &Document) -> (Layout, LayoutReport) {
    let shape = crate::profile::analyze(doc);
    let mut report = LayoutReport {
        profile: "auto",
        components: shape.components,
        ..LayoutReport::default()
    };
    let mut warn = |code: &'static str, message: String| {
        report.warnings.push(LayoutWarning {
            code,
            message,
            id: None,
        });
    };
    if shape.n == 0 {
        return (Layout::Layered, report);
    }
    if shape.is_dense() {
        warn(
            "dense_graph",
            format!(
                "dense graph ({} edges over {} nodes): falling back to relax",
                shape.undirected, shape.n
            ),
        );
        return (Layout::Relax, report);
    }
    if shape.components > 1 {
        warn(
            "components_packed",
            format!("{} components packed separately", shape.components),
        );
        return (Layout::Layered, report);
    }
    if shape.is_cycle() {
        return (Layout::Chain, report);
    }
    if shape.is_chain() {
        return (Layout::Chain, report);
    }
    if shape.is_tree() {
        return (Layout::Tree, report);
    }
    if shape.mostly_tree() || shape.mostly_chain() {
        let tree = shape.mostly_chain() && !shape.mostly_tree();
        let picked = if tree { Layout::Chain } else { Layout::Tree };
        warn(
            "layout_tree_approx",
            if tree {
                "the graph is mostly a chain: laying it out as a chain".to_string()
            } else {
                "the graph is mostly a tree: laying it out as a tree".to_string()
            },
        );
        return (picked, report);
    }
    (Layout::Layered, report)
}

/// Star detection: one hub connected to every other node (others have degree 1, and
/// the edge count is exactly n-1).
/// Returns the hub index
fn star_hub(doc: &Document) -> Option<usize> {
    let n = doc.boxes.len();
    if n < 6 {
        return None;
    }
    let mut deg = vec![0usize; n];
    let mut edges = 0usize;
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            continue;
        }
        deg[a] += 1;
        deg[b] += 1;
        edges += 1;
    }
    if edges != n - 1 {
        return None;
    }
    let hubs: Vec<usize> = (0..n).filter(|&i| deg[i] == n - 1).collect();
    if hubs.len() != 1 {
        return None;
    }
    let hub = hubs[0];
    if (0..n).any(|i| i != hub && deg[i] != 1) {
        return None;
    }
    // When every arrow points into the hub (a funnel), do not fan out: arrow ports
    // cannot be shared, and crowding them on one side of the hub is worse --
    // let the ordinary tree layout make a single bus instead
    let hid = &doc.boxes[hub].id;
    let inbound = doc
        .edges
        .iter()
        .filter(|e| e.from != e.to && e.dir.is_arrow() && &e.to == hid)
        .count();
    let outbound = doc
        .edges
        .iter()
        .filter(|e| e.from != e.to && e.dir.is_arrow() && &e.from == hid)
        .count();
    if inbound > outbound {
        return None;
    }
    Some(hub)
}

/// Star layout: a fan. The hub is centred and leaves are cut into columns in
/// document order, one or two on the right and on the left (right near/far, left
/// near/far), evenly spaced within a column.
/// All edges in a column head for the same hub, so their routes overlap on one
/// vertical line as a trunk;
/// 40 leaves become a 30x20 block instead of a 500-cell-wide strip.
fn star_layout(
    doc: &Document,
    sizes: &[(i32, i32)],
    sp: Spacing,
    hub: usize,
) -> (Vec<(f64, f64)>, LayoutReport) {
    let n = doc.boxes.len();
    let leaves: Vec<usize> = (0..n).filter(|&i| i != hub).collect();
    let k = leaves.len();
    // At most 10 leaves per column (matching examples/limits/star40.gtui) and four
    // columns;
    // two or more leaves split left/right with the hub centred
    let cols = k.div_ceil(10).clamp(1, 4).max(if k >= 2 { 2 } else { 1 });
    let per = k.div_ceil(cols);
    let chunks: Vec<&[usize]> = leaves.chunks(per).collect();
    let col_w: Vec<f64> = chunks
        .iter()
        .map(|c| c.iter().map(|&i| sizes[i].0 as f64).fold(0.0, f64::max))
        .collect();
    let row_h: Vec<f64> = chunks
        .iter()
        .map(|c| c.iter().map(|&i| sizes[i].1 as f64).fold(0.0, f64::max))
        .collect();
    let heights: Vec<f64> = chunks
        .iter()
        .zip(&row_h)
        .map(|(c, h)| c.len() as f64 * h + (c.len() - 1) as f64)
        .collect();
    // The trunk takes one column and the port -> trunk horizontal run takes another;
    // with a gap below 2 the trunk coincides with the hub's port column and routes
    // must take a detour
    let gap = sp.layer_gap.max(3.0) + 2.0;
    let hub_w = sizes[hub].0 as f64;
    let hub_h = sizes[hub].1 as f64;
    // Place the hub on whole cells (centred on 0 when its width is odd): both sides
    // keep equal room, so the trunk never has a single cell on one side
    let hub_left = -((sizes[hub].0 / 2) as f64);
    let total_h = heights.iter().copied().fold(0.0, f64::max);
    // Vertical centre line: the tallest column spans 0..total_h and the hub aligns to
    // its centre
    let cy = total_h / 2.0;
    let mut p = vec![(0.0f64, 0.0f64); n];
    p[hub] = (hub_left, (cy - hub_h / 2.0).round());
    let mut next_right = hub_left + hub_w + gap;
    let mut next_left = hub_left - gap;
    for (ci, chunk) in chunks.iter().enumerate() {
        // Alternate sides: near column right, near column left, then one further out on
        // each side, hub always in the middle
        let (x, right) = if ci % 2 == 0 {
            let x = next_right;
            next_right += col_w[ci] + gap;
            (x, true)
        } else {
            let x = next_left - col_w[ci];
            next_left -= col_w[ci] + gap;
            (x, false)
        };
        let top = cy - heights[ci] / 2.0;
        let mut y = top;
        for &leaf in chunk.iter() {
            // Left columns right-align (towards the trunk), right columns left-align
            let lx = if right {
                x
            } else {
                x + (col_w[ci] - sizes[leaf].0 as f64)
            };
            p[leaf] = (lx, y);
            y += sizes[leaf].1 as f64 + 1.0;
        }
    }
    let report = LayoutReport {
        profile: "star",
        components: 1,
        ..LayoutReport::default()
    };
    (p, report)
}

/// Tree layout: spanning forest of the undirected graph, parents above children and
/// centred.
/// With multiple parents (polytree) the first parent wins; remaining edges go to
/// routing
fn tree_layout(
    doc: &Document,
    sizes: &[(i32, i32)],
    sp: Spacing,
) -> (Vec<(f64, f64)>, LayoutReport) {
    let n = doc.boxes.len();
    let (lane, layer) = pitch(sizes, sp);
    let shape = crate::profile::analyze(doc);
    // Pure star: fan (much narrower than spreading all leaves in one layer)
    if let Some(hub) = star_hub(doc) {
        return star_layout(doc, sizes, sp, hub);
    }
    let mut report = LayoutReport {
        profile: "tree",
        components: shape.components,
        ..LayoutReport::default()
    };
    if !shape.is_tree() {
        report.warnings.push(LayoutWarning {
            code: "layout_tree_approx",
            message:
                "the graph is not a tree: cross edges are routed, layout still uses the skeleton"
                    .to_string(),
            id: None,
        });
    }
    // Undirected adjacency and directed out-edges
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            continue;
        }
        adj[a].push(b);
        adj[b].push(a);
        if e.dir.is_arrow() {
            out_edges[a].push(b);
            indeg[b] += 1;
        }
    }
    for v in adj.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    // Roots: zero in-degree (directed) first, otherwise the lowest-index node of the
    // component
    let mut roots: Vec<usize> = (0..n)
        .filter(|&i| indeg[i] == 0 && !adj[i].is_empty())
        .collect();
    if roots.is_empty() {
        roots.push(0);
    }
    let mut seen = vec![false; n];
    let mut parent = vec![usize::MAX; n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut order: Vec<usize> = Vec::new();
    for &r in &roots {
        if seen[r] {
            continue;
        }
        seen[r] = true;
        order.push(r);
        let mut queue = std::collections::VecDeque::from([r]);
        while let Some(v) = queue.pop_front() {
            // Directed out-edges become children first, then undirected neighbours
            let mut cand: Vec<usize> = out_edges[v].clone();
            cand.extend(adj[v].iter().copied());
            cand.sort_unstable();
            cand.dedup();
            for w in cand {
                if seen[w] {
                    continue;
                }
                seen[w] = true;
                parent[w] = v;
                children[v].push(w);
                order.push(w);
                queue.push_back(w);
            }
        }
    }
    // Unconnected nodes go on their own at the end
    for i in 0..n {
        if !seen[i] {
            parent[i] = usize::MAX;
            seen[i] = true;
            order.push(i);
        }
    }
    // Subtree width including sibling gaps, bottom-up
    let mut width = vec![0.0f64; n];
    for &v in order.iter().rev() {
        let own = sizes[v].0 as f64;
        let kids = &children[v];
        let sum: f64 = kids.iter().map(|&c| width[c]).sum::<f64>()
            + lane * (kids.len().saturating_sub(1)) as f64;
        width[v] = own.max(sum);
    }
    // Coordinates: parent centred over its subtree span, y = depth
    let mut p = vec![(0.0f64, 0.0f64); n];
    let mut depth = vec![0usize; n];
    #[allow(clippy::too_many_arguments)]
    fn place(
        v: usize,
        left: f64,
        depth: usize,
        children: &[Vec<usize>],
        width: &[f64],
        lane: f64,
        layer: f64,
        sizes: &[(i32, i32)],
        p: &mut [(f64, f64)],
        depth_of: &mut [usize],
    ) {
        let span = width[v];
        p[v] = (
            left + span / 2.0 - sizes[v].0 as f64 / 2.0,
            depth as f64 * layer,
        );
        depth_of[v] = depth;
        let kids = &children[v];
        if kids.is_empty() {
            return;
        }
        let total: f64 = kids.iter().map(|&c| width[c]).sum::<f64>()
            + lane * (kids.len().saturating_sub(1)) as f64;
        let mut x = left + (span - total) / 2.0;
        for &c in kids {
            place(
                c,
                x,
                depth + 1,
                children,
                width,
                lane,
                layer,
                sizes,
                p,
                depth_of,
            );
            x += width[c] + lane;
        }
    }
    let mut x = 0.0;
    for &r in &roots {
        if parent[r] != usize::MAX {
            continue;
        }
        place(
            r, x, 0, &children, &width, lane, layer, sizes, &mut p, &mut depth,
        );
        x += width[r] + lane * 2.0;
    }
    for i in 0..n {
        if parent[i] == usize::MAX && depth[i] == 0 && p[i] == (0.0, 0.0) {
            p[i] = (x, 0.0);
            x += sizes[i].0 as f64 + lane;
        }
    }
    (p, report)
}

/// Chain layout: the main path is one row with spurs below; a pure cycle drops one
/// deterministic edge and lays out as a chain
fn chain_layout(
    doc: &Document,
    sizes: &[(i32, i32)],
    sp: Spacing,
) -> (Vec<(f64, f64)>, LayoutReport) {
    let n = doc.boxes.len();
    let (lane, layer) = pitch(sizes, sp);
    let shape = crate::profile::analyze(doc);
    let mut report = LayoutReport {
        profile: "chain",
        components: shape.components,
        ..LayoutReport::default()
    };
    if !(shape.is_chain() || shape.is_cycle()) {
        report.warnings.push(LayoutWarning {
            code: "layout_chain_approx",
            message:
                "the graph is not a chain: the longest path is the main line, the rest are spurs"
                    .to_string(),
            id: None,
        });
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in &doc.edges {
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            continue;
        }
        adj[a].push(b);
        adj[b].push(a);
        if e.dir.is_arrow() {
            out_edges[a].push(b);
            indeg[b] += 1;
        }
    }
    for v in out_edges.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in adj.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    // Start: zero in-degree; in a pure cycle the first node with a neighbour, which
    // keeps it deterministic
    let mut start = (0..n).find(|&i| indeg[i] == 0 && !adj[i].is_empty());
    if start.is_none() {
        start = (0..n).find(|&i| !adj[i].is_empty());
    }
    let start = start.unwrap_or(0);
    // Main path: follow out-edges, else undirected neighbours, skipping visited nodes
    let mut path: Vec<usize> = vec![start];
    let mut seen = vec![false; n];
    seen[start] = true;
    loop {
        let last = *path.last().unwrap();
        let next = out_edges[last]
            .iter()
            .chain(adj[last].iter())
            .copied()
            .find(|&w| !seen[w]);
        match next {
            Some(w) => {
                seen[w] = true;
                path.push(w);
            }
            None => break,
        }
    }
    // Remaining nodes are spurs, at most one main-line length per row, in discovery
    // order
    let rest: Vec<usize> = (0..n).filter(|&i| !seen[i]).collect();
    let mut p = vec![(0.0f64, 0.0f64); n];
    let mut x = 0.0;
    for &v in &path {
        p[v] = (x, 0.0);
        x += sizes[v].0 as f64 + lane;
    }
    let mut x = 0.0;
    let mut row = 1usize;
    for &v in &rest {
        if x > 0.0 && x + sizes[v].0 as f64 > (path.len() as f64) * (lane + 10.0) {
            row += 1;
            x = 0.0;
        }
        p[v] = (x, row as f64 * layer);
        x += sizes[v].0 as f64 + lane;
    }
    (p, report)
}

/// Crossings between adjacent layers: two edges cross when both endpoint orders
/// reverse
/// (edges fanning out of one box share a start and do not cross)
fn crossings_between(layers: &[Vec<usize>], arcs: &[(usize, usize)], rank: &[usize]) -> usize {
    let mut pos = vec![0usize; rank.len()];
    for row in layers {
        for (j, &i) in row.iter().enumerate() {
            pos[i] = j;
        }
    }
    let mut segs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); layers.len().saturating_sub(1)];
    for &(a, b) in arcs {
        if rank[b] != rank[a] + 1 {
            continue;
        }
        segs[rank[a]].push((pos[a], pos[b]));
    }
    let mut total = 0usize;
    for layer in &segs {
        for i in 0..layer.len() {
            for j in i + 1..layer.len() {
                let (x, y) = (layer[i], layer[j]);
                if (x.0 as i64 - y.0 as i64) * (x.1 as i64 - y.1 as i64) < 0 {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Whether the graph has a cycle: directed edges are constraints and a back edge is
/// a cycle.
/// pick uses it to send directed graphs to layered (acyclic) or force (cyclic)
fn has_cycle(n: usize, arcs: &[(usize, usize)]) -> bool {
    break_cycles(n, arcs).1
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // default plus a few tweaks beats a long literal
mod tests {
    use super::*;
    use crate::document::{Box, DocEdge, Group, Layer, Text, ViewState};
    use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

    /// Box size from the label, following `Document::fit_box` (point nodes are 1x1)
    fn size_of(label: &str, style: BoxStyle) -> (u16, u16) {
        if !style.is_boxed() {
            return (1, 1);
        }
        let w = (crate::text::plain_width(label).max(2) + 2).max(4);
        (w as u16, 3)
    }

    /// Build a test document: boxes laid out left to right, plus some edges
    fn doc_of(labels: &[(&str, BoxStyle)], edges: &[(usize, usize, bool)]) -> Document {
        let groups = vec![Group {
            id: "g".to_string(),
            label: "g".to_string(),
            color: (200, 200, 200),
        }];
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let mut boxes = Vec::new();
        let mut texts = Vec::new();
        let mut x = 0;
        for (i, &(label, style)) in labels.iter().enumerate() {
            let size = size_of(label, style);
            let at = (x, 0);
            boxes.push(Box {
                id: format!("b{i}"),
                at,
                size,
                style,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            });
            texts.push(Text {
                id: format!("t{i}"),
                text: label.to_string(),
                at: if style.is_boxed() {
                    (at.0 + 1, at.1 + 1)
                } else {
                    at
                },
                box_id: format!("b{i}"),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            });
            x += size.0 as i32 + 1;
        }
        let edges = edges
            .iter()
            .enumerate()
            .map(|(i, &(a, b, directed))| DocEdge {
                id: format!("e{i}"),
                from: format!("b{a}"),
                to: format!("b{b}"),
                dir: if directed { Dir::Forward } else { Dir::None },
                group: "g".to_string(),
                style: EdgeStyle::Solid,
                color: None,
                bg: None,
                via: Vec::new(),
                corner: Corner::Rounded,
                layer: String::new(),
                from_side: None,
                to_side: None,
                head: None,
            })
            .collect();
        Document::from_parts(
            "test",
            groups,
            layers,
            boxes,
            texts,
            edges,
            ViewState::default(),
        )
        .expect("测试文档要能通过校验")
    }

    /// A 40-leaf star fans out: four columns, two per side, hub on the vertical centre
    /// line
    #[test]
    fn star_fans_out() {
        let mut labels: Vec<(String, BoxStyle)> = vec![("hub".to_string(), BoxStyle::Single)];
        for i in 0..40 {
            labels.push((format!("leaf{i:02}"), BoxStyle::Single));
        }
        let refs: Vec<(&str, BoxStyle)> = labels.iter().map(|(l, s)| (l.as_str(), *s)).collect();
        let edges: Vec<(usize, usize, bool)> = (1..=40).map(|i| (0, i, false)).collect();
        let mut doc = doc_of(&refs, &edges);
        let r = apply_with(&mut doc, Layout::Tree, Spacing::default()).expect("布局");
        assert_eq!(r.profile, "star");
        assert!(no_overlap(&doc), "扇子不该有重叠");
        // Leaves are cut into columns in document order: 10 per column
        let lx = |i: usize| doc.boxes[i + 1].at.0;
        let columns: Vec<i32> = (0..4).map(|c| lx(c * 10)).collect();
        for (c, &x) in columns.iter().enumerate() {
            for k in 1..10 {
                assert_eq!(lx(c * 10 + k), x, "第 {c} 列该对齐");
            }
        }
        // right near, left near, right far, left far
        let hub = doc.boxes[0].at.0;
        assert!(columns[1] < hub && columns[3] < columns[1], "{columns:?}");
        assert!(columns[0] > hub && columns[2] > columns[0], "{columns:?}");
        // Hub on the vertical centre line
        let (y0, y1) = (
            doc.boxes.iter().map(|b| b.at.1).min().unwrap(),
            doc.boxes
                .iter()
                .map(|b| b.at.1 + b.size.1 as i32 - 1)
                .max()
                .unwrap(),
        );
        let hub_mid = doc.boxes[0].at.1 * 2 + doc.boxes[0].size.1 as i32 - 1;
        assert!(
            (hub_mid - (y0 + y1)).abs() <= 2,
            "枢纽该竖直居中: hub_mid={hub_mid} bbox={y0}..{y1}"
        );
        // Much narrower than one flat layer: 10 leaves per column, width below total rows
        let w = doc.bbox().2;
        let h = doc.bbox().3;
        assert!(w < h * 2, "扇子该是竖着的方块: {w}x{h}");
    }

    /// The reversed star (all arrows into the hub) does not fan out: the ports would
    /// crowd on one side
    #[test]
    fn funnel_star_stays_plain_tree() {
        let mut labels: Vec<(String, BoxStyle)> = vec![("dst".to_string(), BoxStyle::Single)];
        for i in 0..12 {
            labels.push((format!("src{i:02}"), BoxStyle::Single));
        }
        let refs: Vec<(&str, BoxStyle)> = labels.iter().map(|(l, s)| (l.as_str(), *s)).collect();
        let edges: Vec<(usize, usize, bool)> = (1..=12).map(|i| (i, 0, true)).collect();
        let mut doc = doc_of(&refs, &edges);
        let r = apply_with(&mut doc, Layout::Tree, Spacing::default()).expect("布局");
        assert_eq!(r.profile, "tree");
    }

    /// Too small or not star-shaped enough: no fan
    #[test]
    fn insufficient_star_is_not_fanned() {
        // 5 nodes (hub + 4 leaves) are below the threshold
        let mut d = doc_of(
            &[
                ("hub", BoxStyle::Single),
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
                ("d", BoxStyle::Single),
            ],
            &[(0, 1, false), (0, 2, false), (0, 3, false), (0, 4, false)],
        );
        assert_eq!(
            apply_with(&mut d, Layout::Tree, Spacing::default())
                .expect("布局")
                .profile,
            "tree"
        );
        // Two hubs of three leaves each: not a single hub
        let mut d = doc_of(
            &[
                ("h1", BoxStyle::Single),
                ("h2", BoxStyle::Single),
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
                ("d", BoxStyle::Single),
                ("e", BoxStyle::Single),
                ("f", BoxStyle::Single),
            ],
            &[
                (0, 1, false),
                (0, 2, false),
                (0, 3, false),
                (0, 4, false),
                (1, 5, false),
                (1, 6, false),
                (1, 7, false),
            ],
        );
        assert_eq!(
            apply_with(&mut d, Layout::Tree, Spacing::default())
                .expect("布局")
                .profile,
            "tree"
        );
    }

    /// Two rectangles are at least one cell apart
    fn clear(a: (i32, i32), asz: (u16, u16), b: (i32, i32), bsz: (u16, u16)) -> bool {
        let (ax1, ay1) = (a.0 as i64 + asz.0 as i64 - 1, a.1 as i64 + asz.1 as i64 - 1);
        let (bx1, by1) = (b.0 as i64 + bsz.0 as i64 - 1, b.1 as i64 + bsz.1 as i64 - 1);
        let (ax0, ay0) = (a.0 as i64, a.1 as i64);
        let (bx0, by0) = (b.0 as i64, b.1 as i64);
        ax1 + 1 < bx0 || bx1 + 1 < ax0 || ay1 + 1 < by0 || by1 + 1 < ay0
    }

    fn no_overlap(doc: &Document) -> bool {
        for i in 0..doc.boxes.len() {
            for j in i + 1..doc.boxes.len() {
                if !clear(
                    doc.boxes[i].at,
                    doc.boxes[i].size,
                    doc.boxes[j].at,
                    doc.boxes[j].size,
                ) {
                    return false;
                }
            }
        }
        true
    }

    fn ats(doc: &Document) -> Vec<(i32, i32)> {
        doc.boxes.iter().map(|b| b.at).collect()
    }

    #[test]
    fn all_layouts_are_integral_and_text_follows() {
        for l in Layout::all() {
            let mut doc = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Single),
                    ("c", BoxStyle::Rounded),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            apply(&mut doc, l).expect("布局");
            for i in 0..doc.boxes.len() {
                let b = &doc.boxes[i];
                let t = doc.text(i).expect("每个框都有文字");
                let want = if b.style.is_boxed() {
                    (b.at.0 + 1, b.at.1 + 1)
                } else {
                    b.at
                };
                assert_eq!(t.at, want, "{} 的文字位置没跟着框", l.name());
            }
            assert!(no_overlap(&doc), "{} 让两个框贴在一起或重叠了", l.name());
        }
    }

    #[test]
    fn layout_is_deterministic() {
        for l in Layout::all() {
            let mut a = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Double),
                    ("c", BoxStyle::Point),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            let mut b = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Double),
                    ("c", BoxStyle::Point),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            apply(&mut a, l).expect("布局");
            apply(&mut b, l).expect("布局");
            assert_eq!(ats(&a), ats(&b), "{} 两次结果不一样", l.name());
        }
    }

    #[test]
    fn geo_keeps_placed_boxes() {
        let mut doc = doc_of(
            &[("a", BoxStyle::Point), ("bbbb", BoxStyle::Single)],
            &[(0, 1, true)],
        );
        let before = ats(&doc);
        apply(&mut doc, Layout::Geo).expect("布局");
        assert_eq!(ats(&doc), before);
    }

    #[test]
    fn cyclic_graph_still_lays_out() {
        let mut cyclic = doc_of(
            &[
                ("a", BoxStyle::Point),
                ("b", BoxStyle::Point),
                ("c", BoxStyle::Point),
            ],
            &[(0, 1, true), (1, 2, true), (2, 0, true)],
        );
        for l in [Layout::Force, Layout::Layered] {
            apply(&mut cyclic, l).expect("布局");
            assert_eq!(cyclic.boxes.len(), 3);
            assert!(no_overlap(&cyclic), "{} 在环上叠了框", l.name());
        }
        let mut plain = doc_of(
            &[
                ("a", BoxStyle::Point),
                ("b", BoxStyle::Point),
                ("c", BoxStyle::Point),
                ("d", BoxStyle::Point),
            ],
            &[(0, 1, false), (1, 2, false), (2, 3, false), (3, 0, false)],
        );
        apply(&mut plain, Layout::Layered).expect("布局");
        assert!(no_overlap(&plain));
    }

    /// Imported / new documents always go to auto; auto_layout picks the concrete one
    #[test]
    fn pick_hands_off_to_auto() {
        for (labels, edges) in [
            (
                vec![("a", BoxStyle::Point), ("b", BoxStyle::Point)],
                vec![(0, 1, true)],
            ),
            (
                vec![
                    ("a", BoxStyle::Point),
                    ("b", BoxStyle::Point),
                    ("c", BoxStyle::Point),
                ],
                vec![(0, 1, true), (1, 2, true), (2, 0, true)],
            ),
            (vec![("a", BoxStyle::Point), ("b", BoxStyle::Point)], vec![]),
        ] {
            assert_eq!(pick(&doc_of(&labels, &edges)), Layout::Auto);
        }
        // auto resolves chains and trees to concrete layouts
        let chain = doc_of(
            &[
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
            ],
            &[(0, 1, true), (1, 2, true)],
        );
        let (l, _report) = auto_layout(&chain);
        assert_eq!(l, Layout::Chain);
        let tree = doc_of(
            &[
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
                ("d", BoxStyle::Single),
            ],
            &[(0, 1, true), (0, 2, true), (0, 3, true)],
        );
        let (l, _) = auto_layout(&tree);
        assert_eq!(l, Layout::Tree);
    }

    #[test]
    fn names_aliases_and_cycle() {
        for l in Layout::all() {
            assert_eq!(Layout::parse(l.name()), Some(l));
            // Six-layout cycle: one full turn returns to the start
            let mut cur = l;
            for _ in 0..Layout::all().len() {
                cur = cur.next();
            }
            assert_eq!(cur, l);
            assert!(!l.label(Lang::Zh).is_empty() && !l.label(Lang::En).is_empty());
        }
        // auto is command-only and not part of the r cycle
        assert!(!Layout::all().contains(&Layout::Auto));
        assert_eq!(Layout::parse(" real "), Some(Layout::Geo));
        assert_eq!(Layout::parse("SPRING"), Some(Layout::Relax));
        assert_eq!(Layout::parse("fr"), Some(Layout::Force));
        assert_eq!(Layout::parse("dag"), Some(Layout::Layered));
        assert_eq!(Layout::parse("sugiyama"), Some(Layout::Layered));
        assert_eq!(Layout::parse("nope"), None);
    }

    /// Two star clusters (two hubs with a string of leaves each): relax must not keep
    /// growing, and an isotropic pitch would stretch one-row-high boxes across dozens of
    /// rows
    #[test]
    fn relax_does_not_keep_growing() {
        let mut labels: Vec<(&str, BoxStyle)> = Vec::new();
        let mut edges: Vec<(usize, usize, bool)> = Vec::new();
        // Two hubs plus 8 leaves each, labels as long as the longest
        labels.push(("hub-a", BoxStyle::Single));
        labels.push(("hub-b", BoxStyle::Single));
        for i in 0..8 {
            labels.push(("leaf-somewhat-long", BoxStyle::Single));
            edges.push((0, 2 + i, true));
            labels.push(("leaf-somewhat-long", BoxStyle::Single));
            edges.push((1, 10 + i, true));
        }
        let mut doc = doc_of(&labels, &edges);
        apply(&mut doc, Layout::Relax).expect("布局");
        assert!(no_overlap(&doc), "框不能叠在一起");
        let (x0, y0, x1, y1) = doc.bbox();
        let (w, h) = (x1 - x0, y1 - y0);
        let n = doc.boxes.len() as f64;
        // 18 boxes of roughly 20 cells: width may reach n box widths, height must stay
        assert!(w <= (n * 24.0) as i32, "太宽:{w}x{h}");
        assert!(h <= 40, "太高:{w}x{h}");
        // under 8 rows
        let a = doc.center(0);
        let b = doc.center(1);
        let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        assert!(d < 120.0, "两团离得太远:{d:.0}");
    }

    /// Layout report: a chain has no back edges and no crossings; a cyclic graph
    /// reports the dropped edges
    #[test]
    fn layout_report() {
        let chain = doc_of(
            &[
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
            ],
            &[(0, 1, true), (1, 2, true)],
        );
        let mut doc = chain.clone();
        let r = apply(&mut doc, Layout::Layered).expect("布局");
        assert_eq!(r.reversed, 0);
        assert_eq!(r.crossings, 0);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);

        // a->b->c->a: one back edge must be dropped to layer it
        let mut doc = doc_of(
            &[
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
            ],
            &[(0, 1, true), (1, 2, true), (2, 0, true)],
        );
        let r = apply(&mut doc, Layout::Layered).expect("布局");
        assert!(r.reversed >= 1, "回边没报:{r:?}");
        assert!(
            r.warnings.iter().any(|w| w.code == "edges_reversed"),
            "{:?}",
            r.warnings
        );
        assert!(r.summary(3).contains("edge"), "{}", r.summary(3));

        // Other layouts leave these numbers at zero
        let mut doc = chain;
        let r = apply(&mut doc, Layout::Relax).expect("布局");
        assert_eq!((r.crossings, r.reversed), (0, 0));
        assert!(r.warnings.is_empty());
    }

    /// Spacing config: larger lane/layer gaps widen and heighten the result; the margin
    /// pushes content to (margin, margin)
    #[test]
    fn spacing_is_configurable() {
        let labels: Vec<(&str, BoxStyle)> = (0..6).map(|_| ("x", BoxStyle::Single)).collect();
        let edges: Vec<(usize, usize, bool)> = vec![(0, 1, true), (1, 2, true), (2, 3, true)];
        let size_of = |doc: &Document, sp: Spacing| {
            let mut d = doc.clone();
            apply_with(&mut d, Layout::Layered, sp).expect("布局");
            let (x0, y0, x1, y1) = d.bbox();
            (x1 - x0, y1 - y0, x0, y0)
        };
        let doc = doc_of(&labels, &edges);
        let (w0, h0, _, _) = size_of(&doc, Spacing::default());
        let wide = Spacing::of(Some(9.0), None, None);
        let (w1, _, _, _) = size_of(&doc, wide);
        assert!(w1 >= w0, "列距变大该更宽:{w0} -> {w1}");
        let tall = Spacing::of(None, Some(10.0), None);
        let (_, h2, _, _) = size_of(&doc, tall);
        assert!(h2 > h0, "行距变大该更高:{h0} -> {h2}");
        // Margin: top-left moves to (margin, margin)
        let pad = Spacing::of(None, None, Some(4.0));
        let (_, _, x, y) = size_of(&doc, pad);
        assert_eq!((x, y), (4, 4), "外边距没生效");
        // Out-of-range values are clamped so the picture is never empty
        let wild = Spacing::of(Some(-5.0), Some(1e9), Some(-1.0));
        assert_eq!(wild.lane_gap, 1.0);
        assert_eq!(wild.layer_gap, 64.0);
        assert_eq!(wild.margin, 0.0);
    }

    /// Crossings: two crossing edges count 1; dummy nodes make long multi-layer edges
    /// count too
    #[test]
    fn crossing_count() {
        // 0 -> 2 (one layer apart) does not cross 1 -> 3; 0 -> 3 spans two layers and needs
        // a dummy
        let d = doc_of(
            &[
                ("a", BoxStyle::Single),
                ("b", BoxStyle::Single),
                ("c", BoxStyle::Single),
                ("d", BoxStyle::Single),
            ],
            &[(0, 1, true), (1, 2, true), (2, 3, true), (0, 3, true)],
        );
        let mut doc = d.clone();
        let r = apply(&mut doc, Layout::Layered).expect("布局");
        assert_eq!(r.reversed, 0);
        // Same input twice gives the same result
        let mut doc2 = d;
        let r2 = apply(&mut doc2, Layout::Layered).expect("布局");
        assert_eq!(r.crossings, r2.crossings);
        assert_eq!(doc.boxes, doc2.boxes);
        assert!(r.crossings <= 2, "不该出现大量交叉:{}", r.crossings);
    }

    /// Dummy cap: too many long edges falls back to no dummies and emits a stable code
    #[test]
    fn dummy_cap_warns() {
        // A 30-node chain plus 28 long edges from the first node to the last: the dummy
        // count far exceeds 4*(n+m)
        let n = 30;
        let labels: Vec<(&str, BoxStyle)> = (0..n).map(|_| ("x", BoxStyle::Single)).collect();
        let mut edges: Vec<(usize, usize, bool)> = (0..n - 1).map(|i| (i, i + 1, true)).collect();
        for i in 2..n {
            edges.push((0, i, true));
        }
        let mut doc = doc_of(&labels, &edges);
        let r = apply(&mut doc, Layout::Layered).expect("布局");
        assert!(
            r.warnings.iter().any(|w| w.code == "dummy_cap"),
            "长边太多该报 dummy_cap:{:?}",
            r.warnings
        );
        // The fallback still lays out without overlap
        assert!(no_overlap(&doc));
        assert_eq!(doc.boxes.len(), n);
    }

    /// Chain layout: the main line is one row in out-edge order, identical cell by cell
    /// across runs
    #[test]
    fn chain_layout_is_a_single_line() {
        let labels: Vec<(&str, BoxStyle)> = (0..4).map(|_| ("x", BoxStyle::Single)).collect();
        let edges: Vec<(usize, usize, bool)> = vec![(0, 1, true), (1, 2, true), (2, 3, true)];
        let mut doc = doc_of(&labels, &edges);
        let r = apply(&mut doc, Layout::Chain).expect("布局");
        assert_eq!(r.profile, "chain");
        let ys: Vec<i32> = (0..4).map(|i| doc.boxes[i].at.1).collect();
        assert!(ys.windows(2).all(|w| w[0] == w[1]), "主线该在同一行:{ys:?}");
        let xs: Vec<i32> = (0..4).map(|i| doc.boxes[i].at.0).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "顺序该沿链:{xs:?}");
        assert!(no_overlap(&doc));
        // Deterministic
        let mut again = doc_of(&labels, &edges);
        apply(&mut again, Layout::Chain).expect("布局");
        assert_eq!(doc.boxes, again.boxes);
    }

    /// Tree layout: parents above and centred over their subtrees, subtrees do not
    /// overlap, repeats are identical
    #[test]
    fn tree_parents_centred_and_no_overlap() {
        // 0 -> 1,2,3;1 -> 4,5
        let labels: Vec<(&str, BoxStyle)> = (0..6).map(|_| ("x", BoxStyle::Single)).collect();
        let edges: Vec<(usize, usize, bool)> = vec![
            (0, 1, true),
            (0, 2, true),
            (0, 3, true),
            (1, 4, true),
            (1, 5, true),
        ];
        let mut doc = doc_of(&labels, &edges);
        let r = apply(&mut doc, Layout::Tree).expect("布局");
        assert_eq!(r.profile, "tree");
        assert!(no_overlap(&doc));
        // Parents above
        for (p, c) in [(0, 1), (0, 2), (0, 3), (1, 4), (1, 5)] {
            assert!(doc.boxes[p].at.1 < doc.boxes[c].at.1, "父该在子上面");
        }
        // The root's centre falls inside its children's span
        let cx = |i: usize| doc.boxes[i].at.0 as f64 + doc.boxes[i].size.0 as f64 / 2.0;
        let lo = cx(1).min(cx(2)).min(cx(3));
        let hi = cx(1).max(cx(2)).max(cx(3));
        assert!(cx(0) >= lo && cx(0) <= hi, "父该居中");
        // Deterministic
        let mut again = doc_of(&labels, &edges);
        apply(&mut again, Layout::Tree).expect("布局");
        assert_eq!(doc.boxes, again.boxes);
    }

    /// Multiple components: auto lays each out, then packs them without overlap or
    /// shared rows
    #[test]
    fn components_are_packed() {
        // Two disconnected chains
        let labels: Vec<(&str, BoxStyle)> = (0..6).map(|_| ("x", BoxStyle::Single)).collect();
        let edges: Vec<(usize, usize, bool)> =
            vec![(0, 1, true), (1, 2, true), (3, 4, true), (4, 5, true)];
        let mut doc = doc_of(&labels, &edges);
        let r = apply(&mut doc, Layout::Auto).expect("布局");
        assert_eq!(r.components, 2);
        assert!(r.warnings.iter().any(|w| w.code == "components_packed"));
        assert!(no_overlap(&doc));
        // The two block bboxes do not intersect
        let bbox = |ids: &[usize]| {
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            for &i in ids {
                let b = &doc.boxes[i];
                x0 = x0.min(b.at.0);
                y0 = y0.min(b.at.1);
                x1 = x1.max(b.at.0 + b.size.0 as i32 - 1);
                y1 = y1.max(b.at.1 + b.size.1 as i32 - 1);
            }
            (x0, y0, x1, y1)
        };
        let (ax0, ay0, ax1, ay1) = bbox(&[0, 1, 2]);
        let (bx0, by0, bx1, by1) = bbox(&[3, 4, 5]);
        let overlap = ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1;
        assert!(!overlap, "两块该分开");
    }

    /// Dense graph: auto falls back to relax and warns with dense_graph (no error)
    #[test]
    fn dense_graph_falls_back_to_relax() {
        let labels: Vec<(&str, BoxStyle)> = (0..5).map(|_| ("x", BoxStyle::Point)).collect();
        let mut edges = Vec::new();
        for a in 0..5 {
            for b in (a + 1)..5 {
                edges.push((a, b, true));
            }
        }
        let mut doc = doc_of(&labels, &edges);
        let r = apply(&mut doc, Layout::Auto).expect("稠密图也要出布局");
        assert_eq!(r.profile, "relax");
        assert!(r.warnings.iter().any(|w| w.code == "dense_graph"));
    }

    #[test]
    fn large_graph_does_not_hang() {
        let n = 6000;
        let labels: Vec<(&str, BoxStyle)> = (0..n).map(|_| ("x", BoxStyle::Point)).collect();
        let edges: Vec<(usize, usize, bool)> = (0..n - 1).map(|i| (i, i + 1, true)).collect();
        let mut doc = doc_of(&labels, &edges);
        apply(&mut doc, Layout::Layered).expect("布局");
        assert_eq!(doc.boxes.len(), n);
        assert!(no_overlap(&doc));
    }
}

#[cfg(test)]
mod cross_tests {
    use super::*;

    /// Two layers of two boxes: a crossing pair counts 1, a parallel pair 0
    #[test]
    fn crossings_follow_geometry() {
        let p = vec![(0.0, 0.0), (10.0, 0.0), (0.0, 5.0), (10.0, 5.0)];
        let rank = vec![0, 0, 1, 1];
        // a(0,0)->d(3), b(1)->c(2): crossing
        assert_eq!(crossings_final(&p, &rank, &[(0, 3), (1, 2)]), 1);
        // a->c, b->d: parallel
        assert_eq!(crossings_final(&p, &rank, &[(0, 2), (1, 3)]), 0);
        // Edges within one layer do not participate
        assert_eq!(crossings_final(&p, &rank, &[(0, 1)]), 0);
        // Three edges in the same direction: 0
        assert_eq!(crossings_final(&p, &rank, &[(0, 2), (0, 3), (1, 3)]), 0);
    }
}
