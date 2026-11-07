//! link 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 复制图片段:剪贴板只收"选中的框 + 它们的内部边 + 显式选中的边".
    /// 返回 (片段, 会因为删这些框而消失的边数).
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
        // 边界边不算片段的一部分(另一头留在文档里),但删的时候照样跟着框一起消失
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
            // 边的两端与折点不参与"原点":另一头可能还留在文档里(位置很远),
            // 拿它当基准会让粘贴落点跟着边跑,和没有边时完全不一样
            if let Some(g) = self.doc.groups.iter().find(|g| g.id == e.group) {
                if !groups.iter().any(|x| x.id == g.id) {
                    groups.push(g.clone());
                }
            }
        }
        let edges = if include_edges { edges } else { Vec::new() };
        let groups = if include_edges { groups } else { Vec::new() };
        // 片段用到的图层:按文档里的顺序抄一份,粘贴时照它建新层
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

    /// 剪贴板内容的宽高(框的外包盒,至少 1)
    pub(super) fn clip_size(&self, clip: &Clipboard) -> (i32, i32) {
        let mut x1 = clip.origin.0 + 1;
        let mut y1 = clip.origin.1 + 1;
        for b in &clip.boxes {
            x1 = x1.max(b.at.0 + b.size.0 as i32);
            y1 = y1.max(b.at.1 + b.size.1 as i32);
        }
        ((x1 - clip.origin.0).max(1), (y1 - clip.origin.1).max(1))
    }

    /// 按这个偏移粘过去,会不会压到文档里已有的框
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

    /// 粘贴落点:先试光标处的精确位置,压到别的框就按右 -> 下 -> 右下错开,
    /// 最多找 32 个候选;都挤就退回精确位置(回显里说明)
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

    /// 新框放哪:光标那一行的右边,离开现有内容两格
    /// 新框放哪:光标那一格;压到别的框就一格一格往右让,直到空出来
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

    /// 新边挂哪个分组:光标节点已有的分组,没有就用第一个分组
    pub(super) fn link_group(&self, i: usize) -> &str {
        self.doc
            .box_groups(i)
            .first()
            .map(|&g| self.doc.groups[g].id.as_str())
            .or_else(|| self.doc.groups.first().map(|g| g.id.as_str()))
            .unwrap_or("g0")
    }

    /// 按 id 或文字找框;文字命中多个就不算(要写 id)
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
            // 没选区就按光标下的东西
            match self.view.pick {
                render::Pick::Box(i) => set.boxes.push(i),
                render::Pick::Edge(e) => set.edges.push(e),
                render::Pick::None => {}
            }
        }
        // 只看一层的时候,动词也只动这一层
        if let Some(layer) = &self.view.layer {
            set.boxes.retain(|i| &self.doc.boxes[*i].layer == layer);
            set.edges.retain(|e| &self.doc.edges[*e].layer == layer);
        }
        set
    }

    /// 连边的源与目标现在对应的框 id(结构变动前后对照用)
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

    /// 文档结构变了(删除 / 撤销 / 重做):按 id 把连边的源和目标找回来;
    /// 源一个都不剩就退出连边模式
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

    /// e 第一段:确定源框,进入连边模式
    pub(super) fn begin_link(&mut self) {
        // 源选区没确认就先确认(选区本来就是源)
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
        // 选区让给"选目标"用
        self.view.sel = None;
        self.view.link = Some(LinkState {
            sources: sources.clone(),
            targets: Vec::new(),
            confirming: false,
        });
        // 源数、目标数这些由状态栏实时报,这里只在"有边被忽略"时说一句
        if ignored > 0 {
            let note = self.view.lang.pick_owned(
                format!("选区里的 {ignored} 条边不参与连边"),
                format!("{} selected edge(s) ignored by linking", ignored),
            );
            self.view.note = Some(note);
        }
    }

    /// e 第二段:确定目标,数量确认后建边
    pub(super) fn link_commit(&mut self) {
        let Some(link) = self.view.link.clone() else {
            return;
        };
        // 目标选区没确认就先确认
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

    /// 批量建边:源 x 目标各一条 Forward,整次只算一步撤销
    pub(super) fn create_edges(&mut self, sources: &[usize], targets: &[usize], total: usize) {
        let _ = total;
        self.require_edit_scale();
        // 只聚焦一个分组就用它,否则每个源用自己第一个分组
        let one = match self.view.focus.as_slice() {
            [gr] if *gr < self.doc.groups.len() => Some(self.doc.groups[*gr].id.clone()),
            _ => None,
        };
        // 最近选中过边:新边照抄它的样式与分组(聚焦了某个分组时聚焦优先)
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
                // 源就是目标:自环,照建
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

    /// Esc:退出连边模式,不动文档
    pub(super) fn cancel_link(&mut self) {
        self.view.link = None;
        self.view.sel = None;
        let note = self
            .view
            .lang
            .pick_owned("取消连边".to_string(), "linking cancelled".to_string());
        self.view.note = Some(note);
    }

    /// 输入框取消:把这次刚建、还没写字的注释框删掉
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

    /// Enter:把选区钉住(之后移动光标不再改变它)
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

    /// 有未确认的选区时,改文档的命令一律拦住
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

    /// Ctrl-hjkl:把选中的框(没选区就是光标框)按整数单元格挪一格
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
        // 框挪了,光标与选择框跟着挪:不然按几下之后就"脱手"了,
        // 光标还停在原地、框已经走开
        self.view.cursor = (self.view.cursor.0 + dx, self.view.cursor.1 + dy);
        if let Some(sel) = self.view.sel.as_mut() {
            sel.anchor = (sel.anchor.0 + dx, sel.anchor.1 + dy);
            sel.end = (sel.end.0 + dx, sel.end.1 + dy);
        }
        self.refresh_pick();
        // 光标挪出画面边距时视图跟过去(和普通方向键同一套)
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

    /// 换层之后说一句:空字符串就是默认层
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

    /// L:全部 -> 各图层 -> 全部(层从 doc.layer_names() 来,默认层排最前)
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

    /// :tolayer:把选中的框(连同它的文字),以及两端都在选区内的边,一起挪到某一层
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

    /// E:在跟光标框相连的边里按下标顺序循环下一条
    /// E:只亮光标节点的邻居与相连的边,其余压暗;再按 E 或 Esc 退出
    pub(super) fn toggle_ego(&mut self) {
        // 开 / 关:开着就是"只亮光标所在节点的邻居",换节点(点击或挪光标)跟着走
        if self.view.ego.is_some() {
            self.view.ego = None;
            let note = self
                .view
                .lang
                .pick_owned("退出邻居高亮".to_string(), "neighbour view off".to_string());
            self.view.note = Some(note);
            return;
        }
        let Some(n) = self.doc.box_at_cell(self.view.cursor) else {
            let note = self.view.lang.pick_owned(
                "光标下没有节点".to_string(),
                "no node under the cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        // 注释框:亮它绑的那条边的两头
        let n = match self.doc.edge_of_box(n) {
            Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
            None => n,
        };
        self.view.ego = Some(n);
        let edges = self.doc.adj(n).len();
        let mut neighbours: Vec<usize> = Vec::new();
        for &e in self.doc.adj(n) {
            if let Some((a, b)) = self.doc.edge_ends(e) {
                let other = if a == n { b } else { a };
                if !neighbours.contains(&other) {
                    neighbours.push(other);
                }
            }
        }
        let name = label_of(&self.doc, n).to_string();
        // 相关的注释框有几条(开关关着时它们会被临时露出来)
        let notes = self
            .doc
            .adj(n)
            .iter()
            .filter(|e| self.doc.note_of_edge(&self.doc.edges[**e].id).is_some())
            .count();
        let note = self.view.lang.pick_owned(
            format!(
                "邻居高亮:{name}({} 个邻居 / {edges} 条边 / {notes} 条注释),E 或 Esc 退出",
                neighbours.len()
            ),
            format!(
                "neighbours of {name} ({} / {edges} edges / {notes} notes), E or Esc to exit",
                neighbours.len()
            ),
        );
        self.view.note = Some(note);
    }

    pub(super) fn cycle_edge(&mut self) {
        self.sync_hit();
        // 完全重叠堆在一起时,先在最上面的框之间轮(E 也能选中被压住的那些)
        let stack = self.doc.boxes_at_cell(self.view.cursor);
        if stack.len() > 1 {
            // 从"当前选中的那个"往下轮:文档改过时 cand 会被清零,靠它记不住位置
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
        // 光标压在节点上:只在这个节点与邻居之间的边里转(不从这儿路过的线)
        if let Some(c) = self.doc.box_at_cell(self.view.cursor) {
            self.cycle_at_node(c);
            return;
        }
        // 光标不在节点上(在线上/空白处):按这个格子上的边候选转.
        // 命中表按**屏幕**格记,光标存的是画布格,得先投影过去,不然视图一挪就选不中
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

    /// 在节点 c 的相连边里轮(E):按文档顺序,到头绕回去
    /// E 模式下的"高亮边":中心连着的那几条.没开 E 时一切都算.
    pub(super) fn ego_lit_edge(&self, e: usize) -> bool {
        match self.view.ego {
            Some(a) => self.doc.adj(a).contains(&e),
            None => true,
        }
    }

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

    /// 选中边之后说一句
    pub(super) fn note_edge(&mut self, e: usize) {
        let (a, b) = match self.doc.edge_ends(e) {
            Some((a, b)) => (label_of(&self.doc, a), label_of(&self.doc, b)),
            None => (String::new(), String::new()),
        };
        let note = self
            .doc
            .note_of_edge(&self.doc.edges[e].id)
            .and_then(|b| self.doc.text(b).map(|t| t.text.clone()))
            .unwrap_or_default();
        self.view.note = Some(self.view.lang.pick_owned(
            if note.is_empty() {
                format!("选边 {a} - {b} (#{e}) -> 无注释")
            } else {
                format!("选边 {a} - {b} (#{e}) -> 注释: {note}")
            },
            if note.is_empty() {
                format!("edge {a} - {b} (#{e}) -> no note")
            } else {
                format!("edge {a} - {b} (#{e}) -> note: {note}")
            },
        ));
    }

    /// 有没有等着跑外部编辑器的请求
    /// 光标下的链接目标(画布格 -> 目标)
    pub(super) fn link_at_cursor(&self) -> Option<String> {
        self.hit.links.get(&self.view.cursor).cloned()
    }

    /// Enter / gf:有未确认选区先确认(老行为),否则打开光标下的链接
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

    /// 从 (x, y) 往右找第一个放得下 `size` 的空位;一行扫完就换下一行
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
