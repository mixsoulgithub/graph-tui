//! edit 相关实现.函数体从 app.rs 原样搬来.

use super::*;

/// 一个字符在终端里占几格(汉字/emoji 两格)
fn ch_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c)
        .unwrap_or(1)
        .max(1)
}

impl App {
    /// 改文档之前压一份撤销快照
    pub fn begin_edit(&mut self, label: &str) {
        if self.previewing {
            return; // 预览不压撤销
        }
        // 动手的地方也算一个跳转点(和搜索、鼠标点一样):插字、建框、改样式之后
        // 走开了还能用 Ctrl-o 回来
        self.note_here();
        self.undo.begin(label, &self.doc);
    }

    /// 这一步没改动就丢掉快照(cmd_apply / 顶部按键路径结束时调)
    pub fn end_edit(&mut self) {
        if self.previewing {
            return;
        }
        if self.undo.prune(&self.doc) {
            self.rev += 1;
        }
    }

    /// 一批边一起改:一次 begin/end,只算一步撤销
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

    /// 这批框的文字(都一样才拿来预填,否则空)
    /// 进入就地编辑:草稿从框里现有的文字开始,光标落在画布光标指到的那个字符上
    /// (光标不在文字里时落到末尾)
    pub(super) fn begin_box_edit(&mut self, i: usize, created: bool) {
        if i >= self.doc.boxes.len() {
            return;
        }
        if self.doc.boxes[i].style.is_point() {
            // 点节点只有一个名字,就地编辑没有意义:退回输入框那条路
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

    /// 画布光标落在框里第几个字符上(不在文字区域就是 None)
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
        // 屏幕列 -> 字符下标:宽字占两格,点只能点在格子上,所以要按宽度走
        // (落在汉字右半边也算点在这个字上,和 nvim 一样)
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

    /// 第 idx 个字符的字节偏移(越界给末尾)
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

    /// 上下挪一行,尽量保住**格**号(和 nvim 一样:落在宽字中间就退到那个字头上)
    pub(super) fn caret_vertical(s: &str, caret: usize, step: i32) -> usize {
        let lines: Vec<&str> = s.split('\n').collect();
        let (row, want) = Self::caret_row_col(s, caret);
        let target = (row as i32 + step).clamp(0, lines.len() as i32 - 1) as usize;
        let line = lines[target];
        // 目标行里最后一个"起点不超过 want"的字符边界
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

    /// 编辑时让画布光标跟着草稿里的光标走(状态行显示 x,y)
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

    /// 光标在第几行、第几**格**(不是第几个字符):宽字占两格,和终端里看到的列对齐
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

    /// 退出编辑:提交(一步撤销)或丢掉草稿
    pub(super) fn commit_box_edit(&mut self, cancel: bool) {
        let Some(edit) = self.view.edit.take() else {
            return;
        };
        if cancel {
            if edit.created {
                // 这个框是这次按 i 建出来的:整步撤掉,不留空框
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
            // 刚建的空框、什么都没打:撤掉它,别留下一个看不见的空节点
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
        // 和 :text 同一条路:单个普通框里的 `<|new box|>` 会拆成几个框
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

    /// Ctrl+G:$EDITOR 直接改选中/光标框的文字,编辑器一退就写回去.
    /// 不弹底行输入框,所以没有"先出现在命令行、再按 Enter"那一步
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

    /// 底行输入框的提交:Enter 与"外部编辑器回来"走同一条路
    pub(super) fn submit_prompt(&mut self, what: Prompt, text: String) {
        match what {
            Prompt::NewBox => self.cmd_new(&text),
            Prompt::Text(ids) => self.cmd_set_text(&ids, &text),
            Prompt::Link(_) => self.cmd_link(&text),
        }
        // 注释框还没写字:删掉,不留下看不见的空框
        self.drop_note_draft();
    }

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

    /// 打开底行输入框,光标停在末尾
    pub(super) fn open_prompt(&mut self, what: Prompt, text: String) {
        self.view.prompt_pos = text.chars().count();
        self.view.prompt = Some((what, text));
    }

    /// 按未转义的标记切段.`\<|new box|>` 是字面量:不切,反斜杠也保留,
    /// 这样下次编辑还知道它是转义形式
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

    /// 文字里有没有未转义的分隔标记
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

    /// 用 `<|new box|>` 把一个框拆成多个:第一段留在原框,后面每段在右边新建.
    /// 返回 true 表示这次提交按拆分处理了(调用方不要再走普通的改文字)
    pub(super) fn split_text(&mut self, i: usize, text: &str) -> bool {
        if !text.contains(Self::SPLIT_MARK) && !text.contains(&format!("\\{}", Self::SPLIT_MARK)) {
            return false;
        }
        let has_unescaped = Self::has_unescaped_mark(text);
        let segs: Vec<String> = Self::split_segments(text)
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        // 只有一个普通框能拆:边注释框、点节点、多选都不动
        let ok = i < self.doc.boxes.len()
            && self.doc.boxes[i].edge.is_none()
            && !self.doc.boxes[i].style.is_point();
        if !ok {
            // 不能拆的框:标记当普通文字写进去
            return false;
        }
        if !has_unescaped {
            // 只有转义形式:按普通文字写回去
            return false;
        }
        if segs.len() < 2 {
            let note = self.view.lang.pick_owned(
                "没有可拆的内容(nothing to split)".to_string(),
                "nothing to split".to_string(),
            );
            self.view.note = Some(note);
            return true;
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
    /// `:relabel [all]`:把注释框重新贴回它那条边旁边(只跑一次,不跟手)。
    /// 不给参数就只动选中的注释框
    pub(super) fn cmd_relabel(&mut self, rest: &str) {
        let all = rest.trim().eq_ignore_ascii_case("all");
        let picked: Vec<usize> = if all {
            (0..self.doc.boxes.len())
                .filter(|&i| self.doc.is_note(i))
                .collect()
        } else {
            self.targets()
                .boxes
                .into_iter()
                .filter(|&i| self.doc.is_note(i))
                .collect()
        };
        if picked.is_empty() {
            let note = self.view.lang.pick_owned(
                "没有注释框可贴(选中注释框,或 :relabel all)".to_string(),
                "no note box to move (select one, or :relabel all)".to_string(),
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
            let at = self.doc.note_spot(e);
            self.doc.place_box(i, at);
        }
        self.doc.rebuild();
        self.end_edit();
        self.refresh_pick();
        self.view.note = Some(self.view.lang.pick_owned(
            format!("注释已重贴({n} 个)"),
            format!("moved {n} note box(es)"),
        ));
    }

    /// 提示行回车:改一批框的文字(整批一步撤销)
    pub(super) fn cmd_set_text(&mut self, ids: &[usize], text: &str) {
        if ids.is_empty() {
            return;
        }
        // 单个普通框 + 拆分标记:走拆分(多选不拆)
        if ids.len() == 1 && self.split_text(ids[0], text) {
            return;
        }
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
        self.view.note = Some(
            self.view
                .lang
                .pick_owned(format!("文字 = {name}{zh}"), format!("text = {name}{en}")),
        );
    }

    /// `i`:有框就在框里直接改字(不弹底行输入框),选的是边或空白就走原来的路
    pub(super) fn cmd_edit_inline(&mut self) {
        // 只编辑光标下的那一个框:多选/区块选择不改形成"批量写同一段文字"
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
            // 光标在边上:仍是"建注释框 + 输入框"
            self.cmd_edit_or_new();
            return;
        }
        // 空白处:先建一个空框,再直接进编辑(建框一步,提交文字另一步)
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
            // 预填现有文字:进来就能改,不用重打一遍
            let text = self.common_text(&set.boxes);
            self.open_prompt(Prompt::Text(set.boxes), text);
            return;
        }
        if !set.edges.is_empty() {
            // 选中边:给这条边建(或打开)注释框
            self.require_edit_scale();
            self.begin_edit("注释");
            let mut boxes = Vec::new();
            let mut fresh: Vec<String> = Vec::new();
            for &e in &set.edges {
                let had = self.doc.note_of_edge(&self.doc.edges[e].id).is_some();
                if let Some(b) = self.doc.new_note(e) {
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
            // 新建的空框记下来:Esc 或空提交时删掉
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
