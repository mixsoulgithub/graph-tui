//! cmd_apply 的 export/export-canvas/save/wq/tab*/e/e! 分支.

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
            // :w 与 :save 同义:vim 的写法与旧习惯都留着
            ("save", _) | ("w", _) => {
                let text = rest.trim();
                if text.is_empty() {
                    self.save(None);
                } else {
                    self.save(Some(text));
                }
            }
            // :wq / :wq <路径>:存成功才退(存失败就留在原地,别把改动丢了)
            ("wq", _) => {
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
            // 多文档 tab
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
                // 当前 tab 有没保存的改动:先停下来问一句(`:e!` 才丢)
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
            // :e! 强制从磁盘重读(丢掉本地改动);带路径就是强制打开那个文件
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
}
