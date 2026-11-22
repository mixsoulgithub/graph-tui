//! Interaction state and events. This layer does no drawing (render does); it only
//! tracks where the view points, where the cursor is, and what is being searched.
//!
//! The data lives in `Document` (box / text / edge separate, coordinates in cells);
//! this layer holds view state (centre, zoom, search, selection) and the commands that
//! edit the document. Every action that changes geometry requires 1:1 zoom: at any
//! other zoom level the picture is read-only.

use std::path::Path;
use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::document::{Document, Hits, Layer, ViewState};
use crate::lang::Lang;
use crate::layout::{self, Layout};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Side};
use crate::paths;
use crate::render;

mod cmd;
mod cmd_clip;
mod cmd_doc;
mod cmd_io;
mod cmd_ports;
mod cmd_style;
mod cmd_view;
mod edit;
mod history;
mod input;
mod io;
mod link;
mod menu;
mod mouse;
mod nav;
mod state;
mod tabs;

/// One full cursor pulse cycle in seconds: about 75 beats per minute, like a heartbeat
const BEAT: f64 = 0.8;

/// Quantisation steps for the pulse animation: a full repaint at 60fps is wasted bytes
/// on a real terminal
const PULSE_STEPS: f64 = 16.0;

/// Pan duration bounds in seconds: distant targets get a little more, never
/// proportionally more
const PAN_MIN: f64 = 0.2;
const PAN_MAX: f64 = 0.42;

/// Zoom range, shared by keyboard and wheel
const ZOOM_MIN: f64 = 0.05;
/// Top out at 16: glyphs do not grow, so zooming further just spreads boxes apart and
/// empties the picture
const ZOOM_MAX: f64 = 16.0;
/// One keyboard step
const ZOOM_STEP: f64 = 1.25;
/// One wheel step (a trackpad tick is one event, so the step must be finer)
const WHEEL_STEP: f64 = 1.1;
/// Snap to 1:1 within this distance instead of resting at an unreadable "x1.02"
const SNAP: f64 = 0.06;
/// A right-drag must move this many cells to count as a pan; less is a click that
/// opens the menu (Chebyshev distance)
const RIGHT_PAN_THRESHOLD: i32 = 2;

/// A pan animation: eased over **duration**, not a fixed number of cells per frame
#[derive(Clone, Copy)]
pub struct Pan {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub start: Instant,
    /// Duration in seconds (distant targets get more, capped)
    pub dur: f64,
}

/// What a held left button is dragging: a box drag on a box, a view pan on empty space
enum Drag {
    /// Pan the view: start cell, view centre on press, projection at press
    Pan {
        from: (u16, u16),
        center: (f64, f64),
        proj: render::Proj,
    },
    /// Drag out a selection: the pressed cell is the anchor (only a real drag becomes a
    /// selection, a click does not)
    Select { anchor: render::Cell },
    /// Drag boxes: start cell, top-left cell of each box in the batch at press, and
    /// whether it really moved (a bare click moves nothing and pushes no undo step)
    Boxes {
        from: (u16, u16),
        at: Vec<(usize, (i32, i32))>,
        /// Projection at press: screen cells map to canvas cells through it whatever the zoom
        proj: render::Proj,
        /// Cursor and selection at press (both move by the same delta)
        cursor0: render::Cell,
        sel0: Option<(render::Cell, render::Cell, bool)>,
        began: bool,
    },
    /// Drag from a box border to link; `moved` = it really dragged (otherwise a plain click)
    Link { source: usize, moved: bool },
    /// Right press on a box: dragging links, not dragging is a click that opens the menu
    RightLink {
        source: usize,
        /// Cell of the press: the menu opens here
        press: (u16, u16),
        moved: bool,
    },
    /// Right press on empty space or an edge: either a pan drag or just a click for the menu
    RightPan {
        /// Cell of the press: the menu opens here, not where the button is released
        press: (u16, u16),
        center: (f64, f64),
        proj: render::Proj,
        /// Already past the threshold: no menu even if the pointer returns to the start
        moved: bool,
    },
    /// Reconnect an existing edge by dragging its end: edge index, which end (true = to)
    Reconnect {
        edge: usize,
        to_end: bool,
        moved: bool,
    },
}

/// File-open picker overlay: a filter line on top, filtered candidates below
#[derive(Clone)]
pub struct FilePick {
    /// Candidates (full paths)
    pub items: Vec<String>,
    /// Index of the selected candidate (an index into the filtered list)
    pub at: usize,
    /// Filter text
    pub filter: String,
}

impl FilePick {
    /// Candidates still visible after filtering (indices into items)
    pub fn shown(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return (0..self.items.len()).collect();
        }
        (0..self.items.len())
            .filter(|i| self.items[*i].to_lowercase().contains(&f))
            .collect()
    }

    /// Path of the current selection
    pub fn current(&self) -> Option<&str> {
        let shown = self.shown();
        shown
            .get(self.at.min(shown.len().saturating_sub(1)))
            .map(|i| self.items[*i].as_str())
    }
}

/// View state: render only reads it, app changes it
#[derive(Clone)]
pub struct View {
    /// Which layout is shown (display and the r-key cycle only; opening a file does not
    /// recompute coordinates from it)
    pub layout: Layout,
    /// Layout orientation: auto or a fixed angle (only affects re-layout by r / :layout)
    pub orientation: layout::Orientation,
    /// In-place box text editing (entered with i): draft + caret (in characters)
    pub edit: Option<BoxEdit>,
    /// Current actual angle (0/90/180/270); `:rotate` takes its delta from it
    pub angle: i32,
    /// Search query being typed (None = not searching)
    pub query: Option<String>,
    /// Box indices of search hits/highlights (kept so n can jump through them)
    pub hits: Vec<usize>,
    /// Focused groups (only these keep their colour, everything else is dimmed)
    pub focus: Vec<usize>,
    /// Which hit is current
    pub hit_at: usize,
    /// Text of the last text search: render fills only the hit characters (None for
    /// digits or an empty query)
    pub hit_text: Option<String>,
    /// Zoom factor (1 = one canvas coordinate per column)
    pub zoom: f64,
    /// View centre (cell coordinates); None = the centre of the whole picture
    pub center: Option<(f64, f64)>,
    /// Pan target (see app::tick)
    pub target: Option<(f64, f64)>,
    /// Pan in progress
    pub pan: Option<Pan>,
    /// Cursor cell on the canvas (the real terminal cursor points at it)
    pub cursor: render::Cell,
    /// What lies under the cursor cell (derived by refresh_pick after each move)
    pub pick: render::Pick,
    /// When several edges share a cell, E cycles through that cell's candidates to this
    /// index
    pub cand: usize,
    /// Whether all node labels are drawn
    pub node_labels: bool,
    /// Cursor drawn into the buffer (real-cursor mode turns this off: the terminal draws
    /// it and glyphs stay untouched)
    pub drawn: bool,
    /// Whether to draw edge labels (text boxes bound to edges); on by default,
    /// `--no-edge-labels` turns it off
    pub edge_labels: bool,
    /// Source path on the left of the status line (`~`-abbreviated, absolute); None =
    /// scratch buffer
    pub file_display: Option<String>,
    /// Current working directory on the right of the status line (`~`-abbreviated)
    pub cwd: String,
    /// Pending key sequence (`12` / `100,100` / `g`), shown at the bottom right
    pub showcmd: String,
    /// Command-line caret (in characters, 0 = start of line)
    pub cmd_pos: usize,
    /// Caret in the bottom prompt line (in characters)
    pub prompt_pos: usize,
    /// Caret in the search query (in characters)
    pub query_pos: usize,
    /// Default edge ink (config `edge_color`): used by edges that set no colour;
    /// None = follow the group colour
    pub edge_ink: Option<(u8, u8, u8)>,
    /// Default text ink (config `text_color`): used by text that sets no colour;
    /// None = follow the box/edge/default bright colour
    pub text_ink: Option<(u8, u8, u8)>,
    /// One tip picked at random for this session (resolved for the current language);
    /// always None for the on-screen frame / export / headless
    pub tip: Option<String>,
    /// E isolate: show only this node, its neighbours and their edges (press E or Esc
    /// again to leave)
    pub ego: Option<usize>,
    /// Node id of the centre: document edits shift indices, so this is how the centre is
    /// found again (leave isolation when it cannot be found)
    pub ego_id: Option<String>,
    /// Pulse phase of the cursor node: 0 = the cell's own colour, 1 = pure red
    pub pulse: f64,
    /// Cells jumped to (search, mouse, half-screen); Ctrl-o / Ctrl-i walk the jump list

    /// Whether the key cheat sheet is shown
    pub help: bool,
    /// Detour tolerance: how many times farther than the global best a same-group
    /// candidate may be
    pub cycle: Option<f64>,
    /// Sideways weight: multiplier for lateral skew in the score
    pub side: f64,
    /// Input buffer of the `:` command line
    pub cmd: Option<String>,
    /// Single-line bottom prompt (new box / edit text / link)
    pub prompt: Option<(Prompt, String)>,
    /// Right-click action menu
    pub menu: Option<Menu>,
    /// Completion popup for the `:` line: candidates + how many are consumed
    pub cmd_menu: Option<(Vec<String>, usize)>,
    /// Path hint on the right of the command line (where it writes / reads), recomputed
    /// per keystroke
    pub cmd_hint: Option<String>,
    /// The source file changed on disk (not yet reloaded / overwritten); flagged at the
    /// right of the status line
    pub stale: bool,
    /// Multiple documents: which of how many (status line only, never persisted)
    pub tab: Option<(usize, usize)>,
    /// Name of the current tab (file name or scratch)
    pub tab_name: String,
    /// Whether the current tab has unsaved changes
    pub tab_dirty: bool,
    /// Dragging boxes: hide the selection rectangle for now (it only gets in the way)
    pub drag_hide_sel: bool,
    /// Level of detail at low zoom; dump / export turn it off to always draw all text
    pub lod: bool,
    /// Point node labels that did not fit in the last frame (hinted on an idle status line)
    pub labels_hidden: usize,
    /// Glyphs to render: Unicode box drawing (default) or plain ASCII (`--ascii`)
    pub glyphs: crate::glyphs::Glyphs,
    /// Bottom notice (command echo)
    pub note: Option<String>,
    /// UI language: only affects the app's own notices
    pub lang: Lang,
    /// Instant the pulse phase is measured from (used by tick)
    pub started: Instant,

    /// Block selection (anchor cell + other end + whether it is confirmed)
    pub sel: Option<RectSel>,
    /// Two-stage link with e (sources chosen, waiting for targets)
    pub link: Option<LinkState>,
    /// Which layer is shown / edited (None = all)
    pub layer: Option<String>,
    /// File-open picker overlay (opened by `:e` with no argument)
    pub file_pick: Option<FilePick>,
}

impl Default for View {
    fn default() -> Self {
        View {
            layout: Layout::Geo,
            orientation: layout::Orientation::Auto,
            edit: None,
            angle: 0,
            query: None,
            hits: Vec::new(),
            focus: Vec::new(),
            hit_at: 0,
            hit_text: None,
            zoom: 1.0,
            center: None,
            target: None,
            pan: None,
            cursor: (0, 0),
            pick: render::Pick::None,
            cand: 0,
            node_labels: false,
            drawn: true,
            edge_labels: true,
            file_display: None,
            cwd: String::new(),
            showcmd: String::new(),
            cmd_pos: 0,
            prompt_pos: 0,
            query_pos: 0,
            edge_ink: None,
            text_ink: None,
            ego: None,
            ego_id: None,
            tip: None,
            pulse: 0.0,

            help: false,
            cycle: None,
            side: 2.0,
            cmd: None,
            prompt: None,
            menu: None,
            cmd_menu: None,
            cmd_hint: None,
            stale: false,
            tab: None,
            tab_name: String::new(),
            tab_dirty: false,
            drag_hide_sel: false,
            lod: true,
            labels_hidden: 0,
            glyphs: crate::glyphs::Glyphs::default(),
            note: None,
            lang: Lang::default(),
            started: Instant::now(),
            sel: None,
            link: None,
            layer: None,
            file_pick: None,
        }
    }
}

/// Three sources for the edge-label switch: auto = follow the document's view.notes,
/// on/off = forced from the command line
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeLabelsMode {
    #[default]
    Auto,
    On,
    Off,
}

impl EdgeLabelsMode {
    pub fn parse(v: &str) -> Option<EdgeLabelsMode> {
        match v.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(EdgeLabelsMode::Auto),
            "on" | "true" | "1" => Some(EdgeLabelsMode::On),
            "off" | "false" | "0" => Some(EdgeLabelsMode::Off),
            _ => None,
        }
    }

    pub fn apply(self, notes: bool) -> bool {
        match self {
            EdgeLabelsMode::Auto => notes,
            EdgeLabelsMode::On => true,
            EdgeLabelsMode::Off => false,
        }
    }
}

pub struct Options {
    pub layout: Option<Layout>,
    pub node_labels: bool,
    /// Edge-label switch: auto = follow the document
    pub edge_labels: EdgeLabelsMode,
    /// false = use the terminal's real cursor instead of drawing one into the buffer
    pub drawn: bool,
    /// Put the cursor on this box at startup (name/alias, fuzzy)
    pub at: Option<String>,
    /// Run a search at startup (same as / , typing, Enter)
    pub find: Option<String>,
    /// UI language from `--lang` / `[prefs] lang`; `None` keeps the document's
    /// `view.lang` (or English when the document has none)
    pub lang: Option<Lang>,
    /// Undo depth limit (default 200)
    pub history: Option<usize>,
    /// Key bindings (from config.ini, defaults in keys.rs)
    pub keys: crate::keys::KeyMap,
    /// Mouse on/off (on by default)
    pub mouse: bool,
    /// Right-drag from a box to link (on by default; off means right-drag only pans)
    pub mouse_edges: bool,
    /// Arrowhead glyph style
    /// Layout spacing (lane gap / layer gap / margin)
    pub spacing: layout::Spacing,
    /// Glyphs to render (config `render = unicode|ascii`)
    pub glyphs: crate::glyphs::Glyphs,
    /// Default edge ink (None = follow the group colour)
    pub edge_ink: Option<(u8, u8, u8)>,
    /// Default text ink (None = follow the box/edge/default bright colour)
    pub text_ink: Option<(u8, u8, u8)>,
    /// Command used to open links (None/empty = auto)
    pub opener: Option<String>,
    /// `[links]`: custom scheme -> command (lowercase scheme)
    pub links: Vec<(String, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            layout: None,
            node_labels: false,
            edge_labels: EdgeLabelsMode::Auto,
            drawn: false,
            at: None,
            find: None,
            lang: None,
            history: None,
            keys: crate::keys::KeyMap::defaults(),
            mouse: true,
            mouse_edges: true,
            spacing: layout::Spacing::default(),
            glyphs: crate::glyphs::Glyphs::default(),
            edge_ink: None,
            text_ink: None,
            opener: None,
            links: Vec::new(),
        }
    }
}

/// In-place box text editing: edits the draft only, Esc writes it back (one undo step)
#[derive(Clone, Debug, PartialEq)]
pub struct BoxEdit {
    /// Index of the box being edited
    pub box_idx: usize,
    /// This box was created by the i press: cancelling drops the whole step, no empty box
    pub created: bool,
    /// Draft (raw text; markdown markers show literally here so the caret lines up)
    pub text: String,
    /// Caret position in characters
    pub caret: usize,
}

impl BoxEdit {
    /// On-screen size in cells the draft needs for this box style: a text node is the text
    /// itself, a boxed one adds two border cells
    pub fn text_size(&self, doc: &crate::document::Document, i: usize) -> (i32, i32) {
        let mut widest = 0i32;
        for line in self.text.split('\n') {
            widest = widest.max(unicode_width::UnicodeWidthStr::width(line) as i32);
        }
        let lines = self.text.split('\n').count().max(1) as i32;
        let bare = !doc
            .boxes
            .get(i)
            .map(|b| b.style.is_boxed())
            .unwrap_or(false);
        // One extra cell so the caret at the end of a line has somewhere to sit instead of
        // landing on the border
        if bare {
            (widest + 1, lines.max(1))
        } else {
            ((widest + 3).max(4), (lines + 2).max(3))
        }
    }
}

/// Edit target handed to Ctrl+G
#[derive(Clone, Debug, PartialEq)]
pub enum EditTarget {
    /// Bottom prompt: submitted as if Enter was pressed when the editor returns
    Prompt(Prompt),
    /// Box text: written into those boxes when the editor returns (one undo step)
    Boxes(Vec<usize>),
}

/// Single-line bottom prompt (same machinery as `:`)
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    NewBox,
    /// Edit the text of a batch of boxes (as many as the selection holds)
    Text(Vec<usize>),
    Link(usize),
}

/// One menu entry: run a verb, or descend into another level
#[derive(Clone, Debug)]
pub enum MenuAction {
    Cmd(String),
    Sub(Vec<MenuItem>),
    /// Link mode: true = confirm the link, false = cancel
    Link(bool),
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub title: String,
    pub action: MenuAction,
    /// Colour swatch: drawn as the background of two spaces, no wide glyph
    pub swatch: Option<(u8, u8, u8)>,
    /// Current value (marked `*` in lists)
    pub checked: bool,
}

impl MenuItem {
    fn new(title: impl Into<String>, action: MenuAction) -> MenuItem {
        MenuItem {
            title: title.into(),
            action,
            swatch: None,
            checked: false,
        }
    }

    fn swatch(mut self, c: (u8, u8, u8)) -> MenuItem {
        self.swatch = Some(c);
        self
    }

    fn checked(mut self, on: bool) -> MenuItem {
        self.checked = on;
        self
    }
}

/// Right-click action menu: opens at the click, one level shown at a time
#[derive(Clone)]
pub struct Menu {
    /// Top-left corner on screen
    pub at: (u16, u16),
    /// Each level (0 is the top)
    pub levels: Vec<Vec<MenuItem>>,
    /// Index within the current level
    pub sel: usize,
    /// Document when the menu opened: a preview returns to it, then applies the current
    /// entry
    pub snapshot: Document,
    /// Cursor and pick when the menu opened (previews may change them; both are restored
    /// on return)
    pub snapshot_cursor: (render::Cell, render::Pick),
    /// Title at the top of the menu (colour menus have one level; the status line shows
    /// "border colour #rrggbb" from it)
    pub title: Option<String>,
    /// Path taken in (each submenu level pushes a label, leaving pops one)
    pub parents: Vec<String>,
    /// Entry each level stopped on when entered: going back (h) must land on it, not the
    /// first
    pub parents_sel: Vec<usize>,
    /// Which menu this is (box / edge / colour-... / canvas): groups the "last picked"
    /// memory
    pub kind: String,
}

impl Menu {
    pub fn level(&self) -> &[MenuItem] {
        self.levels.last().map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Block selection: anchor, current other end, and whether Enter confirmed it
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RectSel {
    pub anchor: render::Cell,
    pub end: render::Cell,
    pub confirmed: bool,
}

/// Two-stage link state for e: pick the sources first, then the targets.
/// Lives only for this session, never written to `.gtui`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    /// Source box indices (deduplicated and sorted)
    pub sources: Vec<usize>,
    /// Explicitly picked targets (Shift+left click); empty means derive from cursor /
    /// selection
    pub targets: Vec<usize>,
    /// Wait for a second Enter when there are many
    pub confirming: bool,
}

/// Elements one command applies to
#[derive(Default, Clone, Debug)]
pub struct TargetSet {
    pub boxes: Vec<usize>,
    pub edges: Vec<usize>,
}

impl TargetSet {
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.edges.is_empty()
    }
}

/// Delete clipboard: a single slot, session-only
#[derive(Default, Clone)]
pub struct Clipboard {
    pub origin: render::Cell,
    pub boxes: Vec<crate::document::Box>,
    pub texts: Vec<crate::document::Text>,
    pub edges: Vec<crate::document::DocEdge>,
    pub groups: Vec<crate::document::Group>,
    /// Layers these boxes/edges use (rebuilt in order on paste)
    pub layers: Vec<crate::document::Layer>,
}

/// Verbs of the `:` command line (for completion; the order is the hint order)
const VERBS: &[&str] = &[
    "box",
    "corner",
    "cycle",
    "del",
    "dir",
    "head",
    "e",
    "edgecolor",
    "export",
    "export-canvas",
    "edgebg",
    "fill",
    "group",
    "history",
    "lang",
    "layer",
    "layout",
    "link",
    "new",
    "nodecolor",
    "node-labels",
    "edge-labels",
    "q",
    "redo",
    "relabel",
    "rotate",
    "save",
    "side",
    "style",
    "text",
    "textalign",
    "textbg",
    "textcolor",
    "tolayer",
    "undo",
    "via",
    "w",
    "wq",
];

#[derive(Default)]
/// How the source file looks on disk: length + mtime. Detects edits by other programs
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileStamp {
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

impl FileStamp {
    /// Current stamp of the file; None when it cannot be read (missing / no permission)
    pub fn of(path: &str) -> Option<FileStamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(FileStamp {
            len: meta.len(),
            mtime: meta.modified().ok(),
        })
    }
}

pub struct History {
    past: Vec<(String, Document)>,
    future: Vec<(String, Document)>,
    cap: usize,
}

impl History {
    pub fn new(cap: usize) -> History {
        History {
            past: Vec::new(),
            future: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// Push a snapshot before editing the document; the label names this step (for echo)
    pub fn begin(&mut self, label: &str, doc: &Document) {
        self.future.clear();
        self.past.push((label.to_string(), doc.clone()));
        if self.past.len() > self.cap {
            self.past.remove(0);
        }
    }

    /// Drop the last step (used to roll back a transaction)
    pub fn drop_last(&mut self) {
        self.past.pop();
    }

    /// Drop the snapshot when the step did not change the document (e.g. setting the same
    /// value); returns whether a snapshot was kept (i.e. the document changed)
    pub fn prune(&mut self, doc: &Document) -> bool {
        if let Some((_, before)) = self.past.last() {
            if before == doc {
                self.past.pop();
                return false;
            }
            return true;
        }
        false
    }

    /// Undo: returns the name of this step
    pub fn undo(&mut self, doc: &mut Document) -> Option<String> {
        let (label, before) = self.past.pop()?;
        self.future.push((label.clone(), doc.clone()));
        *doc = before;
        Some(label)
    }

    /// Redo
    pub fn redo(&mut self, doc: &mut Document) -> Option<String> {
        let (label, after) = self.future.pop()?;
        self.past.push((label.clone(), doc.clone()));
        *doc = after;
        Some(label)
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

/// What the main loop should do
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Continue,
    Quit,
}

/// Style copied by new boxes (from the most recently "selected" box)
#[derive(Clone, Copy)]
pub struct BoxLook {
    pub style: crate::model::BoxStyle,
    pub color: Option<(u8, u8, u8)>,
    pub fill: Option<(u8, u8, u8)>,
    pub text_color: Option<(u8, u8, u8)>,
    pub text_bg: Option<(u8, u8, u8)>,
    pub align: crate::model::Align,
}

/// Style copied by new edges (from the most recently "selected" edge)
#[derive(Clone)]
pub struct EdgeLook {
    pub group: String,
    pub style: crate::model::EdgeStyle,
    pub corner: crate::model::Corner,
    pub color: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    /// New edges reuse this arrowhead glyph (None = follow the global setting)
    pub head: Option<crate::model::ArrowStyle>,
}

pub struct App {
    pub doc: Document,
    pub view: View,
    /// Data source (shown in the status line); None when it came from stdin
    pub source: Option<String>,
    /// Frame area of the last frame (mouse coordinates are mapped through it; refreshed
    /// by draw)
    pub area: Rect,
    /// The first frame has not been drawn yet (it pulls the cursor into view)
    first_frame: bool,
    /// Edge hit table of the last frame (for clicking edges; refreshed by draw)
    pub hit: render::FrameHit,
    /// `:q` asked to quit (the main loop turns it into Flow::Quit)
    pub quit: bool,
    /// Key map (normal mode; config.ini can override it)
    pub keys: crate::keys::KeyMap,
    /// Index of the current tip (advances per session; `view.tip` is it resolved for the
    /// language)
    pub tip_at: usize,
    /// Mouse on/off (when off, mouse events are ignored)
    pub mouse: bool,
    /// Right-drag from a box to link (when off, right-drag only pans; turn it off when
    /// `none` text nodes get linked by accident too often)
    pub mouse_edges: bool,
    /// Layout spacing (used by the r cycle / :layout)
    pub spacing: layout::Spacing,
    /// Glyphs to render (Unicode by default; `--ascii` switches to plain ASCII)
    pub glyphs: crate::glyphs::Glyphs,
    /// Current working directory (`~`-abbreviated) for the status line; computed at startup
    pub cwd: String,
    /// Style copied by new boxes: the last box clicked, or the one just restyled by menu
    /// or command
    pub box_look: Option<BoxLook>,
    /// Style copied by new edges: the most recently selected edge
    pub edge_look: Option<EdgeLook>,
    /// Undo/redo stack
    pub undo: History,
    /// Previewing a menu entry: no undo step, no echo
    pub previewing: bool,
    /// Delete clipboard (one slot, session-only)
    pub clipboard: Option<Clipboard>,
    /// Digit prefix (5l = move 5 boxes)
    pending: Option<u32>,
    /// Pending key sequence (`12`, `100,100`, `g`): shown at the bottom right, like
    /// nvim's showcmd
    seq: String,
    /// Open plan waiting for main to run (Editor / Opener / Spawn / Note); App only
    /// records it
    open_request: Option<crate::target::OpenPlan>,
    /// Command used to open links (`[prefs] opener`; empty = auto)
    opener: Option<String>,
    /// `[links]`: custom scheme -> command (lowercase scheme), from the config file
    links: Vec<(String, String)>,
    /// Jump list: spans tabs, each entry records which cell of which tab
    jumps: Vec<Jump>,
    /// Which jump-list entry the cursor is on
    jump_at: usize,
    /// Path last picked in each menu (menu name -> title and index per level):
    /// restored from this stack next time, including where going back up a level stops
    menu_memory: std::collections::HashMap<String, Vec<(String, usize)>>,
    /// Time and cell of the last left press (double-click detection)
    last_click: Option<(std::time::Instant, (u16, u16))>,
    /// Edge label just created by i (Esc removes it while still empty)
    note_draft: Vec<String>,
    /// Ctrl+G: waits for main to run the external editor (edit target + initial content)
    edit_request: Option<(EditTarget, String)>,
    /// What a held left button is dragging (see Drag)
    drag: Option<Drag>,
    /// Where the cursor was before entering search (Esc restores it)
    search_from: render::Cell,
    /// Focus when the search started (digit group searches change it; Esc restores it)
    search_focus: Vec<usize>,
    /// Stamp of the source file on disk (taken on load / save); detects external edits
    src_stamp: Option<FileStamp>,
    /// Number of local edits; equal to `src_rev` means "untouched", so it can auto-reload
    pub rev: u64,
    /// rev baseline at load / save time
    src_rev: u64,

    /// R was pressed once (waiting for a second press to discard local changes)
    reload_armed: Option<std::time::Instant>,
    /// Multiple documents: each slot is either a parked tab or None (= the one on screen)
    pub tabs: Vec<Option<Parked>>,
    /// Which tab is on screen
    pub tab_at: usize,
}

/// A parked tab: the active tab's state still lives in `App` fields, and its slot is
/// None
pub struct Parked {
    doc: Document,
    view: View,
    source: Option<String>,
    undo: History,
    rev: u64,
    src_rev: u64,
    src_stamp: Option<FileStamp>,
    reload_armed: Option<std::time::Instant>,
}

impl App {
    /// Split marker: writing it in text cuts a new box
    const SPLIT_MARK: &'static str = "<|new box|>";
}

/// Is this cell inside the rectangle selection (works whichever end is larger)
fn rect_has(anchor: render::Cell, end: render::Cell, cell: render::Cell) -> bool {
    let (x0, x1) = (anchor.0.min(end.0), anchor.0.max(end.0));
    let (y0, y1) = (anchor.1.min(end.1), anchor.1.max(end.1));
    cell.0 >= x0 && cell.0 <= x1 && cell.1 >= y0 && cell.1 <= y1
}

/// 1:1 means one canvas coordinate per column (floating-point error allowed)
fn at_one_scale(view: &View) -> bool {
    (view.zoom - 1.0).abs() < 1e-9
}

/// Snap to 1:1 when close: a zoom barely off 1:1 looks the blurriest
fn snap_zoom(z: f64) -> f64 {
    if (z - 1.0).abs() <= SNAP {
        1.0
    } else {
        z
    }
}

fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(ZOOM_MIN, ZOOM_MAX)
    } else {
        1.0
    }
}

/// Centre of the whole picture (cell coordinates)
fn bbox_center(doc: &Document) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox();
    ((x0 as f64 + x1 as f64) / 2.0, (y0 as f64 + y1 as f64) / 2.0)
}

/// Name shown in the status line: only the first line of multi-line text
fn label_of(doc: &Document, i: usize) -> String {
    doc.text(i)
        .and_then(|t| t.text.split('\n').next())
        .map(str::to_string)
        .unwrap_or_default()
}

/// Display a colour as #rrggbb / auto
fn color_name(c: Option<(u8, u8, u8)>) -> String {
    match c {
        Some((r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        None => "auto".to_string(),
    }
}

/// Find a box by name/alias (for --at)
fn find_box(doc: &Document, name: &str) -> Option<usize> {
    match doc.search(name) {
        Hits::Boxes(h) => h.first().copied(),
        _ => None,
    }
}

/// Pulse intensity curve: a sine over the loop position phase in 0..1.
/// 0 -> 1 -> 0, resting smoothly at both ends (a sine's slope is already 0 there),
/// so the pulse does not stutter
fn pulse_at(phase: f64) -> f64 {
    0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()
}

/// Digit prefix: 5l = move 5 boxes. True means this key was consumed as a count
fn count_digit(pending: &mut Option<u32>, c: char) -> bool {
    let Some(d) = c.to_digit(10) else {
        return false;
    };
    if d == 0 && pending.is_none() {
        return false; // 0 still means "reset" when no prefix is pending
    }
    *pending = Some((pending.unwrap_or(0) * 10 + d).min(999));
    true
}

/// Raw text after the command name: strip only the one separating space/tab; later
/// spaces count as content
fn cmd_rest<'a>(line: &'a str, cmd: &str) -> &'a str {
    let s = line.trim_start();
    let r = s.get(cmd.len()..).unwrap_or("");
    r.strip_prefix(' ')
        .or_else(|| r.strip_prefix('\t'))
        .unwrap_or(r)
}

/// Byte offset of the idx-th character (end of string when out of range)
fn char_byte(text: &str, idx: usize) -> usize {
    text.char_indices()
        .nth(idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Insert inline: the caret follows
fn edit_insert(text: &mut String, pos: &mut usize, s: &str) {
    if s.is_empty() {
        return;
    }
    let b = char_byte(text, *pos);
    text.insert_str(b, s);
    *pos += s.chars().count();
}

/// Backspace: delete the character before the caret
fn edit_backspace(text: &mut String, pos: &mut usize) {
    if *pos == 0 {
        return;
    }
    let cur = char_byte(text, *pos);
    let prev = char_byte(text, *pos - 1);
    text.replace_range(prev..cur, "");
    *pos -= 1;
}

/// Delete: remove the character at the caret
fn edit_delete(text: &mut String, pos: usize) {
    if pos >= text.chars().count() {
        return;
    }
    let a = char_byte(text, pos);
    let b = char_byte(text, pos + 1);
    text.replace_range(a..b, "");
}

/// Word character: anything but whitespace (a path like `out/x.gtui` is one word)
fn is_word_char(c: char) -> bool {
    !c.is_whitespace()
}

/// Move back to the start of the previous word (emacs M-b)
fn word_left(text: &str, mut pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    while pos > 0 && !is_word_char(chars[pos - 1]) {
        pos -= 1;
    }
    while pos > 0 && is_word_char(chars[pos - 1]) {
        pos -= 1;
    }
    pos
}

/// Move forward to the end of the next word (emacs M-f)
fn word_right(text: &str, mut pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    while pos < chars.len() && !is_word_char(chars[pos]) {
        pos += 1;
    }
    while pos < chars.len() && is_word_char(chars[pos]) {
        pos += 1;
    }
    pos
}

fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().trim_start_matches('#').as_bytes();
    if hex.len() != 6 || !hex.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| -> Option<u8> {
        let text = std::str::from_utf8(&hex[i..i + 2]).ok()?;
        u8::from_str_radix(text, 16).ok()
    };
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// "x,y x,y ..." -> integer via points; one bad part fails the whole string
fn parse_via(s: &str) -> Option<Vec<(i32, i32)>> {
    if s.trim().is_empty() {
        return None;
    }
    s.split_whitespace()
        .map(|part| {
            let (x, y) = part.split_once(',')?;
            Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?))
        })
        .collect()
}

/// Is this command on PATH (no shell involved, just join PATH entries)
fn which(cmd: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
}

/// Parse a goto sequence: `a,b` = column,row (matching `x,y` in the status line); a
/// lone `n` means vim's "line n", leaving the column unchanged
fn parse_goto(seq: &str, cur: (i32, i32)) -> Option<(i32, i32)> {
    let seq = seq.trim_end_matches('G');
    match seq.split_once(',') {
        Some((a, b)) => {
            if b.is_empty() {
                return None;
            }
            Some((a.parse().ok()?, b.parse().ok()?))
        }
        None => Some((cur.0, seq.parse().ok()?)),
    }
}

/// One jump-list entry: which cell of which tab (needed to get back across buffers)
#[derive(Clone, Copy, PartialEq, Eq)]
struct Jump {
    tab: usize,
    cell: render::Cell,
}

/// Recompute hits while searching: digits = focus those groups, text = box name/alias
fn refresh(doc: &Document, view: &mut View) {
    let text = view.query.clone().unwrap_or_default();
    view.hit_at = 0;
    // Remember the word for text hits: render fills only the matching characters
    view.hit_text = Some(text.trim().to_string()).filter(|t| !t.is_empty());
    match doc.search(&text) {
        // Digits: focus these groups (boxes they pass through all count as hits, cursor stays)
        Hits::Groups(groups) => {
            view.hits = (0..doc.boxes.len())
                .filter(|i| doc.box_groups(*i).iter().any(|g| groups.contains(g)))
                .collect();
            // A digit search has no word to fill: no fill, dimming the rest via focus does the job
            view.hit_text = None;
            view.focus = groups;
        }
        // Text: pick the hit boxes only (they get the search background) and jump the cursor
        // to the first. Do not touch focus -- boxes on one chain already share a group, so
        // focusing would light them all up and the whole chain would look hit
        Hits::Boxes(hits) => {
            if let Some(&first) = hits.first() {
                // Put the cursor on the matched character (the terminal cursor marks the
                // current hit)
                view.cursor = view
                    .hit_text
                    .as_deref()
                    .and_then(|q| doc.hit_cell(first, q))
                    .or_else(|| doc.text(first).map(|t| t.at))
                    .unwrap_or_else(|| (doc.boxes[first].at.0 + 1, doc.boxes[first].at.1 + 1));
            }
            view.hits = hits;
        }
        Hits::Empty => {
            view.hits.clear();
            view.hit_text = None;
        }
    }
}

/// Full arrow-key box selection: returns (chosen, candidates ranked by score
/// [(box, score, same group)]).
///
/// Score is `distance + side_w x lateral skew`. Candidates fall into two tiers:
///   tier 0 = boxes in "the same group"; tier 1 = everything else.
/// The best tier-0 candidate is taken as long as it is within cycle x the global best.
/// Pick the nearest node in a direction. The live cursor never calls this, and the
/// `cycle`/`side` knobs only affect it; it is kept because it is still the right tool
/// for directional choice, and removing it would drop a set of real behaviour tests.
#[allow(dead_code)]
fn step_cfg(
    doc: &Document,
    proj: &render::Proj,
    cursor: usize,
    dir: (i32, i32),
    focus: &[usize],
    cycle: f64,
    side_w: f64,
) -> (usize, Vec<(usize, f64, bool)>) {
    if cursor >= doc.boxes.len() {
        return (cursor, Vec::new());
    }
    let cur = proj.cell(doc.center(cursor));
    let (dc, dr) = (dir.0 as f64, dir.1 as f64);
    let mut cands: Vec<(u8, f64, usize, bool)> = Vec::new();
    for i in 0..doc.boxes.len() {
        if i == cursor {
            continue;
        }
        let c = proj.cell(doc.center(i));
        let (dx, dy) = ((c.0 - cur.0) as f64, (c.1 - cur.1) as f64);
        let along = dx * dc + dy * dr;
        if along <= 0.0 {
            continue; // not in this direction
        }
        let side = (dx * dr - dy * dc).abs();
        if side > along {
            continue; // more than 45 degrees off, not "this direction"
        }
        // The "same group" tier: use the focused groups when any, else the cursor box's own
        // groups
        let mine: &[usize] = if focus.is_empty() {
            doc.box_groups(cursor)
        } else {
            focus
        };
        let focused = doc.box_groups(i).iter().any(|g| mine.contains(g));
        let tier = if focused { 0u8 } else { 1u8 };
        let score = (dx * dx + dy * dy).sqrt() + side * side_w;
        cands.push((tier, score, i, focused));
    }
    cands.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let ranked: Vec<(usize, f64, bool)> = cands.iter().map(|&(_, s, i, f)| (i, s, f)).collect();
    let all_min = cands
        .iter()
        .map(|&(_, s, _, _)| s)
        .fold(f64::INFINITY, f64::min);
    // Take the best tier-0 candidate when it is within cycle x the global best
    if let Some(&(0, score, i, _)) = cands.first() {
        if score <= cycle * all_min {
            return (i, ranked);
        }
    }
    let best = cands
        .iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|&(_, _, i, _)| i)
        .unwrap_or(cursor);
    (best, ranked)
}
fn animate(view: &mut View, doc: &Document, cell: f64) -> bool {
    let Some(to) = view.target else {
        view.pan = None;
        return false;
    };
    let cur = view.center.unwrap_or_else(|| bbox_center(doc));
    let cw = cell.max(1e-6);
    let pan = match view.pan {
        Some(p) if p.to == to => p, // same animation, keep going
        _ => {
            // new segment: duration decides how long it takes
            let cells = ((to.0 - cur.0) / cw).abs().max(((to.1 - cur.1) / cw).abs());
            let pan = Pan {
                from: cur,
                to,
                start: Instant::now(),
                dur: (0.16 + cells * 0.002).clamp(PAN_MIN, PAN_MAX),
            };
            view.pan = Some(pan);
            pan
        }
    };
    let t = (pan.start.elapsed().as_secs_f64() / pan.dur).clamp(0.0, 1.0);
    let e = t * t;
    view.center = Some((
        pan.from.0 + (pan.to.0 - pan.from.0) * e,
        pan.from.1 + (pan.to.1 - pan.from.1) * e,
    ));
    if t >= 1.0 {
        view.center = Some(pan.to);
        view.pan = None;
        view.target = None;
        return false;
    }
    true
}

/// Smoothly move the view to a point (cell coordinates): an animation already running
/// continues from the position currently shown
fn pan_to(view: &mut View, doc: &Document, p: (f64, f64)) {
    if view.center.is_none() {
        view.center = Some(bbox_center(doc));
    }
    view.target = Some(p);
}

/// Fit the bounding box of these points into the map area: returns (zoom, centre).
fn fit_to(map: Rect, pts: impl Iterator<Item = (f64, f64)>) -> Option<(f64, (f64, f64))> {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for p in pts {
        b = Some(match b {
            None => (p.0, p.1, p.0, p.1),
            Some((x0, y0, x1, y1)) => (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1)),
        });
    }
    let (x0, y0, x1, y1) = b?;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let cols = (map.width as f64 - 4.0).max(8.0);
    let rows = (map.height as f64 - 2.0).max(4.0);
    let want = (bw / cols).max(bh / rows);
    Some((
        (1.0 / want).clamp(ZOOM_MIN, ZOOM_MAX),
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
    ))
}

#[cfg(test)]
// tests tweak a few defaults; clearer than a long struct literal
#[allow(clippy::field_reassign_with_default)]
mod tests;
