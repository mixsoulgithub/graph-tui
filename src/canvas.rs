//! Rasterisation: turn canvas coordinates into which directions this cell connects,
//! then compose box-drawing glyphs from the direction bits.
//! This layer knows only character cells, not ratatui (render puts it on screen).

use std::collections::{HashMap, HashSet};

use crate::model::{BoxStyle, Corner, EdgeStyle};

/// Routing cost weights, ordered by how bad they look: coaxial overlap is worst,
/// the later edge is invisible along the whole stretch
const C_BEND: i32 = 10;
const C_CROSS: i32 = 25;
/// Planning result: an edge route and the ports at both ends
#[derive(Clone, Default)]
pub(crate) struct Stroke {
    pub(crate) pts: Vec<(i32, i32)>,
    pub(crate) a_port: Option<(u8, (i32, i32))>,
    pub(crate) b_port: Option<(u8, (i32, i32))>,
}

const C_OVERLAP: i32 = 40;
/// Cost of a planned stroke that is not drawn yet: lighter than real ink, yet
/// enough for later edges to steer around it
const C_USAGE: i32 = 18;
const C_OTHER_GROUP: i32 = 6;
/// Stepping on a foreign port/arrow cell: a soft obstacle, avoid it if possible
const C_FOREIGN_PORT: i32 = 100;

/// One cell of a node box: glyph, colour, in-box fill, which groups own this node,
/// node index (for cursor highlighting)
#[derive(Clone, Copy)]
pub(crate) struct Deco {
    pub(crate) ch: char,
    pub(crate) fg: (u8, u8, u8),
    /// In-box background fill (dropped by blit when dimmed)
    pub(crate) bg: Option<(u8, u8, u8)>,
    pub(crate) groups: u64,
    pub(crate) node: i32,
}

/// Direction bits of a cell: a cell connects up, down, left and right
pub(crate) const LEFT: u8 = 1;
pub(crate) const RIGHT: u8 = 2;
pub(crate) const UP: u8 = 4;
pub(crate) const DOWN: u8 = 8;

// Direction bit combinations (matched as wholes, they cannot be computed on the fly)
const LR: u8 = LEFT | RIGHT;
const UD: u8 = UP | DOWN;
const LRD: u8 = LEFT | RIGHT | DOWN;
const LRU: u8 = LEFT | RIGHT | UP;
const UDR: u8 = UP | DOWN | RIGHT;
const UDL: u8 = UP | DOWN | LEFT;
const LRUD: u8 = LEFT | RIGHT | UP | DOWN;
const DR: u8 = DOWN | RIGHT;
const DL: u8 = DOWN | LEFT;
const UR: u8 = UP | RIGHT;
const UL: u8 = UP | LEFT;

/// Connection point on a border: horizontal bit + which side the line leaves.
/// Double and heavy borders each have their own glyphs
pub(crate) fn junction_glyph(style: BoxStyle, mask: u8) -> char {
    let double = style == BoxStyle::Double;
    let heavy = style == BoxStyle::Heavy;
    match mask {
        LRUD => {
            if double {
                '╬'
            } else if heavy {
                '╋'
            } else {
                '┼'
            }
        }
        LRD => {
            if double {
                '╦'
            } else if heavy {
                '┳'
            } else {
                '┬'
            }
        }
        LRU => {
            if double {
                '╩'
            } else if heavy {
                '┻'
            } else {
                '┴'
            }
        }
        UDR => {
            if double {
                '╠'
            } else if heavy {
                '┣'
            } else {
                '├'
            }
        }
        UDL => {
            if double {
                '╣'
            } else if heavy {
                '┫'
            } else {
                '┤'
            }
        }
        other => box_char(other, Corner::Rounded),
    }
}

/// Direction bits -> box-drawing glyph (straight, T and cross junctions, corners).
/// Corners are either rounded ╭╮╯╰ or sharp ┌┐└┘; only the four corner masks are
/// affected.
/// A single direction is no stroke (a path has at least one step); it falls through
/// to the fallback branch as a vertical bar.
pub(crate) fn box_char(mask: u8, corner: Corner) -> char {
    let sharp = corner == Corner::Sharp;
    match mask {
        0 => ' ',
        LRUD => '┼',
        LRD => '┬',
        LRU => '┴',
        UDR => '├',
        UDL => '┤',
        LR => '─',
        UD => '│',
        DR => {
            if sharp {
                '┌'
            } else {
                '╭'
            }
        }
        DL => {
            if sharp {
                '┐'
            } else {
                '╮'
            }
        }
        UR => {
            if sharp {
                '└'
            } else {
                '╰'
            }
        }
        UL => {
            if sharp {
                '┘'
            } else {
                '╯'
            }
        }
        // Cell with a single direction bit (a line endpoint): horizontal gives a horizontal
        // line, vertical a vertical one
        LEFT | RIGHT => '─',
        _ => '│',
    }
}

/// When a and c lie on the same side of b (both north/south/east/west), the edge
/// should enter b vertically or horizontally; one in and one out (a normal
/// crossing) is left to the main direction.
/// Returns true when this span requires a horizontal entry into the node
pub(crate) fn arrive_hor(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> Option<bool> {
    let (da, dc) = (a.1 - b.1, c.1 - b.1);
    if da != 0 && dc != 0 && da.signum() == dc.signum() {
        return Some(false); // both ends above or both below: vertical entry
    }
    let (xa, xc) = (a.0 - b.0, c.0 - b.0);
    if xa != 0 && xc != 0 && xa.signum() == xc.signum() {
        return Some(true); // both ends left or both right: horizontal entry
    }
    None
}

/// Canvas: edges only record which directions this cell connects to; box-drawing
/// glyphs are composed from the direction bits afterwards, so a bend is naturally
/// ╭╮╯╰ and a crossing ┼, with no per-segment guessing.
/// Where one end of an edge lands: the node box rectangle, which side the line
/// enters and leaves, and that cell
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EndMark {
    pub rect: (i32, i32, i32, i32),
    pub side: u8,
    pub cell: (i32, i32),
    /// Whether this end has a border to carry a T/arrowhead (text nodes do not; their
    /// arrowhead lands in the cell just outside the region)
    pub mark: bool,
    /// Whether the port may land on the four corners (boxed nodes may not; corners
    /// keep their corner glyph).
    /// Only affects self-loop cell scanning; ports of ordinary edges come straight
    /// from render PortPlan
    pub corners: bool,
    /// Port cell already fixed by PortPlan: do not scan the border for a free slot
    pub fixed: bool,
}

/// What a border cell is overwritten with
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BorderMark {
    /// Junction (T): horizontal bit + side the line leaves + this edge colour
    Junction(u8, (u8, u8, u8)),
    /// Arrowhead at the target: glyph + edge colour
    Arrow(char, (u8, u8, u8)),
    /// Reserve without drawing: a text node has no border, but its port cell is still
    /// reserved,
    /// so a further edge on the same side shifts to the next cell and several edges
    /// never crowd one entry point
    Port,
}

/// (arrow cell, owner edge) -> ink colour
pub(crate) type OwnerInk = HashMap<((i32, i32), usize), (u8, u8, u8)>;

/// Role of a port cell: exactly one of these two, never mixed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortKind {
    /// Arrowhead end
    Arrow,
    /// Ordinary connection end (merged into a T)
    Line,
}

/// A routing point (canvas cell)
pub(crate) type Point = (i32, i32);
/// A polyline
pub(crate) type Polyline = Vec<Point>;
/// Search state: position + direction of entry
pub(crate) type State = (Point, usize);

pub(crate) struct Canvas {
    pub(crate) x0: i32,
    pub(crate) y0: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) mask: Vec<u8>,
    pub(crate) fg: Vec<(u8, u8, u8)>,
    /// Which group owns this cell colour (-1 = unclaimed)
    pub(crate) owner: Vec<i16>,
    /// Direction bits of the owning group at this cell (a foreign group passing
    /// through breaks the cell)
    pub(crate) owner_mask: Vec<u8>,
    /// Whether this cell colour belongs to the group under the cursor
    pub(crate) mine: Vec<bool>,
    /// Node: 0 none, 1 ordinary node, 2 hub
    pub(crate) node: Vec<u8>,
    /// Which groups sit on this cell (bit mask; 64 is plenty)
    pub(crate) groups_here: Vec<u64>,
    /// Bounds of the whole picture (top-left, bottom-right, inclusive). Routing stays
    /// inside them and does not follow the viewport
    pub(crate) frame: (i32, i32, i32, i32),
    /// Per-cell routing state, also kept off-canvas: routes must not change when the
    /// view moves
    /// (`.0` = which groups strokes are on this cell, `.1` = claimed stroke axes here)
    pub(crate) ink: HashMap<(i32, i32), (u64, u8)>,
    /// Strokes planned but not drawn yet: the mirror of `ink`, letting later edges
    /// avoid them in advance.
    /// `.0` = which groups pass through here, `.1` = direction bits
    pub(crate) usage: HashMap<(i32, i32), (u64, u8)>,
    /// Route only, draw nothing: used by the planning pass
    pub(crate) planning: bool,
    /// Cells each edge claimed while planning: when its turn to draw comes, drop its
    /// own entries first (it occupies them itself)
    pub(crate) usage_edges: HashMap<usize, Vec<(i32, i32)>>,
    /// Planned strokes in call order; the drawing pass takes them directly instead of
    /// re-running routing
    pub(crate) plans: Vec<Stroke>,
    /// Edge indices parallel to `plans`: an edge skipped by isolate must still consume
    /// its slot,
    /// or the cursor drifts and later edges take someone else route
    pub(crate) plan_edges: Vec<usize>,
    pub(crate) plan_cursor: usize,
    /// Groups this node belongs to as a node (groups merely passing through do not
    /// count)
    pub(crate) node_groups: Vec<u64>,
    /// Cells of all nodes (a boxed node contributes its whole box): edges avoid them
    /// instead of cutting through a node
    pub(crate) station_cells: HashSet<(i32, i32)>,
    /// Line style of the edge owning each cell
    pub(crate) style: Vec<EdgeStyle>,
    /// Corner style of the edge owning each cell (first claim wins)
    pub(crate) corner: Vec<Corner>,
    /// Corner style of the edge being drawn; copied in when claim first takes a cell
    pub(crate) pen_corner: Corner,
    /// Background colour of the current edge; copied into edge_bg when it claims a
    /// cell
    pub(crate) pen_bg: Option<crate::document::Rgb>,
    /// Background colour of the edge on each cell (independent of box fill)
    pub(crate) edge_bg: Vec<Option<crate::document::Rgb>>,
    /// Glyph set (Unicode / ASCII)
    pub(crate) glyphs: crate::glyphs::Glyphs,
    /// Whether a solid stroke has claimed this cell (solid wins; dashes do not break
    /// here)
    pub(crate) solid: Vec<bool>,
    /// Node-box decoration layer: drawn after the edges, on top of them
    pub(crate) deco: Vec<Option<Deco>>,
    /// Glyph a directed edge draws in its arrow cell (None = no arrow here; point
    /// nodes only)
    pub(crate) arrow: Vec<Option<char>>,
    /// The arrow own ink (the cell may be owned by another edge, so colour cannot
    /// follow the cell)
    pub(crate) arrow_ink: Vec<Option<(u8, u8, u8)>>,
    /// Arrow cell -> edges ending their arrowhead here (the first drew the glyph;
    /// same-direction edges share it)
    pub(crate) arrow_owners: HashMap<(i32, i32), Vec<usize>>,
    /// (arrow cell, owner edge) -> that edge own ink: a shared arrow takes the colour
    /// of the currently active owner
    pub(crate) arrow_owner_ink: OwnerInk,
    /// Role of a port cell (Arrow / Line): a cell holds only one
    pub(crate) port_kind: HashMap<(i32, i32), PortKind>,
    /// Port cell -> edges holding it (the first is the visible one)
    pub(crate) port_owners: HashMap<(i32, i32), Vec<usize>>,
    /// Port **outer** cell -> role. Arrows and lines of `none` nodes are really drawn
    /// in the outer cell,
    /// so both cells need a role, or a line end can coincide with another edge arrow
    /// outer cell
    pub(crate) port_out_kind: HashMap<(i32, i32), PortKind>,
    /// Visible stroke of each edge (selection/overlay draws from it rather than
    /// guessing through edge_cells)
    pub(crate) stroke_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// Arrowhead endpoint cells of each edge (from the two ends only, never mid-path)
    pub(crate) arrows_of: HashMap<usize, Vec<(i32, i32)>>,
    /// Port outer cells each edge **actually** uses (where the line starts and the
    /// arrowhead lands).
    /// Authoritative from the drawing pass: planning may lack this end (pick_port is
    /// the fallback when the side is crowded)
    pub(crate) edge_out_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// Port/arrow cell -> edge planned there: routing treats foreign ports as soft
    /// obstacles
    pub(crate) port_guest: HashMap<(i32, i32), usize>,
    /// Port outer cells of the selected edge: foreign arrows there are hidden (else it
    /// reads as <->)
    pub(crate) picked_port_out: Vec<(i32, i32)>,
    /// Border overlay map: Ts and arrows embedded in the border
    pub(crate) border: HashMap<(i32, i32), BorderMark>,
    /// Arrowhead glyph of the edge being drawn (its own `head`, else the default).
    /// ASCII glyphs override it: see `pen_arrow_glyph`
    pub(crate) pen_arrow: crate::model::ArrowStyle,
    /// Preview mode: draw lines only, no T/arrowhead and no port stub (for connection
    /// preview)
    pub(crate) preview: bool,
    /// Focused groups (bit mask; 0 = no focus). Without focus every group draws
    /// normally
    pub(crate) focus: u64,
    /// Groups of the node under the cursor: they win colour conflicts
    pub(crate) priority: Vec<bool>,
    /// Whether this cell content is on the layer being viewed: dimming uses only
    /// this, independent of group focus
    pub(crate) lit: Vec<bool>,
    /// Index of the edge being drawn (set before drawing, copied into edge_of by
    /// claim)
    pub(crate) pen_edge: usize,
    /// Which edge this cell finally shows (-1 = no edge)
    pub(crate) edge_of: Vec<i32>,
    /// Which edge this cell **visible shape** belongs to (-1 = none). On overlap only
    /// it draws; the others break for one cell
    pub(crate) shape_edge: Vec<i32>,
    /// Edges raised to the top (the selected one, the lit ones in E mode): cells and
    /// arrows go to them
    pub(crate) raised: HashSet<usize>,
    /// Edge the user selected: it beats every other edge (even other raised ones), so
    /// the whole edge lights up when selected
    pub(crate) picked: Option<usize>,
    /// Final visible cells of each edge (used when picking cells)
    pub(crate) edge_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// Candidates when several edges share a cell (in draw order)
    pub(crate) edge_cands: HashMap<(i32, i32), Vec<usize>>,
    /// Whether what is being drawn is on the current layer (set temporarily while
    /// drawing; lit is read afterwards to decide dimming)
    pub(crate) layer_lit: bool,
    /// Whether viewing by layer. When not, lit plays no part in any decision
    pub(crate) layer_filter: bool,
}

impl Canvas {
    pub(crate) fn new(
        area: (i32, i32, i32, i32),
        priority: Vec<bool>,
        focus: u64,
        glyphs: crate::glyphs::Glyphs,
    ) -> Canvas {
        let (x0, y0, w, h) = area;
        let n = (w.max(0) * h.max(0)) as usize;
        Canvas {
            x0,
            y0,
            w: w.max(0),
            h: h.max(0),
            mask: vec![0; n],
            fg: vec![(0, 0, 0); n],
            owner: vec![-1; n],
            owner_mask: vec![0; n],
            mine: vec![false; n],
            node: vec![0; n],
            groups_here: vec![0; n],
            node_groups: vec![0; n],
            station_cells: HashSet::new(),
            ink: HashMap::new(),
            usage: HashMap::new(),
            planning: false,
            usage_edges: HashMap::new(),
            plans: Vec::new(),
            plan_edges: Vec::new(),
            plan_cursor: 0,
            frame: (x0, y0, x0 + w.max(0) - 1, y0 + h.max(0) - 1),
            style: vec![EdgeStyle::Solid; n],
            corner: vec![Corner::Rounded; n],
            pen_corner: Corner::Rounded,
            pen_bg: None,
            edge_bg: vec![None; n],
            glyphs,
            solid: vec![false; n],
            deco: vec![None; n],
            arrow: vec![None; n],
            arrow_ink: vec![None; n],
            arrow_owners: HashMap::new(),
            arrow_owner_ink: HashMap::new(),
            port_kind: HashMap::new(),
            port_owners: HashMap::new(),
            port_out_kind: HashMap::new(),
            stroke_cells: HashMap::new(),
            arrows_of: HashMap::new(),
            edge_out_cells: HashMap::new(),
            port_guest: HashMap::new(),
            picked_port_out: Vec::new(),
            border: HashMap::new(),
            pen_arrow: Default::default(),
            preview: false,
            pen_edge: 0,
            edge_of: vec![-1; n],
            shape_edge: vec![-1; n],
            raised: HashSet::new(),
            picked: None,
            edge_cells: HashMap::new(),
            edge_cands: HashMap::new(),
            focus,
            priority,
            lit: vec![false; n],
            layer_lit: true,
            layer_filter: false,
        }
    }

    /// Raise an edge to the top (selected edges and lit edges in E mode)
    pub(crate) fn raise(&mut self, edge: Option<usize>) {
        if let Some(e) = edge {
            self.raised.insert(e);
        }
    }

    /// A cell has one colour and one shape.
    /// Colour precedence: groups of the node under the cursor > the group drawn first.
    /// So when two groups crowd one cell the colour follows the cursor, and from the
    /// current node you can follow the colour to its own group.
    /// Shape follows first-drawn-wins only and does **not** follow the cursor:
    /// otherwise moving the cursor would flip which group breaks here, turning the
    /// glyphs of a whole edge into another group and looking like the edge deforms
    fn claim(&mut self, i: usize, group: usize, bit: u8, fg: (u8, u8, u8), style: EdgeStyle) {
        let cur = self.shape_edge[i];
        let solid = style == EdgeStyle::Solid;
        if self.layer_lit {
            // A line of the current layer passes here: do not dim even if the shape belongs
            // to another layer
            self.lit[i] = true;
        }
        // The same edge passes through this cell again (bend/self-loop): merge direction
        // bits into a corner
        if cur == self.pen_edge as i32 {
            self.note_stroke(i);
            self.mask[i] |= bit;
            if solid {
                self.style[i] = EdgeStyle::Solid;
                self.solid[i] = true;
            }
            if self.priority[group] {
                self.fg[i] = fg;
            }
            return;
        }
        // Free / the cell belonged to the selected edge (highest priority) / this edge is
        // raised
        let mine = self.picked == Some(self.pen_edge) || self.raised.contains(&self.pen_edge);
        let taken = self.picked.is_some_and(|p| p == cur as usize);
        if (cur < 0 || mine) && !taken {
            self.note_stroke(i);
            self.shape_edge[i] = self.pen_edge as i32;
            // Shape follows this edge own direction bits: two overlapping lines never turn
            // into a cross or a T
            self.mask[i] = bit;
            self.owner[i] = group as i16;
            self.owner_mask[i] = bit;
            self.fg[i] = fg;
            self.mine[i] = self.priority[group];
            self.style[i] = style;
            self.solid[i] = solid;
            self.corner[i] = self.pen_corner;
            return;
        }
        // Another edge already owns this cell: it breaks for one cell here and only
        // records a candidate.
        // Shape and colour follow the first edge: the cursor group must not tint just
        // this cell,
        // which would look like someone else line broke. For a fully recoloured edge use
        // raised (selection / E mode).
        self.note_candidate(i);
        if solid {
            self.style[i] = EdgeStyle::Solid;
            self.solid[i] = true;
        }
    }

    /// The edge being drawn ends up owning this cell: it only enters the hit tables
    /// (candidates / union), not the roles
    fn note_hit(&mut self, i: usize) {
        self.edge_of[i] = self.pen_edge as i32;
        if self.pen_bg.is_some() {
            self.edge_bg[i] = self.pen_bg;
        }
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        self.edge_cells.entry(self.pen_edge).or_default().push(cell);
        self.note_candidate(i);
    }

    /// This cell is part of this edge visible stroke
    fn note_stroke(&mut self, i: usize) {
        self.note_hit(i);
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        self.stroke_cells
            .entry(self.pen_edge)
            .or_default()
            .push(cell);
    }

    /// Port cell: a single role (Arrow / Line); the same role may be shared by several
    /// edges.
    /// The outer cell `outside(cell, side)` records a role too: line starts and arrow
    /// landings both live there
    fn note_port(&mut self, cell: (i32, i32), side: u8, kind: PortKind) {
        self.note_port_cell(cell, kind);
        self.note_out_role(Self::outside(cell, side), kind);
        let owners = self.port_owners.entry(cell).or_default();
        if !owners.contains(&self.pen_edge) {
            owners.push(self.pen_edge);
        }
        if let Some(i) = self.idx(cell.0, cell.1) {
            self.note_hit(i);
        }
    }

    /// Role of the outer cell. Same as the port cell: arrowheads win (they draw a
    /// glyph there), line ends only record
    fn note_out_role(&mut self, cell: (i32, i32), kind: PortKind) {
        match self.port_out_kind.get(&cell).copied() {
            None => {
                self.port_out_kind.insert(cell, kind);
            }
            Some(PortKind::Line) if kind == PortKind::Arrow => {
                self.port_out_kind.insert(cell, PortKind::Arrow);
            }
            Some(_) => {}
        }
    }

    /// Record the role of the port cell itself (outer cells go through `note_out_role`)
    fn note_port_cell(&mut self, cell: (i32, i32), kind: PortKind) {
        // A cell has one port role: arrowheads always win (they draw an arrow here or
        // outside).
        // A line end merging into an arrow cell only records; the cell still shows the
        // arrow.
        // An arrow end taking over a line-end reservation upgrades it (port cells of
        // text nodes draw nothing anyway)
        match self.port_kind.get(&cell).copied() {
            None => {
                self.port_kind.insert(cell, kind);
            }
            Some(PortKind::Line) if kind == PortKind::Arrow => {
                self.port_kind.insert(cell, PortKind::Arrow);
            }
            // A line end merging into an arrow cell only records: the cell still shows the
            // arrow, no T is pressed on it
            Some(PortKind::Arrow) if kind == PortKind::Line => {}
            Some(_) => {}
        }
    }

    /// Record a port outer cell an edge actually uses
    fn note_out(&mut self, cell: (i32, i32)) {
        let list = self.edge_out_cells.entry(self.pen_edge).or_default();
        if !list.contains(&cell) {
            list.push(cell);
        }
    }

    /// Arrow cell: if the same direction was drawn already, only add an owner; glyph
    /// and colour stay with the first edge
    fn note_arrow(&mut self, cell: (i32, i32), fg: (u8, u8, u8)) {
        // Each owner keeps its own ink: a shared arrow lights up in its edge colour
        self.arrow_owner_ink
            .entry((cell, self.pen_edge))
            .or_insert(fg);
        let owners = self.arrow_owners.entry(cell).or_default();
        if !owners.contains(&self.pen_edge) {
            owners.push(self.pen_edge);
        }
        let mine = self.arrows_of.entry(self.pen_edge).or_default();
        if !mine.contains(&cell) {
            mine.push(cell);
        }
        if let Some(i) = self.idx(cell.0, cell.1) {
            self.note_hit(i);
        }
    }

    /// Edges on one cell enter the candidate list in draw order (no duplicates)
    fn note_candidate(&mut self, i: usize) {
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        let list = self.edge_cands.entry(cell).or_default();
        if !list.contains(&self.pen_edge) {
            list.push(self.pen_edge);
        }
    }

    pub(crate) fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        Some(((y - self.y0) * self.w + (x - self.x0)) as usize)
    }

    /// Whether this cell is occupied (node labels only write into free cells)
    pub(crate) fn at(&self, x: i32, y: i32) -> u8 {
        match self.idx(x, y) {
            Some(i) => {
                let mut k = self.node[i];
                if self.mask[i] != 0 || self.deco[i].is_some() {
                    k = k.max(1);
                }
                k
            }
            None => u8::MAX,
        }
    }

    /// Record at (x, y) that this cell connects in some direction
    fn link(&mut self, x: i32, y: i32, bit: u8, group: usize, fg: (u8, u8, u8), style: EdgeStyle) {
        // Off-canvas cells are recorded too: routing checks whether this cell holds
        // another stroke, which would jump around if it followed the viewport.
        // Axis bits ignore ownership as well: merged lines look like one line whoever
        // wins the cell.
        let axis = if bit & LR != 0 { LR } else { UD };
        let ink = self.ink.entry((x, y)).or_default();
        ink.0 |= 1 << group;
        ink.1 |= axis;
        let Some(i) = self.idx(x, y) else { return };
        self.groups_here[i] |= 1 << group;
        self.claim(i, group, bit, fg, style);
    }

    /// Add one direction bit to this cell only, touching neither neighbours nor port
    /// cells.
    /// The port-to-outer-cell span of a `none` node draws no line (so it does not cover
    /// text), but the outer cell needs
    /// the bit facing the port, so a route turning here draws a corner instead of
    /// breaking into ─
    fn link_here(
        &mut self,
        cell: (i32, i32),
        bit: u8,
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        let axis = if bit & LR != 0 { LR } else { UD };
        let ink = self.ink.entry(cell).or_default();
        ink.0 |= 1 << group;
        ink.1 |= axis;
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        self.groups_here[i] |= 1 << group;
        self.claim(i, group, bit, fg, style);
    }

    /// A straight run (one row or one column)
    fn run(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        if a.1 == b.1 {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            for x in x0..x1 {
                self.link(x, a.1, RIGHT, group, fg, style);
                self.link(x + 1, a.1, LEFT, group, fg, style);
            }
        } else if a.0 == b.0 {
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            for y in y0..y1 {
                self.link(a.0, y, DOWN, group, fg, style);
                self.link(a.0, y + 1, UP, group, fg, style);
            }
        }
    }

    /// Cells of a straight run (endpoints included)
    fn span(&self, a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        if a.1 == b.1 {
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                out.push((x, a.1));
            }
        } else if a.0 == b.0 {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                out.push((a.0, y));
            }
        }
        out
    }

    /// Starting at from and walking only cells this group has already drawn, can we
    /// reach to
    fn joined(&self, group: usize, from: (i32, i32), to: (i32, i32)) -> bool {
        let bit = 1u64 << group;
        let has = |p: (i32, i32)| self.ink.get(&p).is_some_and(|v| v.0 & bit != 0);
        if !has(from) || !has(to) {
            return false;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        seen.insert(from);
        while let Some(p) = stack.pop() {
            if p == to {
                return true;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let q = (p.0 + dx, p.1 + dy);
                if has(q) && seen.insert(q) {
                    stack.push(q);
                }
            }
        }
        false
    }

    /// Does this cell already hold a stroke in the **same** direction? Crossing for
    /// one cell is fine (very common), but merging for a whole run
    /// makes it impossible to tell how many edges there are: the later edge disappears
    /// and its arrowhead stays on someone else stroke
    fn dup_run(&self, p: (i32, i32), axis: u8) -> bool {
        self.ink.get(&p).is_some_and(|v| v.1 & axis != 0)
    }

    /// How many cells already drawn by **other groups** this polyline steps on
    /// (reusing its own corridor is free,
    /// otherwise down and back up would detour sideways to save cells and fence off a
    /// box)
    fn cost(&self, a: (i32, i32), corner: (i32, i32), b: (i32, i32), group: usize) -> i32 {
        let bit = 1u64 << group;
        let mut n = 0;
        // Whether this stroke touches any cell this group has not claimed yet
        let fresh = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .any(|p| self.ink.get(&p).map(|v| v.0 & bit == 0).unwrap_or(true));
        // Both ends are connected through cells this group already drew, yet this stroke
        // steps on new cells:
        // that closes a loop in the picture, avoid such a bend
        if fresh && (self.joined(group, a, corner) || self.joined(group, corner, b)) {
            n += 1000;
        }
        // Also account for closing a box: after this stroke lands, if all four sides of
        // a cell become this group strokes,
        // the cell is fenced in (a visible box); avoid that bend as well
        let path: HashSet<(i32, i32)> = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .collect();
        let mine =
            |p: (i32, i32)| path.contains(&p) || self.ink.get(&p).is_some_and(|v| v.0 & bit != 0);
        let mut encloses = false;
        'outer: for &(x, y) in &path {
            for (dx, dy) in [(-1i32, -1i32), (-1, 0), (0, -1), (0, 0)] {
                let c = (x + dx, y + dy);
                if mine(c)
                    && mine((c.0 + 1, c.1))
                    && mine((c.0, c.1 + 1))
                    && mine((c.0 + 1, c.1 + 1))
                {
                    encloses = true;
                    break 'outer;
                }
            }
        }
        if encloses {
            n += 1000;
        }
        // Score horizontal and vertical runs separately: merging along the same axis
        // looks far worse than crossing one cell
        for (p0, p1) in [(a, corner), (corner, b)] {
            let axis = if p0.1 == p1.1 { LR } else { UD };
            for p in self.span(p0, p1) {
                if p == a || p == b {
                    continue;
                }
                if self.dup_run(p, axis) {
                    n += 8;
                }
            }
        }
        for p in self.span(a, corner).into_iter().chain(self.span(corner, b)) {
            if p == a || p == b {
                continue; // both endpoints are nodes already
            }
            let groups = self.ink.get(&p).map(|v| v.0).unwrap_or(0);
            if groups != 0 && groups & bit == 0 {
                n += 1;
            }
            // Cutting through a node (or its box) looks bad: the edge runs into the glyph
            // and easily fences a box with neighbouring runs
            if self.station_cells.contains(&p) {
                n += 100;
            }
        }
        n
    }

    /// One solid edge (use this when box information is unavailable; handy in unit
    /// tests)
    #[cfg(test)]
    pub(crate) fn edge(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
    ) {
        self.edge_full(
            a,
            b,
            group,
            fg,
            arrive_hor,
            heads,
            EdgeStyle::Solid,
            None,
            None,
            None,
        );
    }

    /// One edge: connect directly when on the same row or column, otherwise turn one
    /// right angle.
    ///
    /// Two turn orders exist (horizontal first / vertical first). `arrive_hor` is the
    /// entry direction this end requires
    /// (fixed by the neighbouring nodes, see arrive_hor): when a vertical entry is
    /// required, go horizontally first and then turn in vertically,
    /// so down and back up within one group uses the same column instead of fencing a
    /// box around the node.
    /// Only when that would step on many more foreign cells does the cheaper order
    /// win.
    ///
    /// `heads` says which ends carry arrowheads on a directed edge: (draw at a, draw
    /// at b).
    /// Within one group, a -> b and b -> a are independent edges; merged onto this
    /// span they give one arrowhead at each end.
    /// `a_box`/`b_box` are the end node boxes (origin plus size): with a boxed target
    /// the arrowhead lands in the cell just outside the box
    /// One edge: with boxed ends, fix the ports first, then route the minimum-bend
    /// polyline treating the cell outside each port as the free-segment endpoint.
    ///
    /// `a`/`b` are the centre cells of the ends (drawn to directly for point nodes);
    /// `a_end`/`b_end` fix the port cell when the end is boxed:
    /// the line stops just outside the port and the port cell itself is left to the
    /// T/arrowhead overlay.
    // Routing primitive: endpoints, group, colour, turn order, arrowheads and boxes
    // at both ends; all are needed to draw anything, bundling them just keeps the
    // argument pile in one place
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn edge_full(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
        style: EdgeStyle,
        a_end: Option<EndMark>,
        b_end: Option<EndMark>,
        depart_hor: Option<bool>,
    ) {
        let (at_a, at_b) = heads;
        if a == b {
            return;
        }
        // Ports: fix the border cell first, then take the cell outside it as the line end
        let a_port = a_end.and_then(|e| self.pick_port(e, at_a));
        let b_port = b_end.and_then(|e| self.pick_port(e, at_b));
        let share_a = a_end.is_some_and(|e| e.corners);
        let share_b = b_end.is_some_and(|e| e.corners);
        if a_port.is_none() && b_port.is_none() {
            // Both ends are point nodes: route an L as usual (turn order from the
            // neighbouring nodes, avoiding a fenced box)
            self.edge_point(a, b, group, fg, arrive_hor, depart_hor, heads, style);
            return;
        }
        let s = a_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(a);
        let t = b_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(b);
        let out = a_port.map(|(side, _)| Self::outward(side));
        // The last segment heads into the box: the reverse of the target side outward
        // normal
        let into = b_port.map(|(side, _)| {
            let n = Self::outward(side);
            (-n.0, -n.1)
        });
        // Planning pass: route, record, store the result
        if self.planning {
            self.drop_own_usage();
            let pts = self.route(s, out, t, into, group);
            if pts.is_empty() && (a_end.is_some_and(|e| e.fixed) || b_end.is_some_and(|e| e.fixed))
            {
                // The planned port cannot take a single step (it faces another box): retry with
                // scan the border for a free slot so the edge still gets drawn
                // instead of vanishing
                let a2 = a_end.map(|e| EndMark { fixed: false, ..e });
                let b2 = b_end.map(|e| EndMark { fixed: false, ..e });
                self.edge_full(
                    a, b, group, fg, arrive_hor, heads, style, a2, b2, depart_hor,
                );
                return;
            }
            let mut all = Vec::with_capacity(pts.len() + 2);
            if let Some((_, c)) = a_port {
                all.push(c);
            }
            all.extend_from_slice(&pts);
            if let Some((_, c)) = b_port {
                all.push(c);
            }
            self.note_usage(&all, group);
            self.plan_edges.push(self.pen_edge);
            self.plans.push(Stroke {
                pts,
                a_port,
                b_port,
            });
            return;
        }
        // The drawing pass follows the planned route: recomputing is slower and may
        // disagree with the plan
        let (pts, a_port, b_port) = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            (st.pts, st.a_port, st.b_port)
        } else {
            (self.route(s, out, t, into, group), a_port, b_port)
        };
        // Port cells come from the pair finally chosen: the planned ports may differ from
        // the ones picked here
        // (role ownership changes between passes), so the arrowhead/stub must land
        // outside the cell actually used
        let s = a_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(a);
        let t = b_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(b);
        // Port outer cells actually used (fallback paths included): selection hides
        // foreign arrows through them
        if a_port.is_some() {
            self.note_out(s);
        }
        if b_port.is_some() {
            self.note_out(t);
        }
        if pts.is_empty() {
            // No route for this span: draw nothing for the whole edge, no port stub, no
            // arrowhead
            return;
        }
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        // Port-to-outer-cell step: connects the line to the border; the T/arrowhead then
        // covers the port cell.
        // (Preview needs it too: otherwise both line ends keep a single direction bit and
        // render as a vertical bar.)
        // Text nodes have no border: skip this cell so the line does not run over the
        // text
        if let Some((side, c)) = a_port {
            if a_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, s, group, fg, style);
            } else {
                // Text node: the port cell is skipped, but the outer cell needs the bit facing
                // the port,
                // or a route turning here keeps one direction bit and draws a horizontal bar
                // instead of a corner
                let _ = c;
                self.link_here(s, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            } else {
                let _ = c;
                self.link_here(t, Self::into_side(side), group, fg, style);
            }
        }
        // When the bend list holds one cell only (the two outer cells coincide) there is
        // no neighbour point; fall back to the endpoint itself
        let prev = if pts.len() >= 2 {
            pts[pts.len() - 2]
        } else {
            b
        };
        let next = if pts.len() >= 2 { pts[1] } else { a };
        if let Some((side, c)) = b_port {
            let draw = b_end.map(|e| e.mark).unwrap_or(false);
            // A text node has no border cell to draw, but the port is still reserved so
            // several edges do not fight over one entry point
            self.commit_end(side, c, at_b, fg, draw, share_b);
            if !draw && at_b {
                // No border: the arrowhead lands in the cell outside the region
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(prev, pts[pts.len() - 1], group, fg, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw, share_a);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(next, pts[0], group, fg, None);
        }
    }

    /// Self-loop: both ends land on the same node. Leave through the side of `end`,
    /// then turn and return via the next side clockwise.
    /// If both ends crowded onto one side the route would be a straight line and no
    /// loop would show, so the second end takes another side.
    pub(crate) fn edge_loop(
        &mut self,
        end: EndMark,
        heads: (bool, bool),
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        // The planning pass only records the route, never draws: a self-loop picks ports
        // and claims border cells, so drawing early would make the drawing pass pick
        // other ports and break the loop
        if self.planning {
            return;
        }
        let (at_a, at_b) = heads;
        let side2 = match end.side {
            UP => RIGHT,
            RIGHT => DOWN,
            DOWN => LEFT,
            _ => UP,
        };
        // Borderless nodes (text/point): the port cell is on the node own row,
        // and it must be used even if another edge claimed it: a self-loop squeezed out
        // would be invisible,
        // so this end takes the side centre cell without scanning for a slot
        let (s1, c1) = if end.mark {
            match self.pick_port(end, at_a) {
                Some(x) => x,
                None => return,
            }
        } else {
            (end.side, Self::side_base(end.rect, end.side))
        };
        // The second end prefers the cell closest to the first, so the loop hugs the node
        // instead of spanning half the box
        let nearest = Self::side_cells(end.rect, side2, end.corners)
            .into_iter()
            .min_by_key(|c| (c.0 - c1.0).abs() + (c.1 - c1.1).abs())
            .unwrap_or_else(|| Self::side_base(end.rect, side2));
        let probe2 = EndMark {
            fixed: false,
            rect: end.rect,
            side: side2,
            cell: nearest,
            mark: end.mark,
            corners: end.corners,
        };
        // Claim the first end first so the second cannot pick the same cell.
        // Only needed for boxed nodes: for a point/text node spanning one cell both sides
        // are the same cell, and claiming it would leave nothing to pick
        if !self.preview && end.mark {
            self.border.insert(c1, BorderMark::Port);
        }
        let Some((s2, c2)) = self.pick_port(probe2, at_b) else {
            return;
        };
        let s = Self::outside(c1, s1);
        let t = Self::outside(c2, s2);
        self.note_out(s);
        self.note_out(t);
        let out = Some(Self::outward(s1));
        // The last segment heads into the box: reverse of the second side outward normal
        let into = Some({
            let n = Self::outward(s2);
            (-n.0, -n.1)
        });
        let pts = self.route(s, out, t, into, group);
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        // Port-to-outer-cell step (text nodes have no border, so this cell is not drawn)
        if end.mark {
            self.run(c1, s, group, fg, style);
            self.run(c2, t, group, fg, style);
        } else {
            // Text node: skip the port cell, add the bit facing the port on the outer cell
            // (so a turn renders as a corner)
            self.link_here(s, Self::into_side(s1), group, fg, style);
            self.link_here(t, Self::into_side(s2), group, fg, style);
        }
        self.commit_end(s1, c1, at_a, fg, end.mark, end.corners);
        self.commit_end(s2, c2, at_b, fg, end.mark, end.corners);
        if !end.mark && at_a {
            self.outside_arrow(s, s1, fg);
        }
        if !end.mark && at_b {
            self.outside_arrow(t, s2, fg);
        }
    }

    /// The `lane`-th edge on one span: shift the whole straight run perpendicular to
    /// itself by a few cells and hook back to the node at each end.
    /// `lane = 0` is the middle line. Used only when several edges join two point
    /// nodes:
    /// there is no border to shift onto, so without an offset one edge would cover
    /// another
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn edge_lane(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        lane: i32,
        group: usize,
        fg: (u8, u8, u8),
        heads: (bool, bool),
        style: EdgeStyle,
    ) {
        let (at_a, at_b) = heads;
        if a == b {
            return;
        }
        if lane == 0 || (a.0 != b.0 && a.1 != b.1) {
            self.edge_point(a, b, group, fg, None, None, heads, style);
            return;
        }
        let pts: [(i32, i32); 4] = if a.1 == b.1 {
            // Horizontal run: shift vertically
            [a, (a.0, a.1 + lane), (b.0, b.1 + lane), b]
        } else {
            // Vertical run: shift horizontally
            [a, (a.0 + lane, a.1), (b.0 + lane, b.1), b]
        };
        if self.planning {
            // Lanes must be recorded as well, or the planning pass would dirty the ink
            self.note_usage(&pts, group);
            self.plan_edges.push(self.pen_edge);
            self.plans.push(Stroke {
                pts: pts.to_vec(),
                a_port: None,
                b_port: None,
            });
            return;
        }
        // The drawing pass must consume the planned slot even here, or later edges
        // shift
        let pts = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            st.pts
        } else {
            pts.to_vec()
        };
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        if at_b {
            self.head_arrow(pts[2], b, group, fg, None);
        }
        if at_a {
            self.head_arrow(pts[1], a, group, fg, None);
        }
    }

    /// Routing when both ends are point nodes: connect directly on one row/column,
    /// otherwise turn one right angle
    #[allow(clippy::too_many_arguments)]
    fn edge_point(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        depart_hor: Option<bool>,
        heads: (bool, bool),
        style: EdgeStyle,
    ) {
        let (at_a, at_b) = heads;
        // As usual: straight on one row/column, otherwise pick the cheapest L
        let hor_first = (b.0, a.1);
        let ver_first = (a.0, b.1);
        // Start from from_side when the document specifies it (point nodes have no port
        // cell, so this is the only place to honour it); otherwise choose by target
        // entry and the two axis lengths
        let prefer_hor = depart_hor
            .or_else(|| arrive_hor.map(|h| !h))
            .unwrap_or((b.0 - a.0).abs() >= (b.1 - a.1).abs());
        let corner = if a.1 == b.1 || a.0 == b.0 {
            None
        } else {
            let (c1, c2) = (
                self.cost(a, hor_first, b, group),
                self.cost(a, ver_first, b, group),
            );
            Some(if prefer_hor {
                if c1 <= c2 + 1 {
                    hor_first
                } else {
                    ver_first
                }
            } else if c2 <= c1 + 1 {
                ver_first
            } else {
                hor_first
            })
        };
        let mut simple: Vec<(i32, i32)> = vec![a];
        if let Some(c) = corner {
            simple.push(c);
        }
        simple.push(b);
        // The L is blocked by boxes (a wall taller than the search window): upgrade to
        // obstacle-avoiding search
        let blocked = simple.windows(2).any(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            (1..steps).any(|k| {
                self.station_cells
                    .contains(&(w[0].0 + dx.signum() * k, w[0].1 + dy.signum() * k))
            })
        });
        if !self.planning {
            self.drop_own_usage();
        }
        // When the start direction differs from the target entry direction, one L cannot
        // satisfy both (either the start or the entry is wrong):
        // hand it to the search, which can turn twice
        let dir = |h: bool| {
            if h {
                ((b.0 - a.0).signum(), 0)
            } else {
                (0, (b.1 - a.1).signum())
            }
        };

        let pts = if blocked {
            self.route(a, depart_hor.map(dir), b, arrive_hor.map(dir), group)
        } else {
            simple
        };
        if self.planning {
            self.note_usage(&pts, group);
            self.plan_edges.push(self.pen_edge);
            self.plans.push(Stroke {
                pts,
                a_port: None,
                b_port: None,
            });
            return;
        }
        // The drawing pass uses the planned route, no recomputation
        let pts = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            st.pts
        } else {
            pts
        };
        if pts.is_empty() {
            return; // no route: skip the whole edge
        }
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        if at_b {
            let from = if pts.len() >= 2 {
                pts[pts.len() - 2]
            } else {
                a
            };
            self.head_arrow(from, b, group, fg, None);
        }
        if at_a {
            let from = if pts.len() >= 2 { pts[1] } else { b };
            self.head_arrow(from, a, group, fg, None);
        }
    }

    /// An edge with manual via points: source outer -> each via -> target outer, one
    /// minimum-bend hop per segment.
    /// The first segment follows the source side outward normal, the last follows the
    /// target side inward normal; via points themselves stay fixed
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn path(
        &mut self,
        pts: &[(i32, i32)],
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
        heads: (bool, bool),
        a_end: Option<EndMark>,
        b_end: Option<EndMark>,
    ) {
        if pts.len() < 2 {
            return;
        }
        let (at_a, at_b) = heads;
        let a_port = a_end.and_then(|e| self.pick_port(e, at_a));
        let b_port = b_end.and_then(|e| self.pick_port(e, at_b));
        let share_a = a_end.is_some_and(|e| e.corners);
        let share_b = b_end.is_some_and(|e| e.corners);
        let s = a_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[0]);
        let t = b_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[pts.len() - 1]);
        // Manual via points: drop the ones that fold back. Reaching such a via, the
        // horizontal (or vertical) heading reverses
        // (right then left, down then up); that segment only stretches out and retracts
        // along the same path,
        // adding a tail without an endpoint, often on top of another edge
        let vias: Vec<(i32, i32)> = pts[1..pts.len() - 1].to_vec();
        let mut kept: Vec<(i32, i32)> = Vec::with_capacity(vias.len());
        let mut cursor = s;
        for (i, &v) in vias.iter().enumerate() {
            let next = vias.get(i + 1).copied().unwrap_or(t);
            // A. The via and the next point share a column/row while the heading reverses
            // here
            let back_x = v.1 == next.1
                && (v.0 - cursor.0).signum() != 0
                && (v.0 - cursor.0).signum() != (next.0 - v.0).signum();
            let back_y = v.0 == next.0
                && (v.1 - cursor.1).signum() != 0
                && (v.1 - cursor.1).signum() != (next.1 - v.1).signum();
            // B. The segment into the via is straight (one row/column) but the next one
            // shrinks back along the same line:
            //    the via overshot the target and that leg is wasted
            let back_line_x = v.1 == cursor.1
                && next.0 != v.0
                && (next.0 - v.0).signum() == -(v.0 - cursor.0).signum();
            let back_line_y = v.0 == cursor.0
                && next.1 != v.1
                && (next.1 - v.1).signum() == -(v.1 - cursor.1).signum();
            if back_x || back_y || back_line_x || back_line_y {
                continue;
            }
            kept.push(v);
            cursor = v;
        }
        let mut way: Vec<(i32, i32)> = vec![s];
        way.extend(kept);
        way.push(t);
        if self.planning {
            // Edges with manual vias are part of planning, so other edges can avoid them
            self.note_usage(&way, group);
            return;
        }
        // Port outer cells actually used (fallback paths included)
        if a_port.is_some() {
            self.note_out(s);
        }
        if b_port.is_some() {
            self.note_out(t);
        }
        let n = way.len();
        let out = a_port.map(|(side, _)| Self::outward(side));
        let into = b_port.map(|(side, _)| {
            let o = Self::outward(side);
            (-o.0, -o.1)
        });
        for k in 0..n.saturating_sub(1) {
            if way[k] == way[k + 1] {
                continue;
            }
            let d1 = if k == 0 { out } else { None };
            let d2 = if k + 2 == n { into } else { None };
            let seg = self.route(way[k], d1, way[k + 1], d2, group);
            for w in seg.windows(2) {
                self.run(w[0], w[1], group, fg, style);
            }
        }
        if let Some((side, c)) = a_port {
            if a_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, s, group, fg, style);
            } else {
                self.link_here(s, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            } else {
                self.link_here(t, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            let draw = b_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_b, fg, draw, share_b);
            if !draw && at_b {
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(way[n - 2], t, group, fg, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw, share_a);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(way[1], s, group, fg, None);
        }
    }

    /// Outward normal of a side (pointing away from the box)
    pub(crate) fn outward(side: u8) -> (i32, i32) {
        match side {
            UP => (0, -1),
            DOWN => (0, 1),
            LEFT => (-1, 0),
            _ => (1, 0),
        }
    }

    /// Bit pointing from the outer cell towards the port (used to add the hidden bit
    /// of `none` nodes)
    pub(crate) fn into_side(side: u8) -> u8 {
        match side {
            UP => DOWN,
            DOWN => UP,
            LEFT => RIGHT,
            _ => LEFT,
        }
    }

    /// The cell immediately outside a port cell: the line stops here and the port
    /// cell is left to the T/arrowhead
    pub(crate) fn outside(cell: (i32, i32), side: u8) -> (i32, i32) {
        let n = Self::outward(side);
        (cell.0 + n.0, cell.1 + n.1)
    }

    /// One cell per end as a unit vector; returns None when not orthogonally adjacent
    fn seg_dir(a: (i32, i32), b: (i32, i32)) -> Option<(i32, i32)> {
        if a.1 == b.1 && a.0 != b.0 {
            Some(((b.0 - a.0).signum(), 0))
        } else if a.0 == b.0 && a.1 != b.1 {
            Some((0, (b.1 - a.1).signum()))
        } else {
            None
        }
    }

    /// Whether a polyline is valid: every segment orthogonal, the first along out and
    /// the last along into (when given)
    fn fits(pts: &[(i32, i32)], out: Option<(i32, i32)>, into: Option<(i32, i32)>) -> bool {
        if pts.len() < 2 {
            return false;
        }
        let mut dirs = Vec::with_capacity(pts.len() - 1);
        for w in pts.windows(2) {
            match Self::seg_dir(w[0], w[1]) {
                Some(d) => dirs.push(d),
                None => return false,
            }
        }
        if let Some(d) = out {
            if dirs[0] != d {
                return false;
            }
        }
        if let Some(d) = into {
            if *dirs.last().unwrap() != d {
                return false;
            }
        }
        true
    }

    /// Routing candidates in increasing bend count: 0-bend straight, 1-bend L,
    /// 2-bend jog, 3-bend detour
    fn shapes(
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
    ) -> Vec<Vec<(i32, i32)>> {
        let mut cands: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut push = |pts: Vec<(i32, i32)>| {
            let ok = Self::fits(&pts, out, into) && !cands.contains(&pts);
            if ok {
                cands.push(pts);
            }
        };
        if s == t {
            return cands;
        }
        // 0 bends
        push(vec![s, t]);
        // 1 bend
        if s.0 != t.0 && s.1 != t.1 {
            push(vec![s, (t.0, s.1), t]);
            push(vec![s, (s.0, t.1), t]);
        }
        // 2 bends: offset one cell at the target end, then move the bend towards the
        // source
        let mut rows: Vec<i32> = Vec::new();
        if let Some(i) = into {
            rows.push(t.1 - i.1);
        }
        if let Some(o) = out {
            rows.push(s.1 + o.1);
        }
        rows.push((s.1 + t.1) / 2);
        if s.1 != t.1 {
            rows.push(s.1 + (t.1 - s.1).signum());
        }
        rows.dedup();
        for jy in &rows {
            if s.0 != t.0 {
                push(vec![s, (s.0, *jy), (t.0, *jy), t]);
            }
        }
        let mut cols: Vec<i32> = Vec::new();
        if let Some(i) = into {
            cols.push(t.0 - i.0);
        }
        if let Some(o) = out {
            cols.push(s.0 + o.0);
        }
        cols.push((s.0 + t.0) / 2);
        if s.0 != t.0 {
            cols.push(s.0 + (t.0 - s.0).signum());
        }
        cols.dedup();
        for jx in &cols {
            if s.1 != t.1 {
                push(vec![s, (*jx, s.1), (*jx, t.1), t]);
            }
        }
        // 3 bends: step one cell along the exit normal, then reach the row before the
        // target row
        if let (Some(o), Some(i)) = (out, into) {
            if o.0 != 0 && i.1 != 0 {
                for k in 1..=2 {
                    let x1 = s.0 + o.0 * k;
                    if x1 == t.0 {
                        continue;
                    }
                    let y2 = t.1 - i.1;
                    push(vec![s, (x1, s.1), (x1, y2), (t.0, y2), t]);
                }
            } else if o.1 != 0 && i.0 != 0 {
                for k in 1..=2 {
                    let y1 = s.1 + o.1 * k;
                    if y1 == t.1 {
                        continue;
                    }
                    let x2 = t.0 - i.0;
                    push(vec![s, (s.0, y1), (x2, y1), (x2, t.1), t]);
                }
            }
        }
        cands
    }

    /// Cost of a polyline: hitting a box is fatal, everything else is a weighted sum
    /// (see `C_*`).
    /// Endpoints themselves never count as overlap/crossing: fixed ports land there
    /// by design
    fn route_cost(&self, pts: &[(i32, i32)], group: usize) -> Option<i32> {
        let bit = 1u64 << group;
        let last = pts.len() - 1;
        let mut cost = 0;
        for (k, w) in pts.windows(2).enumerate() {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            let axis = if dx != 0 { LR } else { UD };
            for step in 0..=steps {
                let p = (w[0].0 + (dx.signum()) * step, w[0].1 + (dy.signum()) * step);
                let is_end = (k == 0 && step == 0) || (k == last - 1 && step == steps);
                if self.station_cells.contains(&p) && !is_end {
                    return None; // never cut through a box
                }
                cost += 1;
                if is_end {
                    continue;
                }
                if let Some((groups, axes)) = self.ink.get(&p) {
                    if *axes & axis != 0 {
                        // Same axis: merging makes this stretch invisible, the most expensive case
                        cost += C_OVERLAP;
                    } else if *axes != 0 {
                        cost += C_CROSS;
                    }
                    if *groups != 0 && *groups & bit == 0 {
                        cost += C_OTHER_GROUP;
                    }
                } else if let Some((_, axes)) = self.usage.get(&p) {
                    // An edge already planned through here: a light cost, enough to separate two
                    // lines between the same pair of boxes
                    cost += if *axes & axis != 0 {
                        C_USAGE
                    } else {
                        C_USAGE / 3
                    };
                }
            }
            if k > 0 {
                cost += C_BEND;
            }
        }
        Some(cost)
    }

    /// Minimum-bend routing: leave s along out and enter t along into; returns the
    /// bend list including both endpoints.
    /// Try fixed shapes first (they naturally put the jog near the target), then
    /// relax the direction constraint, and only then search a detour
    fn route(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Vec<(i32, i32)> {
        if s == t {
            // The two port outer cells coincide (a gap of exactly one cell): the stubs meet
            return vec![s];
        }
        for (o, i) in [(out, into), (None, None)] {
            if let Some(pts) = self.route_shaped(s, o, t, i, group) {
                return pts;
            }
        }
        for (o, i) in [(out, into), (None, None)] {
            if let Some(pts) = self.route_search(s, o, t, i, group) {
                return pts;
            }
        }
        // Detour impossible: use one L, and draw nothing if it hits a box (no dangling
        // line)
        self.route_clip(s, t)
    }

    /// Pick the cheapest fixed shape. Bends are no longer an absolute priority:
    /// one extra bend is worth it if it avoids covering a long stretch of another
    /// edge
    fn route_shaped(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Option<Vec<(i32, i32)>> {
        let manhattan = (s.0 - t.0).abs() + (s.1 - t.1).abs();
        // Drop candidates that detour too far: better a little more overlap than a huge
        // loop
        let limit = manhattan * 3 + 40;
        let mut best: Option<(i32, Polyline)> = None;
        for cand in Self::shapes(s, out, t, into) {
            let Some(cost) = self.route_cost(&cand, group) else {
                continue;
            };
            let len: i32 = cand
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs())
                .sum();
            if len > limit {
                continue;
            }
            if best.as_ref().map(|(c, _)| cost < *c).unwrap_or(true) {
                best = Some((cost, cand));
            }
        }
        best.map(|b| b.1)
    }

    /// Detour search: bends are costed, boxes are never cut through, and other groups
    /// are avoided where possible
    fn route_search(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Option<Vec<(i32, i32)>> {
        // Try a small window first (cheap) and widen when needed: a wall wider than 6
        // cells can still be rounded.
        // When the whole frame is huge (thousands of cells across) skip the second tier:
        // that is the degraded path for big pictures
        let (fx0, fy0, fx1, fy1) = self.frame;
        let area = (fx1 - fx0).max(0) as i64 * (fy1 - fy0).max(0) as i64;
        if let Some(pts) = self.route_search_in(s, out, t, into, group, 6) {
            return Some(pts);
        }
        if area > 250_000 {
            return None;
        }
        self.route_search_in(s, out, t, into, group, i32::MAX)
    }

    fn route_search_in(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
        pad: i32,
    ) -> Option<Vec<(i32, i32)>> {
        self.route_search_ext(s, out, t, into, group, pad, 40_000)
    }

    /// As above, but with a larger search budget
    #[allow(clippy::too_many_arguments)]
    fn route_search_ext(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
        pad: i32,
        budget: u64,
    ) -> Option<Vec<(i32, i32)>> {
        const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let dir_idx = |d: (i32, i32)| DIRS.iter().position(|x| *x == d);
        let turn = C_BEND;
        let bit = 1u64 << group;
        // Window: the bounding box of both ends plus a few cells, preferably inside the
        // canvas (a detour off-screen is invisible);
        // both ends are always included even when off-canvas
        // pad may be i32::MAX (the whole picture), so use saturating maths to avoid
        // overflow
        let (lx, hx) = (
            s.0.min(t.0).saturating_sub(pad),
            s.0.max(t.0).saturating_add(pad),
        );
        let (ly, hy) = (
            s.1.min(t.1).saturating_sub(pad),
            s.1.max(t.1).saturating_add(pad),
        );
        // Clamp the window to the whole picture: following the viewport would make
        // routes change with the view, and detouring outside the picture is pointless
        let (fx0, fy0, fx1, fy1) = self.frame;
        let lx = lx.max(fx0).min(s.0.min(t.0));
        let hx = hx.min(fx1).max(s.0.max(t.0));
        let ly = ly.max(fy0).min(s.1.min(t.1));
        let hy = hy.min(fy1).max(s.1.max(t.1));
        if t.0 < lx || t.0 > hx || t.1 < ly || t.1 > hy {
            return None;
        }
        let key = |p: (i32, i32), d: usize| (p, d);
        let free = |p: (i32, i32)| -> bool {
            if p == s || p == t {
                return true;
            }
            if p.0 < lx || p.0 > hx || p.1 < ly || p.1 > hy {
                return false;
            }
            !self.station_cells.contains(&p)
        };
        // Heap entries: (estimate, cost so far, point, direction)
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(i32, i32, Point, usize)>> =
            std::collections::BinaryHeap::new();
        let mut best: HashMap<State, i32> = HashMap::new();
        let mut from: HashMap<State, State> = HashMap::new();
        let h = |p: (i32, i32)| (p.0 - t.0).abs() + (p.1 - t.1).abs();
        match out.and_then(dir_idx) {
            Some(d) => {
                heap.push(std::cmp::Reverse((h(s), 0, s, d)));
                best.insert(key(s, d), 0);
            }
            None => {
                for (d, (dx, dy)) in DIRS.iter().copied().enumerate() {
                    // Start direction unknown: try all four, but the first step must really be
                    // walkable
                    let p = (s.0 + dx, s.1 + dy);
                    if !free(p) {
                        continue;
                    }
                    let axis = if dx != 0 { LR } else { UD };
                    let cost = 1 + self.step_cost(p, bit, axis);
                    heap.push(std::cmp::Reverse((cost + h(p), cost, p, d)));
                    best.insert(key(p, d), cost);
                    from.insert(key(p, d), (s, d));
                }
            }
        }
        let start_dir = out.and_then(dir_idx);
        let goal_dir = into.and_then(dir_idx);
        // Hard cap: even the largest window must not exhaust a frame budget
        let mut pops = 0u64;
        let budget: u64 = if pad <= 6 { 12_000 } else { budget };
        while let Some(std::cmp::Reverse((_, cost, p, d))) = heap.pop() {
            pops += 1;
            if pops > budget {
                return None;
            }
            // Drop stale states before checking the goal: the other order would return a
            // worse path as the result
            if best.get(&(p, d)).copied().unwrap_or(i32::MAX) < cost {
                continue;
            }
            if p == t && goal_dir.map(|g| g == d).unwrap_or(true) {
                let mut pts = vec![t];
                let mut cur = (p, d);
                while let Some(&(prev, pd)) = from.get(&cur) {
                    if prev == s {
                        break;
                    }
                    pts.push(prev);
                    cur = (prev, pd);
                }
                pts.push(s);
                pts.reverse();
                return Some(pts);
            }
            for (nd, (dx, dy)) in DIRS.iter().copied().enumerate() {
                let q = (p.0 + dx, p.1 + dy);
                if !free(q) {
                    continue;
                }
                if nd != d && start_dir.is_some() && p == s {
                    continue;
                }
                let axis = if dx != 0 { LR } else { UD };
                let nc = cost + 1 + self.step_cost(q, bit, axis) + if nd == d { 0 } else { turn };
                if nc < best.get(&key(q, nd)).copied().unwrap_or(i32::MAX) {
                    best.insert(key(q, nd), nc);
                    from.insert(key(q, nd), (p, d));
                    heap.push(std::cmp::Reverse((nc + h(q), nc, q, nd)));
                }
            }
        }
        None
    }

    /// An isolated edge is not drawn: its planning slot must be consumed as well,
    /// or later edges take someone else stroke through the cursor and shift the
    /// whole picture
    pub(crate) fn skip_plan(&mut self, e: usize) {
        while self.plan_cursor < self.plans.len() && self.plan_edges[self.plan_cursor] == e {
            self.plan_cursor += 1;
        }
    }

    /// Extra cost of stepping into this cell: `axis` is the direction of this step.
    /// Coaxial overlap = merging into a foreign line (invisible for the stretch), a
    /// crossing is just a knot; other groups are cosmetic
    fn step_cost(&self, p: (i32, i32), bit: u64, axis: u8) -> i32 {
        // Foreign port/arrow cell: a soft obstacle (go around if possible; failing is no
        // error)
        let guest = if self.port_guest.get(&p).is_some_and(|e| *e != self.pen_edge) {
            C_FOREIGN_PORT
        } else {
            0
        };
        if let Some((groups, axes)) = self.ink.get(&p) {
            let mut cost = guest;
            if *axes & axis != 0 {
                cost += C_OVERLAP;
            } else if *axes != 0 {
                cost += C_CROSS;
            }
            if *groups != 0 && *groups & bit == 0 {
                cost += C_OTHER_GROUP;
            }
            return cost;
        }
        let usage = match self.usage.get(&p) {
            Some((_, axes)) if *axes & axis != 0 => C_USAGE,
            Some((_, axes)) if *axes != 0 => C_USAGE / 3,
            _ => 0,
        };
        guest + usage
    }

    /// This edge turn to draw: withdraw its own planning reservations first so its
    /// route does not fight itself
    pub(crate) fn drop_own_usage(&mut self) {
        if let Some(cells) = self.usage_edges.get(&self.pen_edge) {
            for c in cells {
                self.usage.remove(c);
            }
        }
    }

    /// Planning phase: record the cells a stroke claims into `usage`
    pub(crate) fn note_usage(&mut self, pts: &[(i32, i32)], group: usize) {
        let bit = 1u64 << group;
        let mut cells: Vec<(i32, i32)> = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            let axis = if dx != 0 { LR } else { UD };
            for step in 0..=steps {
                let p = (w[0].0 + dx.signum() * step, w[0].1 + dy.signum() * step);
                let e = self.usage.entry(p).or_insert((0, 0));
                e.0 |= bit;
                e.1 |= axis;
                cells.push(p);
            }
        }
        self.usage_edges
            .entry(self.pen_edge)
            .or_default()
            .extend(cells);
    }

    /// Fallback: one L, stopping one cell before a box
    fn route_clip(&self, s: (i32, i32), t: (i32, i32)) -> Vec<(i32, i32)> {
        let mut pts = vec![s];
        let mut cur = s;
        let mut legs = Vec::new();
        if s.0 != t.0 {
            legs.push((t.0, s.1));
        }
        if s.1 != t.1 {
            let last = *pts.last().unwrap();
            legs.push((last.0.max(t.0).min(t.0), t.1));
        }
        for corner in legs {
            let (dx, dy) = ((corner.0 - cur.0).signum(), (corner.1 - cur.1).signum());
            let steps = (corner.0 - cur.0).abs().max((corner.1 - cur.1).abs());
            for _ in 0..steps {
                let next = (cur.0 + dx, cur.1 + dy);
                if self.station_cells.contains(&next) && next != t {
                    // Cannot get through: return an empty path, draw nothing (no half line, no
                    // port/arrowhead)
                    return Vec::new();
                }
                pts.push(next);
                cur = next;
            }
        }
        pts
    }

    /// Centre cell of one side (by width/height parity).
    /// One convention for all four sides: `left + w/2` / `top + h/2`, no separate
    /// parity rules per axis (actual port positions come from render PortPlan; this
    /// only supplies a starting point for self-loops and preview)
    pub(crate) fn side_base(rect: (i32, i32, i32, i32), side: u8) -> (i32, i32) {
        let (left, top, w, h) = rect;
        match side {
            UP => (left + w / 2, top),
            DOWN => (left + w / 2, top + h - 1),
            LEFT => (left, top + h / 2),
            _ => (left + w - 1, top + h / 2),
        }
    }

    /// Cells of this side except the corners, clockwise
    /// Cells of this side in clockwise order.
    /// `corners`: boxed nodes keep lines off the four corners (corners need their
    /// corner glyph);
    /// text nodes have no corners, so the whole side accepts a connection
    pub(crate) fn side_cells(
        rect: (i32, i32, i32, i32),
        side: u8,
        corners: bool,
    ) -> Vec<(i32, i32)> {
        let (left, top, w, h) = rect;
        let (right, bottom) = (left + w - 1, top + h - 1);
        // x1 / y1 are exclusive upper bounds
        let (x0, x1) = if corners {
            (left + 1, right)
        } else {
            (left, right + 1)
        };
        let (y0, y1) = if corners {
            (top + 1, bottom)
        } else {
            (top, bottom + 1)
        };
        match side {
            UP => (x0..x1).map(|x| (x, top)).collect(),
            RIGHT => (y0..y1).map(|y| (right, y)).collect(),
            DOWN => (x0..x1).rev().map(|x| (x, bottom)).collect(),
            _ => (y0..y1).rev().map(|y| (left, y)).collect(),
        }
    }

    /// Find a usable border cell: prefer the preferred one, then scan clockwise and
    /// then counter-clockwise when taken.
    /// Junctions may merge into an existing junction (direction bits combine); an
    /// arrowhead needs a cell of its own
    fn border_slot(&self, end: EndMark, arrow: bool) -> Option<(i32, i32)> {
        let cells = Self::side_cells(end.rect, end.side, end.corners);
        if cells.is_empty() {
            return None;
        }
        let start = cells
            .iter()
            .position(|c| *c == end.cell || *c == Self::side_base(end.rect, end.side))
            .unwrap_or(0);
        // Port roles: an Arrow cell only takes an arrowhead end; a Line cell only takes
        // an ordinary connection,
        // and only a cell already turned into a T admits a second line (a plain
        // reserved cell still holds one)
        let free = |c: &(i32, i32), s: &Self| match s.port_kind.get(c) {
            None => true,
            Some(PortKind::Arrow) => false,
            Some(PortKind::Line) => {
                !arrow && matches!(s.border.get(c), Some(BorderMark::Junction(..)))
            }
        };
        let n = cells.len();
        for k in 0..n {
            let c = cells[(start + k) % n];
            if free(&c, self) {
                return Some(c);
            }
        }
        for k in 1..n {
            let c = cells[(start + n - k) % n];
            if free(&c, self) {
                return Some(c);
            }
        }
        None
    }

    /// Fix the port: use the picking rule of the chosen side; if that side is full,
    /// move to the next side clockwise, opposite side last
    fn pick_port(&self, end: EndMark, arrow: bool) -> Option<(u8, (i32, i32))> {
        // Ports of ordinary edges are fixed cells from PortPlan: cell occupancy on the
        // border is not consulted.
        // But if another box stole this cell role as the other kind, fall back to
        // scanning the border (roles are mutually exclusive by invariant)
        if end.fixed {
            let ok = match self.port_kind.get(&end.cell) {
                None => true,
                Some(k) => (*k == PortKind::Arrow) == arrow,
            };
            if ok {
                return Some((end.side, end.cell));
            }
        }
        let order = match end.side {
            UP => [UP, RIGHT, DOWN, LEFT],
            RIGHT => [RIGHT, DOWN, LEFT, UP],
            DOWN => [DOWN, LEFT, UP, RIGHT],
            _ => [LEFT, UP, RIGHT, DOWN],
        };
        for side in order {
            // The first cell out of this side is a box: not a single step is possible, try
            // the next side
            let base = if side == end.side {
                end.cell
            } else {
                Self::side_base(end.rect, side)
            };
            if self.station_cells.contains(&Self::outside(base, side)) {
                continue;
            }
            let probe = EndMark {
                fixed: false,
                rect: end.rect,
                side,
                // Keep the caller preferred cell: a self-loop picks its closest to the other
                // end slot from it
                cell: if side == end.side {
                    end.cell
                } else {
                    Self::side_base(end.rect, side)
                },
                mark: end.mark,
                corners: end.corners,
            };
            if let Some(c) = self.border_slot(probe, arrow) {
                return Some((side, c));
            }
        }
        None
    }

    /// Stamp the marker on the chosen port cell: an arrowhead end embeds the arrow,
    /// the other end draws a T.
    /// With `draw = false` (text node) only reserve, drawing nothing
    fn commit_end(
        &mut self,
        side: u8,
        cell: (i32, i32),
        arrow: bool,
        fg: (u8, u8, u8),
        draw: bool,
        share: bool,
    ) {
        if self.preview {
            return; // preview draws no T/arrowhead, which would look already connected
        }
        if !draw {
            // A text node has no border: the port cell only records a role (one edge per
            // cell per side by default, shifting to the next cell);
            // when the box is larger than the text (padding left by a style change) follow
            // the boxed rules, where several edges may share one port,
            // keeping port assignment and routing stable across style changes
            if share {
                let bits = match side {
                    UP | DOWN => LR | side,
                    _ => UD | side,
                };
                match self.border.get_mut(&cell) {
                    Some(BorderMark::Junction(m, c)) => {
                        *m |= bits;
                        *c = fg;
                    }
                    _ => {
                        self.border.insert(cell, BorderMark::Junction(bits, fg));
                    }
                }
            }
            let kind = if arrow {
                PortKind::Arrow
            } else {
                PortKind::Line
            };
            self.note_port(cell, side, kind);
            return;
        }
        if arrow {
            let ch = self.pen_arrow_glyph(side);
            // A same-direction arrow already here is shared: glyph and colour stay with the
            // first edge,
            // later edges only join the owner set (selecting/highlighting any of them lights
            // it)
            let shared =
                matches!(self.border.get(&cell), Some(BorderMark::Arrow(a, _)) if *a == ch);
            if !shared {
                self.border.insert(cell, BorderMark::Arrow(ch, fg));
            }
            self.note_port(cell, side, PortKind::Arrow);
            self.note_arrow(cell, fg);
        } else {
            // An arrowhead already holds this cell (another edge landed first): only record,
            // press no T;
            // a cell shows exactly one role
            if self.port_kind.get(&cell) == Some(&PortKind::Arrow) {
                self.note_port(cell, side, PortKind::Line);
                return;
            }
            // Top and bottom sides get a horizontal line plus a vertical stub; left and right
            // get a vertical line plus a horizontal stub
            let bits = match side {
                UP | DOWN => LR | side,
                _ => UD | side,
            };
            match self.border.get_mut(&cell) {
                Some(BorderMark::Junction(m, c)) => {
                    *m |= bits;
                    *c = fg;
                }
                _ => {
                    self.border.insert(cell, BorderMark::Junction(bits, fg));
                }
            }
            self.note_port(cell, side, PortKind::Line);
        }
    }

    /// Arrowhead glyph for this edge: ASCII output is a hard compatibility mode, so an
    /// edge that stores `head = triangles` still draws `^ v < >` there.
    fn pen_arrow_glyph(&self, side: u8) -> char {
        if self.glyphs.ascii {
            crate::model::ArrowStyle::Ascii.arrow(side)
        } else {
            self.pen_arrow.arrow(side)
        }
    }

    /// Arrowhead of a text node (no border): lands in the cell outside the region,
    /// still pointing at the node
    pub(crate) fn outside_arrow(&mut self, cell: (i32, i32), side: u8, fg: (u8, u8, u8)) {
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        let ch = self.pen_arrow_glyph(side);
        if self.node[i] != 0 {
            return;
        }
        // This cell already starts an ordinary connection: an arrowhead here would make
        // that line start look like <->.
        // Same-direction arrows excepted (sharing one arrow slot is fine)
        if self.port_out_kind.get(&cell) == Some(&PortKind::Line) && self.arrow[i] != Some(ch) {
            return;
        }
        self.place_arrow(i, cell, ch, fg);
    }

    /// Land an arrowhead: share when the glyph matches (just add an owner), otherwise
    /// try to displace a foreign one
    fn place_arrow(&mut self, i: usize, cell: (i32, i32), ch: char, fg: (u8, u8, u8)) {
        if self.arrow[i] == Some(ch) {
            self.note_arrow(cell, fg);
            return;
        }
        if !self.arrow_free(cell, ch) {
            return;
        }
        self.put_arrow(i, cell, ch, fg);
    }

    /// Whether an arrowhead may land here: free / same glyph (shared) / raised or
    /// selected edges may displace a foreign one
    fn arrow_free(&self, cell: (i32, i32), ch: char) -> bool {
        let Some(i) = self.idx(cell.0, cell.1) else {
            return true;
        };
        let Some(a) = self.arrow[i] else { return true };
        if a == ch {
            return true;
        }
        let owners = self.arrow_owners.get(&cell);
        let taken_by_picked = owners.is_some_and(|o| self.picked.is_some_and(|p| o.contains(&p)));
        if self.picked == Some(self.pen_edge) {
            return !taken_by_picked;
        }
        if !self.raised.contains(&self.pen_edge) {
            return false;
        }
        let other_raised = owners.is_some_and(|o| o.iter().any(|e| self.raised.contains(e)));
        !taken_by_picked && !other_raised
    }

    /// Arrowheads draw on top (over foreign strokes) with their own edge colour; a
    /// same-direction shared arrow takes its glyph and colour from the first edge only
    fn put_arrow(&mut self, i: usize, cell: (i32, i32), ch: char, fg: (u8, u8, u8)) {
        self.arrow[i] = Some(ch);
        self.arrow_ink[i] = Some(fg);
        self.arrow_owners.insert(cell, vec![self.pen_edge]);
        self.note_arrow(cell, fg);
    }

    /// Arrowhead of a directed edge: drawn in the cell immediately before to (in the
    /// direction from from to to).
    /// With a boxed to it lands in the cell just outside the box along the last
    /// segment heading (`to_box` is the box origin and size).
    /// One arrowhead per cell; skip when it would hit a node glyph or the cell belongs
    /// to another group
    fn head_arrow(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        to_box: Option<(i32, i32, i32, i32)>,
    ) {
        let (dx, dy) = ((to.0 - from.0).signum(), (to.1 - from.1).signum());
        let (cell, ch) = if dx != 0 {
            let cell = match to_box {
                // The box sits at to: the arrowhead lands one cell outside its left/right edge,
                // on the row where the edge enters
                Some((left, _top, w, _h)) => {
                    if dx > 0 {
                        (left - 1, to.1)
                    } else {
                        (left + w, to.1)
                    }
                }
                None => (to.0 - dx, to.1),
            };
            (
                cell,
                // Heading east = entering from the node left
                if dx > 0 {
                    self.pen_arrow_glyph(LEFT)
                } else {
                    self.pen_arrow_glyph(RIGHT)
                },
            )
        } else if dy != 0 {
            let cell = match to_box {
                Some((_left, top, _w, h)) => {
                    if dy > 0 {
                        (to.0, top - 1)
                    } else {
                        (to.0, top + h)
                    }
                }
                None => (to.0, to.1 - dy),
            };
            (
                cell,
                // Heading down = entering from the node top
                if dy > 0 {
                    self.pen_arrow_glyph(UP)
                } else {
                    self.pen_arrow_glyph(DOWN)
                },
            )
        } else {
            return;
        };
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        if self.node[i] != 0 {
            return; // node glyph wins
        }
        // Land the arrowhead even if another edge passes through this cell: arrowheads
        // are on top, and the direction comes from the last cell of this edge route
        let _ = group;
        self.place_arrow(i, cell, ch, fg);
    }

    /// Register the cells a node box occupies up front (edges avoid them instead of
    /// cutting through the box).
    /// `at` is the box origin
    pub(crate) fn reserve_box(&mut self, at: (i32, i32), size: (usize, usize)) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        for y in at.1..at.1 + h {
            for x in at.0..at.0 + w {
                // Off-screen boxes block too: otherwise moving the view would let lines cut
                // through them
                self.station_cells.insert((x, y));
            }
        }
    }

    /// Draw a node box (called after the edges, on top of them).
    /// `at` is the box origin, `size` is width/height in screen cells. Text is drawn
    /// by render via text::put.
    /// `fill` is the in-box background (the whole rectangle, border and text cells
    /// included); blit drops it when dimmed
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn box_at(
        &mut self,
        node: usize,
        at: (i32, i32),
        size: (usize, usize),
        style: BoxStyle,
        groups: u64,
        fg: (u8, u8, u8),
        fill: Option<(u8, u8, u8)>,
    ) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let [hz, vt, tl, tr, bl, br] = self.glyphs.box_chars(style);
        let (left, top) = at;
        for yy in 0..h {
            for xx in 0..w {
                let ch = if yy == 0 {
                    if xx == 0 {
                        tl
                    } else if xx == w - 1 {
                        tr
                    } else {
                        hz
                    }
                } else if yy == h - 1 {
                    if xx == 0 {
                        bl
                    } else if xx == w - 1 {
                        br
                    } else {
                        hz
                    }
                } else if xx == 0 || xx == w - 1 {
                    vt
                } else {
                    ' '
                };
                let (x, y) = (left + xx, top + yy);
                let Some(i) = self.idx(x, y) else { continue };
                // When the border is overlaid by a T or an embedded arrow, use the overlaid
                // glyph and colour
                let (ch, fg) = match self.border.get(&(x, y)) {
                    Some(BorderMark::Junction(m, c)) => (self.glyphs.junction(style, *m), *c),
                    Some(BorderMark::Arrow(c, col)) => (*c, *col),
                    // Reserved-only port: draw the border as is
                    Some(BorderMark::Port) | None => (ch, fg),
                };
                self.station_cells.insert((x, y));
                // The node box covers an edge: whether this cell counts as the current layer is
                // decided by the box own layer
                self.lit[i] = self.layer_lit;
                self.deco[i] = Some(Deco {
                    ch,
                    fg,
                    bg: fill,
                    groups,
                    node: node as i32,
                });
            }
        }
    }

    /// Region border: only glyph and colour, no node/station reservation (edges may
    /// cross it).
    /// Drawn before the boxes, so later boxes/edges cover it
    pub(crate) fn plate_at(
        &mut self,
        at: (i32, i32),
        size: (usize, usize),
        style: BoxStyle,
        fg: (u8, u8, u8),
    ) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let [hz, vt, tl, tr, bl, br] = self.glyphs.box_chars(style);
        let (left, top) = at;
        for yy in 0..h {
            for xx in 0..w {
                let ch = if yy == 0 {
                    if xx == 0 {
                        tl
                    } else if xx == w - 1 {
                        tr
                    } else {
                        hz
                    }
                } else if yy == h - 1 {
                    if xx == 0 {
                        bl
                    } else if xx == w - 1 {
                        br
                    } else {
                        hz
                    }
                } else if xx == 0 || xx == w - 1 {
                    vt
                } else {
                    continue;
                };
                let (x, y) = (left + xx, top + yy);
                let Some(i) = self.idx(x, y) else { continue };
                self.lit[i] = self.layer_lit;
                self.deco[i] = Some(Deco {
                    ch,
                    fg,
                    bg: None,
                    groups: 0,
                    node: -1,
                });
            }
        }
    }

    /// Region title: painted straight onto the border (no node reservation); later
    /// boxes cover the overlap
    pub(crate) fn stamp_at(&mut self, x: i32, y: i32, text: &str, fg: (u8, u8, u8)) {
        let mut cx = x;
        for ch in text.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as i32;
            let Some(i) = self.idx(cx, y) else { break };
            self.lit[i] = self.layer_lit;
            self.deco[i] = Some(Deco {
                ch,
                fg,
                bg: None,
                groups: 0,
                node: -1,
            });
            cx += w.max(1);
        }
    }

    /// group_mask: every group this node passes through; group: which colour to use
    /// (fg comes from it too).
    /// An isolated node belongs to no group and passes None: cells are still drawn,
    /// in neutral colours
    pub(crate) fn node_at(
        &mut self,
        x: i32,
        y: i32,
        hub: bool,
        group_mask: u64,
        group: Option<usize>,
        fg: (u8, u8, u8),
    ) {
        let Some(i) = self.idx(x, y) else { return };
        self.node[i] = if hub { 2 } else { 1 };
        // Dimming of node cells follows the node own layer
        self.lit[i] = self.layer_lit;
        self.groups_here[i] |= group_mask;
        self.node_groups[i] = group_mask;
        self.ink.entry((x, y)).or_default().0 |= group_mask;
        match group {
            Some(g) => {
                self.owner[i] = g as i16;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = self.priority[g];
            }
            None => {
                self.owner[i] = -1;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = false;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests build defaults then tweak
// a few fields; clearer than a long struct literal
mod tests;
