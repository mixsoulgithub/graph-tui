//! cmd_apply 的 paste/split/merge 分支与实现.

use super::*;

impl App {
    /// p:把剪贴板里的东西按当前光标偏移粘一份新的
    pub(super) fn cmd_paste(&mut self) {
        if self.blocked_by_selection() {
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            let note = self
                .view
                .lang
                .pick_owned("剪贴板为空".to_string(), "clipboard is empty".to_string());
            self.view.note = Some(note);
            return;
        };
        self.require_edit_scale();
        self.begin_edit("粘贴");
        let (dx, dy, crowded) = self.paste_offset(&clip);

        // 新 id:框和边分开编号,避开已有 id
        let mut box_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut n = 1;
        for b in &clip.boxes {
            loop {
                let id = format!("b{n}");
                n += 1;
                if self.doc.box_idx(&id).is_none() && box_ids.values().all(|v| *v != id) {
                    box_ids.insert(b.id.clone(), id);
                    break;
                }
            }
        }
        let mut text_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut t = 1;
        for tx in &clip.texts {
            loop {
                let id = format!("t{t}");
                t += 1;
                if !self.doc.texts.iter().any(|x| x.id == id) && text_ids.values().all(|v| *v != id)
                {
                    text_ids.insert(tx.id.clone(), id);
                    break;
                }
            }
        }
        // 分组:id 或 label 对上就复用,否则新建
        let mut group_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for g in &clip.groups {
            let reuse = self
                .doc
                .groups
                .iter()
                .find(|x| x.id == g.id || (!g.label.is_empty() && x.label == g.label))
                .map(|x| x.id.clone());
            if let Some(id) = reuse {
                group_of.insert(g.id.clone(), id);
            } else {
                let mut k = self.doc.groups.len();
                let id = loop {
                    let id = format!("g{k}");
                    if self.doc.group_idx(&id).is_none() {
                        break id;
                    }
                    k += 1;
                };
                self.doc.groups.push(crate::document::Group {
                    id: id.clone(),
                    label: g.label.clone(),
                    color: g.color,
                });
                group_of.insert(g.id.clone(), id);
            }
        }
        // 图层:有就复用,没有就建
        let mut layer_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for name in clip
            .boxes
            .iter()
            .map(|b| b.layer.clone())
            .chain(clip.edges.iter().map(|e| e.layer.clone()))
        {
            if name.is_empty() || self.doc.layers.iter().any(|l| l.name == name) {
                layer_of.insert(name.clone(), name);
                continue;
            }
            let order = self.doc.layers.iter().map(|l| l.order).max().unwrap_or(0) + 1;
            self.doc.layers.push(crate::document::Layer {
                name: name.clone(),
                order,
            });
            layer_of.insert(name.clone(), name);
        }

        // 框 + 文字
        let mut new_boxes: Vec<usize> = Vec::new();
        for b in &clip.boxes {
            let mut nb = b.clone();
            nb.id = box_ids[&b.id].clone();
            nb.at = (b.at.0 + dx, b.at.1 + dy);
            nb.layer = layer_of.get(&b.layer).cloned().unwrap_or_default();
            let at = nb.at;
            new_boxes.push(self.doc.boxes.len());
            self.doc.boxes.push(nb);
            if let Some(t) = clip.texts.iter().find(|t| t.box_id == b.id) {
                let mut nt = t.clone();
                nt.id = text_ids[&t.id].clone();
                nt.box_id = box_ids[&b.id].clone();
                nt.at = if b.style.is_boxed() {
                    (at.0 + 1, at.1 + 1)
                } else {
                    (t.at.0 + dx, t.at.1 + dy)
                };
                nt.layer = layer_of.get(&t.layer).cloned().unwrap_or_default();
                self.doc.texts.push(nt);
            }
        }
        // 边:两端都在剪贴板里就换新 id,另一端还在就保留,否则跳过
        let mut nb_edges = 0usize;
        let mut skipped = 0usize;
        for e in &clip.edges {
            let from = box_ids.get(&e.from).cloned();
            let to = box_ids.get(&e.to).cloned();
            let endpoint = |id: &String, mapped: Option<String>| -> Option<String> {
                if let Some(new) = mapped {
                    return Some(new);
                }
                // 剪贴板里的框被粘出来之前就已经没了:看原文档里还在不在
                self.doc.box_idx(id).map(|_| id.clone())
            };
            let (Some(f), Some(t)) = (endpoint(&e.from, from), endpoint(&e.to, to)) else {
                skipped += 1;
                continue;
            };
            let mut ne = e.clone();
            let mut k = 1;
            let id = loop {
                let id = format!("e{k}");
                k += 1;
                if !self.doc.edges.iter().any(|x| x.id == id) {
                    break id;
                }
            };
            ne.id = id;
            ne.from = f;
            ne.to = t;
            ne.group = group_of.get(&e.group).cloned().unwrap_or(e.group.clone());
            ne.layer = layer_of.get(&e.layer).cloned().unwrap_or_default();
            ne.via = e.via.iter().map(|&(x, y)| (x + dx, y + dy)).collect();
            self.doc.edges.push(ne);
            nb_edges += 1;
        }
        self.doc.rebuild();
        self.view.sel = None;
        // 旧命中表里的下标对不上新文档了,直接丢掉
        self.hit = render::FrameHit::default();
        self.view.cand = 0;
        // 光标落到第一份新框上:下一次 d/x/样式命令默认作用于刚粘的那份
        if let Some(&i) = new_boxes.first() {
            self.view.cursor = self.cell_of(i);
        }
        self.refresh_pick();
        self.end_edit();
        let nb = clip.boxes.len();
        let mut zh = format!("粘贴 {nb} 框 {nb_edges} 边");
        let mut en = format!("pasted {nb} box(es), {nb_edges} edge(s)");
        if skipped > 0 {
            zh.push_str(&format!(",跳过 {skipped} 条悬空边"));
            en.push_str(&format!(", skipped {skipped} dangling edge(s)"));
        }
        if crowded {
            zh.push_str(",位置拥挤,已叠放");
            en.push_str(", crowded: pasted on top");
        }
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// S:在光标所在行与下一行之间把框切开(第一段留原框,其余进新框)
    pub(super) fn cmd_split_box(&mut self) {
        let Some(i) = render::box_at(&self.doc, self.view.cursor, &self.view) else {
            let note = self.view.lang.pick_owned(
                "光标不在框上,没法拆".to_string(),
                "the cursor is not on a box".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        if self.doc.boxes[i].style.is_point() || self.doc.is_note(i) {
            let note = self.view.lang.pick_owned(
                "点节点和边注释不能拆".to_string(),
                "a point node or an edge note cannot be split".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let Some(t) = self.doc.text(i) else {
            return;
        };
        let (text, t_at) = (t.text.clone(), t.at);
        let b = self.doc.boxes[i].clone();
        let origin = if b.style.is_boxed() {
            (b.at.0 + 1, b.at.1 + 1)
        } else {
            t_at
        };
        let row = self.view.cursor.1 - origin.1;
        let lines: Vec<&str> = text.split('\n').collect();
        if row < 0 || row as usize >= lines.len() {
            let note = self.view.lang.pick_owned(
                "光标不在框里的文字上,没法拆".to_string(),
                "the cursor is not on the box text".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let row = row as usize;
        if row + 1 >= lines.len() {
            let note = self.view.lang.pick_owned(
                "光标在最后一行,后面没有内容".to_string(),
                "nothing after this line".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        let head: String = lines[..=row].join("\n");
        let tail: String = lines[row + 1..].join("\n");
        self.begin_edit("拆框");
        self.doc.set_text(i, head);
        // 新框放右边,顶部对齐、隔一格;那儿有东西就往下/往右找空位
        let mut at = (b.at.0 + b.size.0 as i32 + 1, b.at.1);
        for _ in 0..32 {
            if self.doc.boxes.iter().all(|o| {
                let (ox, oy, ow, oh) = (o.at.0, o.at.1, o.size.0 as i32, o.size.1 as i32);
                at.0 + 1 > ox + ow
                    || at.0 < ox - 1
                    || at.1 + 1 > oy + oh
                    || at.1 < oy - 1
                    || (o.at == b.at)
            }) {
                break;
            }
            at = (at.0, at.1 + b.size.1 as i32 + 1);
        }
        let style = b.style;
        let Ok(n) = self.doc.add_box("", at, style) else {
            self.end_edit();
            return;
        };
        self.doc.set_text(n, tail);
        {
            let src = self.doc.boxes[i].clone();
            let dst = &mut self.doc.boxes[n];
            dst.color = src.color;
            dst.fill = src.fill;
            dst.layer = src.layer.clone();
        }
        if let (Some(st), Some(dt)) = (self.doc.text(i).cloned(), self.doc.text_of(n)) {
            let t = &mut self.doc.texts[dt];
            t.color = st.color;
            t.bg = st.bg;
            t.align = st.align;
            t.layer = st.layer;
        }
        self.doc.rebuild();
        self.view.cursor = origin;
        self.refresh_pick();
        self.end_edit();
        let note = self.view.lang.pick_owned(
            "已在下一行处拆开".to_string(),
            "split below the cursor line".to_string(),
        );
        self.view.note = Some(note);
    }

    /// M:把选区里的多个框合成一个(文字按阅读顺序接起来)
    pub(super) fn cmd_merge_boxes(&mut self) {
        if self.blocked_by_selection() {
            return;
        }
        let set = self.targets();
        if set.boxes.len() < 2 {
            let note = self.view.lang.pick_owned(
                "至少要选中两个框".to_string(),
                "select at least two boxes".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        // 目标框:光标所在的那个,否则阅读顺序第一个(上到下、左到右)
        let mut order = set.boxes.clone();
        order.sort_by_key(|&i| (self.doc.boxes[i].at.1, self.doc.boxes[i].at.0));
        let target = render::box_at(&self.doc, self.view.cursor, &self.view)
            .filter(|t| order.contains(t))
            .unwrap_or(order[0]);
        // 文字:按阅读顺序拼,空的跳过
        let mut parts: Vec<String> = Vec::new();
        for &i in &order {
            if let Some(t) = self.doc.text(i) {
                if !t.text.trim().is_empty() {
                    parts.push(t.text.clone());
                }
            }
        }
        let merged = parts.join("\n");
        let kept: std::collections::HashSet<String> = set
            .boxes
            .iter()
            .map(|&i| self.doc.boxes[i].id.clone())
            .collect();
        let target_id = self.doc.boxes[target].id.clone();
        self.require_edit_scale();
        self.begin_edit("合并");
        self.doc.set_text(target, merged);
        // 边:内部边删掉,边界边改接到目标框
        let mut doomed: Vec<String> = Vec::new();
        let mut rewired: Vec<(String, crate::document::DocEdge)> = Vec::new();
        for e in self.doc.edges.clone() {
            let from_in = kept.contains(&e.from);
            let to_in = kept.contains(&e.to);
            match (from_in, to_in) {
                (true, true) => doomed.push(e.id.clone()),
                (true, false) => {
                    let mut ne = e.clone();
                    ne.from = target_id.clone();
                    rewired.push((e.id.clone(), ne));
                }
                (false, true) => {
                    let mut ne = e.clone();
                    ne.to = target_id.clone();
                    rewired.push((e.id.clone(), ne));
                }
                (false, false) => {}
            }
        }
        for id in &doomed {
            let _ = self.doc.remove_edge(id);
        }
        // 改接:先删后加(端点相同、方向/分组/线型一样的合成一条)
        for (old_id, ne) in rewired {
            if ne.from == ne.to && ne.id == old_id {
                // 自己连自己:保持原样
                continue;
            }
            let _ = self.doc.remove_edge(&old_id);
            let dup = self.doc.edges.iter().any(|x| {
                x.from == ne.from
                    && x.to == ne.to
                    && x.dir == ne.dir
                    && x.group == ne.group
                    && x.style == ne.style
            });
            if dup || ne.from == ne.to {
                continue;
            }
            if let Ok(k) = self.doc.add_edge(&ne.from, &ne.to, &ne.group, ne.dir) {
                self.doc.edges[k].style = ne.style;
                self.doc.edges[k].color = ne.color;
                self.doc.edges[k].bg = ne.bg;
                self.doc.edges[k].corner = ne.corner;
                self.doc.edges[k].from_side = ne.from_side;
                self.doc.edges[k].to_side = ne.to_side;
                self.doc.edges[k].layer = ne.layer;
                self.doc.edges[k].head = ne.head;
            }
        }
        // 删掉其它框(连它们的文字)
        for &i in order.iter().filter(|&&i| i != target) {
            let id = self.doc.boxes[i].id.clone();
            let _ = self.doc.remove_box(&id);
        }
        self.doc.rebuild();
        self.view.sel = None;
        self.view.cursor = self.cell_of(self.doc.box_idx(&target_id).unwrap_or(0));
        self.refresh_pick();
        self.end_edit();
        let note = self.view.lang.pick_owned(
            format!("合并了 {} 个框", set.boxes.len()),
            format!("merged {} boxes", set.boxes.len()),
        );
        self.view.note = Some(note);
    }
}
