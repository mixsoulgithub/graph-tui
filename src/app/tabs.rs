//! tabs 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 把当前 tab 打包停靠到槽位(槽位里原来那个应当已经被取走)
    pub(super) fn park_current(&mut self) {
        let parked = Parked {
            doc: std::mem::replace(&mut self.doc, Document::scratch()),
            view: self.view.clone(),
            source: self.source.clone(),
            undo: std::mem::replace(&mut self.undo, History::new(200)),
            rev: self.rev,
            src_rev: self.src_rev,
            src_stamp: self.src_stamp.clone(),
            reload_armed: self.reload_armed,
        };
        if self.tab_at < self.tabs.len() {
            self.tabs[self.tab_at] = Some(parked);
        }
    }

    /// 把槽位里的 tab 取出来当作活动 tab
    pub(super) fn unpark(&mut self, parked: Parked) {
        self.doc = parked.doc;
        self.view = parked.view;
        self.view.glyphs = self.glyphs;
        self.source = parked.source;
        self.undo = parked.undo;
        self.rev = parked.rev;
        self.src_rev = parked.src_rev;
        self.src_stamp = parked.src_stamp;
        self.reload_armed = parked.reload_armed;
        // 帧状态不属于任何 tab:重画一帧就重建
        self.first_frame = true;
        self.hit = render::FrameHit::default();
        self.drag = None;
        self.view.drag_hide_sel = false;
        self.pending = None;
        self.clear_seq();
        self.note_draft.clear();
        self.last_click = None;
        // 临时 UI 一律收起
        self.view.cmd = None;
        self.view.cmd_menu = None;
        self.view.cmd_hint = None;
        self.view.prompt = None;
        self.view.menu = None;
        self.view.file_pick = None;
        self.view.query = None;
        self.view.hits.clear();
        self.refresh_pick();
    }

    /// 切到第 `i` 个 tab
    pub fn switch_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.tab_at {
            return;
        }
        self.park_current();
        let Some(parked) = self.tabs[i].take() else {
            self.tab_at = i;
            return;
        };
        self.tab_at = i;
        self.unpark(parked);
        // 后台期间文件可能被改过:只标 stale,不自动覆盖
        self.check_disk();
        let note = self.view.lang.pick_owned(
            format!(
                "[{}/{}] {}",
                self.tab_at + 1,
                self.tabs.len(),
                self.tab_name()
            ),
            format!(
                "[{}/{}] {}",
                self.tab_at + 1,
                self.tabs.len(),
                self.tab_name()
            ),
        );
        self.view.note = Some(note);
    }

    /// 当前 tab 显示用的名字(来源文件名,没有来源就是 scratch)
    pub fn tab_name(&self) -> String {
        match &self.source {
            Some(p) => std::path::Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone()),
            None => self.view.lang.pick("草稿", "scratch").to_string(),
        }
    }

    /// 这个 tab 有没有没保存的改动
    pub(super) fn dirty(&self) -> bool {
        self.rev != self.src_rev
    }

    /// 新开一个 tab(带路径就打开它),返回新槽位
    pub fn new_tab(&mut self, path: Option<&str>) -> usize {
        // 同一个路径已经开着就切过去,不重复建
        if let Some(p) = path {
            if let Some(i) = self.tab_of_path(p) {
                self.switch_tab(i);
                return i;
            }
        }
        self.park_current();
        let at = self.tabs.len();
        self.tabs.push(None);
        self.tab_at = at;
        // 新 tab 的初始状态:空白文档或打开的文件
        let mut fresh =
            App::new(Document::scratch(), Options::default()).expect("空白文档不可能非法");
        fresh.keys = self.keys.clone();
        fresh.mouse = self.mouse;
        fresh.mouse_edges = self.mouse_edges;
        fresh.spacing = self.spacing;
        fresh.glyphs = self.glyphs;
        fresh.view.glyphs = self.glyphs;
        fresh.view.lang = self.view.lang;
        fresh.tabs = vec![None];
        fresh.tab_at = 0;
        let mut parked = Parked {
            doc: fresh.doc,
            view: fresh.view,
            source: None,
            undo: fresh.undo,
            rev: 0,
            src_rev: 0,
            src_stamp: None,
            reload_armed: None,
        };
        if let Some(p) = path {
            let expanded = crate::paths::expand(p);
            match Self::read_doc(&expanded) {
                Ok((doc, _)) => {
                    parked.doc = doc;
                    parked.source = Some(expanded.clone());
                    parked.src_stamp = FileStamp::of(&expanded);
                    parked.view = View {
                        glyphs: self.glyphs,
                        lang: self.view.lang,
                        ..View::default()
                    };
                }
                Err(e) => {
                    self.view.note = Some(e);
                    // 打开失败:撤掉这个空槽位,回到原来的 tab
                    self.tabs.pop();
                    self.tab_at = at.saturating_sub(1);
                    return self.tab_at;
                }
            }
        }
        self.tabs[at] = None;
        self.unpark(parked);
        let note = self.view.lang.pick_owned(
            format!("[{}/{}] {}", at + 1, self.tabs.len(), self.tab_name()),
            format!("[{}/{}] {}", at + 1, self.tabs.len(), self.tab_name()),
        );
        self.view.note = Some(note);
        at
    }

    /// 某个路径对应的 tab 下标(规范化比较)
    pub(super) fn tab_of_path(&self, path: &str) -> Option<usize> {
        let want = std::fs::canonicalize(crate::paths::expand(path)).ok();
        for (i, slot) in self.tabs.iter().enumerate() {
            let src = if i == self.tab_at {
                self.source.clone()
            } else {
                slot.as_ref().and_then(|p| p.source.clone())
            };
            let Some(src) = src else { continue };
            if want.is_some() && std::fs::canonicalize(&src).ok() == want {
                return Some(i);
            }
            if src == crate::paths::expand(path) {
                return Some(i);
            }
        }
        None
    }

    /// 关掉当前 tab;脏且没强制就拒绝
    pub(super) fn close_tab(&mut self, force: bool) -> bool {
        if self.dirty() && !force {
            let note = self.view.lang.pick_owned(
                "这个 tab 有没保存的改动::tabclose! 丢弃".to_string(),
                "this tab has unsaved changes: :tabclose! to discard".to_string(),
            );
            self.view.note = Some(note);
            return false;
        }
        if self.tabs.len() <= 1 {
            // 只剩一个:直接退出(和 :q 一样)
            self.quit = true;
            return true;
        }
        // 跳转历史里指向这个 tab 的条目作废,后面的下标往前挪一格
        let gone = self.tab_at;
        self.jumps.retain(|j| j.tab != gone);
        for j in &mut self.jumps {
            if j.tab > gone {
                j.tab -= 1;
            }
        }
        self.jump_at = self.jump_at.min(self.jumps.len().saturating_sub(1));
        self.tabs.remove(self.tab_at);
        // 停靠状态作废:取下一个槽位当活动 tab
        let next = self.tab_at.min(self.tabs.len() - 1);
        // 去掉了一个槽位后,当前显示的内容本来就在 App 里,重新载入 next 的内容
        let Some(parked) = self.tabs[next].take() else {
            // next 就是原来的活动槽位(被移走了):用空白文档兜底
            self.doc = Document::scratch();
            self.view = View::default();
            self.view.glyphs = self.glyphs;
            self.source = None;
            self.undo = History::new(200);
            self.rev = 0;
            self.src_rev = 0;
            self.src_stamp = None;
            self.tab_at = next;
            self.first_frame = true;
            self.hit = render::FrameHit::default();
            return true;
        };
        self.tab_at = next;
        self.unpark(parked);
        let note = format!("[{}/{}] {}", next + 1, self.tabs.len(), self.tab_name());
        self.view.note = Some(note);
        true
    }
}
