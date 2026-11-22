//! Input handling: event dispatch and the line-editing key tables.

use super::*;

impl App {
    /// One event.
    pub fn handle(&mut self, ev: Event) -> Flow {
        if self.quit {
            return Flow::Quit;
        }
        if self.view.edit.is_some() {
            match &ev {
                // Clicking while editing: commit first, then treat the click normally
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
        // File picker overlay: it swallows every key first
        if self.view.file_pick.is_some() {
            self.handle_pick(code);
            return Flow::Continue;
        }
        // Cheat-sheet overlay: any key closes it
        if self.view.help {
            self.view.help = false;
            return Flow::Continue;
        }
        // Group choice when walking one stop along a line: digits pick, j/k move,
        // Enter accepts, esc cancels. Context menu: up/down, Enter opens/runs, esc backs out.
        if self.view.menu.is_some() {
            self.handle_menu(code);
            return Flow::Continue;
        }
        // Bottom line single-line input: it owns every key
        if self.view.prompt.is_some() {
            self.handle_prompt(code, modifiers);
            return Flow::Continue;
        }
        // ":" command line: the input line owns every key
        if self.view.cmd.is_some() {
            self.handle_cmd(code, modifiers);
            // :q completed here hands over immediately, no need to wait for the next event
            return if self.quit {
                Flow::Quit
            } else {
                Flow::Continue
            };
        }
        // Search mode: the input line owns every key
        if self.view.query.is_some() {
            self.handle_search(code, modifiers);
            return Flow::Continue;
        }
        // Link mode: Esc cancels, Enter creates, pressing e again only prints a hint
        if self.view.link.is_some()
            && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match code {
                KeyCode::Esc => {
                    self.cancel_link();
                    return Flow::Continue;
                }
                // Enter and space both confirm the link; space is the easiest key to reach
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.link_commit();
                    return Flow::Continue;
                }
                KeyCode::Char('e') => {
                    let note = self.view.lang.pick_owned(
                        "连边中:Enter 或空格创建,Esc 取消".to_string(),
                        "linking: Enter or space to create, Esc to cancel".to_string(),
                    );
                    self.view.note = Some(note);
                    return Flow::Continue;
                }
                _ => {}
            }
        }
        // Non-character keys (arrows, Esc, ...) drop an unfinished sequence
        if !matches!(code, KeyCode::Char(_)) {
            self.clear_seq();
        }
        // Numeric prefix (5l), coordinate jump (100,100G), gg: eaten here, shown bottom right
        if let KeyCode::Char(c) = code {
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && self.seq_key(c) {
                return Flow::Continue;
            }
        }
        // Everything else goes through the key table: unbound keys just reset the count
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
            // esc: leave isolate first, then the selection
            Action::Escape => {
                if self.view.ego.is_some() {
                    self.view.ego = None;
                    self.view.ego_id = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("退出隔离".to_string(), "isolate off".to_string());
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
            // Opening help advances to the next tip so a session does not repeat them
            Action::Help => {
                self.next_tip();
                self.view.help = true;
            }
            // z: bring the box under the cursor to the centre of the picture
            Action::Center => self.center_cursor(),
            // f: fit the focused group (or the cursor's group) into the picture
            Action::Fit => {
                self.fit_focus();
                self.note_zoom();
            }
            // F: fit the whole picture (no centre = back to the picture's centre)
            Action::FitAll => {
                self.fit_all();
                self.view.pan = None;
                self.note_zoom();
            }
            Action::Link => self.begin_link(),
            Action::BlockSelect => self.start_rect_select(),
            Action::CycleLayer => self.cycle_layer(),
            Action::EgoToggle => self.toggle_ego(),
            Action::ToggleEdgeLabels => self.cmd_edge_labels(None),
            Action::CycleEdge => self.cycle_edge(),
            // Half-page jump: for long trips
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
            // Move the cursor: with a count, walk N cells
            Action::MoveLeft | Action::MoveRight | Action::MoveUp | Action::MoveDown => {
                let steps = self.pending.take().unwrap_or(1);
                let d = dir(action);
                self.move_cursor(d, steps);
            }
            // Nudge boxes: move the selected (or cursor) boxes
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
                // Only reset zoom to 1:1, not the view: gg / {x},{y}G recentres
                self.view.zoom = 1.0;
                self.view.target = None;
                self.view.pan = None;
                self.note_zoom();
            }
            Action::ToggleNodeLabels => {
                self.view.node_labels = !self.view.node_labels;
                let on = self.view.node_labels;
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
            // n / N: next / previous matching box (skips hidden ones under isolate)
            Action::NextHit => {
                let vis = self.visible_hits();
                if let Some(next) = self.step_hit(&vis, 1) {
                    self.jump_to_hit(next);
                }
            }
            Action::PrevHit => {
                let vis = self.visible_hits();
                if let Some(next) = self.step_hit(&vis, -1) {
                    self.jump_to_hit(next);
                }
            }
            // r: cycle through the four layouts
            Action::CycleLayout => self.cycle_layout(),
            Action::Undo => self.undo_step(),
            Action::Redo => self.redo_step(),
            // d: cut with edges; y: copy only; p: paste
            Action::Cut => self.cmd_del(true),
            Action::Yank => self.cmd_yank(),
            Action::SplitBox => self.cmd_split_box(),
            Action::MergeBoxes => self.cmd_merge_boxes(),
            Action::EditOrNew => self.cmd_edit_inline(),
            // w / b: shoot a ray forwards / backwards, land on the first box or edge hit
            Action::JumpNext => self.jump_ray(false),
            Action::JumpPrev => self.jump_ray(true),
            Action::Paste => self.cmd_paste(),
            // Enter: open a link under the cursor, else confirm the selection
            Action::OpenLink => self.open_link(),
            Action::ConfirmSelection => self.confirm_selection(),
            // Ctrl+G: edit the cursor box's text in an external editor; applies on exit
            Action::EditTextExternal => self.edit_text_external(),
            // c: colour menu (border colour for boxes, line colour for edges); link via e
            Action::ColorMenu => self.open_color_menu(),
            // s: full style menu
            Action::StyleMenu => self.open_style_menu(),
            Action::Command => {
                self.view.cmd = Some(String::new());
                self.view.cmd_pos = 0;
                self.view.note = None;
            }
            // Only :q quits (as in vim): q just prints a hint
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

    /// Keys in edit mode: touch the draft only, never the document
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
            // Alt+Backspace: delete the previous word (same as the command line/search)
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
            // Up/down move by column and stop at the end of a shorter line
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

    /// Prompt line: Enter runs, esc cancels, other keys edit the buffer as a line.
    /// Cursor lives at `view.prompt_pos` (in chars); basic emacs keys and word jumps.
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
            // Ctrl+G: hand the whole string to $VISUAL / $EDITOR (read back on exit)
            KeyCode::Char('g') if ctrl => {
                if let Some((what, buf)) = &self.view.prompt {
                    self.edit_request = Some((EditTarget::Prompt(what.clone()), buf.clone()));
                }
            }
            // Start / end of line
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // Word (Alt-Left/Right / Alt-B / Alt-F); must precede the single-char cases
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
            // Single char
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+Backspace: delete the previous word (same as Ctrl-W)
            KeyCode::Backspace if alt => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let from = word_left(t, pos);
                    let (a, b) = (char_byte(t, from), char_byte(t, pos));
                    t.replace_range(a..b, "");
                    want = Some(from);
                }
            }
            // Delete
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

    /// Search input line: every keystroke recomputes the hits
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
            // Ctrl+G is for text input only; a search term is a single line
            KeyCode::Char('g') if ctrl => {
                let note = self.view.lang.pick_owned(
                    "Ctrl+G 只用于文字输入(框文字 / 新建节点)".to_string(),
                    "Ctrl+G only works for text input (box text / new node)".to_string(),
                );
                self.view.note = Some(note);
            }
            // esc: roll back both the query and the hits
            KeyCode::Esc => {
                self.view.query = None;
                self.view.query_pos = 0;
                self.view.hits.clear();
                self.view.focus = self.search_focus.clone();
                self.view.cursor = self.search_from;
            }
            // Enter: keep the hits so n can step through them
            KeyCode::Enter => {
                self.view.query = None;
                self.view.query_pos = 0;
                // A search jump counts as a jump: Ctrl-o returns, as with n / Tab
                self.refresh_pick();
                self.note_here();
                // All hits hidden by isolate: say so rather than look like no match
                if !self.view.hits.is_empty() && self.visible_hits().is_empty() {
                    let note = self.view.lang.pick_owned(
                        "命中的框都被隔离藏起来了(E 退出后再看)".to_string(),
                        "every match is hidden by isolate (E to exit)".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            // Start / end of line
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // Word (before the single-char cases)
            KeyCode::Left if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.query.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => {
                want = self.view.query.as_ref().map(|t| word_right(t, pos))
            }
            // Single char
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+Backspace: delete the previous word (same as Ctrl-W)
            KeyCode::Backspace if alt => {
                if let Some(q) = self.view.query.as_mut() {
                    let from = word_left(q, pos);
                    let (a, b) = (char_byte(q, from), char_byte(q, pos));
                    q.replace_range(a..b, "");
                    want = Some(from);
                }
                touched = true;
            }
            // Delete
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
