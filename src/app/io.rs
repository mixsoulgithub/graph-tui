//! io 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 按画布大小开一个够大的 Buffer,把整幅图画进去再转成 SVG
    pub(super) fn export_svg(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (x0, y0, x1, y1) = self.doc.bbox();
        // 上限 1000x400:再大就不是一张终端图了
        let w = ((x1 - x0 + 1).max(8) as u16)
            .saturating_add(4)
            .clamp(8, 1000);
        let h = ((y1 - y0 + 1).max(4) as u16)
            .saturating_add(2)
            .clamp(4, 400);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        // 导出永远 1:1:缓冲已经按文档包围盒开好,缩放会把格子压扁、文字行被边框吃掉
        render::render_map(&mut buf, area, &self.doc, &self.view, 1.0);
        let body = crate::svg::of_buffer(&buf);
        // .png 交给 rsvg-convert 栅格化(终端自己画不了字),其余按文本落盘
        if path.to_ascii_lowercase().ends_with(".png") {
            let res =
                crate::paths::ensure_parent(path).and_then(|()| crate::svg::write_png(&body, path));
            self.note_export(path, res);
        } else {
            self.write_export(path, &body);
        }
    }

    /// :export <path>.dot:best-effort 导出,把警告放在回显位(最多三条)
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

    /// 导出结果说一句(成功 / 失败原因)
    pub(super) fn note_export(&mut self, path: &str, res: Result<(), String>) {
        self.view.note = Some(match res {
            Ok(()) => self
                .view
                .lang
                .pick_owned(format!("已导出到 {path}"), format!("exported to {path}")),
            Err(e) => e,
        });
    }

    /// :export-canvas:交给 canvas_io 出 JSON Canvas(它会带回表达不了的警告)
    pub(super) fn export_canvas(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (body, warnings) = crate::canvas_io::export(&self.doc);
        if self.write_export(path, &body) {
            self.note_export_warnings(path, &warnings);
        }
    }

    /// 导出成功后把警告叠在回显上(最多三条,多的说一句"还有 N 条")
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

    /// 导出的落盘:和存盘一样先写 .tmp 再改名,失败把原因说在回显位.
    /// 返回是否写成功(调用方要在成功时再叠加警告).
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

    /// "没选边"的提示(style / edgecolor / via 都要用)
    pub(super) fn note_no_edge(&mut self) {
        let note = self.view.lang.pick_owned(
            "没有选中边(先按 E 选一条)".to_string(),
            "no edge selected (press E to pick one)".to_string(),
        );
        self.view.note = Some(note);
    }

    /// 把当前文档写到 path:先 .tmp 再 rename;写前校验,失败不覆盖原文件
    pub fn save_document(&mut self, path: &str) -> Result<(), String> {
        let path = &crate::paths::expand(path);
        // 存档头里的来源标记:写的是谁保存的,不是导入自哪种格式
        self.doc.source = format!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        self.sync_view();
        self.doc.validate()?;
        let body = self.doc.save();
        crate::paths::write_atomic(path, &body)
    }

    /// 默认的 native 存档路径:`.gtui` 写回原处,`.dot`/`.canvas` 写同目录 <stem>.gtui.
    /// stdin 来源没有路径,返回 None(要求显式给一个).
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

    /// 换来源文件的同时记下磁盘标记:后面 `check_disk` 拿它比,判断有没有被外部改过
    pub fn set_source(&mut self, path: Option<String>) {
        self.src_stamp = path.as_deref().and_then(FileStamp::of);
        self.src_rev = self.rev;
        self.view.stale = false;
        self.reload_armed = None;
        self.source = path;
    }

    /// 看一眼源文件还在不在、有没有被别的程序改过(main 的轮询里每次超时都调,开销一次 stat).
    /// 返回 true 表示画面该重画:标了 stale,或者已经自动重新载入了
    pub fn check_disk(&mut self) -> bool {
        let Some(path) = self.source.clone() else {
            return false;
        };
        let now = FileStamp::of(&path);
        if now == self.src_stamp {
            return false;
        }
        // 第一次记标记(比如 `--file` 用绝对路径打开):以当前状态为准,不算改动
        if self.src_stamp.is_none() {
            self.src_stamp = now;
            self.src_rev = self.rev;
            return false;
        }
        // 本地没动过:直接换掉,和 nvim 的 autoread 一样省一步.
        // 载入失败(文件正在写 / 内容坏了)就退回"只提醒",别每 250ms 重试一次
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

    /// 从磁盘重新读一遍源文件(丢掉本地改动).返回是否成功
    pub(super) fn reload_now(&mut self) -> bool {
        if let Some(path) = self.source.as_deref() {
            // 新文件还没落盘:别拿"空文档"把它盖掉
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

    /// R:从磁盘重新载入.本地有改动时要按两次(第一次只是提醒)
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

    /// 按扩展名把文本解析成文档(:e / :e! / R 共用).
    /// 和 nvim 一样:`.gtui` 还不存在就当"新文件"——空白文档 + 这个名字,`:w` 才落盘.
    /// `.dot` / `.canvas` 只是导入源,编不出来,缺了照旧报错.
    /// 返回的第二项表示"这是还没写过盘的新文件"
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

    /// 存盘:只写 .gtui.显式路径也必须是 .gtui,绝不写回 .dot.
    /// 返回 true 表示真的写成功了(`:wq` 要靠它决定退不退)
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
        // 存下的来源路径要展开过的,不然下次 :w(不给路径)还会拿到 `~`
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

    /// 还没有文件名时(空白缓冲 / stdin)存盘要说一句:要一个显式路径
    pub(super) fn note_save_hint(&mut self) {
        let note = self.view.lang.pick_owned(
            "还没有文件名:用 :w <路径.gtui>(或 :wq <路径.gtui> 存完就退)".to_string(),
            "no file name yet: use :w <path.gtui> (or :wq <path.gtui> to save and quit)"
                .to_string(),
        );
        self.view.note = Some(note);
    }

    /// :e <path>:按扩展名打开一份文档;失败时保持当前文档不动
    pub(super) fn open_doc(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        match Self::read_doc(path) {
            Ok((doc, fresh)) => {
                self.doc = doc;
                self.set_source(Some(path.to_string()));
                self.view_from_doc();
                self.reset_view();
                // 换了文档,撤销历史不再适用
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

    /// :e 不带参数:把当前目录与源文件目录里的可编辑文件列在回显位(最多 8 个)
    /// 当前目录 + 数据来源所在目录里所有能打开的文档
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

    /// `:e` 不带参数:弹出选择浮层(过滤 + 上下选 + 回车打开)
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

    /// 选择浮层里的按键:过滤、上下选、回车打开、esc 取消.
    /// 只有方向键能挪(字母全部归过滤词,不然文件名里的 j/k 会被当成导航)
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

    /// 解析目标并执行:图内跳转当场做,其余登记给 main(它才碰终端和进程)
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

    /// 图内跳转:当前文档直接跳,已开着的切过去,没开的开新 tab 再跳.
    /// 不动文档,也不进撤销栈
    pub(super) fn open_jump(&mut self, path: Option<std::path::PathBuf>, cell: Option<(i32, i32)>) {
        // 先记下"从哪儿跳走的":换 tab 之后再记就记成新 tab 的默认格了,
        // Ctrl-o 会回到新 tab 的 (0,0) 而不是原来的位置
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

    /// 打开链接用哪条命令:配置 > `$OPENER` > 自动挑
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
        // auto:PATH 里有 opener 就用它(它会自己决定 edit/open)
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

    /// 状态行上的错误提示(打开链接失败之类)
    pub fn note_error(&mut self, msg: String) {
        self.view.note = Some(msg);
    }

    /// 当前文档所在目录:`./`、`../` 这类链接按它解析
    pub fn doc_dir(&self) -> Option<std::path::PathBuf> {
        self.source.as_deref().and_then(|p| {
            std::path::Path::new(&crate::paths::expand(p))
                .parent()
                .map(|d| d.to_path_buf())
        })
    }

    /// 进程的工作目录(没有文档路径时当基准)
    pub fn cwd_path(&self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    /// 在 graph-tui 里打开一个本地图档:新标签(已经开着就切过去)
    pub fn open_path(&mut self, path: &std::path::Path) {
        let s = path.display().to_string();
        self.new_tab(Some(&s));
    }

    /// main 取一次待执行的打开计划(取走即清);Jump 不会出现在这里
    pub fn take_open_request(&mut self) -> Option<crate::target::OpenPlan> {
        self.open_request.take()
    }

    pub fn edit_pending(&self) -> bool {
        self.edit_request.is_some()
    }

    /// 要交给外部编辑器的初始内容(不清请求:回填时还要用它对应的输入目标)
    pub fn edit_initial(&self) -> Option<String> {
        self.edit_request.as_ref().map(|(_, text)| text.clone())
    }

    /// 外部编辑器的结论:框里的字直接写进框,输入框内容直接提交,都是一步撤销
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
                        // 连边的目标名是单行语义:多行直接拒,原内容不动
                        if matches!(what, Prompt::Link(_)) && text.contains('\n') {
                            Some(
                                self.view.lang.pick_owned(
                                    "这个字段只能是一行,编辑器里的换行没有注入".to_string(),
                                    "this field is single-line: the newlines were not injected"
                                        .to_string(),
                                ),
                            )
                        } else {
                            // 输入框这一趟已经结束了:按 Enter 的方式直接提交
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
