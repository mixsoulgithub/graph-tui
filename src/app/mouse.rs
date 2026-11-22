//! Mouse handling: wheel, clicks and drags.

use super::*;

impl App {
    /// Mouse: the wheel zooms (Ctrl pans), a left click jumps to the box under it and
    /// holding then dragging moves things.
    pub(super) fn handle_mouse(&mut self, me: MouseEvent) {
        match me.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => self.mouse_scroll(me),
            MouseEventKind::Moved => self.mouse_moved(me),
            MouseEventKind::Down(MouseButton::Right) => self.mouse_right_down(me),
            MouseEventKind::Drag(MouseButton::Right) => self.mouse_right_drag(me),
            MouseEventKind::Up(MouseButton::Right) => self.mouse_right_up(me),
            MouseEventKind::Down(MouseButton::Middle) => self.mouse_middle_down(me),
            MouseEventKind::Down(MouseButton::Left) if self.view.menu.is_some() => {
                self.mouse_left_down_menu(me)
            }
            MouseEventKind::Down(MouseButton::Left) => self.mouse_left_down(me),
            MouseEventKind::Drag(MouseButton::Middle) => self.mouse_middle_drag(me),
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_left_drag(me),
            MouseEventKind::Up(MouseButton::Left) => self.mouse_left_up(me),
            _ => {}
        }
    }

    pub(super) fn mouse_scroll(&mut self, me: MouseEvent) {
        if matches!(
            me.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let up = matches!(me.kind, MouseEventKind::ScrollUp);
            let ctrl = me.modifiers.contains(KeyModifiers::CONTROL);
            let shift = me.modifiers.contains(KeyModifiers::SHIFT);
            if !ctrl && !shift {
                let k = if up { WHEEL_STEP } else { 1.0 / WHEEL_STEP };
                let at = (me.column as i32, me.row as i32);
                self.zoom_at(self.view.zoom * k, at);
            } else {
                let p = render::panels(&self.doc, self.area, &self.view);
                let from = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                let step = if up { -3 } else { 3 };
                let (dc, dr) = if shift { (step, 0) } else { (0, step) };
                self.view.center = Some(render::pan_center(&p.proj, from, dc, dr));
                self.view.target = None;
                let note = if shift {
                    self.view
                        .lang
                        .pick_owned(format!("平移 {step} 列"), format!("pan {step} columns"))
                } else {
                    self.view
                        .lang
                        .pick_owned(format!("平移 {step} 行"), format!("pan {step} rows"))
                };
                self.view.note = Some(note);
            }
        }
    }

    pub(super) fn mouse_moved(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Moved) {
            // With a menu open, hovering an item previews that item.
            if let Some(m) = &self.view.menu {
                if let Some(i) = render::menu_hit(m, self.area, me.column, me.row) {
                    if i != m.sel {
                        if let Some(m) = self.view.menu.as_mut() {
                            m.sel = i;
                        }
                        self.menu_preview();
                    }
                }
            }
        }
    }

    pub(super) fn mouse_right_down(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Down(MouseButton::Right)) {
            if self.view.menu.is_some() {
                // A menu is open: the right button only cancels, no drag starts.
                self.menu_cancel();
            } else if self.drag.is_none() {
                // Remember the right press first: on a box it means "drag out a link",
                // elsewhere "drag to pan". Both only start after two cells of drag; a
                // release without dragging opens the menu.
                let p = render::panels(&self.doc, self.area, &self.view);
                let press = (me.column, me.row);
                // `mouse_edges = false`: the right button pans even on a box and never
                // links (a small drag on a one-line text node easily links a neighbour).
                if self.mouse_edges {
                    if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                        self.drag = Some(Drag::RightLink {
                            source: i,
                            press,
                            moved: false,
                        });
                        return;
                    }
                }
                let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                self.drag = Some(Drag::RightPan {
                    press,
                    center,
                    proj: p.proj,
                    moved: false,
                });
            }
        }
    }

    pub(super) fn mouse_right_drag(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Drag(MouseButton::Right)) {
            if let Some(Drag::RightLink { press, moved, .. }) = self.drag.as_mut() {
                let dc = me.column as i32 - press.0 as i32;
                let dr = me.row as i32 - press.1 as i32;
                if !*moved && dc.abs().max(dr.abs()) < RIGHT_PAN_THRESHOLD {
                    return;
                }
                let first = !*moved;
                *moved = true;
                let source = match self.drag {
                    Some(Drag::RightLink { source, .. }) => source,
                    _ => return,
                };
                if first {
                    // Actually dragging: link from the pressed box and clear any menu or
                    // selection that would get in the way.
                    self.view.menu = None;
                    self.view.sel = None;
                    self.view.link = Some(LinkState {
                        sources: vec![source],
                        targets: Vec::new(),
                        confirming: false,
                    });
                }
                let p = render::panels(&self.doc, self.area, &self.view);
                let cell = p.proj.canvas(me.column as i32, me.row as i32);
                self.view.cursor = cell;
                let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                if let Some(link) = self.view.link.as_mut() {
                    link.targets = target.filter(|t| *t != source).into_iter().collect();
                }
                self.view.cand = 0;
                self.refresh_pick();
                self.autopan(me.column, me.row);
                return;
            }
            if let Some(Drag::RightPan {
                press,
                center,
                proj,
                moved,
            }) = self.drag.as_mut()
            {
                let dc = me.column as i32 - press.0 as i32;
                let dr = me.row as i32 - press.1 as i32;
                if !*moved && dc.abs().max(dr.abs()) < RIGHT_PAN_THRESHOLD {
                    return;
                }
                *moved = true;
                let (center, proj) = (*center, *proj);
                self.view.center = Some(render::pan_center(&proj, center, -dc, -dr));
                self.view.target = None;
                self.view.pan = None;
            }
        }
    }

    pub(super) fn mouse_right_up(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Up(MouseButton::Right)) {
            match self.drag.take() {
                Some(Drag::RightPan { press, moved, .. }) => {
                    if !moved {
                        // No drag past the threshold: open the menu at the pressed cell,
                        // not where the button came up.
                        self.open_menu(press.0, press.1);
                    }
                }
                Some(Drag::RightLink {
                    source,
                    press,
                    moved,
                }) => {
                    if !moved {
                        self.view.link = None;
                        self.open_menu(press.0, press.1);
                        return;
                    }
                    self.view.link = None;
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                    match target {
                        Some(t) => self.create_edges(&[source], &[t], 1),
                        None => {
                            let note = self.view.lang.pick_owned(
                                "松开时不在框上:没连边".to_string(),
                                "released outside a box: nothing linked".to_string(),
                            );
                            self.view.note = Some(note);
                        }
                    }
                }
                other => self.drag = other,
            }
        }
    }

    pub(super) fn mouse_middle_down(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Down(MouseButton::Middle)) {
            let p = render::panels(&self.doc, self.area, &self.view);
            let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
            self.drag = Some(Drag::Pan {
                from: (me.column, me.row),
                center,
                proj: p.proj,
            });
        }
    }

    pub(super) fn mouse_left_down_menu(&mut self, me: MouseEvent) {
        match me.kind {
            MouseEventKind::Down(MouseButton::Left) if self.view.menu.is_some() => {
                let hit = self
                    .view
                    .menu
                    .as_ref()
                    .and_then(|m| render::menu_hit(m, self.area, me.column, me.row));
                match hit {
                    Some(i) => {
                        if let Some(m) = self.view.menu.as_mut() {
                            m.sel = i;
                        }
                        self.menu_choose();
                    }
                    None => self.menu_cancel(),
                }
            }
            _ => {}
        }
    }

    pub(super) fn mouse_left_down(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Down(MouseButton::Left)) {
            let p = render::panels(&self.doc, self.area, &self.view);
            // When the press lands exactly on the endpoint cell of the selected edge,
            // this is a re-anchor drag. Capture it before refresh_pick: picking by cell
            // replaces `pick`.
            if let Some(ei) = self.view.pick.edge() {
                let press_cell = (me.column as i32, me.row as i32);
                if let Some((a, b)) = self.usable_hit().edge_ends.get(&ei).copied() {
                    if press_cell == a || press_cell == b {
                        self.drag = Some(Drag::Reconnect {
                            edge: ei,
                            to_end: press_cell == b,
                            moved: false,
                        });
                        self.view.link = Some(LinkState {
                            sources: vec![self.doc.box_idx(&self.doc.edges[ei].from).unwrap_or(0)],
                            targets: Vec::new(),
                            confirming: false,
                        });
                        let note = self.view.lang.pick_owned(
                            "拖到另一个框上改接(空白处松开取消)".to_string(),
                            "drop on another box to reconnect (empty space cancels)".to_string(),
                        );
                        self.view.note = Some(note);
                        return;
                    }
                }
            }
            // Link mode + Shift: add or remove this box in the target set (a plain click
            // just sets it as the target).
            if self.view.link.is_some() && me.modifiers.contains(KeyModifiers::SHIFT) {
                if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                    // First turn the current targets into an explicit set, then add or
                    // remove within that set.
                    let mut targets = render::link_targets(&self.doc, &p, &self.view);
                    match targets.iter().position(|t| *t == i) {
                        Some(pos) => {
                            targets.remove(pos);
                        }
                        None => {
                            targets.push(i);
                            targets.sort_unstable();
                        }
                    }
                    let n = targets.len();
                    if let Some(link) = self.view.link.as_mut() {
                        link.targets = targets;
                        link.confirming = false;
                    }
                    let note = self
                        .view
                        .lang
                        .pick_owned(format!("目标 {n} 个"), format!("{n} target(s)"));
                    self.view.note = Some(note);
                }
                return;
            }
            // A click outside the map (legend or status line) leaves the cursor alone
            // and starts no drag.
            let inside = me.column >= p.map.x
                && me.column < p.map.x + p.map.width
                && me.row >= p.map.y
                && me.row < p.map.y + p.map.height;
            if !inside {
                return;
            }
            // A click puts the cursor on the cell that was hit (no snapping to a node
            // corner).
            let cell = p.proj.canvas(me.column as i32, me.row as i32);
            // Double click = pressing `i`: create a node on empty space (prompt first),
            // edit the text on a node.
            let now = std::time::Instant::now();
            let dbl = matches!(
                self.last_click,
                Some((t, c)) if c == (me.column, me.row)
                    && now.duration_since(t) <= std::time::Duration::from_millis(400)
            );
            self.last_click = if dbl {
                None
            } else {
                Some((now, (me.column, me.row)))
            };
            // Only a hit on empty space (neither a node nor a line) counts as create; two
            // clicks on a line in the same cell cycle through overlapping edges and must
            // not be taken for a double click.
            let empty = render::node_at(&self.doc, &p, self.area, me.column, me.row).is_none()
                && self
                    .usable_hit()
                    .edge_at((me.column as i32, me.row as i32))
                    .is_none();
            if dbl && empty {
                self.view.cursor = cell;
                self.view.cand = 0;
                self.refresh_pick();
                self.cmd_edit_or_new();
                return;
            }
            // For a confirmed selection, a click outside it cancels; a click inside keeps
            // it (so it stays usable).
            if let Some(sel) = self.view.sel {
                if !rect_has(sel.anchor, sel.end, cell) {
                    self.view.sel = None;
                }
            }
            let same_cell = self.view.cursor == cell;
            self.view.cursor = cell;
            if !same_cell {
                self.view.cand = 0;
            }
            self.refresh_pick();
            self.note_here();
            if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                // Link mode: clicking a box makes it the target.
                if let Some(link) = self.view.link.as_mut() {
                    link.targets = vec![i];
                    link.confirming = false;
                    let note = self.view.lang.pick_owned(
                        "目标 1 个(Shift+点击可多选)".to_string(),
                        "1 target (Shift+click to pick more)".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                // Pressed on the border of a boxed node: dragging out creates a link;
                // only a press inside moves the box. The position is already updated, so a
                // click without a drag still only places the cursor.
                if self.doc.boxes[i].style.is_boxed() {
                    let r = render::rect_of(&self.doc, i, &p.proj);
                    let on_border = me.column as i32 == r.0
                        || me.column as i32 == r.0 + r.2 - 1
                        || me.row as i32 == r.1
                        || me.row as i32 == r.1 + r.3 - 1;
                    if on_border && self.view.sel.is_none() {
                        self.drag = Some(Drag::Link {
                            source: i,
                            moved: false,
                        });
                        self.view.link = Some(LinkState {
                            sources: vec![i],
                            targets: Vec::new(),
                            confirming: false,
                        });
                        return;
                    }
                }
                // Pressed on a box: dragging moves this batch of boxes. Only real movement
                // switches to 1:1 and pushes undo, so a click is still a pure cursor move.
                let sel = render::selected(&self.doc, &p, &self.view);
                let ids: Vec<usize> = if sel.contains(&i) { sel } else { vec![i] };
                let at = ids
                    .into_iter()
                    .filter_map(|k| self.doc.boxes.get(k).map(|b| (k, b.at)))
                    .collect();
                let sel0 = self.view.sel.map(|x| (x.anchor, x.end, x.confirmed));
                self.drag = Some(Drag::Boxes {
                    from: (me.column, me.row),
                    at,
                    proj: p.proj,
                    cursor0: self.view.cursor,
                    sel0,
                    began: false,
                });
                return;
            }
            if let Some(e) = self.usable_hit().edge_at((me.column as i32, me.row as i32)) {
                // A click on a line in empty space selects that edge (the mouse's own way
                // to select edges). When several edges share the cell, another click moves
                // to the next one.
                let cell = (me.column as i32, me.row as i32);
                let cands = self.usable_hit().candidates_at(cell).to_vec();
                if cands.len() > 1 {
                    let here = cands.iter().position(|&x| x == e).unwrap_or(0);
                    self.view.cand = if same_cell {
                        match self
                            .view
                            .pick
                            .edge()
                            .and_then(|cur| cands.iter().position(|&x| x == cur))
                        {
                            Some(pos) => (pos + 1) % cands.len(),
                            None => here,
                        }
                    } else {
                        here
                    };
                } else {
                    self.view.cand = 0;
                }
                self.view.pick = render::Pick::Edge(e);
                self.refresh_pick();
                // In E mode, if the cell only holds edges outside the highlight,
                // refresh_pick clears the pick.
                let note = match self.view.pick.edge() {
                    Some(picked) => self.view.lang.pick_owned(
                        format!("选中边 {picked}"),
                        format!("edge {picked} selected"),
                    ),
                    None => self.view.lang.pick_owned(
                        "E:只认中心相连的边".to_string(),
                        "E: only edges of the centre".to_string(),
                    ),
                };
                self.view.note = Some(note);
            }
            // Drag: Ctrl (or link mode) pans; otherwise a drag on empty space pulls out a
            // selection.
            let ctrl = me.modifiers.contains(KeyModifiers::CONTROL);
            if ctrl || self.view.link.is_some() {
                let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                self.drag = Some(Drag::Pan {
                    from: (me.column, me.row),
                    center,
                    proj: p.proj,
                });
            } else {
                self.drag = Some(Drag::Select { anchor: cell });
            }
        }
    }

    pub(super) fn mouse_middle_drag(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Drag(MouseButton::Middle)) {
            if let Some(Drag::Pan { from, center, proj }) = &self.drag {
                let dc = me.column as i32 - from.0 as i32;
                let dr = me.row as i32 - from.1 as i32;
                if dc != 0 || dr != 0 {
                    self.view.center = Some(render::pan_center(proj, *center, -dc, -dr));
                    self.view.target = None;
                    self.view.pan = None;
                }
            }
        }
    }

    pub(super) fn mouse_left_drag(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Drag(MouseButton::Left)) {
            match &self.drag {
                Some(Drag::Select { anchor }) => {
                    // The selection appears once dragging starts; the cursor follows the
                    // mouse and the grey border follows. Any previous selection (confirmed or
                    // not) is replaced by the new one.
                    let anchor = *anchor;
                    let fresh = self.view.sel.is_none();
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let end = p.proj.canvas(me.column as i32, me.row as i32);
                    self.view.sel = Some(RectSel {
                        anchor,
                        end,
                        confirmed: false,
                    });
                    if fresh {
                        let note = self.view.lang.pick_owned(
                            format!("区块选择:锚点 ({},{})", anchor.0, anchor.1),
                            format!("block select from ({},{})", anchor.0, anchor.1),
                        );
                        self.view.note = Some(note);
                    }
                    self.view.cursor = end;
                    self.view.cand = 0;
                    self.refresh_pick();
                }
                Some(Drag::Pan { from, center, proj }) => {
                    let dc = me.column as i32 - from.0 as i32;
                    let dr = me.row as i32 - from.1 as i32;
                    if dc != 0 || dr != 0 {
                        self.view.center = Some(render::pan_center(proj, *center, -dc, -dr));
                        self.view.target = None;
                        self.view.pan = None;
                    }
                }
                // Dragging a link or an endpoint: a box under the cursor becomes the
                // target (the preview follows too) and near the border the view pans one
                // cell that way.
                Some(Drag::Link { .. }) | Some(Drag::Reconnect { .. }) => {
                    match self.drag.as_mut() {
                        Some(Drag::Link { moved, .. }) | Some(Drag::Reconnect { moved, .. }) => {
                            *moved = true
                        }
                        _ => {}
                    }
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                    if let Some(link) = self.view.link.as_mut() {
                        link.targets = target.into_iter().collect();
                    }
                    if let Some(i) = target {
                        self.view.cursor = self.doc.boxes[i].at;
                        self.view.pick = render::Pick::Box(i);
                    } else {
                        // Not on a target box yet: the cursor follows the pointer so the
                        // preview has an endpoint to draw.
                        self.view.cursor = p.proj.canvas(me.column as i32, me.row as i32);
                        self.view.cand = 0;
                        self.refresh_pick();
                    }
                    self.autopan(me.column, me.row);
                }
                Some(Drag::Boxes {
                    from,
                    at,
                    proj,
                    cursor0,
                    sel0,
                    began,
                }) => {
                    // Screen cell -> canvas cell: convert with the projection captured at
                    // press time and use "original position + total delta" rather than
                    // accumulating per frame, which would build up rounding error.
                    let a = proj.canvas_f(from.0 as i32, from.1 as i32);
                    let b = proj.canvas_f(me.column as i32, me.row as i32);
                    let d = ((b.0 - a.0).round() as i32, (b.1 - a.1).round() as i32);
                    if d == (0, 0) {
                        return;
                    }
                    let (from, at, began) = (*from, at.clone(), *began);
                    let (proj, cursor0, sel0) = (*proj, *cursor0, *sel0);
                    if !began {
                        self.begin_edit("挪框");
                        self.drag = Some(Drag::Boxes {
                            from,
                            at: at.clone(),
                            proj,
                            cursor0,
                            sel0,
                            began: true,
                        });
                    }
                    for &(k, at0) in &at {
                        let cur = self.doc.boxes[k].at;
                        self.doc
                            .move_box(k, at0.0 + d.0 - cur.0, at0.1 + d.1 - cur.1);
                    }
                    // The cursor and selection take the same delta (otherwise the box looks
                    // detached).
                    self.view.cursor = (cursor0.0 + d.0, cursor0.1 + d.1);
                    if let Some((anchor, end, confirmed)) = sel0 {
                        self.view.sel = Some(RectSel {
                            anchor: (anchor.0 + d.0, anchor.1 + d.1),
                            end: (end.0 + d.0, end.1 + d.1),
                            confirmed,
                        });
                    }
                    // Hide the selection rectangle while dragging (it is drawn again on
                    // release).
                    self.view.drag_hide_sel = true;
                    self.refresh_pick();
                }
                // A right drag is in progress: left-button drag events do not concern it.
                Some(Drag::RightPan { .. }) | Some(Drag::RightLink { .. }) | None => {}
            }
        }
    }

    pub(super) fn mouse_left_up(&mut self, me: MouseEvent) {
        if matches!(me.kind, MouseEventKind::Up(MouseButton::Left)) {
            if matches!(
                self.drag,
                Some(Drag::RightPan { .. }) | Some(Drag::RightLink { .. })
            ) {
                return; // left released mid right-drag: not our business
            }
            let drag = self.drag.take();
            if let Some(Drag::Boxes { at, began, .. }) = &drag {
                let moved = *began;
                self.view.drag_hide_sel = false;
                self.refresh_pick();
                self.end_edit();
                if moved {
                    let n = at.len();
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
                    let note = self
                        .view
                        .lang
                        .pick_owned(format!("拖动完成{zh}"), format!("drag done{en}"));
                    self.view.note = Some(note);
                }
                return;
            }
            if let Some(Drag::Link { source, moved }) = drag {
                if !moved {
                    self.view.link = None;
                    return;
                }
                let target = render::node_at(
                    &self.doc,
                    &render::panels(&self.doc, self.area, &self.view),
                    self.area,
                    me.column,
                    me.row,
                );
                self.view.link = None;
                if let Some(t) = target {
                    self.create_edges(&[source], &[t], 1);
                } else {
                    let note = self.view.lang.pick_owned(
                        "松开时不在框上:没连边".to_string(),
                        "released outside a box: nothing linked".to_string(),
                    );
                    self.view.note = Some(note);
                }
                return;
            }
            if let Some(Drag::Reconnect {
                edge,
                to_end,
                moved,
            }) = drag
            {
                if !moved {
                    self.view.link = None;
                    return;
                }
                let target = render::node_at(
                    &self.doc,
                    &render::panels(&self.doc, self.area, &self.view),
                    self.area,
                    me.column,
                    me.row,
                );
                self.view.link = None;
                let Some(t) = target else {
                    let note = self.view.lang.pick_owned(
                        "松开时不在框上:连线没改".to_string(),
                        "released outside a box: edge unchanged".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                };
                let id = self.doc.boxes[t].id.clone();
                self.begin_edit("改接边");
                if let Some(e) = self.doc.edges.get_mut(edge) {
                    if to_end {
                        e.to = id.clone();
                    } else {
                        e.from = id.clone();
                    }
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("边 {edge} 改接到 {id}"),
                    format!("edge {edge} now ends at {id}"),
                );
                self.view.note = Some(note);
                return;
            }
            if let Some(Drag::Select { .. }) = drag {
                // The press itself may be swallowed by the terminal or tmux (pane switch):
                // on release, replay the click here.
                if self.view.sel.is_none() {
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let inside = me.column >= p.map.x
                        && me.column < p.map.x + p.map.width
                        && me.row >= p.map.y
                        && me.row < p.map.y + p.map.height;
                    if inside {
                        self.view.cursor = p.proj.canvas(me.column as i32, me.row as i32);
                        self.view.cand = 0;
                        self.refresh_pick();
                        self.note_here();
                    }
                    return;
                }
            }
            if matches!(drag, Some(Drag::Select { .. })) && self.view.sel.is_some() {
                self.confirm_selection();
                if self.view.sel.is_some_and(|s| !s.confirmed) {
                    // Nothing inside the rectangle: do not leave an empty selection in the
                    // way of editing.
                    self.view.sel = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("选区为空".to_string(), "the selection is empty".to_string());
                    self.view.note = Some(note);
                }
            }
        }
    }
}
