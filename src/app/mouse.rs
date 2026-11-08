//! mouse 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 鼠标:滚轮缩放(按住 Ctrl 改成平移),左键点中框跳过去,按住拖动
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
            // 菜单开着时,鼠标划到哪一项就预览哪一项
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
                // 菜单开着:右键只取消,不开始拖动
                self.menu_cancel();
            } else if self.drag.is_none() {
                // 按住右键先记着:框上是"拖出来连边",别处是"拖动平移";
                // 两种都是拖过两格才动,松开没拖过就弹菜单
                let p = render::panels(&self.doc, self.area, &self.view);
                let press = (me.column, me.row);
                // `mouse_edges = false`:右键在框上也只平移,不连边
                // (一行文字节点上拖一下就可能连到隔壁,误连太多)
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
                    // 真的拖起来了:从按下的框开始连边,顺便清掉菜单/选区干扰
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
                        // 没拖过阈值:按"按下那一格"弹菜单,不是松开的地方
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
            // 按下的位置上若正好是"选中边的端点格",这一下是拖端点改接.
            // 要在 refresh_pick 之前记下来:按格子选东西会把 pick 换掉
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
            // 连边模式 + Shift:在目标集合里加/减这个框(普通点击就是"设为目标")
            if self.view.link.is_some() && me.modifiers.contains(KeyModifiers::SHIFT) {
                if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                    // 先把手上的目标落进显式集合,再在这个集合里加/减
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
            // 点在地图之外(图例/状态行)不动光标,也不起拖动
            let inside = me.column >= p.map.x
                && me.column < p.map.x + p.map.width
                && me.row >= p.map.y
                && me.row < p.map.y + p.map.height;
            if !inside {
                return;
            }
            // 单击就把光标落在点到的这一格(不做"吸到节点的角上")
            let cell = p.proj.canvas(me.column as i32, me.row as i32);
            // 双击 = 按一下 i:空处新建节点(先弹输入框),节点上改文字
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
            // 只有点在"空处"(既不在节点上、也不在线上)才算新建;
            // 同一格连点一条线是"在重叠的边之间轮",不能被当成双击
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
            // 已经选好的一块,点它外面就是"取消";点在里面则留着(好继续操作)
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
                // 连边模式:点框就是"把目标设成它"
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
                // 按在带框节点的边框上:拖出去是连边;按在框内才是挪框.
                // 位置已经更新过了,单击(没拖)仍旧只是落光标
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
                // 按在框上:拖起来挪这一批框.真的动了才切 1:1、才压撤销,
                // 所以单击还是纯粹的"落光标"
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
                // 空处点到线:选中这条边(鼠标是选边的另一条路).
                // 这一格压着好几条边时,再点一下就轮到下一条
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
                // E 模式下这一格如果只有"不在高亮里的边",refresh_pick 会把它清掉
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
            // 拖动:按住 Ctrl(或者在连边模式里)是平移,其余在空处拉选区
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
                    // 拖起来了才成选区;光标跟着鼠标跑,灰边跟着变.
                    // 旧选区(不管确认没有)直接换掉:新拖出来的就是新的
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
                // 拖连边 / 拖端点:光标下是框就把它当目标(预览也跟过去),
                // 靠边时顺手把视图往那边推一格
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
                        // 还没到目标框:光标跟着指针走,预览才有终点可画
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
                    // 屏幕格 -> 画布格:任何缩放下都按按下时的投影换算,
                    // 用"原始位置 + 总位移",不逐帧累加,避免取整误差
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
                    // 光标和选区跟着走同一个位移(不然看起来像框脱手了)
                    self.view.cursor = (cursor0.0 + d.0, cursor0.1 + d.1);
                    if let Some((anchor, end, confirmed)) = sel0 {
                        self.view.sel = Some(RectSel {
                            anchor: (anchor.0 + d.0, anchor.1 + d.1),
                            end: (end.0 + d.0, end.1 + d.1),
                            confirmed,
                        });
                    }
                    // 拖动期间把选区矩形藏起来(松手后再画)
                    self.view.drag_hide_sel = true;
                    self.refresh_pick();
                }
                // 右键正在拖:左键的拖动事件插进来也不关它的事
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
                return; // 右键拖到一半时松了左键:不关它的事
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
                // 按下的那一下可能被终端/tmux 吃掉(切窗格):松手补一次"点这里"
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
                    // 圈出来的地方什么都没有:不留一个空选区挡着编辑
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
