//! The export/export-canvas/save/wq/tab*/e/e! arms of cmd_apply.

use super::cmd::CmdFlow;
use super::*;

impl App {
    pub(super) fn cmd_io_verbs(&mut self, cmd: &str, arg: Option<&str>, rest: &str) -> CmdFlow {
        match (cmd, arg) {
            ("export", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "export 缺参数:要写到哪个路径(.svg/.png/.dot/.canvas)".to_string(),
                        "export needs a path (.svg/.png/.dot/.canvas)".to_string(),
                    );
                    self.view.note = Some(note);
                } else if path.to_ascii_lowercase().ends_with(".dot") {
                    self.export_dot(path);
                } else if path.to_ascii_lowercase().ends_with(".canvas") {
                    self.export_canvas(path);
                } else {
                    self.export_svg(path);
                }
            }
            ("export-canvas", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "export-canvas 缺参数:要写到哪个 .canvas".to_string(),
                        "export-canvas needs a path (.canvas)".to_string(),
                    );
                    self.view.note = Some(note);
                } else {
                    self.export_canvas(path);
                }
            }
            // :w is a synonym for :save; both spellings work
            ("save", _) | ("w", _) => {
                let text = rest.trim();
                if text.is_empty() {
                    self.save(None);
                } else {
                    self.save(Some(text));
                }
            }
            // :wq / :wq <path>: quit only after a successful save (a failed save stays
            // put so changes are not lost)
            ("wq", _) => {
                let text = rest.trim();
                let ok = if text.is_empty() {
                    self.save(None)
                } else {
                    self.save(Some(text))
                };
                if !ok {
                    return CmdFlow::Handled;
                }
                // vim's E37: another buffer still has unsaved changes
                if self.other_dirty() {
                    self.view.note = Some(
                        self.view.lang.pick_owned(
                            "其它标签页有未保存的改动::wqa 全部存盘后退出,:wq! 丢弃后退出"
                                .to_string(),
                            "other tabs have unsaved changes: :wqa saves them, :wq! discards them"
                                .to_string(),
                        ),
                    );
                    return CmdFlow::Handled;
                }
                self.quit = true;
            }
            // Save this buffer anyway and quit, dropping whatever the other tabs hold
            ("wq!", _) => {
                let text = rest.trim();
                let ok = if text.is_empty() {
                    self.save(None)
                } else {
                    self.save(Some(text))
                };
                if ok {
                    self.quit = true;
                }
            }
            ("wa", _) => {
                self.save_all();
            }
            ("wqa", _) => {
                let (_, failed) = self.save_all();
                if failed == 0 {
                    self.quit = true;
                }
            }
            // :q refuses while a buffer has unsaved changes and names the force form;
            // :q! drops them. (:wq saves first, so it never reaches this guard)
            // Multi-document tabs
            ("tabnew", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    self.new_tab(None);
                } else {
                    self.new_tab(Some(path));
                }
            }
            ("tabnext", _) => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + 1) % n);
            }
            ("tabprev", _) => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + n - 1) % n);
            }
            ("tab", Some(a)) => match a.parse::<usize>() {
                Ok(n) if n >= 1 && n <= self.tabs.len() => self.switch_tab(n - 1),
                _ => {
                    let note = self.view.lang.pick_owned(
                        format!("tab: 1..={} 的数字,不是 {a}", self.tabs.len()),
                        format!("tab: a number 1..={}, not {a}", self.tabs.len()),
                    );
                    self.view.note = Some(note);
                }
            },
            ("tabclose", _) | ("tabclose!", _) => {
                self.close_tab(cmd == "tabclose!");
            }
            ("tabs", _) => {
                let mut parts: Vec<String> = Vec::new();
                for i in 0..self.tabs.len() {
                    let (name, dirty) = if i == self.tab_at {
                        (self.tab_name(), self.dirty())
                    } else {
                        match self.tabs[i].as_ref() {
                            Some(p) => (
                                match &p.source {
                                    Some(path) => std::path::Path::new(path)
                                        .file_name()
                                        .map(|f| f.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| path.clone()),
                                    None => self.view.lang.pick("草稿", "scratch").to_string(),
                                },
                                p.rev != p.src_rev,
                            ),
                            None => (String::new(), false),
                        }
                    };
                    parts.push(format!(
                        "{}{} {}{}",
                        if i == self.tab_at { ">" } else { " " },
                        i + 1,
                        name,
                        if dirty { "*" } else { "" }
                    ));
                }
                let list = parts.join("  ");
                self.view.note = Some(list);
            }
            ("e", _) => {
                let path = rest.trim();
                // The current tab has unsaved changes: stop and ask (`:e!` discards them)
                if self.dirty() {
                    let note = self.view.lang.pick_owned(
                        "当前文档有没保存的改动:用 :e! 丢弃,或用 :tabnew 另开一个".to_string(),
                        "unsaved changes: use :e! to discard, or :tabnew to open elsewhere"
                            .to_string(),
                    );
                    self.view.note = Some(note);
                    return CmdFlow::ReturnNow;
                }
                if path.is_empty() {
                    self.open_picker();
                } else {
                    self.open_doc(path);
                }
            }
            // :e! re-reads from disk forcibly (discarding local changes); with a path it
            // opens that file forcibly
            ("e!", _) => {
                let path = rest.trim();
                if path.is_empty() || self.source.as_deref() == Some(path) {
                    self.reload_now();
                } else {
                    self.open_doc(path);
                }
            }
            _ => return CmdFlow::NotMine,
        }
        CmdFlow::Handled
    }
    /// `:q` / `:q!`: quitting is refused while any tab has unsaved changes (`:q!` drops
    /// them, `:w` saves). The note names the force form so the user does not have to
    /// look it up.
    pub(super) fn cmd_quit(&mut self, force: bool) {
        if !force && self.any_dirty() {
            self.view.note = Some(self.view.lang.pick_owned(
                "有未保存的改动::q! 丢弃后退出,或 :w 存盘".to_string(),
                "unsaved changes: :q! quits and discards them, :w saves".to_string(),
            ));
            return;
        }
        self.quit = true;
    }
    /// Whether a tab other than the current one has unsaved changes.
    pub(super) fn other_dirty(&self) -> bool {
        self.tabs.iter().flatten().any(|p| p.rev != p.src_rev)
    }

    /// `:wa`: save every dirty buffer. Parked tabs are visited through the normal save path,
    /// then the view returns to the tab the user was on. Returns (saved, failed).
    pub(super) fn save_all(&mut self) -> (usize, usize) {
        let back = self.tab_at;
        let mut saved = 0usize;
        let mut failed = 0usize;
        // The current tab first: after visiting another one, `back` is no longer the
        // active tab, so its own save must happen before any switching
        if self.dirty() {
            if self.save(None) {
                saved += 1;
            } else {
                failed += 1;
            }
        }
        for i in 0..self.tabs.len() {
            if i == back {
                continue;
            }
            let dirty = self.tabs[i].as_ref().is_some_and(|p| p.rev != p.src_rev);
            if !dirty {
                continue;
            }
            self.switch_tab(i);
            if self.save(None) {
                saved += 1;
            } else {
                failed += 1;
            }
        }
        if self.tab_at != back {
            self.switch_tab(back);
        }
        let note = if saved == 0 && failed == 0 {
            self.view
                .lang
                .pick_owned("没有要存的改动".to_string(), "nothing to save".to_string())
        } else {
            self.view.lang.pick_owned(
                format!("存了 {saved} 个标签页,失败 {failed} 个"),
                format!("saved {saved} buffer(s), {failed} failed"),
            )
        };
        self.view.note = Some(note);
        (saved, failed)
    }
}
