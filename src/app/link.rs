//! Link-related implementations. The function bodies were moved here verbatim from app.rs.

use super::*;

impl App {
    /// Copy a picture fragment: the clipboard takes only "the selected boxes + their internal
    /// edges + explicitly selected edges".
    /// Returns (fragment, number of edges that vanish because their boxes are removed).
    pub(super) fn take_for_clipboard(
        &self,
        set: &TargetSet,
        include_edges: bool,
    ) -> (Clipboard, usize) {
        let mut boxes = Vec::new();
        let mut texts = Vec::new();
        let mut edges = Vec::new();
        let mut groups: Vec<crate::document::Group> = Vec::new();
        let mut origin: Option<(i32, i32)> = None;
        let mut note = |c: (i32, i32)| {
            origin = Some(match origin {
                Some(o) => (o.0.min(c.0), o.1.min(c.1)),
                None => c,
            });
        };
        let removed: std::collections::HashSet<&str> = set
            .boxes
            .iter()
            .map(|&i| self.doc.boxes[i].id.as_str())
            .collect();
        for &i in &set.boxes {
            let b = self.doc.boxes[i].clone();
            note(b.at);
            boxes.push(b);
            if let Some(t) = self.doc.text(i) {
                note(t.at);
                texts.push(t.clone());
            }
        }
        // Boundary edges are not part of the fragment (the other end stays in the document),
        // but they vanish with the boxes on delete
        let mut removed_edges = 0usize;
        let mut seen = std::collections::HashSet::new();
        for (k, e) in self.doc.edges.iter().enumerate() {
            let touches = removed.contains(e.from.as_str()) || removed.contains(e.to.as_str());
            if touches {
                removed_edges += 1;
            }
            let inside = removed.contains(e.from.as_str()) && removed.contains(e.to.as_str());
            let take = set.edges.contains(&k) || inside;
            if !take || !seen.insert(k) {
                continue;
            }
            edges.push(e.clone());
            // Edge endpoints and via points do not contribute to the origin: the other end may
            // stay in the document, possibly far away, and using it as the base would make the
            // paste position follow the edges, unlike a fragment without edges
            if let Some(g) = self.doc.groups.iter().find(|g| g.id == e.group) {
                if !groups.iter().any(|x| x.id == g.id) {
                    groups.push(g.clone());
                }
            }
        }
        let edges = if include_edges { edges } else { Vec::new() };
        let groups = if include_edges { groups } else { Vec::new() };
        // Layers used by the fragment: copied in document order, and paste creates new layers
        // from them
        let mut layers: Vec<crate::document::Layer> = Vec::new();
        for name in boxes
            .iter()
            .map(|b| b.layer.clone())
            .chain(edges.iter().map(|e| e.layer.clone()))
        {
            if let Some(l) = self.doc.layers.iter().find(|l| l.name == name) {
                if !layers.iter().any(|x| x.name == l.name) {
                    layers.push(l.clone());
                }
            }
        }
        (
            Clipboard {
                origin: origin.unwrap_or(self.view.cursor),
                boxes,
                texts,
                edges,
                groups,
                layers,
            },
            removed_edges,
        )
    }

    /// Size of the clipboard content (the bounding box around its boxes, at least 1)
    pub(super) fn clip_size(&self, clip: &Clipboard) -> (i32, i32) {
        let mut x1 = clip.origin.0 + 1;
        let mut y1 = clip.origin.1 + 1;
        for b in &clip.boxes {
            x1 = x1.max(b.at.0 + b.size.0 as i32);
            y1 = y1.max(b.at.1 + b.size.1 as i32);
        }
        ((x1 - clip.origin.0).max(1), (y1 - clip.origin.1).max(1))
    }

    /// Would pasting at this offset collide with boxes already in the document?
    pub(super) fn paste_overlaps(&self, clip: &Clipboard, d: (i32, i32)) -> bool {
        clip.boxes.iter().any(|b| {
            let (w, h) = (b.size.0 as i32, b.size.1 as i32);
            let (x, y) = (b.at.0 + d.0, b.at.1 + d.1);
            self.doc.boxes.iter().any(|o| {
                let (ow, oh) = (o.size.0 as i32, o.size.1 as i32);
                x < o.at.0 + ow && o.at.0 < x + w && y < o.at.1 + oh && o.at.1 < y + h
            })
        })
    }

    /// Paste position: try the exact cursor position first, then step right -> down -> down-right
    /// on collision, up to 32 candidates; if all are taken, fall back to the exact position (the
    /// echo says so)
    pub(super) fn paste_offset(&self, clip: &Clipboard) -> (i32, i32, bool) {
        let base = (
            self.view.cursor.0 - clip.origin.0,
            self.view.cursor.1 - clip.origin.1,
        );
        if clip.boxes.is_empty() {
            return (base.0, base.1, false);
        }
        let (w, h) = self.clip_size(clip);
        for k in 0..32i32 {
            let (ox, oy) = if k == 0 {
                (0, 0)
            } else {
                let n = (k + 2) / 3;
                match k % 3 {
                    0 => (n * (w + 1), 0),
                    1 => (0, n * (h + 1)),
                    _ => (n * (w + 1), n * (h + 1)),
                }
            };
            let d = (base.0 + ox, base.1 + oy);
            if !self.paste_overlaps(clip, d) {
                return (d.0, d.1, false);
            }
        }
        (base.0, base.1, true)
    }

    /// Where a new box goes: the cursor cell; on collision it shifts right one cell at a time
    /// until the spot is free
    pub(super) fn free_spot(&self, label: &str) -> (i32, i32) {
        let w = (crate::text::plain_width(label).max(2) + 2).max(4) as i32;
        let mut at = self.view.cursor;
        loop {
            let hit = self.doc.boxes.iter().any(|b| {
                let (bw, bh) = (b.size.0 as i32, b.size.1 as i32);
                at.0 < b.at.0 + bw && b.at.0 < at.0 + w && at.1 < b.at.1 + bh && b.at.1 < at.1 + 3
            });
            if !hit {
                return at;
            }
            at.0 += 1;
        }
    }

    /// Which group a new edge joins: the cursor node's existing group, else the first group
    pub(super) fn link_group(&self, i: usize) -> &str {
        self.doc
            .box_groups(i)
            .first()
            .map(|&g| self.doc.groups[g].id.as_str())
            .or_else(|| self.doc.groups.first().map(|g| g.id.as_str()))
            .unwrap_or("g0")
    }

    /// Find a box by id or text; text matching several boxes does not count (write the id)
    pub(super) fn resolve_box(&self, target: &str) -> Option<usize> {
        let t = target.trim();
        if let Some(i) = self.doc.box_idx(t) {
            return Some(i);
        }
        let hits: Vec<usize> = (0..self.doc.boxes.len())
            .filter(|&i| label_of(&self.doc, i) == t)
            .collect();
        match hits.len() {
            1 => hits.first().copied(),
            _ => None,
        }
    }

    pub(super) fn targets(&self) -> TargetSet {
        let p = render::panels(&self.doc, self.area, &self.view);
        let mut set = TargetSet::default();
        if self.view.sel.is_some() {
            let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, self.usable_hit());
            set = TargetSet { boxes, edges };
        } else {
            // Without a selection, use whatever is under the cursor
            match self.view.pick {
                render::Pick::Box(i) => set.boxes.push(i),
                render::Pick::Edge(e) => set.edges.push(e),
                render::Pick::None => {}
            }
        }
        // When a single layer is shown, verbs only act on that layer too
        if let Some(layer) = &self.view.layer {
            set.boxes.retain(|i| &self.doc.boxes[*i].layer == layer);
            set.edges.retain(|e| &self.doc.edges[*e].layer == layer);
        }
        set
    }

    /// Box ids the link sources and targets currently map to (for rebinding across structural
    /// changes)
    pub(super) fn link_ids(&self) -> (Vec<String>, Vec<String>) {
        let Some(link) = &self.view.link else {
            return (Vec::new(), Vec::new());
        };
        let ids = |v: &[usize]| -> Vec<String> {
            v.iter()
                .filter_map(|i| self.doc.boxes.get(*i).map(|b| b.id.clone()))
                .collect()
        };
        (ids(&link.sources), ids(&link.targets))
    }

    /// The document structure changed (delete / undo / redo): rebind the link sources and targets
    /// by id; when no source is left, leave link mode
    pub(super) fn rebind_link(&mut self, ids: (Vec<String>, Vec<String>)) {
        let doc = &self.doc;
        let Some(link) = self.view.link.as_mut() else {
            return;
        };
        link.sources = ids.0.iter().filter_map(|id| doc.box_idx(id)).collect();
        link.targets = ids.1.iter().filter_map(|id| doc.box_idx(id)).collect();
        link.confirming = false;
        if link.sources.is_empty() {
            self.view.link = None;
            let note = self.view.lang.pick_owned(
                "源框已经被删掉,连边取消".to_string(),
                "the source boxes are gone: linking cancelled".to_string(),
            );
            self.view.note = Some(note);
        }
    }

    /// e, first stage: fix the source boxes and enter link mode
    pub(super) fn begin_link(&mut self) {
        // Confirm an unconfirmed source selection first (the selection already is the source)
        if matches!(self.view.sel, Some(sel) if !sel.confirmed) {
            self.confirm_selection();
        }
        let set = self.targets();
        let mut sources = set.boxes;
        sources.sort_unstable();
        sources.dedup();
        if sources.is_empty() {
            let note = self.view.lang.pick_owned(
                "先在框上开始(光标停在框上,或用 v 框住几个)".to_string(),
                "start on a box (put the cursor on one, or v to select several)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let ignored = set.edges.len();
        self.require_edit_scale();
        // Hand the selection over to "pick targets"
        self.view.sel = None;
        self.view.link = Some(LinkState {
            sources: sources.clone(),
            targets: Vec::new(),
            confirming: false,
        });
        // The status line reports source and target counts live; only mention ignored edges here
        if ignored > 0 {
            let note = self.view.lang.pick_owned(
                format!("选区里的 {ignored} 条边不参与连边"),
                format!("{} selected edge(s) ignored by linking", ignored),
            );
            self.view.note = Some(note);
        }
    }

    /// e, second stage: fix the targets and create the edges once the count is confirmed
    pub(super) fn link_commit(&mut self) {
        let Some(link) = self.view.link.clone() else {
            return;
        };
        // Confirm an unconfirmed target selection first
        if matches!(self.view.sel, Some(sel) if !sel.confirmed) {
            self.confirm_selection();
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let mut targets = render::link_targets(&self.doc, &p, &self.view);
        targets.retain(|t| *t < self.doc.boxes.len());
        if targets.is_empty() {
            let note = self.view.lang.pick_owned(
                "还没有目标:光标移到目标框上,或用 v 框住再 Enter".to_string(),
                "no target yet: move onto a target box, or v to select some".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let total = link.sources.len() * targets.len();
        if total > 512 {
            let note = self.view.lang.pick_owned(
                format!("一次最多建 512 条(现在 {total} 条),把选区缩小一点"),
                format!("at most 512 edges at a time ({total} now): narrow the selection"),
            );
            self.view.note = Some(note);
            return;
        }
        if total > 64 && !link.confirming {
            if let Some(l) = self.view.link.as_mut() {
                l.confirming = true;
                l.targets = targets;
            }
            let note = self.view.lang.pick_owned(
                format!("再按 Enter 创建 {total} 条"),
                format!("press Enter again to create {total} edges"),
            );
            self.view.note = Some(note);
            return;
        }
        self.create_edges(&link.sources, &targets, total);
    }

    /// Batch edge creation: one Forward edge per source x target, the whole batch is one undo
    /// step
    pub(super) fn create_edges(&mut self, sources: &[usize], targets: &[usize], total: usize) {
        let _ = total;
        self.require_edit_scale();
        // With exactly one focused group use it, otherwise each source uses its first group
        let one = match self.view.focus.as_slice() {
            [gr] if *gr < self.doc.groups.len() => Some(self.doc.groups[*gr].id.clone()),
            _ => None,
        };
        // An edge was selected recently: new edges copy its style and group (focus wins when a
        // group is focused)
        let look = self.edge_look_now();
        let mut made = 0usize;
        let mut dup = 0usize;
        let mut last = None;
        self.begin_edit("连边");
        for &s in sources {
            if s >= self.doc.boxes.len() {
                continue;
            }
            let group = one
                .clone()
                .or_else(|| look.as_ref().map(|l| l.group.clone()))
                .unwrap_or_else(|| self.link_group(s).to_string());
            let from = self.doc.boxes[s].id.clone();
            for &t in targets {
                if t >= self.doc.boxes.len() {
                    continue;
                }
                // Source equals target: a self loop, created as usual
                let to = self.doc.boxes[t].id.clone();
                match self.doc.add_edge(&from, &to, &group, Dir::Forward) {
                    Ok(e) => {
                        if let Some(l) = &look {
                            self.apply_edge_look(e, l);
                        }
                        made += 1;
                        last = Some(t);
                    }
                    Err(_) => dup += 1,
                }
            }
        }
        self.end_edit();
        self.view.link = None;
        self.view.sel = None;
        if let Some(t) = last {
            self.jump_to(t);
        }
        self.refresh_pick();
        let mut zh = format!("建了 {made} 条边");
        let mut en = format!("created {made} edge(s)");
        if dup > 0 {
            zh.push_str(&format!(",重复 {dup}"));
            en.push_str(&format!(", {dup} already existed"));
        }
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// Esc: leave link mode without touching the document
    pub(super) fn cancel_link(&mut self) {
        self.view.link = None;
        self.view.sel = None;
        let note = self
            .view
            .lang
            .pick_owned("取消连边".to_string(), "linking cancelled".to_string());
        self.view.note = Some(note);
    }

    /// Prompt-line cancel: delete the edge label boxes just created and still empty
    pub(super) fn drop_note_draft(&mut self) {
        let ids = std::mem::take(&mut self.note_draft);
        if ids.is_empty() {
            return;
        }
        self.begin_edit("撤销注释");
        let mut gone = 0;
        for id in ids {
            let Some(i) = self.doc.box_idx(&id) else {
                continue;
            };
            let empty = self.doc.text(i).map(|t| t.text.is_empty()).unwrap_or(true);
            if empty {
                let _ = self.doc.remove_box(&id);
                gone += 1;
            }
        }
        self.end_edit();
        if gone > 0 {
            self.refresh_pick();
        }
    }

    /// Enter: pin the selection (moving the cursor no longer changes it)
    pub(super) fn confirm_selection(&mut self) {
        let Some(sel) = self.view.sel else {
            return;
        };
        if sel.confirmed {
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, &self.hit);
        if boxes.is_empty() && edges.is_empty() {
            let note = self
                .view
                .lang
                .pick_owned("选区为空".to_string(), "the selection is empty".to_string());
            self.view.note = Some(note);
            return;
        }
        if let Some(s) = self.view.sel.as_mut() {
            s.end = self.view.cursor;
            s.confirmed = true;
        }
        let note = self.view.lang.pick_owned(
            format!("已确认:{} 框 {} 边", boxes.len(), edges.len()),
            format!(
                "confirmed: {} box(es), {} edge(s)",
                boxes.len(),
                edges.len()
            ),
        );
        self.view.note = Some(note);
    }

    /// While a selection is unconfirmed, block every command that mutates the document
    pub(super) fn blocked_by_selection(&mut self) -> bool {
        if let Some(sel) = self.view.sel {
            if !sel.confirmed {
                let note = self.view.lang.pick_owned(
                    "先按 Enter 确认选区".to_string(),
                    "press Enter to confirm the selection first".to_string(),
                );
                self.view.note = Some(note);
                return true;
            }
        }
        false
    }

    /// Ctrl-hjkl: move the selected boxes (the cursor box when there is no selection) by one
    /// cell
    pub(super) fn move_boxes(&mut self, dir: (i32, i32), steps: i32) {
        let steps = steps.max(1);
        if self.blocked_by_selection() {
            return;
        }
        self.require_edit_scale();
        let ids = self.targets().boxes;
        if ids.is_empty() {
            let note = self
                .view
                .lang
                .pick_owned("没有可移动的框".to_string(), "no box to move".to_string());
            self.view.note = Some(note);
            return;
        }
        let (dx, dy) = (dir.0 * steps, dir.1 * steps);
        self.begin_edit("挪框");
        let n = ids.len();
        for i in ids {
            self.doc.move_box(i, dx, dy);
        }
        // The boxes moved, so the cursor and the selection move with them: otherwise they slip
        // apart after a few presses, the cursor staying put while the boxes walk away
        self.view.cursor = (self.view.cursor.0 + dx, self.view.cursor.1 + dy);
        if let Some(sel) = self.view.sel.as_mut() {
            sel.anchor = (sel.anchor.0 + dx, sel.anchor.1 + dy);
            sel.end = (sel.end.0 + dx, sel.end.1 + dy);
        }
        self.refresh_pick();
        // When the cursor leaves the visible margin the view follows (same as the arrow keys)
        self.follow_cursor();
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
        self.view.note = Some(self.view.lang.pick_owned(
            format!("挪了 {steps} 格{zh}"),
            format!("moved {steps} cell(s){en}"),
        ));
        self.end_edit();
    }

    /// Say one line after a layer change: the empty name is the default layer
    pub(super) fn note_layer(&mut self) {
        let text = match self.view.layer.clone() {
            None => self
                .view
                .lang
                .pick_owned("图层 = 全部".to_string(), "layer = all".to_string()),
            Some(l) => {
                let name = if l.is_empty() {
                    self.view.lang.pick("默认层", "default layer").to_string()
                } else {
                    l
                };
                self.view
                    .lang
                    .pick_owned(format!("图层 = {name}"), format!("layer = {name}"))
            }
        };
        self.view.note = Some(text);
    }

    /// L: all -> each layer -> all (layers come from doc.layer_names(), default layer first)
    pub(super) fn cycle_layer(&mut self) {
        let layers = self.doc.layer_names();
        let next = match self.view.layer.clone() {
            None => layers.first().cloned(),
            Some(cur) => {
                let at = layers.iter().position(|l| *l == cur);
                match at {
                    Some(i) if i + 1 < layers.len() => Some(layers[i + 1].clone()),
                    _ => None,
                }
            }
        };
        self.view.layer = next;
        self.note_layer();
    }

    /// :tolayer: move the selected boxes (with their text) and the edges whose both ends are
    /// inside the selection onto one layer
    pub(super) fn move_to_layer(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            let note = self.view.lang.pick_owned(
                "tolayer 缺参数:要移到哪一层".to_string(),
                "tolayer needs a layer name".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        if !self.doc.layers.iter().any(|l| l.name == name) {
            let order = self.doc.layers.iter().map(|l| l.order).max().unwrap_or(0) + 1;
            self.doc.layers.push(Layer {
                name: name.to_string(),
                order,
            });
        }
        let ids = self.targets().boxes;
        let mut inside = vec![false; self.doc.boxes.len()];
        for &i in &ids {
            if i < inside.len() {
                inside[i] = true;
            }
        }
        for &i in &ids {
            if i < self.doc.boxes.len() {
                self.doc.boxes[i].layer = name.to_string();
                if let Some(ti) = self.doc.text_of(i) {
                    self.doc.texts[ti].layer = name.to_string();
                }
            }
        }
        let mut edges = 0usize;
        let index: std::collections::HashMap<String, usize> = self
            .doc
            .boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.clone(), i))
            .collect();
        for e in self.doc.edges.iter_mut() {
            let both = index
                .get(&e.from)
                .and_then(|i| inside.get(*i))
                .copied()
                .unwrap_or(false)
                && index
                    .get(&e.to)
                    .and_then(|i| inside.get(*i))
                    .copied()
                    .unwrap_or(false);
            if both {
                e.layer = name.to_string();
                edges += 1;
            }
        }
        let n = ids.len();
        self.view.note = Some(self.view.lang.pick_owned(
            format!("{n} 个框,{edges} 条边移到图层 {name}"),
            format!("moved {n} box(es) and {edges} edge(s) to layer {name}"),
        ));
    }

    /// E: light only the cursor node's neighbours and incident edges and dim the rest; press E or
    /// Esc again to exit
    pub(super) fn toggle_ego(&mut self) {
        // On / off. On means isolate: only the cursor node's neighbours and incident edges stay,
        // everything else really disappears. A node change (cursor move / click on a visible
        // neighbour) follows along
        if self.view.ego.is_some() {
            self.view.ego = None;
            self.view.ego_id = None;
            let note = self
                .view
                .lang
                .pick_owned("退出隔离".to_string(), "isolate off".to_string());
            self.view.note = Some(note);
            self.refresh_pick();
            return;
        }
        let Some(n) = render::box_at(&self.doc, self.view.cursor, &self.view) else {
            let note = self.view.lang.pick_owned(
                "光标下没有节点".to_string(),
                "no node under the cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        // Edge label box: use the source end of the edge it is bound to as the centre
        let n = match self.doc.edge_of_box(n) {
            Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
            None => n,
        };
        self.view.ego = Some(n);
        self.view.ego_id = Some(self.doc.boxes[n].id.clone());
        // Hidden elements must not stay operable: clear the selection, dragging and linking, and
        // close the menu
        self.view.sel = None;
        self.view.link = None;
        self.view.menu = None;
        let edges = self.doc.adj(n).len();
        let neighbours = self.doc.adj(n).len();
        let notes = self
            .doc
            .adj(n)
            .iter()
            .filter(|e| self.doc.edge_label_of(&self.doc.edges[**e].id).is_some())
            .count();
        let name = label_of(&self.doc, n).to_string();
        let note = self.view.lang.pick_owned(
            format!("隔离:{name}({neighbours} 邻居 / {edges} 边 / {notes} 边标签),E 或 Esc 退出"),
            format!(
                "isolate: {name} ({neighbours} neighbours / {edges} edges / {notes} edge labels), E or Esc to exit"
            ),
        );
        self.view.note = Some(note);
        self.refresh_pick();
    }

    pub(super) fn cycle_edge(&mut self) {
        self.sync_hit();
        // When boxes overlap completely, cycle the topmost ones first (E also reaches the covered
        // ones). Boxes hidden by isolation do not take part in overlap cycling
        let ego = render::ego_visible(&self.doc, &self.view);
        let stack: Vec<usize> = self
            .doc
            .boxes_at_cell(self.view.cursor)
            .into_iter()
            .filter(|i| !render::ego_hides(ego.as_ref(), *i))
            .collect();
        if stack.len() > 1 {
            // Cycle from the currently selected one: a document change clears cand, so it cannot
            // remember the position
            let pos = stack
                .iter()
                .position(|&i| self.view.pick == render::Pick::Box(i));
            let next = pos.map(|p| (p + 1) % stack.len()).unwrap_or(0);
            self.view.cand = next;
            let i = stack[next];
            self.view.pick = render::Pick::Box(i);
            let name = label_of(&self.doc, i).to_string();
            self.view.note = Some(self.view.lang.pick_owned(
                format!("框 {name}(重叠 {}/{})", self.view.cand + 1, stack.len()),
                format!("box {name} (stack {}/{})", self.view.cand + 1, stack.len()),
            ));
            return;
        }
        // The cursor is on a node: cycle only among that node's incident edges, not lines that
        // merely pass through
        if let Some(c) = render::box_at(&self.doc, self.view.cursor, &self.view) {
            self.cycle_at_node(c);
            return;
        }
        // The cursor is not on a node (on a line or empty space): cycle the edge candidates on
        // this cell. The hit table records **screen** cells while the cursor holds canvas cells,
        // so project first; otherwise selection breaks as soon as the view shifts
        let p = render::panels(&self.doc, self.area, &self.view);
        let screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let cands: Vec<usize> = self
            .usable_hit()
            .candidates_at(screen)
            .iter()
            .copied()
            .filter(|e| self.ego_lit_edge(*e))
            .collect();
        if cands.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可选的边".to_string(),
                "nothing to select here".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.view.cand = (self.view.cand + 1) % cands.len();
        let e = cands[self.view.cand];
        self.view.pick = render::Pick::Edge(e);
        self.note_edge(e);
    }

    /// The "lit edges" in E mode: those incident to the centre. With E off everything counts.
    pub(super) fn ego_lit_edge(&self, e: usize) -> bool {
        match self.view.ego {
            Some(a) => self.doc.adj(a).contains(&e),
            None => true,
        }
    }

    /// Cycle among node c's incident edges (E): document order, wrapping at the end
    pub(super) fn cycle_at_node(&mut self, c: usize) {
        let incident: Vec<usize> = self.doc.adj(c).to_vec();
        let Some(&first) = incident.first() else {
            self.view.pick = render::Pick::None;
            let note = self.view.lang.pick_owned(
                "这个框没有相连的边".to_string(),
                "this box has no edges".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        let next = match self.view.pick.edge() {
            Some(cur) => incident.iter().copied().find(|&e| e > cur).unwrap_or(first),
            None => first,
        };
        self.view.pick = render::Pick::Edge(next);
        self.note_edge(next);
    }

    /// Say one line after selecting an edge
    pub(super) fn note_edge(&mut self, e: usize) {
        let (a, b) = match self.doc.edge_ends(e) {
            Some((a, b)) => (label_of(&self.doc, a), label_of(&self.doc, b)),
            None => (String::new(), String::new()),
        };
        let note = self
            .doc
            .edge_label_of(&self.doc.edges[e].id)
            .and_then(|b| self.doc.text(b).map(|t| t.text.clone()))
            .unwrap_or_default();
        self.view.note = Some(self.view.lang.pick_owned(
            if note.is_empty() {
                format!("选边 {a} - {b} (#{e}) -> 无注释")
            } else {
                format!("选边 {a} - {b} (#{e}) -> 边标签: {note}")
            },
            if note.is_empty() {
                format!("edge {a} - {b} (#{e}) -> no edge label")
            } else {
                format!("edge {a} - {b} (#{e}) -> edge label: {note}")
            },
        ));
    }

    /// Link target under the cursor (canvas cell -> target)
    pub(super) fn link_at_cursor(&self) -> Option<String> {
        self.hit.links.get(&self.view.cursor).cloned()
    }

    /// Enter / gf: confirm an unconfirmed selection first (old behaviour), otherwise open the
    /// link under the cursor
    pub(super) fn open_link(&mut self) {
        if self.view.sel.is_some_and(|s| !s.confirmed) {
            self.confirm_selection();
            return;
        }
        let Some(target) = self.link_at_cursor() else {
            let note = self.view.lang.pick_owned(
                "光标下没有链接".to_string(),
                "no link under cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        self.request_open(&target);
    }

    /// From (x, y) find the first free spot to the right that fits `size`; after one row, take
    /// the next row
    pub(super) fn next_free_spot(
        &self,
        x: i32,
        y: i32,
        size: (i32, i32),
        skip: usize,
    ) -> (i32, i32) {
        let mut at = (x, y);
        let mut guard = 0;
        while guard < 5000 {
            let hit = self.doc.boxes.iter().enumerate().any(|(i, b)| {
                if i == skip {
                    return false;
                }
                let (bw, bh) = (b.size.0 as i32, b.size.1 as i32);
                at.0 < b.at.0 + bw
                    && b.at.0 < at.0 + size.0
                    && at.1 < b.at.1 + bh
                    && b.at.1 < at.1 + size.1
            });
            if !hit {
                return at;
            }
            at.0 += 1;
            if at.0 > x + 120 {
                at = (x, at.1 + 1);
            }
            guard += 1;
        }
        at
    }
}
