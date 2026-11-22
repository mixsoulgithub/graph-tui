//! I/O: SVG/DOT/JSON Canvas export, save, load, reload and the open plan.

use super::*;

impl App {
    /// Open a Buffer big enough for the canvas, render the whole picture into it, then
    /// turn it into SVG
    pub(super) fn export_svg(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (x0, y0, x1, y1) = self.doc.bbox();
        // capped at 1000x400: anything larger is no longer a terminal picture
        let w = ((x1 - x0 + 1).max(8) as u16)
            .saturating_add(4)
            .clamp(8, 1000);
        let h = ((y1 - y0 + 1).max(4) as u16)
            .saturating_add(2)
            .clamp(4, 400);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        // export is always 1:1: the buffer already matches the document bbox, and zoom
        // would squash cells and let borders eat text rows
        render::render_map(&mut buf, area, &self.doc, &self.view, 1.0);
        let body = crate::svg::of_buffer(&buf);
        // .png is rasterised by rsvg-convert (the terminal cannot draw text); everything
        // else is written as text
        if path.to_ascii_lowercase().ends_with(".png") {
            let res =
                crate::paths::ensure_parent(path).and_then(|()| crate::svg::write_png(&body, path));
            self.note_export(path, res);
        } else {
            self.write_export(path, &body);
        }
    }

    /// :export <path>.dot: best-effort export; warnings go to the note slot (max three)
    pub(super) fn export_dot(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        match crate::dot_write::export(&self.doc) {
            Ok((body, warnings)) => {
                let ok = self.write_export(path, &body);
                if ok && !warnings.is_empty() {
                    let shown: Vec<&str> = warnings.iter().take(3).map(|w| w.as_str()).collect();
                    let mut text = shown.join("; ");
                    if warnings.len() > shown.len() {
                        text.push_str("; ...");
                    }
                    self.view.note = Some(self.view.lang.pick_owned(
                        format!("已导出到 {path}(警告:{text})"),
                        format!("exported to {path} (warnings: {text})"),
                    ));
                }
            }
            Err(e) => self.view.note = Some(e),
        }
    }

    /// Report the export result (success / failure reason)
    pub(super) fn note_export(&mut self, path: &str, res: Result<(), String>) {
        self.view.note = Some(match res {
            Ok(()) => self
                .view
                .lang
                .pick_owned(format!("已导出到 {path}"), format!("exported to {path}")),
            Err(e) => e,
        });
    }

    /// :export-canvas: canvas_io writes JSON Canvas and returns warnings for whatever it
    /// cannot express
    pub(super) fn export_canvas(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (body, warnings) = crate::canvas_io::export(&self.doc);
        if self.write_export(path, &body) {
            self.note_export_warnings(path, &warnings);
        }
    }

    /// After a successful export, layer warnings onto the note (at most three; the rest
    /// are summarised as "+N more")
    pub(super) fn note_export_warnings(&mut self, path: &str, warnings: &[String]) {
        if warnings.is_empty() {
            return;
        }
        let mut text = String::new();
        for w in warnings.iter().take(3) {
            text.push_str(w);
            text.push(' ');
        }
        if warnings.len() > 3 {
            text.push_str(&self.view.lang.pick_owned(
                format!("…还有 {} 条", warnings.len() - 3),
                format!("…+{} more", warnings.len() - 3),
            ));
        }
        let note = self.view.lang.pick_owned(
            format!("已导出到 {path};{text}"),
            format!("exported to {path}; {text}"),
        );
        self.view.note = Some(note);
    }

    /// Write an export to disk: like saving, write .tmp then rename; a failure puts the
    /// reason in the note slot. Returns whether the write succeeded (the caller adds
    /// warnings on success).
    pub(super) fn write_export(&mut self, path: &str, body: &str) -> bool {
        if let Err(e) = crate::paths::ensure_parent(path) {
            self.view.note = Some(e);
            return false;
        }
        let tmp = format!("{path}.tmp");
        let res = std::fs::write(&tmp, body.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
        match res {
            Ok(()) => {
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("已导出到 {path}"), format!("exported to {path}")),
                );
                true
            }
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("导出失败 {path}: {err}"),
                    format!("cannot export {path}: {err}"),
                ));
                false
            }
        }
    }

    /// "no edge selected" note (used by style / edgecolor / via)
    pub(super) fn note_no_edge(&mut self) {
        let note = self.view.lang.pick_owned(
            "没有选中边(先按 E 选一条)".to_string(),
            "no edge selected (press E to pick one)".to_string(),
        );
        self.view.note = Some(note);
    }

    /// Write the current document to path: .tmp then rename; validated first, so a
    /// failure never clobbers the original
    pub fn save_document(&mut self, path: &str) -> Result<(), String> {
        let path = &crate::paths::expand(path);
        // source marker in the file header: who saved it, not which format it came from
        self.doc.source = format!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        self.sync_view();
        self.doc.validate()?;
        let body = self.doc.save();
        crate::paths::write_atomic(path, &body)
    }

    /// Default native save path: `.gtui` is written back in place, `.dot`/`.canvas` get
    /// <stem>.gtui next to them. A stdin source has no path, so this returns None (an
    /// explicit one is required).
    pub(super) fn default_save_path(&self) -> Option<String> {
        let src = self.source.as_deref()?;
        if src.to_ascii_lowercase().ends_with(".gtui") {
            Some(src.to_string())
        } else {
            Some(
                Path::new(src)
                    .with_extension("gtui")
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    /// Switch the source file and record its disk stamp; `check_disk` compares against it
    /// to spot external edits
    pub fn set_source(&mut self, path: Option<String>) {
        self.src_stamp = path.as_deref().and_then(FileStamp::of);
        self.src_rev = self.rev;
        self.view.stale = false;
        self.reload_armed = None;
        self.source = path;
    }

    /// Check whether the source file still exists and was changed by another program
    /// (called on every poll timeout from main; costs one stat). Returns true when the
    /// screen should be redrawn: stale was set, or the file was reloaded automatically
    pub fn check_disk(&mut self) -> bool {
        let Some(path) = self.source.clone() else {
            return false;
        };
        let now = FileStamp::of(&path);
        if now == self.src_stamp {
            return false;
        }
        // first stamp (e.g. `--file` opened by absolute path): the current state is the
        // baseline, not a change
        if self.src_stamp.is_none() {
            self.src_stamp = now;
            self.src_rev = self.rev;
            return false;
        }
        // no local edits: swap it in directly, saving a step like nvim's autoread.
        // A failed load (file mid-write / broken) falls back to "just warn" instead of
        // retrying every 250ms
        if !self.view.stale && self.rev == self.src_rev && now.is_some() {
            if self.reload_now() {
                self.view.note = Some(self.view.lang.pick_owned(
                    "文件已在磁盘上更新,已重新载入".to_string(),
                    "the file changed on disk; reloaded".to_string(),
                ));
            } else {
                self.view.stale = true;
            }
            return true;
        }
        if !self.view.stale {
            self.view.stale = true;
            let gone = now.is_none();
            self.view.note = Some(crate::render::stale_note(&self.view, gone));
            return true;
        }
        false
    }

    /// Re-read the source file from disk, discarding local edits. Returns whether it worked
    pub(super) fn reload_now(&mut self) -> bool {
        if let Some(path) = self.source.as_deref() {
            // the new file is not on disk yet: do not clobber it with an empty document
            if !std::path::Path::new(path).exists() {
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("{path} 还不存在:先 :w 保存"),
                    format!("{path} does not exist yet: save it with :w"),
                ));
                self.src_stamp = None;
                return false;
            }
        }
        let Some(path) = self.source.clone() else {
            self.view.note = Some(self.view.lang.pick_owned(
                "没有来源文件,无从载入".to_string(),
                "no source file to reload from".to_string(),
            ));
            return false;
        };
        match Self::read_doc(&path) {
            Ok((doc, _)) => {
                self.doc = doc;
                self.view_from_doc();
                self.undo.clear();
                self.rev += 1;
                self.src_stamp = FileStamp::of(&path);
                self.src_rev = self.rev;
                self.view.stale = false;
                self.reload_armed = None;
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("已重新载入 {path}"), format!("reloaded {path}"));
                self.after_history(&note);
                true
            }
            Err(e) => {
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("重新载入失败: {e}"), format!("cannot reload: {e}")),
                );
                false
            }
        }
    }

    /// R: reload from disk. With local edits it takes two presses (the first only warns)
    pub(super) fn reload_step(&mut self) {
        if self.source.is_none() {
            self.view.note = Some(self.view.lang.pick_owned(
                "没有来源文件,无从载入".to_string(),
                "no source file to reload from".to_string(),
            ));
            return;
        }
        let dirty = self.rev != self.src_rev;
        let armed = self
            .reload_armed
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(4));
        if dirty && !armed {
            self.reload_armed = Some(std::time::Instant::now());
            self.view.note = Some(self.view.lang.pick_owned(
                "有没保存的改动:再按一次 R 丢弃本地改动并重新载入".to_string(),
                "unsaved changes: press R again to drop them and reload".to_string(),
            ));
            return;
        }
        self.reload_now();
    }

    /// Parse text into a document by extension (shared by :e / :e! / R).
    /// As in nvim, a missing `.gtui` counts as a new file: an empty document under that
    /// name, written only by `:w`. `.dot` / `.canvas` are import sources that cannot be
    /// created, so a missing one is still an error. The second item reports a file that
    /// has never been written to disk.
    pub(super) fn read_doc(path: &str) -> Result<(Document, bool), String> {
        let lower = path.to_ascii_lowercase();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && lower.ends_with(".gtui") => {
                return Ok((Document::scratch(), true));
            }
            Err(e) => return Err(format!("cannot read {path}: {e}")),
        };
        let doc = if lower.ends_with(".gtui") {
            Document::load(&text)
        } else if lower.ends_with(".canvas") {
            crate::canvas_io::import(&text)
        } else {
            crate::dot::parse_document(&text, ViewState::default(), "dot")
        }?;
        Ok((doc, false))
    }

    /// Save: .gtui only. An explicit path must be .gtui too; never write back to .dot.
    /// Returns true only if the write succeeded (`:wq` uses it to decide whether to quit)
    pub(super) fn save(&mut self, path: Option<&str>) -> bool {
        let target = match path {
            Some(p) => {
                let p = p.trim();
                if p.is_empty() {
                    self.note_save_hint();
                    return false;
                }
                if !p.to_ascii_lowercase().ends_with(".gtui") {
                    let note = self.view.lang.pick_owned(
                        format!("只写 .gtui:不写 {p}"),
                        format!("only .gtui is written: refusing {p}"),
                    );
                    self.view.note = Some(note);
                    return false;
                }
                p.to_string()
            }
            None => match self.default_save_path() {
                Some(p) => p,
                None => {
                    self.note_save_hint();
                    return false;
                }
            },
        };
        // the stored source path must be expanded, or a later :w (no path) sees `~` again
        let target = crate::paths::expand(&target);
        match self.save_document(&target) {
            Ok(()) => {
                self.set_source(Some(target.clone()));
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("已保存到 {target}"), format!("saved to {target}")),
                );
                true
            }
            Err(err) => {
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("保存失败 {target}: {err}"),
                    format!("cannot save {target}: {err}"),
                ));
                false
            }
        }
    }

    /// Saving with no file name yet (scratch buffer / stdin) asks for an explicit path
    pub(super) fn note_save_hint(&mut self) {
        let note = self.view.lang.pick_owned(
            "还没有文件名:用 :w <路径.gtui>(或 :wq <路径.gtui> 存完就退)".to_string(),
            "no file name yet: use :w <path.gtui> (or :wq <path.gtui> to save and quit)"
                .to_string(),
        );
        self.view.note = Some(note);
    }

    /// :e <path>: open a document by extension; on failure the current one stays put
    pub(super) fn open_doc(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        match Self::read_doc(path) {
            Ok((doc, fresh)) => {
                self.doc = doc;
                self.set_source(Some(path.to_string()));
                self.view_from_doc();
                self.reset_view();
                // the undo history does not apply to a different document
                self.undo.clear();
                if !self.doc.boxes.is_empty() {
                    self.note_here();
                }
                self.view.note = Some(if fresh {
                    self.view.lang.pick_owned(
                        format!("新文件 {path}(:w 写入)"),
                        format!("new file {path} (:w to write it)"),
                    )
                } else {
                    self.view
                        .lang
                        .pick_owned(format!("已打开 {path}"), format!("opened {path}"))
                });
            }
            Err(e) => self.view.note = Some(e),
        }
    }

    /// :e with no argument: list the editable documents in the current directory and in
    /// the source file's directory (at most 8)
    pub(super) fn collect_candidates(&self) -> Vec<String> {
        let mut dirs: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from(".")];
        if let Some(src) = &self.source {
            if let Some(parent) = std::path::Path::new(src).parent() {
                dirs.push(parent.to_path_buf());
            }
        }
        let mut seen: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        let mut found: Vec<String> = Vec::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in rd.flatten() {
                let p = entry.path();
                let lower = p.to_string_lossy().to_ascii_lowercase();
                if !(lower.ends_with(".gtui")
                    || lower.ends_with(".dot")
                    || lower.ends_with(".canvas"))
                {
                    continue;
                }
                let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                if !seen.insert(key) {
                    continue;
                }
                found.push(p.to_string_lossy().into_owned());
            }
        }
        found.sort();
        found
    }

    /// `:e` with no argument: raise the picker overlay (filter + up/down + Enter to open)
    pub(super) fn open_picker(&mut self) {
        let items = self.collect_candidates();
        if items.is_empty() {
            let note = self.view.lang.pick_owned(
                "这个目录里没有可编辑的文件(.gtui/.dot/.canvas)".to_string(),
                "no editable file here (.gtui/.dot/.canvas)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.view.file_pick = Some(FilePick {
            items,
            at: 0,
            filter: String::new(),
        });
    }

    /// Picker keys: filter, up/down, Enter to open, esc to cancel. Only the arrow keys
    /// move the selection; all letters feed the filter, otherwise j/k in a file name
    /// would navigate
    pub(super) fn handle_pick(&mut self, code: KeyCode) {
        let Some(mut pick) = self.view.file_pick.take() else {
            return;
        };
        match code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                if let Some(p) = pick.current().map(|s| s.to_string()) {
                    self.open_doc(&p);
                }
                return;
            }
            KeyCode::Up => {
                let n = pick.shown().len();
                if n > 0 {
                    pick.at = (pick.at + n - 1) % n;
                }
            }
            KeyCode::Down => {
                let n = pick.shown().len();
                if n > 0 {
                    pick.at = (pick.at + 1) % n;
                }
            }
            KeyCode::Backspace => {
                pick.filter.pop();
                pick.at = 0;
            }
            KeyCode::Char(c) => {
                pick.filter.push(c);
                pick.at = 0;
            }
            _ => {}
        }
        self.view.file_pick = Some(pick);
    }

    /// Plan the target and act: in-document jumps happen here, the rest is queued for
    /// main, which is what touches the terminal and spawns processes
    pub(super) fn request_open(&mut self, raw: &str) {
        let opener = self.opener_command();
        let cwd = self.cwd_path();
        let doc_dir = self.doc_dir();
        let ctx = crate::target::OpenContext {
            doc_dir: doc_dir.as_deref(),
            cwd: &cwd,
            opener: &opener,
            links: &self.links,
        };
        match crate::target::plan_raw(&ctx, raw) {
            crate::target::OpenPlan::Jump { path, cell } => self.open_jump(path, cell),
            crate::target::OpenPlan::Note(msg) => self.note_error(msg),
            plan => self.open_request = Some(plan),
        }
    }

    /// In-document jump: jump in the current document, switch to an already open tab, or
    /// open a new tab and jump there. Touches no document and no undo stack
    pub(super) fn open_jump(&mut self, path: Option<std::path::PathBuf>, cell: Option<(i32, i32)>) {
        // record where we jumped from first: doing it after the tab switch would record
        // the new tab's default cell, and Ctrl-o would land on (0,0) instead of the origin
        self.note_here();
        if let Some(p) = path.as_ref() {
            let s = p.display().to_string();
            match self.tab_of_path(&s) {
                Some(i) => self.switch_tab(i),
                None => {
                    self.new_tab(Some(&s));
                }
            }
        }
        if let Some(cell) = cell {
            self.view.cursor = cell;
            self.view.cand = 0;
            self.refresh_pick();
            self.follow_cursor_jump();
        }
    }

    /// Command used to open links: config > `$OPENER` > auto-detect
    pub fn opener_command(&self) -> String {
        if let Some(o) = self.opener.as_deref().filter(|o| !o.is_empty()) {
            return o.to_string();
        }
        if let Some(o) = std::env::var("OPENER")
            .ok()
            .filter(|o| !o.trim().is_empty())
        {
            return o;
        }
        // auto: use opener from PATH when present (it decides between edit and open)
        if which("opener").is_some() {
            return "opener".to_string();
        }
        if cfg!(target_os = "macos") {
            "open".to_string()
        } else if cfg!(windows) {
            "explorer".to_string()
        } else {
            "xdg-open".to_string()
        }
    }

    /// Error note on the status line (a failed link open, say)
    pub fn note_error(&mut self, msg: String) {
        self.view.note = Some(msg);
    }

    /// Directory of the current document: `./` and `../` links resolve against it
    pub fn doc_dir(&self) -> Option<std::path::PathBuf> {
        self.source.as_deref().and_then(|p| {
            std::path::Path::new(&crate::paths::expand(p))
                .parent()
                .map(|d| d.to_path_buf())
        })
    }

    /// Process working directory (the base when there is no document path)
    pub fn cwd_path(&self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    /// Open a local map file in graph-tui: a new tab, or switch to it if already open
    pub fn open_path(&mut self, path: &std::path::Path) {
        let s = path.display().to_string();
        self.new_tab(Some(&s));
    }

    /// main takes the pending open plan once (taking clears it); Jump never appears here
    pub fn take_open_request(&mut self) -> Option<crate::target::OpenPlan> {
        self.open_request.take()
    }

    pub fn edit_pending(&self) -> bool {
        self.edit_request.is_some()
    }

    /// Initial content handed to the external editor (the request stays: applying the
    /// result still needs its target)
    pub fn edit_initial(&self) -> Option<String> {
        self.edit_request.as_ref().map(|(_, text)| text.clone())
    }

    /// Result of the external editor: box text goes straight into the box, prompt text is
    /// submitted; both are one undo step
    pub fn apply_edit_result(&mut self, out: crate::external::Outcome) {
        use crate::external::Outcome;
        let Some((target, _initial)) = self.edit_request.take() else {
            return;
        };
        match target {
            EditTarget::Boxes(ids) => {
                let note = match out {
                    Outcome::Changed(text) => {
                        self.cmd_set_text(&ids, &text);
                        Some(self.view.lang.pick_owned(
                            "框里的文字已更新".to_string(),
                            "box text updated".to_string(),
                        ))
                    }
                    Outcome::Unchanged => Some(self.view.lang.pick_owned(
                        "编辑器没有改动,框里的文字保持原样".to_string(),
                        "editor changed nothing: the box text is unchanged".to_string(),
                    )),
                    Outcome::Failed(n) => Some(self.view.lang.pick_owned(
                        format!("编辑器退出码 {n},框里的文字保持原样"),
                        format!("editor exited {n}: the box text is unchanged"),
                    )),
                    Outcome::NoFile => Some(self.view.lang.pick_owned(
                        "编辑器删了临时文件,什么都没写".to_string(),
                        "the editor removed the file: nothing was written".to_string(),
                    )),
                    Outcome::BadEncoding => Some(self.view.lang.pick_owned(
                        "编辑器写回的不是 UTF-8,什么都没写".to_string(),
                        "the editor wrote non-UTF-8: nothing was written".to_string(),
                    )),
                    Outcome::BadText => Some(self.view.lang.pick_owned(
                        "写回的内容里有回车或 NUL,没有采用".to_string(),
                        "the editor wrote CR or NUL: nothing was written".to_string(),
                    )),
                    Outcome::Unsupported => Some(self.view.lang.pick_owned(
                        "这个平台没有 unix shell,Ctrl+G 外部编辑器用不了".to_string(),
                        "no Unix shell on this platform: Ctrl+G editor is unavailable".to_string(),
                    )),
                };
                self.view.note = note;
            }
            EditTarget::Prompt(what) => {
                let note = match out {
                    Outcome::Changed(text) => {
                        // a link target is single-line by definition: reject multi-line
                        // input and leave the original untouched
                        if matches!(what, Prompt::Link(_)) && text.contains('\n') {
                            Some(
                                self.view.lang.pick_owned(
                                    "这个字段只能是一行,编辑器里的换行没有注入".to_string(),
                                    "this field is single-line: the newlines were not injected"
                                        .to_string(),
                                ),
                            )
                        } else {
                            // the prompt round trip is over: submit as if Enter was pressed
                            self.view.prompt = None;
                            self.view.prompt_pos = 0;
                            self.submit_prompt(what, text);
                            Some(self.view.lang.pick_owned(
                                "编辑器内容已应用".to_string(),
                                "editor content applied".to_string(),
                            ))
                        }
                    }
                    Outcome::Unchanged => Some(self.view.lang.pick_owned(
                        "编辑器没有改动,输入行保持原样".to_string(),
                        "editor changed nothing: the line is unchanged".to_string(),
                    )),
                    Outcome::Failed(n) => Some(self.view.lang.pick_owned(
                        format!("编辑器退出码 {n},输入行保持原样"),
                        format!("editor exited {n}: the line is unchanged"),
                    )),
                    Outcome::NoFile => Some(self.view.lang.pick_owned(
                        "编辑器删了临时文件,什么都没注入".to_string(),
                        "editor removed the file: nothing injected".to_string(),
                    )),
                    Outcome::BadEncoding => Some(self.view.lang.pick_owned(
                        "编辑器写回的不是 UTF-8,输入行保持原样".to_string(),
                        "the editor wrote non-UTF-8: the line is unchanged".to_string(),
                    )),
                    Outcome::BadText => Some(self.view.lang.pick_owned(
                        "写回的内容里有回车或 NUL,输入行保持原样".to_string(),
                        "the editor wrote CR or NUL: the line is unchanged".to_string(),
                    )),
                    Outcome::Unsupported => Some(self.view.lang.pick_owned(
                        "这个平台没有 unix shell,Ctrl+G 外部编辑器用不了".to_string(),
                        "no Unix shell on this platform: Ctrl+G editor is unavailable".to_string(),
                    )),
                };
                self.view.note = note;
            }
        }
    }
}
