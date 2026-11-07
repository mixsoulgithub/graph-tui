//! cmd 相关实现.函数体从 app.rs 原样搬来.

use super::*;

/// cmd_apply 分支的归属结果
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CmdFlow {
    NotMine,
    Handled,
    ReturnNow,
}

impl App {
    /// ":" 动词的名字(撤销回显用)
    pub(super) fn verb_label(cmd: &str) -> &'static str {
        match cmd {
            "box" => "框型",
            "nodecolor" => "边框色",
            "fill" => "填色",
            "textcolor" => "文字色",
            "textbg" => "文字底色",
            "edgebg" => "边底色",
            "textalign" => "对齐",
            "text" => "文字",
            "style" => "线型",
            "edgecolor" => "边色",
            "dir" => "方向",
            "corner" => "拐角",
            "head" => "箭头",
            "via" => "折点",
            "new" => "建框",
            "link" => "连边",
            "del" | "delete" => "删除",
            "tolayer" => "换层",
            _ => "修改",
        }
    }

    /// 修改图之前调用:不在 1:1 就先切到 1:1 并把视图居中到光标框.
    /// 返回 true 表示本来就已经 1:1(可以继续做这次修改);false 表示刚切过来,
    /// 本次修改不执行 -- 放大/缩小的状态下只允许查看.
    pub(super) fn require_edit_scale(&mut self) {
        if at_one_scale(&self.view) {
            return;
        }
        // 以光标为锚回到 1:1:光标在屏幕上的位置尽量不变(不再跳回"居中到框").
        // 光标本来就在视口外才退回居中,否则编辑目标又看不见了
        let p = render::panels(&self.doc, self.area, &self.view);
        let at = self.cursor_screen_cell();
        let inside = at.0 >= p.map.x as i32
            && at.0 < (p.map.x + p.map.width) as i32
            && at.1 >= p.map.y as i32
            && at.1 < (p.map.y + p.map.height) as i32;
        if inside {
            self.zoom_at(1.0, at);
            // zoom_at 会写回显,这里换成编辑相关的说明
            let note = self.view.lang.pick_owned(
                "编辑要在 1:1:已切回".to_string(),
                "editing needs 1:1: switched back".to_string(),
            );
            self.view.note = Some(note);
        } else {
            self.view.zoom = 1.0;
            self.view.target = None;
            self.view.pan = None;
            if let Some(i) = self.cursor_box() {
                self.view.center = Some(self.doc.center(i));
            }
            let note = self.view.lang.pick_owned(
                "编辑要在 1:1:已切回并居中".to_string(),
                "editing needs 1:1: switched back and centered".to_string(),
            );
            self.view.note = Some(note);
        }
    }

    /// ":" 命令行:layout <名>,side <倍数>,cycle <倍数|auto>,lang <zh|en>,
    /// layer / tolayer / export / export-canvas / w / save / e,
    /// 以及编辑动词 box / nodecolor / fill / textcolor / textalign / style / edgecolor / dir / corner / via / text
    pub fn cmd_apply(&mut self, line: &str) {
        let mut it = line.split_whitespace();
        let (cmd, arg) = (it.next().unwrap_or(""), it.next());
        // 改文档的动词先压一份撤销快照
        // 边动词(style/edgecolor/dir/corner/via)由 edit_edges 自己压一步,
        // 这里再包一层就会变成两步撤销
        let edits = matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "text" | "tolayer"
        );
        if edits && self.blocked_by_selection() {
            return;
        }
        if edits {
            self.begin_edit(Self::verb_label(cmd));
        }
        // 用菜单/命令改过样式的框也算"选中":新建框时照抄它的样式
        let styled_box = matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "textbg"
        );
        // 改元素的动词先看光标/选区有没有东西
        // (边的动词自己有"没选中边"的提示,这里只管框)
        if matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "text" | "tolayer"
        ) && self.targets().is_empty()
        {
            let note = self.view.lang.pick_owned(
                "光标下没有可编辑项".to_string(),
                "nothing under the cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        // 命令名之后的原文: text 与 via 要保留整行(空格也是内容)
        let rest = cmd_rest(line, cmd);
        match (cmd, arg) {
            ("q", _) | ("quit", _) => self.quit = true,
            ("undo", _) => self.undo_step(),
            ("redo", _) => self.redo_step(),
            ("history", _) => {
                let (p, f) = self.undo.counts();
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("可撤销 {p} 步,可重做 {f} 步"),
                    format!("{p} to undo, {f} to redo"),
                ));
            }
            ("", None) => self.view.note = None,
            _ => match self.cmd_dispatch(cmd, arg, rest) {
                CmdFlow::ReturnNow => return,
                CmdFlow::NotMine => self.note_unknown(line),
                CmdFlow::Handled => {}
            },
        }
        if styled_box {
            if let Some(&i) = self.targets().boxes.first() {
                self.remember_box_look(i);
            }
        }
        if edits {
            self.end_edit();
        }
    }

    pub(super) fn note_unknown(&mut self, line: &str) {
        let note = self.view.lang.pick_owned(
            format!(
                "未知命令:{line}(有 layout/side/cycle/lang/layer/tolayer/export/export-canvas,box/nodecolor/fill/textcolor/textalign/style/edgecolor/dir/corner/via/text,w/save/wq/e)"
            ),
            format!(
                "unknown command: {line} (try layout/side/cycle/lang/layer/tolayer/export/export-canvas, box/nodecolor/fill/textcolor/textalign/style/edgecolor/dir/corner/via/text, w/save/e)"
            ),
        );
        self.view.note = Some(note);
    }

    /// 依次试各动词族;NotMine 才试下一族
    pub(super) fn cmd_dispatch(&mut self, cmd: &str, arg: Option<&str>, rest: &str) -> CmdFlow {
        let mut flow = self.cmd_view_verbs(cmd, arg, rest);
        if matches!(flow, CmdFlow::NotMine) {
            flow = self.cmd_doc_verbs(cmd, arg, rest);
        }
        if matches!(flow, CmdFlow::NotMine) {
            flow = self.cmd_style_verbs(cmd, arg, rest);
        }
        if matches!(flow, CmdFlow::NotMine) {
            flow = self.cmd_ports_verbs(cmd, arg, rest);
        }
        if matches!(flow, CmdFlow::NotMine) {
            flow = self.cmd_io_verbs(cmd, arg, rest);
        }
        flow
    }

    /// ":" 输入行
    /// `:` 的补全候选:还没输参数就补动词,已经写了动词就补它的取值
    pub(super) fn completions(&self, line: &str) -> Vec<String> {
        // 还没打空格:这一整个词就是动词前缀
        if !line.contains(' ') {
            let low = line.to_lowercase();
            return VERBS
                .iter()
                .map(|v| v.to_string())
                .filter(|c| c.to_lowercase().starts_with(&low))
                .collect();
        }
        // 打了空格:参数是"动词之后的整行"(路径里可以有空格),不按空格切词.
        // 路径类动词按真实目录补,只有按 Tab 时才会扫盘
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let path_exts: Option<&[&str]> = match verb {
            "e" => Some(&["gtui", "dot", "canvas"]),
            "w" | "save" | "export" | "export-canvas" => Some(&[]),
            _ => None,
        };
        if let Some(exts) = path_exts {
            return paths::complete_path(rest, exts);
        }
        let prefix = rest.trim();
        let pick = |all: &[String]| -> Vec<String> {
            all.iter()
                .filter(|c| c.to_lowercase().starts_with(&prefix.to_lowercase()))
                .cloned()
                .collect()
        };
        let names = |doc: &Document| -> Vec<String> {
            let mut out = Vec::new();
            for b in &doc.boxes {
                out.push(b.id.clone());
                let label = label_of(doc, doc.box_idx(&b.id).unwrap_or(0));
                if !label.is_empty() && !out.contains(&label) {
                    out.push(label);
                }
            }
            out
        };
        let values: Vec<String> = match verb {
            "box" => ["point", "none", "single", "rounded", "double", "heavy"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "style" => ["solid", "dashed", "dotted"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "dir" => ["none", "forward", "back", "both"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "corner" => ["rounded", "sharp"].iter().map(|s| s.to_string()).collect(),
            "head" => ["auto", "ascii", "arrows", "triangles"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "textalign" => ["left", "center", "right"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "layout" => ["auto", "geo", "relax", "force", "layered", "tree", "chain"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "lang" => ["zh", "en"].iter().map(|s| s.to_string()).collect(),
            "notes" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
            "rotate" => ["auto", "0", "90", "180", "270"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "cycle" => vec!["auto".to_string()],
            "layer" => std::iter::once("all".to_string())
                .chain(self.doc.layer_names())
                .collect(),
            "tolayer" => self.doc.layer_names(),
            "link" => names(&self.doc),
            _ => Vec::new(),
        };
        pick(&values)
    }

    /// Tab:用下一/上一个候选替换当前这个词(第一次先把候选表定下来,之后在原地轮换)
    pub(super) fn complete_cmd(&mut self, back: bool) {
        let Some(line) = self.view.cmd.clone() else {
            return;
        };
        // `:w` / `:w ` 这类空参数按 Tab:把默认存盘路径整段填进去(还能接着改)
        if !back {
            for verb in ["w", "wq", "save"] {
                if line != verb && line != format!("{verb} ") {
                    continue;
                }
                let Some(default) = self.default_save_path() else {
                    self.view.note = Some(self.view.lang.pick_owned(
                        "没有默认路径:敲一个".to_string(),
                        "no default path; type one".to_string(),
                    ));
                    return;
                };
                self.view.cmd = Some(format!("{verb} {}", paths::absolute_display(&default)));
                self.view.cmd_pos = self
                    .view
                    .cmd
                    .as_ref()
                    .map(|c| c.chars().count())
                    .unwrap_or(0);
                self.view.cmd_menu = None;
                self.refresh_cmd_hint();
                return;
            }
        }
        if self.view.cmd_menu.is_none() {
            let items = self.completions(&line);
            if items.is_empty() {
                return;
            }
            self.view.cmd_menu = Some((items, 0));
        }
        let Some((items, at)) = self.view.cmd_menu.clone() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let n = items.len();
        // at 是"下一个要插的";往回退就是回到当前这个的再前一个
        let idx = if back {
            if at == 0 {
                n - 1
            } else {
                (at + n - 2) % n
            }
        } else {
            at % n
        };
        let chosen = items[idx].clone();
        // 参数是"动词之后的整行"(路径里可以有空格),所以整段替掉,
        // 不能按最后一个空格切
        let new_line = match line.split_once(' ') {
            Some((verb, _)) => format!("{verb} {chosen}"),
            None => chosen.clone(),
        };
        self.view.cmd_pos = new_line.chars().count();
        self.view.cmd = Some(new_line);
        // 参数里可能带空格(文件名),补完就不用再加空格
        self.view.cmd_menu = Some((items, (idx + 1) % n));
        self.refresh_cmd_hint();
    }

    pub(super) fn handle_cmd(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.cmd {
            Some(t) => {
                let n = t.chars().count();
                (n, self.view.cmd_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        // 改过内容才要重算路径提示 / 关掉补全菜单;纯移动光标不用
        let mut touched = false;
        match code {
            // Ctrl+G 只给文字输入用;命令行是单行语义,按了说一声
            KeyCode::Char('g') if ctrl => {
                let note = self.view.lang.pick_owned(
                    "Ctrl+G 只用于文字输入(框文字 / 新建节点)".to_string(),
                    "Ctrl+G only works for text input (box text / new node)".to_string(),
                );
                self.view.note = Some(note);
            }
            KeyCode::Esc => {
                self.view.cmd = None;
                self.view.cmd_menu = None;
                self.view.cmd_hint = None;
                self.view.cmd_pos = 0;
            }
            KeyCode::Enter => {
                self.view.cmd_menu = None;
                self.view.cmd_hint = None;
                self.view.cmd_pos = 0;
                let line = self.view.cmd.take().unwrap_or_default();
                self.cmd_apply(&line);
            }
            KeyCode::Tab => self.complete_cmd(false),
            KeyCode::BackTab => self.complete_cmd(true),
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(要排在"一字"前面)
            KeyCode::Left if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let from = word_left(b, pos);
                    let (a, e) = (char_byte(b, from), char_byte(b, pos));
                    b.replace_range(a..e, "");
                    want = Some(from);
                }
                touched = true;
            }
            // 删
            KeyCode::Backspace => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let mut p = pos;
                    edit_backspace(b, &mut p);
                    want = Some(p);
                }
                touched = true;
            }
            KeyCode::Delete => {
                if let Some(b) = self.view.cmd.as_mut() {
                    edit_delete(b, pos);
                }
                touched = true;
            }
            KeyCode::Char('d') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    edit_delete(b, pos);
                }
                touched = true;
            }
            KeyCode::Char('k') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let at = char_byte(b, pos);
                    b.truncate(at);
                }
                want = Some(pos);
                touched = true;
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let at = char_byte(b, pos);
                    b.replace_range(..at, "");
                }
                want = Some(0);
                touched = true;
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let from = word_left(b, pos);
                    let (a, e) = (char_byte(b, from), char_byte(b, pos));
                    b.replace_range(a..e, "");
                    want = Some(from);
                }
                touched = true;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let mut p = pos;
                    edit_insert(b, &mut p, &c.to_string());
                    want = Some(p);
                }
                touched = true;
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.cmd_pos = p;
        }
        if touched {
            self.view.cmd_menu = None;
            self.refresh_cmd_hint();
        }
    }

    pub(super) fn refresh_cmd_hint(&mut self) {
        let hint = match self.view.cmd.as_deref() {
            Some(line) => self.cmd_hint_for(line),
            None => None,
        };
        self.view.cmd_hint = hint;
    }

    pub(super) fn cmd_hint_for(&self, line: &str) -> Option<String> {
        let (verb, arg) = match line.split_once(' ') {
            Some((v, rest)) => (v, rest.trim()),
            None => (line, ""),
        };
        match verb {
            // 存盘:参数空就显示"不写路径会存到哪"
            "w" | "wq" | "save" => {
                if !arg.is_empty() {
                    return Some(paths::absolute_display(arg));
                }
                let Some(default) = self.default_save_path() else {
                    return Some(self.view.lang.pick_owned(
                        "还没有文件名:敲个路径".to_string(),
                        "no file name yet; type a path".to_string(),
                    ));
                };
                let abs = paths::absolute_display(&default);
                let src = self.source.clone().unwrap_or_default();
                let text = if src.to_ascii_lowercase().ends_with(".gtui") {
                    self.view.lang.pick_owned(
                        format!("{abs}  (覆盖原文件)"),
                        format!("{abs}  (overwrite)"),
                    )
                } else {
                    let ext = src
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_string())
                        .unwrap_or_default();
                    self.view.lang.pick_owned(
                        format!("{abs}  (源文件 .{ext} 不动)"),
                        format!("{abs}  (source .{ext} stays untouched)"),
                    )
                };
                Some(text)
            }
            "e" => {
                if arg.is_empty() {
                    return Some(self.view.lang.pick_owned(
                        "回车:文件列表(当前目录 + 源目录);Tab:补路径".to_string(),
                        "Enter: file list (cwd + source dir); Tab: complete a path".to_string(),
                    ));
                }
                Some(self.view.lang.pick_owned(
                    format!("打开 {}", paths::absolute_display(arg)),
                    format!("open {}", paths::absolute_display(arg)),
                ))
            }
            "e!" => Some(self.view.lang.pick_owned(
                if arg.is_empty() {
                    "强制重新载入(丢弃本地改动)".to_string()
                } else {
                    format!("强制打开 {}", paths::absolute_display(arg))
                },
                if arg.is_empty() {
                    "force reload (drops local edits)".to_string()
                } else {
                    format!("force open {}", paths::absolute_display(arg))
                },
            )),
            "export" | "export-canvas" => {
                if arg.is_empty() {
                    return Some(
                        self.view
                            .lang
                            .pick_owned("需要一个路径".to_string(), "needs a path".to_string()),
                    );
                }
                Some(self.view.lang.pick_owned(
                    format!("导出 {}", paths::absolute_display(arg)),
                    format!("export {}", paths::absolute_display(arg)),
                ))
            }
            _ => None,
        }
    }
}
