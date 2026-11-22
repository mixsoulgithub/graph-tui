//! Command implementations: the `:` command line, completion, and verb dispatch.

use super::*;

/// Ownership result of one `cmd_apply` branch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CmdFlow {
    NotMine,
    Handled,
    ReturnNow,
}

impl App {
    /// Display name of a `:` verb (used in the undo echo)
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

    /// Called before changing the graph: if not at 1:1, switch to 1:1 first.
    /// Returns true when it was already 1:1 (the caller may go ahead with the change);
    /// false when it just switched, meaning this change is skipped -- at any other zoom
    /// level the picture is read-only.
    pub(super) fn require_edit_scale(&mut self) {
        if at_one_scale(&self.view) {
            return;
        }
        // Back to 1:1 anchored at the cursor: keep its screen position where it is (rather
        // than jumping back to "centred on the box"). Fall back to centring only when the
        // cursor was outside the viewport, or the edit target would be off-screen again
        let p = render::panels(&self.doc, self.area, &self.view);
        let at = self.cursor_screen_cell();
        let inside = at.0 >= p.map.x as i32
            && at.0 < (p.map.x + p.map.width) as i32
            && at.1 >= p.map.y as i32
            && at.1 < (p.map.y + p.map.height) as i32;
        if inside {
            self.zoom_at(1.0, at);
            // zoom_at writes its own note; replace it with an edit-specific one
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

    /// The `:` command line: layout <name>, side <factor>, cycle <factor|auto>,
    /// lang <zh|en>, layer / tolayer / export / export-canvas / w / save / e,
    /// plus the edit verbs box / nodecolor / fill / textcolor / textalign / style /
    /// edgecolor / dir / corner / via / text
    pub fn cmd_apply(&mut self, line: &str) {
        let mut it = line.split_whitespace();
        let (cmd, arg) = (it.next().unwrap_or(""), it.next());
        // Document verbs push an undo snapshot up front. Edge verbs
        // (style/edgecolor/dir/corner/via) push their own step inside edit_edges, so
        // wrapping them here would make two undo steps
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
        // A box styled by menu or command also counts as "selected": new boxes copy its style
        let styled_box = matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "textbg"
        );
        // Verbs that change elements need something under the cursor/selection.
        // (edge verbs report "no edge selected" themselves; this covers boxes only)
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
        // Raw text after the command name: text and via keep the whole line (spaces count)
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

    /// Try each verb family in turn; only NotMine moves on to the next
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

    /// Completion candidates for a `:` line: complete the verb while no argument is
    /// typed, otherwise complete that verb's values
    pub(super) fn completions(&self, line: &str) -> Vec<String> {
        // No space yet: the whole word is a verb prefix
        if !line.contains(' ') {
            let low = line.to_lowercase();
            return VERBS
                .iter()
                .map(|v| v.to_string())
                .filter(|c| c.to_lowercase().starts_with(&low))
                .collect();
        }
        // A space is present: the argument is "the whole line after the verb" (paths may
        // contain spaces), so do not split on spaces.
        // Path verbs complete against real directories, and only Tab scans the disk
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
            "edge-labels" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
            "node-labels" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
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

    /// Tab: replace the current word with the next/previous candidate (the list is fixed
    /// on the first Tab, then cycles in place)
    pub(super) fn complete_cmd(&mut self, back: bool) {
        let Some(line) = self.view.cmd.clone() else {
            return;
        };
        // Empty argument on `:w` / `:w ` plus Tab: fill in the whole default save path
        // (still editable)
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
        // `at` is "the next one to insert"; going back lands on the one before the current
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
        // The argument is "the whole line after the verb" (paths may contain spaces), so
        // replace it as a unit rather than splitting at the last space
        let new_line = match line.split_once(' ') {
            Some((verb, _)) => format!("{verb} {chosen}"),
            None => chosen.clone(),
        };
        self.view.cmd_pos = new_line.chars().count();
        self.view.cmd = Some(new_line);
        // The argument may contain spaces (file names), so no trailing space is added after
        // completion
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
        // Only a content change needs a fresh path hint / closing the completion menu; plain
        // cursor moves do not
        let mut touched = false;
        match code {
            // Ctrl+G is for text input only; the command line is single-line, so just say so
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
            // start / end of line
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // one word (must come before the single-character arms)
            KeyCode::Left if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            // one character
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+Backspace: delete the previous word (same as Ctrl-W)
            KeyCode::Backspace if alt => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let from = word_left(b, pos);
                    let (a, e) = (char_byte(b, from), char_byte(b, pos));
                    b.replace_range(a..e, "");
                    want = Some(from);
                }
                touched = true;
            }
            // delete
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
            // save: with an empty argument, show where the file would go
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
