//! input 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 一个事件
    pub fn handle(&mut self, ev: Event) -> Flow {
        if self.quit {
            return Flow::Quit;
        }
        if self.view.edit.is_some() {
            match &ev {
                // 编辑时点一下鼠标:先提交,再按平常处理这次点击
                Event::Mouse(_) => self.commit_box_edit(false),
                Event::Key(KeyEvent {
                    code, modifiers, ..
                }) => {
                    self.handle_edit_key(*code, *modifiers);
                    return Flow::Continue;
                }
                _ => return Flow::Continue,
            }
        }
        if let Event::Mouse(me) = &ev {
            if !self.mouse {
                return Flow::Continue;
            }
            self.sync_hit();
            self.handle_mouse(*me);
            return Flow::Continue;
        }
        self.sync_hit();
        let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = ev
        else {
            return Flow::Continue;
        };
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
            self.note_quit_hint();
            return Flow::Continue;
        }
        // 打开文件的选择浮层:它优先吃掉所有按键
        if self.view.file_pick.is_some() {
            self.handle_pick(code);
            return Flow::Continue;
        }
        // 速查浮层:任意键关掉
        if self.view.help {
            self.view.help = false;
            return Flow::Continue;
        }
        // 选"沿线走一站"走哪条分组:数字直选,j k 挪,回车走,esc 取消
        // 右键菜单:上下选、回车进/执行、esc 退一层
        if self.view.menu.is_some() {
            self.handle_menu(code);
            return Flow::Continue;
        }
        // 底行单行输入:所有按键都归它
        if self.view.prompt.is_some() {
            self.handle_prompt(code, modifiers);
            return Flow::Continue;
        }
        // ":" 命令行:所有按键都归输入行
        if self.view.cmd.is_some() {
            self.handle_cmd(code, modifiers);
            // :q 在这一下里敲完的,立刻交出去,不用等下一个事件
            return if self.quit {
                Flow::Quit
            } else {
                Flow::Continue
            };
        }
        // 搜索态:所有按键都归输入行
        if self.view.query.is_some() {
            self.handle_search(code, modifiers);
            return Flow::Continue;
        }
        // 连边模式:Esc 取消、Enter 建边、再按 e 只提示
        if self.view.link.is_some()
            && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match code {
                KeyCode::Esc => {
                    self.cancel_link();
                    return Flow::Continue;
                }
                KeyCode::Enter => {
                    self.link_commit();
                    return Flow::Continue;
                }
                KeyCode::Char('e') => {
                    let note = self.view.lang.pick_owned(
                        "连边中:Enter 创建,Esc 取消".to_string(),
                        "linking: Enter to create, Esc to cancel".to_string(),
                    );
                    self.view.note = Some(note);
                    return Flow::Continue;
                }
                _ => {}
            }
        }
        // 非字符键(方向、Esc 等)把没走完的序列作废
        if !matches!(code, KeyCode::Char(_)) {
            self.clear_seq();
        }
        // 数字前缀(5l)、坐标跳转(100,100G)、gg:先吃掉,序列挂在右下角
        if let KeyCode::Char(c) = code {
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && self.seq_key(c) {
                return Flow::Continue;
            }
        }
        // 剩下的都归键位表:没绑的键(含没认领的 Ctrl/Alt 组合)什么都不做,只清计数
        let Some(action) = self.keys.get(code, modifiers) else {
            self.pending = None;
            return Flow::Continue;
        };
        let dir = |a: crate::keys::Action| -> (i32, i32) {
            use crate::keys::Action as K;
            match a {
                K::MoveLeft | K::NudgeLeft => (-1, 0),
                K::MoveRight | K::NudgeRight => (1, 0),
                K::MoveUp | K::NudgeUp => (0, -1),
                _ => (0, 1),
            }
        };
        use crate::keys::Action;
        match action {
            // esc:先退邻居高亮,再退选区
            Action::Escape => {
                if self.view.ego.is_some() {
                    self.view.ego = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("退出邻居高亮".to_string(), "neighbour view off".to_string());
                    self.view.note = Some(note);
                } else if self.view.sel.is_some() {
                    self.view.sel = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("取消选择".to_string(), "selection off".to_string());
                    self.view.note = Some(note);
                }
            }
            Action::Help => self.view.help = true,
            // z / 空格:把光标那个框挪到画面正中
            Action::Center => self.center_cursor(),
            // f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
            Action::Fit => {
                self.fit_focus();
                self.note_zoom();
            }
            // F:把整幅图铺满画面(不设中心 = 回到整幅图的中心)
            Action::FitAll => {
                let zoom = render::zoom_for_fit(
                    &self.doc,
                    self.area.width,
                    self.area.height,
                    self.view.notes,
                );
                self.view.zoom = clamp_zoom(zoom);
                self.view.center = None;
                self.view.target = None;
                self.view.pan = None;
                self.note_zoom();
            }
            Action::Link => self.begin_link(),
            Action::BlockSelect => self.start_rect_select(),
            Action::CycleLayer => self.cycle_layer(),
            Action::EgoToggle => self.toggle_ego(),
            Action::ToggleNotes => self.cmd_notes(None),
            Action::CycleEdge => self.cycle_edge(),
            // 半屏跳:远距离旅行用
            Action::LeapDown => {
                let next = self.leap((0, 1));
                self.jump_to(next);
            }
            Action::LeapUp => {
                let next = self.leap((0, -1));
                self.jump_to(next);
            }
            Action::JumpBack => {
                self.jump_history(true);
            }
            Action::JumpForward => {
                self.jump_history(false);
            }
            // 移动光标:带计数就是走 N 格
            Action::MoveLeft | Action::MoveRight | Action::MoveUp | Action::MoveDown => {
                let steps = self.pending.take().unwrap_or(1);
                let d = dir(action);
                self.move_cursor(d, steps);
            }
            // 挪框:先把选中(或光标)的框动起来
            Action::NudgeLeft | Action::NudgeRight | Action::NudgeUp | Action::NudgeDown => {
                let steps = self.pending.take().unwrap_or(1).max(1) as i32;
                let d = dir(action);
                self.move_boxes(d, steps);
            }
            Action::ZoomIn => {
                let at = self.cursor_screen_cell();
                self.zoom_at(self.view.zoom * ZOOM_STEP, at);
            }
            Action::ZoomOut => {
                let at = self.cursor_screen_cell();
                self.zoom_at(self.view.zoom / ZOOM_STEP, at);
            }
            Action::ZoomReset => {
                // 只把缩放拨回 1:1,不动画面:要回原点用 gg / {x},{y}G
                self.view.zoom = 1.0;
                self.view.target = None;
                self.view.pan = None;
                self.note_zoom();
            }
            Action::ToggleLabels => {
                self.view.labels = !self.view.labels;
                let on = self.view.labels;
                let note = if on {
                    self.view
                        .lang
                        .pick_owned("标节点名:开".to_string(), "labels: on".to_string())
                } else {
                    self.view
                        .lang
                        .pick_owned("标节点名:关".to_string(), "labels: off".to_string())
                };
                self.view.note = Some(note);
            }
            Action::Search => {
                self.search_from = self.view.cursor;
                self.search_focus = self.view.focus.clone();
                self.view.query = Some(String::new());
                self.view.query_pos = 0;
                self.view.hits.clear();
            }
            // n / N:下一个 / 上一个命中的框
            Action::NextHit => {
                if !self.view.hits.is_empty() {
                    self.view.hit_at = (self.view.hit_at + 1) % self.view.hits.len();
                    let next = self.view.hits[self.view.hit_at];
                    self.jump_to(next);
                }
            }
            Action::PrevHit => {
                if !self.view.hits.is_empty() {
                    self.view.hit_at =
                        (self.view.hit_at + self.view.hits.len() - 1) % self.view.hits.len();
                    let next = self.view.hits[self.view.hit_at];
                    self.jump_to(next);
                }
            }
            // r:四套布局轮着换
            Action::CycleLayout => self.cycle_layout(),
            Action::Undo => self.undo_step(),
            Action::Redo => self.redo_step(),
            // d:带边剪切;y:只复制不删;p:粘贴
            Action::Cut => self.cmd_del(true),
            Action::Yank => self.cmd_yank(),
            Action::SplitBox => self.cmd_split_box(),
            Action::MergeBoxes => self.cmd_merge_boxes(),
            Action::EditOrNew => self.cmd_edit_inline(),
            // w / b:朝前 / 朝后射一条线,光标落到第一个撞到的框或边
            Action::JumpNext => self.jump_ray(false),
            Action::JumpPrev => self.jump_ray(true),
            Action::Paste => self.cmd_paste(),
            // Enter:光标下有链接就打开;否则确认选区(没有选区时什么都不做)
            Action::OpenLink => self.open_link(),
            Action::ConfirmSelection => self.confirm_selection(),
            // Ctrl+G:直接拿外部编辑器改光标框的文字,退出即生效(不经过底行)
            Action::EditTextExternal => self.edit_text_external(),
            // c:颜色菜单(框是边框色,边是边颜色);连边走 e 或 :link
            Action::ColorMenu => self.open_color_menu(),
            // s:完整样式菜单
            Action::StyleMenu => self.open_style_menu(),
            Action::Command => {
                self.view.cmd = Some(String::new());
                self.view.cmd_pos = 0;
                self.view.note = None;
            }
            // 退出只认 :q(和 vim 一样):q 提示一句
            Action::QuitHint => self.note_quit_hint(),
            Action::Reload => self.reload_step(),
            Action::TabNew => {
                self.new_tab(None);
            }
            Action::TabNext => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + 1) % n);
            }
            Action::TabPrev => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + n - 1) % n);
            }
            Action::TabClose => {
                self.close_tab(false);
            }
        }
        Flow::Continue
    }

    /// 编辑态下的按键:只动草稿,不动文档
    pub(super) fn handle_edit_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let Some(mut edit) = self.view.edit.take() else {
            return;
        };
        let len = edit.text.chars().count();
        let mut keep = true;
        match code {
            KeyCode::Esc => {
                self.view.edit = Some(edit);
                self.commit_box_edit(false);
                return;
            }
            KeyCode::Char('c') if ctrl => {
                self.view.edit = Some(edit);
                self.commit_box_edit(true);
                return;
            }
            KeyCode::Enter => {
                edit.text
                    .insert(Self::byte_of(&edit.text, edit.caret), '\n');
                edit.caret += 1;
            }
            // Alt+退格:删掉光标前的一个词(和命令行 / 搜索栏一样)
            KeyCode::Backspace if alt => {
                let from = word_left(&edit.text, edit.caret);
                let (a, b) = (
                    Self::byte_of(&edit.text, from),
                    Self::byte_of(&edit.text, edit.caret),
                );
                edit.text.replace_range(a..b, "");
                edit.caret = from;
            }
            KeyCode::Backspace => {
                if edit.caret > 0 {
                    let at = Self::byte_of(&edit.text, edit.caret - 1);
                    edit.text.remove(at);
                    edit.caret -= 1;
                }
            }
            KeyCode::Delete => {
                if edit.caret < len {
                    let at = Self::byte_of(&edit.text, edit.caret);
                    edit.text.remove(at);
                }
            }
            KeyCode::Home => edit.caret = Self::line_start(&edit.text, edit.caret),
            KeyCode::End => edit.caret = Self::line_end(&edit.text, edit.caret),
            KeyCode::Left if alt => edit.caret = word_left(&edit.text, edit.caret),
            KeyCode::Right if alt => edit.caret = word_right(&edit.text, edit.caret),
            KeyCode::Left => edit.caret = edit.caret.saturating_sub(1),
            KeyCode::Right => edit.caret = (edit.caret + 1).min(len),
            // 上下按"列"走,超出行尾就停在行尾
            KeyCode::Up => edit.caret = Self::caret_vertical(&edit.text, edit.caret, -1),
            KeyCode::Down => edit.caret = Self::caret_vertical(&edit.text, edit.caret, 1),
            KeyCode::Char(c) if !ctrl && !alt => {
                edit.text.insert(Self::byte_of(&edit.text, edit.caret), c);
                edit.caret += 1;
            }
            _ => keep = false,
        }
        if keep {
            self.view.edit = Some(edit);
            self.sync_edit_cursor();
        } else {
            self.view.edit = Some(edit);
        }
    }

    /// 底行提示:回车执行,esc 取消,其余按键按行编辑规则改缓冲.
    /// 光标在 `view.prompt_pos`(按字符数),支持 emacs 基本键和按词跳
    pub(super) fn handle_prompt(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.prompt {
            Some((_, t)) => {
                let n = t.chars().count();
                (n, self.view.prompt_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        match code {
            KeyCode::Esc => {
                self.view.prompt = None;
                self.drop_note_draft();
            }
            KeyCode::Enter => {
                let Some((what, text)) = self.view.prompt.take() else {
                    return;
                };
                self.view.prompt_pos = 0;
                self.submit_prompt(what, text);
            }
            // Ctrl+G:整串交给 $VISUAL / $EDITOR(退出后读回来替换)
            KeyCode::Char('g') if ctrl => {
                if let Some((what, buf)) = &self.view.prompt {
                    self.edit_request = Some((EditTarget::Prompt(what.clone()), buf.clone()));
                }
            }
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(Alt-左右 / Alt-B / Alt-F),要排在"一字"前面
            KeyCode::Left if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_left(t, pos))
            }
            KeyCode::Right if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_right(t, pos))
            }
            KeyCode::Char('b') if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_left(t, pos))
            }
            KeyCode::Char('f') if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_right(t, pos))
            }
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let from = word_left(t, pos);
                    let (a, b) = (char_byte(t, from), char_byte(t, pos));
                    t.replace_range(a..b, "");
                    want = Some(from);
                }
            }
            // 删
            KeyCode::Backspace => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let mut p = pos;
                    edit_backspace(t, &mut p);
                    want = Some(p);
                }
            }
            KeyCode::Delete => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    edit_delete(t, pos);
                }
            }
            KeyCode::Char('d') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    edit_delete(t, pos);
                }
            }
            KeyCode::Char('k') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let b = char_byte(t, pos);
                    t.truncate(b);
                }
                want = Some(pos);
            }
            KeyCode::Char('u') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let b = char_byte(t, pos);
                    t.replace_range(..b, "");
                }
                want = Some(0);
            }
            KeyCode::Char('w') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let from = word_left(t, pos);
                    let (a, b) = (char_byte(t, from), char_byte(t, pos));
                    t.replace_range(a..b, "");
                    want = Some(from);
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some((_, b)) = self.view.prompt.as_mut() {
                    let mut p = pos;
                    edit_insert(b, &mut p, &c.to_string());
                    want = Some(p);
                }
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.prompt_pos = p;
        }
    }

    /// 搜索输入行:每敲一下都重算命中
    pub(super) fn handle_search(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.query {
            Some(t) => {
                let n = t.chars().count();
                (n, self.view.query_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        let mut touched = false;
        match code {
            // Ctrl+G 只给文字输入用;搜索词是单行
            KeyCode::Char('g') if ctrl => {
                let note = self.view.lang.pick_owned(
                    "Ctrl+G 只用于文字输入(框文字 / 新建节点)".to_string(),
                    "Ctrl+G only works for text input (box text / new node)".to_string(),
                );
                self.view.note = Some(note);
            }
            // esc:连人带命中一起退回去
            KeyCode::Esc => {
                self.view.query = None;
                self.view.query_pos = 0;
                self.view.hits.clear();
                self.view.focus = self.search_focus.clone();
                self.view.cursor = self.search_from;
            }
            // 回车:命中留着,之后可以按 n 一个个跳
            KeyCode::Enter => {
                self.view.query = None;
                self.view.query_pos = 0;
                // 搜索跳过去也算一次跳转:n / Tab 之外,Ctrl-o 也能退回来
                self.refresh_pick();
                self.note_here();
            }
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(排在"一字"前面)
            KeyCode::Left if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.query.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => {
                want = self.view.query.as_ref().map(|t| word_right(t, pos))
            }
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some(q) = self.view.query.as_mut() {
                    let from = word_left(q, pos);
                    let (a, b) = (char_byte(q, from), char_byte(q, pos));
                    q.replace_range(a..b, "");
                    want = Some(from);
                }
                touched = true;
            }
            // 删
            KeyCode::Backspace => {
                if let Some(q) = self.view.query.as_mut() {
                    let mut p = pos;
                    edit_backspace(q, &mut p);
                    want = Some(p);
                }
                touched = true;
            }
            KeyCode::Delete => {
                if let Some(q) = self.view.query.as_mut() {
                    edit_delete(q, pos);
                }
                touched = true;
            }
            KeyCode::Char('d') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    edit_delete(q, pos);
                }
                touched = true;
            }
            KeyCode::Char('k') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let at = char_byte(q, pos);
                    q.truncate(at);
                }
                want = Some(pos);
                touched = true;
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let at = char_byte(q, pos);
                    q.replace_range(..at, "");
                }
                want = Some(0);
                touched = true;
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let from = word_left(q, pos);
                    let (a, b) = (char_byte(q, from), char_byte(q, pos));
                    q.replace_range(a..b, "");
                    want = Some(from);
                }
                touched = true;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(q) = self.view.query.as_mut() {
                    let mut p = pos;
                    edit_insert(q, &mut p, &c.to_string());
                    want = Some(p);
                }
                touched = true;
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.query_pos = p;
        }
        if touched {
            refresh(&self.doc, &mut self.view);
        }
    }
}
