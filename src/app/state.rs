//! App state: construction, view synchronisation, layout and per-frame drawing.

use super::*;

impl App {
    pub fn new(doc: Document, opts: Options) -> Result<App, String> {
        let mut app = App {
            doc,
            view: View::default(),
            source: None,
            area: Rect::default(),
            hit: render::FrameHit::default(),
            first_frame: true,
            quit: false,
            undo: History::new(200),
            previewing: false,
            clipboard: None,
            pending: None,
            jumps: Vec::new(),
            jump_at: 0,
            seq: String::new(),
            open_request: None,
            opener: opts.opener.clone(),
            links: opts.links.clone(),
            menu_memory: std::collections::HashMap::new(),
            last_click: None,
            note_draft: Vec::new(),
            edit_request: None,
            drag: None,
            search_from: (0, 0),
            search_focus: Vec::new(),
            src_stamp: None,
            rev: 0,
            src_rev: 0,
            reload_armed: None,
            tabs: vec![None],
            tab_at: 0,
            keys: opts.keys.clone(),
            tip_at: 0,
            mouse: opts.mouse,
            mouse_edges: opts.mouse_edges,
            spacing: opts.spacing,
            glyphs: opts.glyphs,
            cwd: std::env::current_dir()
                .map(|p| crate::paths::tildify(&p.to_string_lossy()))
                .unwrap_or_default(),
            box_look: None,
            edge_look: None,
        };
        app.view_from_doc();
        app.view.glyphs = app.glyphs;
        // Default ink from the config: used by edges/text without a colour
        app.view.edge_ink = opts.edge_ink;
        app.view.text_ink = opts.text_ink;
        // Open at 1:1 centred on the whole picture; stored zoom only applies to export.
        app.reset_view();
        // An explicit view from the CLI beats the view state stored in the file.
        // `layout` only affects display and the `r` cycle: opening never recomputes.
        if let Some(l) = opts.layout {
            app.view.layout = l;
        }
        if opts.node_labels {
            app.view.node_labels = true;
        }
        app.view.edge_labels = opts.edge_labels.apply(app.view.edge_labels);
        app.view.drawn = opts.drawn;
        if let Some(n) = opts.history {
            app.undo = History::new(n);
        }
        // An explicit `--lang` / `[prefs] lang` wins over the file, English included.
        if let Some(lang) = opts.lang {
            app.view.lang = lang;
        }
        // --at: look up by name/alias; if missing, say so on the prompt line and keep going
        if let Some(name) = &opts.at {
            match find_box(&app.doc, name) {
                Some(i) => app.jump_to(i),
                None => {
                    let note = app.view.lang.pick_owned(
                        format!("没有叫 {name} 的框"),
                        format!("no box named {name}"),
                    );
                    app.view.note = Some(note);
                }
            }
        }
        // --find: same as typing / then Enter; hits stay so `n` can jump between them
        if let Some(text) = &opts.find {
            if !text.trim().is_empty() {
                app.view.query = Some(text.clone());
                refresh(&app.doc, &mut app.view);
                app.view.query = None;
                if app.view.hits.is_empty() {
                    let note = app
                        .view
                        .lang
                        .pick_owned(format!("没搜到 {text}"), format!("nothing matched {text}"));
                    app.view.note = Some(note);
                }
            }
        }
        app.search_from = app.view.cursor;
        if !app.doc.boxes.is_empty() {
            app.note_here();
        }
        Ok(app)
    }

    /// Reset the view after opening a file: exactly like pressing `0` (1:1, centred).
    /// Document zoom/centre drive `--export -` and export, but not the interactive open.
    pub fn reset_view(&mut self) {
        self.view.zoom = 1.0;
        self.view.center = None;
        self.view.target = None;
        self.view.pan = None;
    }

    /// Map the document's view state into View and clear transient, non-document state.
    pub(super) fn view_from_doc(&mut self) {
        let z = self.doc.view.zoom;
        self.view.zoom = if z.is_finite() && z > 0.0 {
            clamp_zoom(z)
        } else {
            1.0
        };
        let c = self.doc.view.center;
        self.view.center = if c.0.is_finite() && c.1.is_finite() {
            Some(c)
        } else {
            None
        };
        let layout =
            Layout::parse(&self.doc.view.layout).unwrap_or_else(|| crate::layout::pick(&self.doc));
        self.view.layout = layout;
        self.view.node_labels = self.doc.view.node_labels;
        self.view.edge_labels = self.doc.view.edge_labels;
        if let Some(l) = Lang::parse(&self.doc.view.lang) {
            self.view.lang = l;
        }
        self.view.layer = self.doc.view.layer.clone();
        self.view.side = if self.doc.view.side.is_finite() {
            self.doc.view.side
        } else {
            2.0
        };
        self.view.cycle = self.doc.view.cycle.filter(|c| c.is_finite());
        self.view.orientation = layout::Orientation::parse(&self.doc.view.orientation)
            .unwrap_or(layout::Orientation::Auto);
        self.view.angle = self.view.orientation.degrees();
        let focus: Vec<usize> = self
            .doc
            .view
            .focus
            .iter()
            .filter_map(|id| self.doc.group_idx(id))
            .collect();
        self.view.focus = focus;
        // The document stores a cursor cell (v1 stored a box id; both become cells here).
        self.view.cursor = self.doc.view.cursor_cell(&self.doc);
        self.refresh_pick();
        // Opening a document does not auto-fit: zoom is document state (a new document
        // is 1:1) and scaling cells never changes glyphs; fitting only squashes the
        // graph into illegible blocks. Press F to fit. Everything below is transient.
        self.view.query = None;
        self.view.hits.clear();
        self.view.hit_at = 0;
        self.view.sel = None;
        self.view.cmd = None;
        self.view.help = false;
        self.view.target = None;
        self.view.pan = None;
        self.view.pulse = 0.0;
        self.view.started = Instant::now();
        self.pending = None;
        self.clear_seq();
        self.drag = None;
        self.search_from = self.view.cursor;
        self.search_focus = self.view.focus.clone();
    }

    /// Recompute what lies under the cursor cell; candidate edges are picked by index.
    pub fn refresh_pick(&mut self) {
        self.sync_hit();
        // The isolation centre follows the cursor: moving onto another **visible** node
        // moves it (hidden boxes count as blank); on an edge label, highlight the edge's
        if self.view.ego.is_some() {
            if let Some(n) = render::box_at(&self.doc, self.view.cursor, &self.view) {
                let n = match self.doc.edge_of_box(n) {
                    Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
                    None => n,
                };
                if self.view.ego != Some(n) {
                    self.view.ego = Some(n);
                    self.view.ego_id = Some(self.doc.boxes[n].id.clone());
                }
            }
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        self.view.pick =
            render::pick_at(&self.doc, &self.hit, self.view.cursor, screen, &self.view);
        if let render::Pick::Edge(_) = self.view.pick {
            // Candidates are keyed by screen cell; when several edges overlap here,
            // `cand` cycles them. Isolate mode only counts edges attached to the
            // highlighted centre, so its own edges are seen first when a group overlaps.
            let cands: Vec<usize> = self
                .hit
                .candidates_at(screen)
                .iter()
                .copied()
                .filter(|e| self.ego_lit_edge(*e))
                .collect();
            if cands.is_empty() {
                self.view.pick = render::Pick::None;
            } else if cands.len() > 1 {
                let e = cands[self.view.cand % cands.len()];
                self.view.pick = render::Pick::Edge(e);
            }
        }
        self.remember_look();
    }

    /// The hit table is only valid for the document revision that produced it (it stores
    /// array indices); any document change invalidates it, so never pick edges by old index.
    pub(super) fn sync_hit(&mut self) {
        if self.hit.rev != self.doc.rev() {
            self.hit = render::FrameHit::default();
            self.view.cand = 0;
        }
    }

    /// The hit table usable for the current revision (absent when it does not match).
    pub(super) fn usable_hit(&self) -> &render::FrameHit {
        static EMPTY: std::sync::OnceLock<render::FrameHit> = std::sync::OnceLock::new();
        if self.hit.rev == self.doc.rev() {
            &self.hit
        } else {
            EMPTY.get_or_init(render::FrameHit::default)
        }
    }

    /// Whether any animation is running (otherwise fewer frames need drawing).
    pub fn busy(&self) -> bool {
        // Selected elements keep the pulse animation going; so do pan tweens.
        self.view.target.is_some()
            || self.view.pan.is_some()
            || self.view.pick != render::Pick::None
            || self.view.sel.is_some()
    }

    /// Advance the animations (pan tween, cursor pulse). Returns whether this frame may
    /// differ from the last: the pulse is quantised and a tween must really move, else
    /// dozens of full-screen diffs per second all write the same bytes to the terminal.
    pub fn tick(&mut self) -> bool {
        let before = self.view.pulse;
        let phase = self.view.started.elapsed().as_secs_f64() / BEAT;
        let q = (pulse_at(phase) * PULSE_STEPS).round() / PULSE_STEPS;
        self.view.pulse = q;
        let (center, zoom) = (self.view.center, self.view.zoom);
        let cell = render::panels(&self.doc, self.area, &self.view).proj.scale;
        animate(&mut self.view, &self.doc, cell);
        (q - before).abs() > f64::EPSILON || self.view.center != center || self.view.zoom != zoom
    }

    /// Draw one frame (also recording the picture area, needed for mouse mapping).
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        self.area = area;
        // The status line shows [n/m] and the dirty marker
        self.view.tab = (self.tabs.len() > 1).then_some((self.tab_at + 1, self.tabs.len()));
        self.view.tab_name = self.tab_name();
        self.view.tab_dirty = self.dirty();
        // Top row: file path and working directory, `~` abbreviated
        self.view.file_display = self
            .source
            .as_deref()
            .map(|p| crate::paths::tildify(&crate::paths::absolute_display(p)));
        self.view.cwd = self.cwd.clone();
        // Indices drift after delete/undo: re-anchor the centre by id, else leave isolate
        if let Some(n) = self.view.ego {
            let id = self.view.ego_id.clone().unwrap_or_default();
            let same = n < self.doc.boxes.len() && self.doc.boxes[n].id == id;
            if !same {
                match self.doc.box_idx(&id) {
                    Some(k) => self.view.ego = Some(k),
                    None => {
                        self.view.ego = None;
                        self.view.ego_id = None;
                    }
                }
            }
        }
        // The selection holds nothing (e.g. its boxes were deleted): cancel it,
        // exactly like clicking outside the selection; never leave an empty selection.
        if self.view.sel.is_some_and(|s| s.confirmed) {
            let p = render::panels(&self.doc, area, &self.view);
            let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, &self.hit);
            if boxes.is_empty() && edges.is_empty() {
                self.view.sel = None;
            }
        }
        // On open the cursor may be off-screen: centre it once on the first frame
        if self.first_frame {
            self.first_frame = false;
            if self.cursor_cell().is_none() && !self.doc.boxes.is_empty() {
                self.view.center = Some((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
                self.view.target = None;
            }
        }
        // Lend the previous frame's hit table; move it out and back to avoid a deep copy
        let prev = std::mem::take(&mut self.hit);
        let (_, hit) = render::render_hit(buf, area, &self.doc, &self.view, &prev);
        // Warn about labels that did not fit; the previous frame's count is fresh enough.
        self.view.labels_hidden = hit.labels_skipped.len();
        self.hit = hit;
        if self.view.file_pick.is_some() {
            render::render_pick(buf, area, &self.view);
        }
    }

    /// Say a word on the echo slot after zoom changes (the status row only shows node names)
    pub(super) fn note_zoom(&mut self) {
        let pct = (self.view.zoom * 100.0).round() as i64;
        let note = self
            .view
            .lang
            .pick_owned(format!("缩放 {pct}%"), format!("zoom {pct}%"));
        self.view.note = Some(note);
    }

    /// Rotate to absolute angle deg (requires 1:1)
    pub(super) fn rotate_to(&mut self, o: layout::Orientation, deg: i32) {
        self.require_edit_scale();
        self.begin_edit("转方向");
        let k = (deg - self.view.angle).rem_euclid(360) / 90;
        if k != 0 {
            self.doc.rotate_quarters(k);
        }
        self.view.orientation = o;
        self.view.angle = deg.rem_euclid(360);
        let note = self.view.lang.pick_owned(
            format!("方向 = {}(坐标已转)", self.view.angle),
            format!("orientation = {} (coordinates rotated)", self.view.angle),
        );
        self.view.note = Some(note);
        self.end_edit();
    }

    /// Reply when the user wants to quit: only :q quits
    pub(super) fn note_quit_hint(&mut self) {
        self.view.note = Some(
            self.view
                .lang
                .pick_owned("退出用 :q".to_string(), "quit with :q".to_string()),
        );
    }

    /// Write the document-owned View state back into doc.view (required before saving)
    pub(super) fn sync_view(&mut self) {
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let cursor = Some(crate::document::Cursor::Cell(self.view.cursor));
        let focus: Vec<String> = self
            .view
            .focus
            .iter()
            .filter_map(|&g| self.doc.groups.get(g).map(|x| x.id.clone()))
            .collect();
        // view.layer in a file must reference an existing layer, else it will not reload
        let layer = self
            .view
            .layer
            .clone()
            .filter(|l| self.doc.layers.iter().any(|x| &x.name == l));
        let view = ViewState {
            center,
            zoom: self.view.zoom,
            layout: self.view.layout.name().to_string(),
            cursor,
            focus,
            node_labels: self.view.node_labels,
            lang: self.view.lang.code().to_string(),
            layer,
            side: self.view.side,
            cycle: self.view.cycle,
            orientation: self.view.orientation.name().to_string(),
            edge_labels: self.view.edge_labels,
        };
        self.doc.view = view;
    }

    /// Change layout: call layout::apply to write integer cell coordinates. Works at any
    /// zoom; the view stays at 1:1.
    pub(super) fn apply_layout(&mut self, l: Layout) {
        self.begin_edit("换布局");
        match layout::apply_with(&mut self.doc, l, self.spacing) {
            Ok(report) => {
                let k = self.layout_quarters(l);
                if k != 0 {
                    self.doc.rotate_quarters(k);
                }
                self.view.angle = self.view.orientation.degrees();
                self.view.layout = l;
                self.view.pan = None;
                self.view.target = None;
                self.note_layout(l, &report);
            }
            Err(e) => self.view.note = Some(e),
        }
        self.end_edit();
    }

    /// Announce the new layout on the prompt line; append the first three warnings
    pub(super) fn note_layout(&mut self, l: Layout, report: &layout::LayoutReport) {
        // auto: report the chosen layout too, e.g. auto(tree) / auto(layered)
        let n = if l == Layout::Auto && !report.profile.is_empty() {
            format!("auto({})", report.profile)
        } else {
            l.name().to_string()
        };
        let mut note = self
            .view
            .lang
            .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
        let _ = &n;
        if !report.warnings.is_empty() {
            note.push_str("  (");
            note.push_str(&report.summary(3));
            note.push(')');
        }
        self.view.note = Some(note);
    }

    /// r: cycle to the next layout
    pub(super) fn cycle_layout(&mut self) {
        let l = self.view.layout.next();
        self.apply_layout(l);
    }
}
