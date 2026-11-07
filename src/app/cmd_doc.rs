//! cmd_apply 的 new/link/del/paste/rotate/notes/relabel/layer/tolayer/region 分支与文档动词.

use super::cmd::CmdFlow;
use super::*;

impl App {
    pub(super) fn cmd_doc_verbs(&mut self, cmd: &str, arg: Option<&str>, rest: &str) -> CmdFlow {
        match (cmd, arg) {
            // ---- 图层:看/动哪一层,以及把选中的东西归到某层 ----
            // ---- 从零建图:新建框 / 连边 / 删除 ----
            ("new", _) => self.cmd_new(rest.trim()),
            ("link", Some(a)) => self.cmd_link(a),
            ("link", None) => {
                let note = self.view.lang.pick_owned(
                    "link 缺参数:要连到哪个框(id 或文字)".to_string(),
                    "link needs a target box (id or label)".to_string(),
                );
                self.view.note = Some(note);
            }
            ("del", _) | ("delete", _) => self.cmd_del(true),
            ("paste", _) | ("put", _) => self.cmd_paste(),
            ("rotate", Some(a)) => self.cmd_rotate(Some(a)),
            ("rotate", None) => self.cmd_rotate(None),
            // 开关是视图状态;title / add / rm 改文档(进撤销栈)
            ("notes", arg) => self.cmd_notes(arg),
            ("relabel", _) => self.cmd_relabel(rest),
            ("layer", Some(a)) => {
                self.require_edit_scale();
                if a.eq_ignore_ascii_case("all") {
                    self.view.layer = None;
                } else {
                    self.view.layer = Some(a.to_string());
                }
                self.note_layer();
            }
            ("layer", None) => self.note_layer(),
            ("tolayer", Some(a)) => self.move_to_layer(a),
            ("tolayer", None) => {
                let note = self.view.lang.pick_owned(
                    "tolayer 缺参数:要移到哪一层".to_string(),
                    "tolayer needs a layer name".to_string(),
                );
                self.view.note = Some(note);
            }
            // :region ... —— 区域的增删改(collapse 只影响显示,不动数据)
            ("region", _) => self.cmd_region(arg, rest),
            _ => return CmdFlow::NotMine,
        }
        CmdFlow::Handled
    }
    /// 区域(cluster)动词.参数是子命令;`rest` 保留整行(标题可以有空格)
    pub(super) fn cmd_region(&mut self, sub: Option<&str>, rest: &str) {
        let sub = sub.unwrap_or("");
        // 子命令之后的原文:`region label 一段文字` 里标题是"一段文字"
        let tail = {
            let r = rest.trim_start();
            r.strip_prefix(sub).unwrap_or("").trim()
        };
        // 目标:光标框所属的区域,或者显式给的 id
        let by_id = |app: &App, id: &str| app.doc.region_idx(id);
        let pick_region = |app: &App, arg: &str| -> Option<usize> {
            if !arg.is_empty() {
                return by_id(app, arg);
            }
            app.cursor_box().and_then(|b| app.doc.box_region(b))
        };
        // 参与这次操作的框:选区里的,否则光标下的
        let members = |app: &App| -> Vec<String> {
            let boxes = app.targets().boxes;
            boxes
                .iter()
                .filter_map(|&b| app.doc.boxes.get(b).map(|x| x.id.clone()))
                .collect()
        };
        match sub {
            "" => {
                let n = self.doc.regions.len();
                let note = if n == 0 {
                    self.view.lang.pick_owned(
                        "还没有区域:选中几个框再 :region new 标签".to_string(),
                        "no regions yet: select boxes, then :region new <label>".to_string(),
                    )
                } else {
                    let names: Vec<String> = self
                        .doc
                        .regions
                        .iter()
                        .map(|r| format!("{}({})", r.id, r.members.len()))
                        .collect();
                    self.view.lang.pick_owned(
                        format!("{n} 个区域: {}", names.join(" ")),
                        format!("{n} region(s): {}", names.join(" ")),
                    )
                };
                self.view.note = Some(note);
            }
            "new" => {
                let ids = members(self);
                if ids.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "区域要有成员:先选中几个框(或把光标放在框上)".to_string(),
                        "a region needs members: select boxes or put the cursor on one".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("建区域");
                let label = tail;
                let n = self.doc.regions.len() + 1;
                let id = format!("r{n}");
                self.doc.regions.push(crate::document::Region {
                    id: id.clone(),
                    label: label.to_string(),
                    color: None,
                    style: crate::model::BoxStyle::Rounded,
                    layer: String::new(),
                    padding: 1,
                    members: ids.clone(),
                    collapsed: false,
                });
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("区域 {id}:{} 个框", ids.len()),
                    format!("region {id}: {} box(es)", ids.len()),
                );
                self.view.note = Some(note);
            }
            "add" | "rm" => {
                let Some(ri) = pick_region(self, tail) else {
                    let note = self.view.lang.pick_owned(
                        "光标下的框不属于任何区域(:region add <区域 id> 直接指定)".to_string(),
                        "the cursor box is in no region (:region add <region id> to name one)"
                            .to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                };
                let ids = members(self);
                if ids.is_empty() {
                    return;
                }
                self.begin_edit(if sub == "add" {
                    "区域加成员"
                } else {
                    "区域移成员"
                });
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    if sub == "add" {
                        for id in ids {
                            if !r.members.contains(&id) {
                                r.members.push(id);
                            }
                        }
                    } else {
                        r.members.retain(|m| !ids.contains(m));
                    }
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!(
                        "区域 {} 现在 {} 个成员",
                        self.doc.regions[ri].id,
                        self.doc.regions[ri].members.len()
                    ),
                    format!(
                        "region {} now has {} member(s)",
                        self.doc.regions[ri].id,
                        self.doc.regions[ri].members.len()
                    ),
                );
                self.view.note = Some(note);
            }
            "del" => {
                let Some(ri) = pick_region(self, tail) else {
                    return;
                };
                self.begin_edit("删区域");
                let id = self.doc.regions[ri].id.clone();
                self.doc.regions.remove(ri);
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("删掉区域 {id}(框都留着)"),
                    format!("removed region {id} (boxes stay)"),
                );
                self.view.note = Some(note);
            }
            "collapse" | "expand" => {
                let Some(ri) = pick_region(self, tail) else {
                    return;
                };
                self.begin_edit(if sub == "collapse" {
                    "折叠区域"
                } else {
                    "展开区域"
                });
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.collapsed = sub == "collapse";
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!(
                        "区域 {} {}",
                        self.doc.regions[ri].id,
                        if sub == "collapse" {
                            "已折叠"
                        } else {
                            "已展开"
                        }
                    ),
                    format!(
                        "region {} {}",
                        self.doc.regions[ri].id,
                        if sub == "collapse" {
                            "collapsed"
                        } else {
                            "expanded"
                        }
                    ),
                );
                self.view.note = Some(note);
            }
            "label" => {
                let Some(ri) = pick_region(self, "") else {
                    self.view.note = Some(self.view.lang.pick_owned(
                        "光标下的框不属于任何区域".to_string(),
                        "the cursor box is in no region".to_string(),
                    ));
                    return;
                };
                self.begin_edit("区域标题");
                let text = tail.to_string();
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.label = text;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            "color" => {
                let Some(ri) = pick_region(self, "") else {
                    return;
                };
                let v = tail;
                let color = if v.is_empty() || v == "auto" || v == "none" {
                    None
                } else {
                    parse_color(v)
                };
                if v != "auto" && v != "none" && color.is_none() {
                    let note = self.view.lang.pick_owned(
                        format!("认不出颜色 {v}(用 auto 清掉)"),
                        format!("unknown colour {v} (use auto to clear)"),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("区域颜色");
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.color = color;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            "style" => {
                let Some(ri) = pick_region(self, "") else {
                    return;
                };
                let Some(st) = self
                    .doc
                    .regions
                    .get(ri)
                    .map(|r| crate::model::BoxStyle::parse(tail).unwrap_or(r.style))
                else {
                    return;
                };
                if !st.is_boxed() {
                    let note = self.view.lang.pick_owned(
                        "区域只能用带边框的样式(single/rounded/double/heavy)".to_string(),
                        "a region needs a boxed style (single/rounded/double/heavy)".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("区域样式");
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.style = st;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            other => {
                let note = self.view.lang.pick_owned(
                    format!("region 只认 new/add/rm/del/collapse/expand/label/color/style,不认 {other}"),
                    format!("region takes new/add/rm/del/collapse/expand/label/color/style, not {other}"),
                );
                self.view.note = Some(note);
            }
        }
    }

    /// [--glyph solid|dashed|dotted|arrow] <文字>` / `rm <n>` 改文档
    /// `:new <文字>` / i:在光标那一格新建一个框,光标跟过去
    pub(super) fn cmd_new(&mut self, label: &str) {
        self.require_edit_scale();
        self.begin_edit("建框");
        let (x, y) = self.free_spot(label);
        // 默认样式就是 none(只有文字的节点);最近选中过框就照抄它的样式/颜色/对齐.
        // 区块选区时 view.sel 有值,那不算"选中一个元素",照默认来
        let look = self.box_look_now();
        let style = look.map(|l| l.style).unwrap_or_default();
        match self.doc.add_box(label, (x, y), style) {
            Ok(i) => {
                if label.is_empty() {
                    // 没给文字就拿框 id 当文字,不然框里是空的,看不见
                    let id = self.doc.boxes[i].id.clone();
                    self.doc.set_text(i, id);
                }
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
                let name = label_of(&self.doc, i);
                let note = self.view.lang.pick_owned(
                    format!("新建 {name} @({x},{y})"),
                    format!("added {name} at ({x},{y})"),
                );
                self.view.note = Some(note);
            }
            Err(e) => self.view.note = Some(e),
        }
        self.end_edit();
    }

    /// `:link <目标>`:从光标节点拉一条边过去(目标可写 id 或唯一的文字)
    pub(super) fn cmd_link(&mut self, target: &str) {
        self.require_edit_scale();
        self.begin_edit("连边");
        let Some(i) = self.cursor_box() else {
            let note = self.view.lang.pick_owned(
                "没有光标节点:先用 :new 建一个".to_string(),
                "no cursor box: create one with :new".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        let Some(j) = self.resolve_box(target) else {
            let note = self.view.lang.pick_owned(
                format!("找不到唯一的框:{target}"),
                format!("no unique box matches {target}"),
            );
            self.view.note = Some(note);
            return;
        };
        let look = self.edge_look_now();
        let group = look
            .as_ref()
            .map(|l| l.group.clone())
            .unwrap_or_else(|| self.link_group(i).to_string());
        let from = self.doc.boxes[i].id.clone();
        let to = self.doc.boxes[j].id.clone();
        match self.doc.add_edge(&from, &to, &group, Dir::Forward) {
            Ok(e) => {
                if let Some(l) = &look {
                    self.apply_edge_look(e, l);
                }
                let a = label_of(&self.doc, i);
                let b = label_of(&self.doc, j);
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("连上 {a} -> {b}"), format!("linked {a} -> {b}"));
                self.view.note = Some(note);
            }
            Err(e) => {
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("连不上:{e}"), format!("cannot link: {e}"));
                self.view.note = Some(note);
            }
        }
        self.end_edit();
    }

    /// `:del`:有选中边就删边,否则删光标框(连它的文字与边)
    pub(super) fn cmd_del(&mut self, include_edges: bool) {
        if self.blocked_by_selection() {
            return;
        }
        let set = self.targets();
        if set.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可删的东西".to_string(),
                "nothing to delete here".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        // 至少留一个框:整批删完就一个不剩时,整次拒绝
        let left = self.doc.boxes.len() - set.boxes.len();
        if !set.boxes.is_empty() && left < 1 {
            let note = self.view.lang.pick_owned(
                "最后一个框不能删(空文档没法编辑)".to_string(),
                "cannot remove every box (nothing left to edit)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let link = self.link_ids();
        self.begin_edit("删除");
        let (clip, ne) = self.take_for_clipboard(&set, include_edges);
        let nb = set.boxes.len();
        // 先删边,再删框(框删了也会带走它的边).
        // id 要先全抄下来:删一条后面的下标就全移了
        let edge_ids: Vec<String> = set
            .edges
            .iter()
            .filter_map(|e| self.doc.edges.get(*e).map(|e| e.id.clone()))
            .collect();
        for id in edge_ids {
            let _ = self.doc.remove_edge(&id);
        }
        for &i in set.boxes.iter().rev() {
            let id = self.doc.boxes[i].id.clone();
            let _ = self.doc.remove_box(&id);
        }
        self.clipboard = Some(clip);
        self.view.sel = None;
        self.view.cand = 0;
        // 光标留在原地:删掉的东西挪走了,光标格不该跟着跑
        self.refresh_pick();
        self.end_edit();
        let note = if include_edges {
            format!("剪切 {nb} 框 {ne} 边(含边)")
        } else {
            format!("剪切 {nb} 框 {ne} 边(仅框)")
        };
        let note_en = if include_edges {
            format!("cut {nb} box(es), {ne} edge(s) (with edges)")
        } else {
            format!("cut {nb} box(es), {ne} edge(s) (boxes only)")
        };
        let note = self.view.lang.pick_owned(note, note_en);
        self.view.note = Some(note);
        // 连边还开着的话,按 id 把源和目标找回来(源没了它会自己退出)
        self.rebind_link(link);
    }

    /// y:把图片段(框 + 文字 + 内部边/选中的边)复制到剪贴板,不动文档
    pub(super) fn cmd_yank(&mut self) {
        let set = self.targets();
        if set.is_empty() {
            let note = self.view.lang.pick_owned(
                "没有可复制的东西".to_string(),
                "nothing to yank".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let (clip, _) = self.take_for_clipboard(&set, true);
        let (nb, ne) = (clip.boxes.len(), clip.edges.len());
        self.clipboard = Some(clip);
        let note = format!("yanked {nb} boxes + {ne} edges");
        self.view.note = Some(note);
    }
}
