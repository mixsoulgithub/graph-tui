//! Text editing: inline box editing and the prompt line.

use super::*;

/// How many cells a character takes in the terminal (wide glyphs and emoji take
/// two).
fn ch_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c)
        .unwrap_or(1)
        .max(1)
}

impl App {
    /// Starts a session: picks a tip from the time and process id (called from the
    /// interactive entry only; dump and tests keep it None).
    pub fn seed_tip(&mut self) {
        self.tip_at = crate::tips::pick(crate::tips::seed());
        self.show_tip();
    }

    /// Moves to the next tip (advanced when `?` is open); the status line follows.
    pub fn next_tip(&mut self) {
        let n = crate::tips::all().len();
        if n == 0 {
            return;
        }
        self.tip_at = (self.tip_at + 1) % n;
        self.show_tip();
    }

    fn show_tip(&mut self) {
        let tips = crate::tips::all();
        if let Some(t) = tips.get(self.tip_at) {
            self.view.tip = Some(t.text(self.view.lang).to_string());
        }
    }

    /// Takes an undo snapshot before changing the document.
    pub fn begin_edit(&mut self, label: &str) {
        if self.previewing {
            return; // previews do not push undo
        }
        // Every edit site is a jump point too (like a search or a mouse click): after
        // typing, creating a box or changing a style, Ctrl-o brings you back.
        self.note_here();
        self.undo.begin(label, &self.doc);
    }

    /// Drops the snapshot when nothing changed (called at the end of cmd_apply and of
    /// the top-level key path).
    pub fn end_edit(&mut self) {
        if self.previewing {
            return;
        }
        if self.undo.prune(&self.doc) {
            self.rev += 1;
        }
    }

    /// Edits a batch of edges: one begin/end pair, a single undo step.
    pub(super) fn edit_edges(
        &mut self,
        mut f: impl FnMut(&mut crate::document::Document, usize),
        zh: String,
        en: String,
    ) {
        if self.blocked_by_selection() {
            return;
        }
        let es = self.targets().edges;
        if es.is_empty() {
            self.note_no_edge();
            return;
        }
        self.require_edit_scale();
        self.begin_edit("改边");
        for e in es {
            if e < self.doc.edges.len() {
                f(&mut self.doc, e);
            }
        }
        self.end_edit();
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// Enters in-place editing: the draft starts from the box's current text and the
    /// caret sits on the character under the canvas cursor (at the end when the cursor
    /// is outside the text).
    pub(super) fn begin_box_edit(&mut self, i: usize, created: bool) {
        if i >= self.doc.boxes.len() {
            return;
        }
        if self.doc.boxes[i].style.is_point() {
            // A point node has a single name; in-place editing makes no sense, so fall
            // back to the prompt.
            self.open_prompt(Prompt::Text(vec![i]), self.common_text(&[i]));
            return;
        }
        let text = self.doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
        let caret = self
            .caret_from_cursor(i)
            .unwrap_or_else(|| text.chars().count());
        self.view.edit = Some(BoxEdit {
            box_idx: i,
            created,
            text,
            caret,
        });
        self.note_edit_help();
        self.sync_edit_cursor();
    }

    /// Character index inside the box that the canvas cursor is on (None when outside
    /// the text area).
    pub(super) fn caret_from_cursor(&self, i: usize) -> Option<usize> {
        let b = self.doc.boxes.get(i)?;
        let t = self.doc.text(i)?;
        let origin = if b.style.is_boxed() {
            (b.at.0 + 1, b.at.1 + 1)
        } else {
            t.at
        };
        let (col, row) = (self.view.cursor.0 - origin.0, self.view.cursor.1 - origin.1);
        if col < 0 || row < 0 {
            return None;
        }
        let lines: Vec<&str> = t.text.split('\n').collect();
        let row = row as usize;
        if row >= lines.len() {
            return None;
        }
        // Screen column -> character index: wide glyphs take two cells and the pointer
        // lands on a cell, so walk by width (the right half of a wide glyph counts as
        // that glyph, as in nvim).
        let col = col as usize;
        let (mut idx, mut cx) = (0usize, 0usize);
        for (i, ch) in lines[row].chars().enumerate() {
            let w = ch_width(ch);
            if col < cx + w {
                idx = i;
                break;
            }
            cx += w;
            idx = i + 1;
        }
        let mut at = 0usize;
        for line in lines.iter().take(row) {
            at += line.chars().count() + 1;
        }
        Some(at + idx)
    }

    pub(super) fn note_edit_help(&mut self) {
        let note = self.view.lang.pick_owned(
            "改字:Enter 换行,Esc 提交,Ctrl-C 取消".to_string(),
            "editing: Enter newline, Esc commits, Ctrl-C cancels".to_string(),
        );
        self.view.note = Some(note);
    }

    /// Byte offset of character `idx` (past the end gives the end).
    pub(super) fn byte_of(s: &str, idx: usize) -> usize {
        s.char_indices()
            .nth(idx)
            .map(|(b, _)| b)
            .unwrap_or_else(|| s.len())
    }

    pub(super) fn line_start(s: &str, caret: usize) -> usize {
        let chars: Vec<char> = s.chars().take(caret).collect();
        chars.iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1)
    }

    pub(super) fn line_end(s: &str, caret: usize) -> usize {
        let mut idx = 0usize;
        for (i, c) in s.chars().enumerate() {
            if i < caret {
                continue;
            }
            if c == '\n' {
                return i;
            }
            idx = i + 1;
        }
        idx.max(caret.min(s.chars().count()))
    }

    /// Moves one line up or down, keeping the **cell** column where possible (as in
    /// nvim: landing mid wide glyph retreats to its head).
    pub(super) fn caret_vertical(s: &str, caret: usize, step: i32) -> usize {
        let lines: Vec<&str> = s.split('\n').collect();
        let (row, want) = Self::caret_row_col(s, caret);
        let target = (row as i32 + step).clamp(0, lines.len() as i32 - 1) as usize;
        let line = lines[target];
        // The last character boundary in the target line that starts within `want`.
        let (mut best, mut col) = (0usize, 0usize);
        for (i, c) in line.chars().enumerate() {
            let w = ch_width(c);
            if col + w > want {
                break;
            }
            col += w;
            best = i + 1;
        }
        let at: usize = lines[..target].iter().map(|l| l.chars().count() + 1).sum();
        at + best
    }

    /// During editing the canvas cursor follows the draft caret (the status line shows
    /// x,y).
    pub(super) fn sync_edit_cursor(&mut self) {
        let Some(edit) = self.view.edit.clone() else {
            return;
        };
        let Some(t) = self.doc.text(edit.box_idx) else {
            return;
        };
        let bare = !self.doc.boxes[edit.box_idx].style.is_boxed();
        let origin = if bare {
            (t.at.0, t.at.1)
        } else {
            (
                self.doc.boxes[edit.box_idx].at.0 + 1,
                self.doc.boxes[edit.box_idx].at.1 + 1,
            )
        };
        let (row, col) = Self::caret_row_col(&edit.text, edit.caret);
        self.view.cursor = (origin.0 + col as i32, origin.1 + row as i32);
        self.refresh_pick();
    }

    /// Row and **cell** column of the caret (not char index): wide glyphs take two
    /// cells, matching the columns seen in the terminal.
    pub(super) fn caret_row_col(s: &str, caret: usize) -> (usize, usize) {
        let (mut row, mut col) = (0usize, 0usize);
        for (i, c) in s.chars().enumerate() {
            if i == caret {
                break;
            }
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += ch_width(c);
            }
        }
        (row, col)
    }

    /// Leaves editing: commit (one undo step) or drop the draft.
    pub(super) fn commit_box_edit(&mut self, cancel: bool) {
        let Some(edit) = self.view.edit.take() else {
            return;
        };
        if cancel {
            if edit.created {
                // The box was created by this `i`: undo the whole step so no empty box is
                // left behind.
                self.undo_step();
            }
            let note = self
                .view
                .lang
                .pick_owned("改字已取消".to_string(), "edit cancelled".to_string());
            self.view.note = Some(note);
            return;
        }
        let old = self
            .doc
            .text(edit.box_idx)
            .map(|t| t.text.clone())
            .unwrap_or_default();
        if old == edit.text {
            // A freshly created empty box with nothing typed: remove it instead of
            // leaving an invisible empty node.
            if edit.created && edit.text.is_empty() {
                self.undo_step();
            }
            let note = self
                .view
                .lang
                .pick_owned("文字没变".to_string(), "text unchanged".to_string());
            self.view.note = Some(note);
            return;
        }
        // Same path as `:text`: `<|new box|>` in a single plain box splits it into
        // several boxes.
        let text = edit.text.clone();
        let ids = vec![edit.box_idx];
        self.cmd_set_text(&ids, &text);
        if self.view.note.is_none() {
            let note = self
                .view
                .lang
                .pick_owned("文字已更新".to_string(), "text updated".to_string());
            self.view.note = Some(note);
        }
    }

    /// Ctrl+G: edit the text of the selected or cursor box in `$EDITOR` and write it
    /// back as soon as the editor exits. No prompt line appears, so there is no
    /// "show the text, then press Enter" step.
    pub(super) fn edit_text_external(&mut self) {
        let ids = self.targets().boxes;
        if ids.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可编辑的框".to_string(),
                "no box under the cursor to edit".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        if self.blocked_by_selection() {
            return;
        }
        let text = self.common_text(&ids);
        self.edit_request = Some((EditTarget::Boxes(ids), text));
    }

    /// Prompt line submit: Enter and "returned from the external editor" take the same
    /// path.
    pub(super) fn submit_prompt(&mut self, what: Prompt, text: String) {
        match what {
            Prompt::NewBox => self.cmd_new(&text),
            Prompt::Text(ids) => self.cmd_set_text(&ids, &text),
            Prompt::Link(_) => self.cmd_link(&text),
        }
        // An edge label with no text yet: drop it instead of leaving an invisible empty
        // box.
        self.drop_note_draft();
    }

    /// The text of this batch (used for prefill when they all match, otherwise empty).
    pub(super) fn common_text(&self, ids: &[usize]) -> String {
        let mut it = ids
            .iter()
            .filter_map(|&i| self.doc.text(i).map(|t| t.text.clone()));
        let first = it.next().unwrap_or_default();
        if it.all(|t| t == first) {
            first
        } else {
            String::new()
        }
    }

    /// Opens the prompt line with the caret at the end.
    pub(super) fn open_prompt(&mut self, what: Prompt, text: String) {
        self.view.prompt_pos = text.chars().count();
        self.view.prompt = Some((what, text));
    }

    /// Splits on the marker unless it is escaped. `\<|new box|>` is literal: no split
    /// and the backslash stays, so the next edit still sees it as escaped.
    pub(super) fn split_segments(text: &str) -> Vec<String> {
        let mark = Self::SPLIT_MARK;
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut i = 0usize;
        while i < text.len() {
            if text[i..].starts_with(mark) {
                out.push(std::mem::take(&mut cur));
                i += mark.len();
                continue;
            }
            if text[i..].starts_with('\\') && text[i + 1..].starts_with(mark) {
                cur.push('\\');
                cur.push_str(mark);
                i += 1 + mark.len();
                continue;
            }
            let ch = text[i..].chars().next().expect("字符边界");
            cur.push(ch);
            i += ch.len_utf8();
        }
        out.push(cur);
        out
    }

    /// Whether the text contains an unescaped separator marker.
    pub(super) fn has_unescaped_mark(text: &str) -> bool {
        let mark = Self::SPLIT_MARK;
        let mut i = 0usize;
        while i < text.len() {
            if text[i..].starts_with(mark) {
                return true;
            }
            if text[i..].starts_with('\\') && text[i + 1..].starts_with(mark) {
                i += 1 + mark.len();
                continue;
            }
            i += text[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        }
        false
    }

    /// Splits one box into several on `<|new box|>`: the first segment stays in the
    /// original box and each following segment gets a new box to the right.
    /// Returns true when this submit was handled as a split (the caller must not run
    /// the ordinary text update).
    pub(super) fn split_text(&mut self, i: usize, text: &str) -> bool {
        if !text.contains(Self::SPLIT_MARK) && !text.contains(&format!("\\{}", Self::SPLIT_MARK)) {
            return false;
        }
        let has_unescaped = Self::has_unescaped_mark(text);
        let segs: Vec<String> = Self::split_segments(text)
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        // Only a single plain box can split: edge labels, point nodes and
        // multi-selections are left alone.
        let ok = i < self.doc.boxes.len()
            && self.doc.boxes[i].edge.is_none()
            && !self.doc.boxes[i].style.is_point();
        if !ok {
            // A box that cannot split: the marker is written as ordinary text.
            return false;
        }
        if !has_unescaped {
            // Only escaped forms: write the text back unchanged.
            return false;
        }
        // The box already contains an unescaped marker: that is document content, not a
        // split request. Splitting again would turn every edit into "split once more"
        // (or write nothing), so in-place editing could never change anything.
        if self
            .doc
            .text(i)
            .is_some_and(|t| Self::has_unescaped_mark(&t.text))
        {
            return false;
        }
        if segs.len() < 2 {
            // Nothing to split, so the marker is written as ordinary text. Returning true
            // here would swallow the whole draft (with an unescaped marker already in the
            // box, `i` could never change anything).
            let note = self.view.lang.pick_owned(
                "没有可拆的内容(nothing to split),标记按文字写入".to_string(),
                "nothing to split; the marker stays as text".to_string(),
            );
            self.view.note = Some(note);
            return false;
        }
        let (style, color, fill, layer) = {
            let b = &self.doc.boxes[i];
            (b.style, b.color, b.fill, b.layer.clone())
        };
        let (tcolor, tbg, align) = self
            .doc
            .text(i)
            .map(|t| (t.color, t.bg, t.align))
            .unwrap_or((None, None, crate::model::Align::Left));
        self.begin_edit("拆分文字");
        self.doc.set_text(i, segs[0].clone());
        let mut prev = i;
        for seg in segs.iter().skip(1) {
            let (px, py, pw) = {
                let b = &self.doc.boxes[prev];
                (b.at.0, b.at.1, b.size.0 as i32)
            };
            let Ok(n) = self.doc.add_box(seg, (px + pw + 1, py), style) else {
                continue;
            };
            {
                let b = &mut self.doc.boxes[n];
                b.color = color;
                b.fill = fill;
                b.layer = layer.clone();
            }
            if let Some(ti) = self.doc.text_of(n) {
                self.doc.texts[ti].color = tcolor;
                self.doc.texts[ti].bg = tbg;
                self.doc.texts[ti].align = align;
            }
            self.doc.set_text(n, seg.clone());
            let size = {
                let b = &self.doc.boxes[n];
                (b.size.0 as i32, b.size.1 as i32)
            };
            let at = self.next_free_spot(px + pw + 1, py, size, n);
            self.doc.place_box(n, at);
            prev = n;
        }
        self.doc.rebuild();
        self.refresh_pick();
        self.end_edit();
        let n = segs.len();
        let note = self
            .view
            .lang
            .pick_owned(format!("拆成 {n} 个框"), format!("split into {n} boxes"));
        self.view.note = Some(note);
        true
    }
    /// `:relabel [all]`: re-anchors edge labels next to their edge (a one-shot run, not
    /// live). Without an argument only the selected edge labels move.
    pub(super) fn cmd_relabel(&mut self, rest: &str) {
        let all = rest.trim().eq_ignore_ascii_case("all");
        let picked: Vec<usize> = if all {
            (0..self.doc.boxes.len())
                .filter(|&i| self.doc.is_edge_label(i))
                .collect()
        } else {
            self.targets()
                .boxes
                .into_iter()
                .filter(|&i| self.doc.is_edge_label(i))
                .collect()
        };
        if picked.is_empty() {
            let note = self.view.lang.pick_owned(
                "没有注释框可贴(选中注释框,或 :relabel all)".to_string(),
                "no edge label to move (select one, or :relabel all)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.begin_edit("重贴注释");
        let n = picked.len();
        for i in picked {
            let Some(e) = self.doc.edge_of_box(i) else {
                continue;
            };
            let at = self.doc.edge_label_spot(e);
            self.doc.place_box(i, at);
        }
        self.doc.rebuild();
        self.end_edit();
        self.refresh_pick();
        self.view.note = Some(self.view.lang.pick_owned(
            format!("注释已重贴({n} 个)"),
            format!("moved {n} edge label(s)"),
        ));
    }

    /// Prompt line Enter: sets the text of a batch of boxes (the batch is one undo step).
    pub(super) fn cmd_set_text(&mut self, ids: &[usize], text: &str) {
        if ids.is_empty() {
            return;
        }
        // A single plain box with a split marker: go through the split (a
        // multi-selection does not split).
        let note0 = self.view.note.clone();
        if ids.len() == 1 && self.split_text(ids[0], text) {
            return;
        }
        // The split step may have left a note (e.g. "nothing to split; the marker stays
        // as text"): restore it verbatim after the write.
        let split_note = (self.view.note != note0)
            .then(|| self.view.note.clone())
            .flatten();
        self.require_edit_scale();
        self.begin_edit("文字");
        for &i in ids {
            if i < self.doc.boxes.len() {
                self.doc.set_text(i, text.to_string());
            }
        }
        self.refresh_pick();
        self.end_edit();
        let name = label_of(&self.doc, ids[0]);
        let n = ids.len();
        let zh = if n > 1 {
            format!("({n} 个框)")
        } else {
            String::new()
        };
        let en = if n > 1 {
            format!(" ({n} boxes)")
        } else {
            String::new()
        };
        self.view.note = match split_note {
            Some(n) => Some(n),
            None => Some(
                self.view
                    .lang
                    .pick_owned(format!("文字 = {name}{zh}"), format!("text = {name}{en}")),
            ),
        };
    }

    /// `i`: edit a box in place (no prompt line); an edge or empty space follows the old
    /// path.
    pub(super) fn cmd_edit_inline(&mut self) {
        // Edit only the box under the cursor: a multi-selection or block selection must
        // not become "write the same text into every box".
        if self.view.sel.is_some() {
            let note = self.view.lang.pick_owned(
                "选中了一个区块:请只把光标放在一个框上再按 i".to_string(),
                "a block is selected: put the cursor on one box to edit".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        if let Some(i) = render::box_at(&self.doc, self.view.cursor, &self.view) {
            self.begin_box_edit(i, false);
            return;
        }
        if matches!(self.view.pick, render::Pick::Edge(_)) {
            // The cursor is on an edge: still "create an edge label box and open the
            // prompt".
            self.cmd_edit_or_new();
            return;
        }
        // Empty space: create an empty box, then edit it directly (creating it is one
        // step, committing the text another).
        self.require_edit_scale();
        self.begin_edit("建框");
        let (x, y) = self.free_spot("");
        let look = self.box_look_now();
        let style = look.map(|l| l.style).unwrap_or_default();
        let Ok(i) = self.doc.add_box("", (x, y), style) else {
            self.end_edit();
            return;
        };
        if let Some(l) = look {
            self.doc.boxes[i].color = l.color;
            self.doc.boxes[i].fill = l.fill;
            if let Some(ti) = self.doc.text_of(i) {
                self.doc.texts[ti].color = l.text_color;
                self.doc.texts[ti].bg = l.text_bg;
                self.doc.texts[ti].align = l.align;
            }
            self.doc.rebuild();
        }
        self.view.cursor = self.cell_of(i);
        self.view.sel = None;
        self.view.cand = 0;
        self.refresh_pick();
        self.end_edit();
        self.begin_box_edit(i, true);
    }

    pub(super) fn cmd_edit_or_new(&mut self) {
        let set = self.targets();
        if !set.boxes.is_empty() {
            if self.blocked_by_selection() {
                return;
            }
            // Prefill the existing text so it can be edited without retyping.
            let text = self.common_text(&set.boxes);
            self.open_prompt(Prompt::Text(set.boxes), text);
            return;
        }
        if !set.edges.is_empty() {
            // Selected edge: create (or open) the edge label box for it.
            self.require_edit_scale();
            self.begin_edit("注释");
            let mut boxes = Vec::new();
            let mut fresh: Vec<String> = Vec::new();
            for &e in &set.edges {
                let had = self.doc.edge_label_of(&self.doc.edges[e].id).is_some();
                if let Some(b) = self.doc.new_edge_label(e) {
                    boxes.push(b);
                    if !had {
                        fresh.push(self.doc.boxes[b].id.clone());
                    }
                }
            }
            self.end_edit();
            self.refresh_pick();
            if boxes.is_empty() {
                return;
            }
            // Remember the new empty box so Esc or an empty submit removes it.
            self.note_draft = fresh;
            let first = boxes[0];
            let text = self
                .doc
                .text(first)
                .map(|t| t.text.clone())
                .unwrap_or_default();
            self.open_prompt(Prompt::Text(boxes), text);
            return;
        }
        self.open_prompt(Prompt::NewBox, String::new());
    }
}
