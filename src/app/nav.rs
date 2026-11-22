//! Navigation: cursor movement, jumping, zooming and view fitting.

use super::*;

impl App {
    /// Auto-scroll while dragging near the viewport edge: within 2 cells of it, further
    /// out pans faster (max 3 cells). Only called while dragging, not on mouse moves.
    pub(super) fn autopan(&mut self, col: u16, row: u16) {
        let p = render::panels(&self.doc, self.area, &self.view);
        if p.map.width == 0 || p.map.height == 0 {
            return;
        }
        let (x0, y0) = (p.map.x as i32, p.map.y as i32);
        let (x1, y1) = (x0 + p.map.width as i32 - 1, y0 + p.map.height as i32 - 1);
        let (c, r) = (col as i32, row as i32);
        let edge_dist = |v: i32, lo: i32, hi: i32| -> i32 {
            if v < lo + 2 {
                (lo + 2 - v).min(3)
            } else if v > hi - 2 {
                (v - (hi - 2)).min(3)
            } else {
                0
            }
        };
        // Positive = view pushed left/up (content moves right/down)
        let dx = edge_dist(c, x0, x1);
        let dy = edge_dist(r, y0, y1);
        if dx == 0 && dy == 0 {
            return;
        }
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let proj = p.proj;
        self.view.center = Some(render::pan_center(&proj, center, dx, dy));
        self.view.target = None;
        self.view.pan = None;
    }

    /// Screen position of the cursor cell (zoom anchor)
    pub(super) fn cursor_screen_cell(&self) -> render::Cell {
        let p = render::panels(&self.doc, self.area, &self.view);
        p.proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64))
    }

    /// Zoom around screen cell `anchor`: the canvas point under that cell stays put,
    /// so the pointer/cursor does not slip during the zoom.
    pub(super) fn zoom_at(&mut self, zoom: f64, anchor: render::Cell) {
        let zoom = snap_zoom(clamp_zoom(zoom));
        if (zoom - self.view.zoom).abs() < 1e-9 {
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let point = p.proj.canvas_f(anchor.0, anchor.1);
        let (cx, cy) = p.proj.center_for(point, anchor.0, anchor.1, zoom);
        self.view.zoom = zoom;
        self.view.center = Some((cx, cy));
        self.view.target = None;
        self.view.pan = None;
        self.note_zoom();
    }

    /// Whether a box sits under the cursor cell
    pub(super) fn cursor_box(&self) -> Option<usize> {
        self.view.pick.box_at()
    }

    /// Where the real terminal cursor goes: end of the input line for command/search,
    /// else the text start of the cursor box; None when outside the viewport (hide it).
    pub fn cursor_cell(&self) -> Option<(u16, u16)> {
        let area = self.area;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if self.view.help || self.view.file_pick.is_some() {
            return None;
        }
        let status_y = area.y + area.height - 1;
        if let Some((what, typed)) = &self.view.prompt {
            let prefix = match what {
                Prompt::NewBox => "框: ",
                Prompt::Text(_) => "文字: ",
                Prompt::Link(_) => "连到: ",
            };
            let shown = render::prompt_display(typed, self.view.lang);
            // Cursor sits at `prompt_pos`; folded multi-line text counts the first line only
            let first_line: String = typed
                .split('\n')
                .next()
                .unwrap_or("")
                .chars()
                .take(self.view.prompt_pos)
                .collect();
            let _ = &shown;
            let before = render::raw_width(&first_line);
            let x = area.x + render::raw_width(prefix) as u16 + before as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(c) = &self.view.cmd {
            // The status row draws the `:w hell` string itself; the cursor is one cell past it.
            // Widths must be raw: `/` in paths and `*` in text are not markup.
            let before: String = c.chars().take(self.view.cmd_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(q) = &self.view.query {
            let before: String = q.chars().take(self.view.query_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if self.doc.boxes.is_empty() {
            return None;
        }
        let p = render::panels(&self.doc, area, &self.view);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let (x, y) = (cx as u16, cy as u16);
        let inside =
            x >= p.map.x && x < p.map.x + p.map.width && y >= p.map.y && y < p.map.y + p.map.height;
        inside.then_some((x, y))
    }

    /// Quarter turns for this relayout (0 = none).
    /// auto only applies to force / layered: geo and relax inputs are orientation-aware.
    pub(super) fn layout_quarters(&self, l: Layout) -> i32 {
        if let Some(k) = self.view.orientation.quarters() {
            return k;
        }
        if !matches!(l, Layout::Force | Layout::Layered) {
            return 0;
        }
        let area = render::panels(&self.doc, self.area, &self.view).map;
        let (x0, y0, x1, y1) = self.doc.bbox();
        let (gw, gh) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
        if gw <= 0.0 || gh <= 0.0 || area.width == 0 || area.height == 0 {
            return 0;
        }
        let cols = area.width as f64;
        let rows = area.height as f64;
        let normal = (gw / cols).max(gh / rows);
        let rotated = (gh / cols).max(gw / rows);
        // Below a 5% gain do not rotate, to avoid flip-flopping near the threshold
        if rotated < normal * 0.95 {
            1
        } else {
            0
        }
    }

    /// `w` / `b`: shoot a straight line from the cursor to the right / left and jump
    /// to the first **other** box or edge it hits, then scroll the view there.
    ///
    /// The ray runs to the picture's horizontal edge, not the visible area, so
    /// off-screen targets still work; `goto_cell` scrolls them into view.
    ///
    /// When the row holds nothing (common in table-like layouts) fall back to the
    /// nearest box in the front / back half: front = right or below, back = left or
    /// above, with the distance measured to the **box edge**. Ties prefer boxes on
    /// the same row/column.
    pub(super) fn jump_ray(&mut self, back: bool) {
        let (bx0, _, bx1, _) = self.doc.bbox();
        let p = render::panels(&self.doc, self.area, &self.view);
        let start_box = render::box_at(&self.doc, self.view.cursor, &self.view);
        let start_screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let start_edge = self.hit.edge_at(start_screen);
        // The ray runs to the picture's left/right edge, not the viewport
        let steps = if back {
            self.view.cursor.0 - bx0 + 1
        } else {
            bx1 - self.view.cursor.0 + 1
        }
        .max(0);
        let dx = if back { -1 } else { 1 };
        // The hit table covers the visible picture only (edges are measured from
        // strokes); outside it, only boxes count.
        let (mx0, my0) = (p.map.x as i32, p.map.y as i32);
        let (mx1, my1) = (
            (p.map.x + p.map.width) as i32,
            (p.map.y + p.map.height) as i32,
        );
        let mut cell = self.view.cursor;
        for _ in 0..steps {
            cell = (cell.0 + dx, cell.1);
            // Check boxes first (whatever a box covers does not count), then edges
            if let Some(b) = render::box_at(&self.doc, cell, &self.view) {
                if Some(b) != start_box {
                    self.goto_cell(cell);
                    return;
                }
                continue;
            }
            let screen = p.proj.cell((cell.0 as f64, cell.1 as f64));
            if screen.0 < mx0 || screen.1 < my0 || screen.0 >= mx1 || screen.1 >= my1 {
                continue;
            }
            if let Some(e) = self.hit.edge_at(screen) {
                if Some(e) != start_edge {
                    self.goto_cell(cell);
                    return;
                }
            }
        }
        // The row is empty: nearest box in the front / back half (to box edge, same row wins)
        let ego = render::ego_visible(&self.doc, &self.view);
        let cur = self.view.cursor;
        let mut best: Option<(i64, u8, usize, render::Cell)> = None;
        for i in 0..self.doc.boxes.len() {
            if Some(i) == start_box || self.doc.box_hidden(i) || render::ego_hides(ego.as_ref(), i)
            {
                continue;
            }
            // Cursor and box rectangles are both canvas cells (screen coords break under zoom)
            let b = &self.doc.boxes[i];
            let (x0, y0) = (b.at.0, b.at.1);
            let (w, h) = (b.size.0 as i32, b.size.1 as i32);
            let (x1, y1) = (x0 + w.max(1) - 1, y0 + h.max(1) - 1);
            let gap = |a: i32, lo: i32, hi: i32| {
                if a < lo {
                    lo - a
                } else if a > hi {
                    a - hi
                } else {
                    0
                }
            };
            let (gx, gy) = (gap(cur.0, x0, x1), gap(cur.1, y0, y1));
            // front = right or below, back = left or above
            let ahead = if back { x1 < cur.0 } else { x0 > cur.0 };
            let below = if back { y1 < cur.1 } else { y0 > cur.1 };
            if !ahead && !below {
                continue;
            }
            let dist = (gx as i64) * (gx as i64) + (gy as i64) * (gy as i64);
            // Prefer the same row/column (the ray should hit it first), then distance
            let tie = u8::from(!ahead);
            let key = (dist, tie, i, self.cell_of(i));
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
        match best {
            Some((_, _, _, c)) => self.goto_cell(c),
            None => {
                let note = if back {
                    self.view.lang.pick_owned(
                        "这个方向后面没有框了".to_string(),
                        "no box behind the cursor".to_string(),
                    )
                } else {
                    self.view.lang.pick_owned(
                        "这个方向前面没有框了".to_string(),
                        "no box ahead of the cursor".to_string(),
                    )
                };
                self.view.note = Some(note);
            }
        }
    }

    /// Jump to a box: move the cursor, record it in history, keep it on screen
    pub fn jump_to(&mut self, i: usize) {
        let cell = self.cell_of(i);
        self.jump_cell(i, cell);
    }

    /// Jump to a hit box: the cursor lands on the matching character, not the text start
    /// (otherwise the terminal cursor sits on the first char and looks painted too).
    pub(super) fn jump_to_hit(&mut self, i: usize) {
        let cell = self.hit_cell_of(i);
        self.jump_cell(i, cell);
    }

    fn jump_cell(&mut self, i: usize, cell: render::Cell) {
        if i >= self.doc.boxes.len() {
            return;
        }
        // Target hidden by isolate: leave isolate first or the landing point is invisible
        if render::ego_hides(render::ego_visible(&self.doc, &self.view).as_ref(), i) {
            self.view.ego = None;
        }
        if cell == self.view.cursor {
            // Cursor did not move, but the picked element may have changed (e.g. from an edge)
            self.view.pick = render::Pick::Box(i);
            return;
        }
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.note_here();
        self.follow_cursor_jump();
    }

    /// Cursor cell of a hit box: cell of the first matching character, else the text start
    fn hit_cell_of(&self, i: usize) -> render::Cell {
        let q = self
            .view
            .hit_text
            .as_deref()
            .filter(|_| self.view.hits.contains(&i));
        q.and_then(|q| self.doc.hit_cell(i, q))
            .unwrap_or_else(|| self.cell_of(i))
    }

    /// Cursor cell of a box: text start for boxed nodes, the node itself for point nodes
    pub(super) fn cell_of(&self, i: usize) -> render::Cell {
        match self.doc.text(i) {
            Some(t) => t.at,
            None => (self.doc.boxes[i].at.0 + 1, self.doc.boxes[i].at.1 + 1),
        }
    }

    /// v / Ctrl-q: start a block selection at the cursor cell
    pub(super) fn start_rect_select(&mut self) {
        self.view.sel = Some(RectSel {
            anchor: self.view.cursor,
            end: self.view.cursor,
            confirmed: false,
        });
        let (x, y) = self.view.cursor;
        let note = self.view.lang.pick_owned(
            format!("区块选择:锚点 ({x},{y})(移动光标扩大,esc 取消)"),
            format!("block select from ({x},{y}) (move to grow, esc to cancel)"),
        );
        self.view.note = Some(note);
    }

    /// Hits visible under isolate: hidden boxes do not count
    pub(super) fn visible_hits(&self) -> Vec<usize> {
        let ego = render::ego_visible(&self.doc, &self.view);
        self.view
            .hits
            .iter()
            .copied()
            .filter(|i| !render::ego_hides(ego.as_ref(), *i))
            .collect()
    }

    /// n / N: step one entry through the hit table in direction `dir`, skipping boxes
    /// hidden by isolate. Say so when nothing is visible, rather than look broken.
    pub(super) fn step_hit(&mut self, vis: &[usize], dir: isize) -> Option<usize> {
        let n = self.view.hits.len();
        if vis.is_empty() {
            if n > 0 {
                let note = self.view.lang.pick_owned(
                    "命中的框都被隔离藏起来了(E 退出后再看)".to_string(),
                    "every match is hidden by isolate (E to exit)".to_string(),
                );
                self.view.note = Some(note);
            }
            return None;
        }
        let mut idx = (self.view.hit_at as isize + dir).rem_euclid(n as isize) as usize;
        for _ in 0..n {
            let hit = self.view.hits[idx];
            if vis.contains(&hit) {
                self.view.hit_at = idx;
                return Some(hit);
            }
            idx = (idx as isize + dir).rem_euclid(n as isize) as usize;
        }
        None
    }

    /// z: centre the picture on the box under the cursor
    pub(super) fn center_cursor(&mut self) {
        let p = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, p);
    }

    /// F: fit everything; with isolate on, fit only the visible nodes
    pub(super) fn fit_all(&mut self) {
        if let Some(ego) = render::ego_visible(&self.doc, &self.view) {
            let p = render::panels(&self.doc, self.area, &self.view);
            let pts: Vec<(f64, f64)> = (0..self.doc.boxes.len())
                .filter(|&i| !render::ego_hides(Some(&ego), i))
                .map(|i| self.doc.center(i))
                .collect();
            if let Some((z, c)) = fit_to(p.map, pts.into_iter()) {
                self.view.zoom = z;
                self.view.center = Some(c);
                self.view.target = None;
                return;
            }
        }
        let zoom = render::zoom_for_fit(
            &self.doc,
            self.area.width,
            self.area.height,
            self.view.edge_labels,
        );
        self.view.zoom = clamp_zoom(zoom);
        self.view.center = None;
        self.view.target = None;
    }

    /// f: fit the focused group (or the cursor's group when nothing is focused)
    pub(super) fn fit_focus(&mut self) {
        let p = render::panels(&self.doc, self.area, &self.view);
        let groups = match self.cursor_box() {
            Some(c) if self.view.focus.is_empty() => self.doc.box_groups(c).to_vec(),
            _ => self.view.focus.clone(),
        };
        let ego = render::ego_visible(&self.doc, &self.view);
        let pts: Vec<(f64, f64)> = (0..self.doc.boxes.len())
            .filter(|&i| {
                self.doc.box_groups(i).iter().any(|g| groups.contains(g))
                    && !render::ego_hides(ego.as_ref(), i)
            })
            .map(|i| self.doc.center(i))
            .collect();
        if let Some((z, c)) = fit_to(p.map, pts.into_iter()) {
            self.view.zoom = z;
            self.view.center = Some(c);
            self.view.target = None;
        }
    }

    /// Keyboard cursor movement: at the margin, pan the view one chunk (not per row)
    /// and leave a few cells of slack, without dragging the cursor to the centre.
    pub(super) fn follow_cursor(&mut self) {
        // Before the first frame there is no viewport, so visibility cannot be judged
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // While a tween runs, use its target: else each key press starts outside the margin
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        // Two cells of margin: pan as soon as the cursor touches the border, not after
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // Like nvim: once at the margin the cursor stays put on screen and only its
        // logical position advances while the picture scrolls cell by cell, so only the
        // overshoot is corrected. Land immediately, no tween: a tween would let the
        // cursor step once and then be pulled back, which reads as a jitter.
        let zoom = self.view.zoom.max(ZOOM_MIN);
        let cur = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let mut want = cur;
        if cx < x0 {
            want.0 = cur.0 + (cx - x0) as f64 / zoom;
        } else if cx > x1 {
            want.0 = cur.0 + (cx - x1) as f64 / zoom;
        }
        if cy < y0 {
            want.1 = cur.1 + (cy - y0) as f64 / zoom;
        } else if cy > y1 {
            want.1 = cur.1 + (cy - y1) as f64 / zoom;
        }
        self.view.center = Some(want);
        self.view.target = None;
        self.view.pan = None;
    }

    /// Sync the sequence to the bottom-right corner (showcmd)
    pub(super) fn show_seq(&mut self) {
        self.view.showcmd = self.seq.clone();
    }

    /// Drop the current sequence
    pub(super) fn clear_seq(&mut self) {
        if self.seq.is_empty() && self.view.showcmd.is_empty() {
            return;
        }
        self.seq.clear();
        self.pending = None;
        self.show_seq();
    }

    /// Eat keys that are not settled yet: digits / comma / g / G. true = consumed.
    pub(super) fn seq_key(&mut self, c: char) -> bool {
        if c.is_ascii_digit() {
            // A leading 0 is not swallowed: 0 means "back to 1:1"
            if c == '0' && self.seq.is_empty() {
                return false;
            }
            self.seq.push(c);
            if self.seq.contains(',') {
                self.pending = None; // the second half of a cell is not a count
            } else {
                count_digit(&mut self.pending, c);
            }
            self.show_seq();
            return true;
        }
        if c == ',' {
            if self.seq.is_empty() || !self.seq.chars().all(|x| x.is_ascii_digit()) {
                return false;
            }
            self.seq.push(c);
            self.pending = None;
            self.show_seq();
            return true;
        }
        if c == 'g' {
            if self.seq.is_empty() {
                self.seq.push('g');
                self.show_seq();
                return true;
            }
            if self.seq == "g" {
                self.seq.clear();
                self.pending = None;
                self.show_seq();
                self.goto_cell((0, 0)); // gg: back to the origin
                return true;
            }
            self.clear_seq();
            return false;
        }
        if c == 'f' && self.seq == "g" {
            // gf: open the link under the cursor (not the "fit" f)
            self.clear_seq();
            self.open_link();
            return true;
        }
        if c == 'G' {
            if self.seq.is_empty() {
                return false;
            }
            let cell = parse_goto(&self.seq, self.view.cursor);
            self.clear_seq();
            if let Some(cell) = cell {
                self.goto_cell(cell);
            }
            return true;
        }
        // Any other key: drop the sequence and handle the key normally
        if !self.seq.is_empty() {
            self.seq.clear();
            self.show_seq();
        }
        false
    }

    /// Jump to a canvas cell (gg / a,bG): record it in the jump list and scroll there
    pub(super) fn goto_cell(&mut self, cell: (i32, i32)) {
        if self.view.cursor == cell {
            return;
        }
        self.note_here();
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor_jump();
    }

    /// Record "here" in the jump history (Ctrl-o / Ctrl-i). It stores tab + cell,
    /// so a jump made in another buffer still comes back.
    pub(super) fn note_here(&mut self) {
        let cur = Jump {
            tab: self.tab_at,
            cell: self.view.cursor,
        };
        if self.jumps.get(self.jump_at) == Some(&cur) {
            return;
        }
        if self.jumps.last() == Some(&cur) {
            self.jump_at = self.jumps.len() - 1;
            return;
        }
        self.jumps.truncate(self.jump_at + 1); // jumping from mid-history drops the tail
        self.jumps.push(cur);
        if self.jumps.len() > 64 {
            self.jumps.remove(0);
        }
        self.jump_at = self.jumps.len() - 1;
    }

    /// Step to the previous / next jump point (may switch tabs). false = at the end
    pub(super) fn jump_history(&mut self, back: bool) -> bool {
        if self.jumps.is_empty() {
            return false;
        }
        if back {
            // Standing somewhere unrecorded (we walked here): record it first and
            // then step back, so Ctrl-o returns to the last edit/search spot.
            let here = Jump {
                tab: self.tab_at,
                cell: self.view.cursor,
            };
            if self.jumps.get(self.jump_at) != Some(&here) {
                self.note_here();
            }
            if self.jump_at == 0 {
                return false;
            }
            self.jump_at -= 1;
        } else {
            if self.jump_at + 1 >= self.jumps.len() {
                return false;
            }
            self.jump_at += 1;
        }
        let j = self.jumps[self.jump_at];
        if j.tab != self.tab_at {
            // Cross-buffer: switch that tab back first, then place the cursor
            self.switch_tab(j.tab);
        }
        self.view.cursor = j.cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.view.note = None;
        self.follow_cursor_jump();
        true
    }

    /// Arrive from elsewhere (search / click / jump history): bring the cursor into view
    pub(super) fn follow_cursor_jump(&mut self) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // While a tween runs, use its target, else the cursor looks off-screen each time
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // Off-screen jump: centre the cursor directly. A 0.85 approach only suits
        // small overshoots; going from 1000,1000 to 10,10 it stops halfway and looks stuck.
        let want = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, want);
    }

    /// Half-page jump: move half a screen towards `dir` and land on the nearest box
    pub(super) fn leap(&self, dir: (i32, i32)) -> usize {
        let p = render::panels(&self.doc, self.area, &self.view);
        let c = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let dx = dir.0 * (p.map.width as i32 / 2).max(1);
        let dy = dir.1 * (p.map.height as i32 / 2).max(1);
        let (ax, ay) = (p.map.x as i32, p.map.y as i32);
        let x_max = ax + p.map.width as i32 - 1;
        let y_max = ay + p.map.height as i32 - 1;
        let x = (c.0 + dx).clamp(ax, x_max.max(ax));
        let y = (c.1 + dy).clamp(ay, y_max.max(ay));
        render::node_at(&self.doc, &p, self.area, x as u16, y as u16)
            .or_else(|| {
                // No box there: pick the nearest box
                (0..self.doc.boxes.len()).min_by_key(|&i| {
                    let (bx, by) = p.proj.cell(self.doc.center(i));
                    (bx - x).abs() + (by - y).abs()
                })
            })
            .unwrap_or(0)
    }

    /// Move the cursor: one cell per step, the numeric prefix is the cell count
    pub(super) fn move_cursor(&mut self, dir: (i32, i32), steps: u32) {
        let steps = steps.max(1) as i32;
        self.view.cursor = (
            self.view.cursor.0 + dir.0 * steps,
            self.view.cursor.1 + dir.1 * steps,
        );
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor();
    }
}
