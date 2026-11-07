//! menu 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 菜单作用于哪些元素:选区里和光标 Pick 同类型的那批(没选区就是光标下那个)
    pub(super) fn menu_scope(&self) -> (Vec<usize>, Vec<usize>) {
        let all = self.targets();
        match self.view.pick {
            render::Pick::Box(_) => (all.boxes, Vec::new()),
            render::Pick::Edge(_) => (Vec::new(), all.edges),
            render::Pick::None => (Vec::new(), Vec::new()),
        }
    }

    /// 一组框/边在这个属性上是不是同一个值;不是就返回 None(菜单里不打 *)
    pub(super) fn common<T: PartialEq + Clone>(values: impl Iterator<Item = T>) -> Option<T> {
        let mut it = values;
        let first = it.next()?;
        if it.all(|v| v == first) {
            Some(first)
        } else {
            None
        }
    }

    /// 菜单里那一串颜色(带色块、当前值打 *)
    pub(super) fn color_items(
        &self,
        prefix: &str,
        none_label: &str,
        none_line: &str,
        current: Option<(u8, u8, u8)>,
        none_when_auto: bool,
    ) -> Vec<MenuItem> {
        let mut out = vec![
            MenuItem::new(none_label, MenuAction::Cmd(none_line.to_string()))
                .checked(none_when_auto),
        ];
        for (i, c) in crate::model::PALETTE.iter().enumerate() {
            let hex = format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2);
            out.push(
                MenuItem::new(
                    format!("{i:02} {hex}"),
                    MenuAction::Cmd(format!("{prefix} {hex}")),
                )
                .swatch(*c)
                .checked(current == Some(*c)),
            );
        }
        out
    }

    /// 菜单里的图层项(选中元素挪到那一层)
    pub(super) fn layer_items(&self, on: &str) -> Vec<MenuItem> {
        let cur = if on == "box" {
            Self::common(
                self.menu_scope()
                    .0
                    .iter()
                    .map(|i| self.doc.boxes[*i].layer.clone()),
            )
        } else {
            Self::common(
                self.menu_scope()
                    .1
                    .iter()
                    .map(|i| self.doc.edges[*i].layer.clone()),
            )
        };
        self.doc
            .layers
            .iter()
            .map(|l| {
                let name = if l.name.is_empty() {
                    self.view.lang.pick("(默认)", "(default)").to_string()
                } else {
                    l.name.clone()
                };
                MenuItem::new(
                    format!("{name} → {}", l.name),
                    MenuAction::Cmd(format!("tolayer {}", l.name)),
                )
                .checked(cur.as_deref() == Some(l.name.as_str()))
            })
            .collect()
    }

    /// 这是哪一类菜单:记忆按它分组,框和边各记各的
    pub(super) fn menu_kind(&self, color_only: bool) -> String {
        let edge = self.view.pick.edge().is_some();
        let boxed = self.view.pick.box_at().is_some();
        match (color_only, edge, boxed) {
            (true, true, _) => "colour-edge",
            (true, _, _) => "colour-box",
            (false, true, _) => "edge",
            (false, _, true) => "box",
            _ => "canvas",
        }
        .to_string()
    }

    /// 按上次的选择还原菜单:记住的子菜单直接进去,记住的项停在那个位置
    pub(super) fn menu_from_memory(
        &self,
        items: Vec<MenuItem>,
        kind: &str,
    ) -> (Vec<Vec<MenuItem>>, Vec<String>, Vec<usize>, usize) {
        let mut levels = vec![items];
        let mut parents: Vec<String> = Vec::new();
        let mut parents_sel: Vec<usize> = Vec::new();
        let mut sel: Option<usize> = None;
        if let Some(path) = self.menu_memory.get(kind) {
            for (want, want_sel) in path {
                let Some(level) = levels.last() else { break };
                let Some(found) = level.iter().position(|it| &it.title == want) else {
                    break;
                };
                // 标题对上就用记住的下标(菜单项变了也能对上标题)
                let i = if level.get(*want_sel).map(|it| &it.title) == Some(want) {
                    *want_sel
                } else {
                    found
                };
                match &level[i].action {
                    MenuAction::Sub(sub) => {
                        let sub = sub.clone();
                        parents.push(want.clone());
                        parents_sel.push(i);
                        levels.push(sub);
                    }
                    _ => {
                        sel = Some(i);
                        break;
                    }
                }
            }
        }
        let sel = sel.unwrap_or_else(|| {
            levels
                .last()
                .and_then(|l| l.iter().position(|i| i.checked))
                .unwrap_or(0)
        });
        (levels, parents, parents_sel, sel)
    }

    /// 菜单内容:按光标下的类型给一套(右键和 s 共用)
    pub(super) fn menu_for(&self) -> Vec<MenuItem> {
        let cmd = |title: &str, line: &str| MenuItem::new(title, MenuAction::Cmd(line.to_string()));
        // 菜单标题跟界面语言走(值本身是命令里的关键字,保持英文)
        let t = |zh: &'static str, en: &'static str| self.view.lang.pick(zh, en);
        if self.view.link.is_some() {
            return vec![
                MenuItem::new(
                    self.view.lang.pick("确认连边", "create edges"),
                    MenuAction::Link(true),
                ),
                MenuItem::new(
                    self.view.lang.pick("取消连边", "cancel linking"),
                    MenuAction::Link(false),
                ),
            ];
        }
        let pick = self.view.pick;
        if pick.edge().is_some() {
            let (_, edges) = self.menu_scope();
            let style = Self::common(edges.iter().map(|e| self.doc.edges[*e].style));
            let dir = Self::common(edges.iter().map(|e| self.doc.edges[*e].dir));
            let corner = Self::common(edges.iter().map(|e| self.doc.edges[*e].corner));
            let head = Self::common(edges.iter().map(|e| self.doc.edges[*e].head));
            let color = Self::common(edges.iter().map(|e| self.doc.edges[*e].color));
            let auto = color == Some(None);
            let color = color.flatten();
            let bg_raw = Self::common(edges.iter().map(|e| self.doc.edges[*e].bg));
            let bg_auto = bg_raw == Some(None);
            let bg = bg_raw.flatten();
            let group = Self::common(edges.iter().map(|e| self.doc.edges[*e].group.clone()));
            let from_side = Self::common(edges.iter().map(|e| self.doc.edges[*e].from_side));
            let to_side = Self::common(edges.iter().map(|e| self.doc.edges[*e].to_side));
            let named = |name: &str, value: &str, line: &str, on: bool| {
                MenuItem::new(name, MenuAction::Cmd(format!("{line} {value}"))).checked(on)
            };
            // 端口:auto 交回自动,其余四侧;选中的几条边值不同就都不打勾
            let sides = |verb: &str, cur: Option<Option<Side>>| {
                let mut out = vec![named("auto", "auto", verb, cur == Some(None))];
                for s in [Side::Left, Side::Right, Side::Up, Side::Down] {
                    out.push(named(s.name(), s.name(), verb, cur == Some(Some(s))));
                }
                out
            };
            let mut items = vec![
                MenuItem::new(
                    t("线型", "line style"),
                    MenuAction::Sub(vec![
                        named("solid", "solid", "style", style == Some(EdgeStyle::Solid)),
                        named(
                            "dashed",
                            "dashed",
                            "style",
                            style == Some(EdgeStyle::Dashed),
                        ),
                        named(
                            "dotted",
                            "dotted",
                            "style",
                            style == Some(EdgeStyle::Dotted),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("方向", "direction"),
                    MenuAction::Sub(vec![
                        named("none", "none", "dir", dir == Some(Dir::None)),
                        named("forward", "forward", "dir", dir == Some(Dir::Forward)),
                        named("back", "back", "dir", dir == Some(Dir::Back)),
                        named("both", "both", "dir", dir == Some(Dir::Both)),
                    ]),
                ),
                MenuItem::new(
                    t("箭头", "arrowhead"),
                    MenuAction::Sub(vec![
                        named("auto", "auto", "head", head == Some(None)),
                        named(
                            "ascii",
                            "ascii",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Ascii)),
                        ),
                        named(
                            "arrows",
                            "arrows",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Arrows)),
                        ),
                        named(
                            "triangles",
                            "triangles",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Triangles)),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("端口", "ports"),
                    MenuAction::Sub(vec![
                        MenuItem::new(
                            t("起点", "from"),
                            MenuAction::Sub(sides("fromside", from_side)),
                        ),
                        MenuItem::new(t("终点", "to"), MenuAction::Sub(sides("toside", to_side))),
                    ]),
                ),
                MenuItem::new(
                    t("拐角", "corner"),
                    MenuAction::Sub(vec![
                        named(
                            "rounded",
                            "rounded",
                            "corner",
                            corner == Some(crate::model::Corner::Rounded),
                        ),
                        named(
                            "sharp",
                            "sharp",
                            "corner",
                            corner == Some(crate::model::Corner::Sharp),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("颜色", "colour"),
                    MenuAction::Sub(self.color_items(
                        "edgecolor",
                        "auto",
                        "edgecolor auto",
                        color,
                        auto,
                    )),
                ),
            ];
            items.push(MenuItem::new(
                t("边底色", "edge background"),
                MenuAction::Sub(self.color_items("edgebg", "none", "edgebg none", bg, bg_auto)),
            ));
            if !self.doc.groups.is_empty() {
                items.push(MenuItem::new(
                    t("分组", "group"),
                    MenuAction::Sub(
                        self.doc
                            .groups
                            .iter()
                            .map(|g| {
                                MenuItem::new(
                                    format!("{} {}", g.id, g.label),
                                    MenuAction::Cmd(format!("group {}", g.id)),
                                )
                                .swatch(g.color)
                                .checked(group.as_deref() == Some(g.id.as_str()))
                            })
                            .collect(),
                    ),
                ));
            }
            if self.doc.layers.len() > 1 {
                items.push(MenuItem::new(
                    t("图层", "layer"),
                    MenuAction::Sub(self.layer_items("edge")),
                ));
            }
            items.push(cmd(t("清掉折点", "clear waypoints"), "via off"));
            items.push(cmd(t("删除这条边", "delete this edge"), "del"));
            return items;
        }
        if pick.box_at().is_some() {
            let (boxes, _) = self.menu_scope();
            let style = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].style));
            let fill_raw = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].fill));
            let border_raw = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].color));
            let text_raw = Self::common(
                boxes
                    .iter()
                    .flat_map(|i| self.doc.text(*i).map(|t| t.color)),
            );
            let (fill, no_fill) = (fill_raw.flatten(), fill_raw == Some(None));
            let (border, auto_border) = (border_raw.flatten(), border_raw == Some(None));
            let (text, auto_text) = (text_raw.flatten(), text_raw == Some(None));
            let align = Self::common(
                boxes
                    .iter()
                    .flat_map(|i| self.doc.text(*i).map(|t| t.align)),
            );
            let text_bg_raw =
                Self::common(boxes.iter().flat_map(|i| self.doc.text(*i).map(|t| t.bg)));
            let (text_bg, text_bg_auto) = (text_bg_raw.flatten(), text_bg_raw == Some(None));
            let mut items = vec![
                MenuItem::new(
                    t("框型", "box style"),
                    MenuAction::Sub(vec![
                        MenuItem::new("point", MenuAction::Cmd("box point".into()))
                            .checked(style == Some(BoxStyle::Point)),
                        MenuItem::new("none", MenuAction::Cmd("box none".into()))
                            .checked(style == Some(BoxStyle::None)),
                        MenuItem::new("single", MenuAction::Cmd("box single".into()))
                            .checked(style == Some(BoxStyle::Single)),
                        MenuItem::new("rounded", MenuAction::Cmd("box rounded".into()))
                            .checked(style == Some(BoxStyle::Rounded)),
                        MenuItem::new("double", MenuAction::Cmd("box double".into()))
                            .checked(style == Some(BoxStyle::Double)),
                        MenuItem::new("heavy", MenuAction::Cmd("box heavy".into()))
                            .checked(style == Some(BoxStyle::Heavy)),
                    ]),
                ),
                MenuItem::new(
                    t("边框色", "border colour"),
                    MenuAction::Sub(self.color_items(
                        "nodecolor",
                        "auto",
                        "nodecolor auto",
                        border,
                        auto_border,
                    )),
                ),
                MenuItem::new(
                    t("填充", "fill"),
                    MenuAction::Sub(self.color_items("fill", "none", "fill none", fill, no_fill)),
                ),
                MenuItem::new(
                    t("文字色", "text colour"),
                    MenuAction::Sub(self.color_items(
                        "textcolor",
                        "auto",
                        "textcolor auto",
                        text,
                        auto_text,
                    )),
                ),
                MenuItem::new(
                    t("文字底色", "text background"),
                    MenuAction::Sub(self.color_items(
                        "textbg",
                        "none",
                        "textbg none",
                        text_bg,
                        text_bg_auto,
                    )),
                ),
                MenuItem::new(
                    t("对齐", "align"),
                    MenuAction::Sub(vec![
                        MenuItem::new("left", MenuAction::Cmd("textalign left".into()))
                            .checked(align == Some(Align::Left)),
                        MenuItem::new("center", MenuAction::Cmd("textalign center".into()))
                            .checked(align == Some(Align::Center)),
                        MenuItem::new("right", MenuAction::Cmd("textalign right".into()))
                            .checked(align == Some(Align::Right)),
                    ]),
                ),
            ];
            if self.doc.layers.len() > 1 {
                items.push(MenuItem::new(
                    t("图层", "layer"),
                    MenuAction::Sub(self.layer_items("box")),
                ));
            }
            items.push(cmd(t("删除", "delete"), "del"));
            return items;
        }
        vec![
            MenuItem::new(t("新建框…", "new box…"), MenuAction::Cmd(String::new())),
            MenuItem::new(t("居中", "centre"), MenuAction::Cmd(String::new())),
        ]
    }

    /// 颜色菜单(只一层):框给边框色,边给边颜色
    pub(super) fn color_menu_for(&self) -> Vec<MenuItem> {
        let pick = self.view.pick;
        if pick.edge().is_some() {
            let (_, edges) = self.menu_scope();
            let cur = Self::common(edges.iter().map(|e| self.doc.edges[*e].color));
            let auto = cur == Some(None);
            return self.color_items("edgecolor", "auto", "edgecolor auto", cur.flatten(), auto);
        }
        if pick.box_at().is_some() {
            let (boxes, _) = self.menu_scope();
            let cur = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].color));
            let auto = cur == Some(None);
            return self.color_items("nodecolor", "auto", "nodecolor auto", cur.flatten(), auto);
        }
        Vec::new()
    }

    /// s:在光标处打开完整样式菜单(不动光标)
    pub(super) fn open_style_menu(&mut self) {
        self.open_menu_here(false);
    }

    /// c:直接打开颜色子菜单
    pub(super) fn open_color_menu(&mut self) {
        self.open_menu_here(true);
    }

    /// 键盘打开菜单:位置贴着光标格右下,快照与右键那条路完全一样
    pub(super) fn open_menu_here(&mut self, color_only: bool) {
        // 光标下什么都没有:菜单没有可作用的元素
        if self.view.link.is_none() && self.view.pick == render::Pick::None {
            let note = self.view.lang.pick_owned(
                "光标下没有可改样式的元素".to_string(),
                "nothing under the cursor to restyle".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let items = if color_only {
            self.color_menu_for()
        } else {
            self.menu_for()
        };
        if items.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可改样式的元素".to_string(),
                "nothing under the cursor to restyle".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let at = ((cx.max(0) as u16).saturating_add(2), cy.max(0) as u16);
        let kind = self.menu_kind(color_only);
        let (levels, parents, parents_sel, sel) = self.menu_from_memory(items, &kind);
        self.view.menu = Some(Menu {
            at,
            levels,
            sel,
            parents_sel,
            kind,
            snapshot: self.doc.clone(),
            snapshot_cursor: (self.view.cursor, self.view.pick),
            parents,
            title: if color_only {
                Some(if self.view.pick.edge().is_some() {
                    self.view.lang.pick("颜色", "colour").to_string()
                } else {
                    self.view.lang.pick("边框色", "border colour").to_string()
                })
            } else {
                None
            },
        });
        self.menu_preview();
    }

    /// 右键:把光标挪到点击格,弹菜单
    pub(super) fn open_menu(&mut self, col: u16, row: u16) {
        let p = render::panels(&self.doc, self.area, &self.view);
        let inside = col >= p.map.x
            && col < p.map.x + p.map.width
            && row >= p.map.y
            && row < p.map.y + p.map.height;
        // 连边模式弹菜单不动光标:目标就是当前那个,别被右键位置带跑
        if inside && self.view.link.is_none() {
            self.view.cursor = p.proj.canvas(col as i32, row as i32);
            self.view.cand = 0;
            self.refresh_pick();
        }
        let kind = self.menu_kind(false);
        let (levels, parents, parents_sel, sel) = self.menu_from_memory(self.menu_for(), &kind);
        self.view.menu = Some(Menu {
            at: (col, row),
            levels,
            sel,
            parents_sel,
            kind,
            snapshot: self.doc.clone(),
            snapshot_cursor: (self.view.cursor, self.view.pick),
            title: None,
            parents,
        });
        // 一进来就把第一项套上:不用点就能看到效果
        self.menu_preview();
    }

    /// 把当前选中项"预演"到文档上:先回到弹菜单时的样子,再套用这一项
    pub(super) fn menu_preview(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel).cloned() else {
            return;
        };
        self.doc = menu.snapshot.clone();
        self.view.cursor = menu.snapshot_cursor.0;
        self.view.pick = menu.snapshot_cursor.1;
        if let MenuAction::Cmd(line) = &item.action {
            if !line.is_empty() {
                self.previewing = true;
                self.cmd_apply(line);
                self.previewing = false;
            }
        }
        // Link 那两个按钮不动文档,没有预览
        let path = {
            let mut p = menu.parents.clone();
            if let Some(t) = &menu.title {
                p.insert(0, t.clone());
            }
            p.push(item.title.clone());
            p.join(" › ")
        };
        let note = if matches!(item.action, MenuAction::Sub(_)) {
            self.view.lang.pick_owned(
                format!("{path}   l 进入 / h 返回 / Enter 应用 / esc 取消"),
                format!("{path}   l open / h back / Enter apply / esc cancel"),
            )
        } else {
            self.view.lang.pick_owned(
                format!("{path}   (预览;Enter 应用 / h 返回 / esc 取消)"),
                format!("{path}   (preview; Enter applies / h back / esc cancels)"),
            )
        };
        self.view.note = Some(note);
    }

    /// 关掉菜单:文档回到弹菜单之前
    pub(super) fn menu_cancel(&mut self) {
        if let Some(menu) = self.view.menu.take() {
            self.doc = menu.snapshot;
            self.view.cursor = menu.snapshot_cursor.0;
            self.view.pick = menu.snapshot_cursor.1;
            self.refresh_pick();
            self.view.note = None;
        }
    }

    /// 菜单上执行一项:子菜单就进一层,动词就执行
    pub(super) fn menu_choose(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel).cloned() else {
            self.view.menu = None;
            return;
        };
        let snapshot = menu.snapshot.clone();
        let snapshot_cursor = menu.snapshot_cursor;
        match item.action {
            MenuAction::Sub(items) => {
                if let Some(m) = self.view.menu.as_mut() {
                    m.levels.push(items);
                    m.parents.push(item.title.clone());
                    m.parents_sel.push(m.sel);
                    m.sel = 0;
                }
                self.menu_preview();
            }
            MenuAction::Cmd(line) if line.is_empty() => {
                // 空命令是"要输入"的那几项
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                match item.title.as_str() {
                    t if t.starts_with("新建框") => {
                        self.open_prompt(Prompt::NewBox, String::new());
                    }
                    t if t.starts_with("改文字") => {
                        if let Some(i) = self.cursor_box() {
                            let text = self.doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
                            self.open_prompt(Prompt::Text(vec![i]), text);
                        }
                    }
                    _ => self.center_cursor(),
                }
            }
            MenuAction::Link(ok) => {
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                self.view.pick = snapshot_cursor.1;
                if ok {
                    self.link_commit();
                } else {
                    self.cancel_link();
                }
            }
            MenuAction::Cmd(line) => {
                // 记住这次选的路径:下次开同一个菜单,直接进到这一层、停在这一项
                let mut path: Vec<(String, usize)> = menu
                    .parents
                    .iter()
                    .cloned()
                    .zip(menu.parents_sel.iter().copied())
                    .collect();
                path.push((item.title.clone(), menu.sel));
                self.menu_memory.insert(menu.kind.clone(), path);
                // 回到弹菜单之前,再正常执行一次:预览不算数,这一步才进撤销
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                // 菜单是在这个元素上开的:框和边同格时 refresh 会挑框,这里把类型保住
                self.view.pick = snapshot_cursor.1;
                self.cmd_apply(&line);
            }
        }
    }

    /// 菜单打开时的按键
    pub(super) fn handle_menu(&mut self, code: KeyCode) {
        let Some(menu) = self.view.menu.as_mut() else {
            return;
        };
        let n = menu.level().len();
        match code {
            // h / Esc / ←:回上一层菜单,顶层就关掉
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if menu.levels.len() > 1 {
                    menu.levels.pop();
                    menu.parents.pop();
                    menu.sel = menu.parents_sel.pop().unwrap_or(0);
                    // 回到上一层:把预览收回去,并按这一层的当前项重新预演
                    self.menu_preview();
                } else {
                    self.menu_cancel();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if n > 0 {
                    menu.sel = (menu.sel + n - 1) % n;
                }
                self.menu_preview();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    menu.sel = (menu.sel + 1) % n;
                }
                self.menu_preview();
            }
            // Enter:应用当前项(子菜单就进去,叶子就执行)
            KeyCode::Enter => self.menu_choose(),
            // l / →:只往下钻一层;叶子项不执行(要执行按 Enter)
            KeyCode::Right | KeyCode::Char('l') => self.menu_enter_level(),
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let i = c as usize - '1' as usize;
                if i < n {
                    menu.sel = i;
                    self.menu_choose();
                }
            }
            _ => {}
        }
    }

    /// 菜单里按 l / →:当前项是子菜单才进去,叶子项什么都不做
    pub(super) fn menu_enter_level(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel) else {
            return;
        };
        if matches!(item.action, MenuAction::Sub(_)) {
            self.menu_choose();
        }
    }

    /// 记住"最近选中/点到的框"与"最近选中的边":新建元素时照抄它们的样式.
    /// 区块选区(拖框/`v`)选中的是一片,不是一个元素,这时不更新模板
    pub(super) fn remember_look(&mut self) {
        if self.view.sel.is_some() {
            return;
        }
        if let render::Pick::Box(i) = self.view.pick {
            self.box_look = self.look_of_box(i);
        }
        if let render::Pick::Edge(e) = self.view.pick {
            if e < self.doc.edges.len() {
                let ed = &self.doc.edges[e];
                self.edge_look = Some(EdgeLook {
                    group: ed.group.clone(),
                    style: ed.style,
                    corner: ed.corner,
                    color: ed.color,
                    bg: ed.bg,
                    head: ed.head,
                });
            }
        }
    }

    /// 某个框现在的样式快照
    pub(super) fn look_of_box(&self, i: usize) -> Option<BoxLook> {
        if i >= self.doc.boxes.len() {
            return None;
        }
        let b = &self.doc.boxes[i];
        let t = self.doc.text(i);
        Some(BoxLook {
            style: b.style,
            color: b.color,
            fill: b.fill,
            text_color: t.and_then(|t| t.color),
            text_bg: t.and_then(|t| t.bg),
            align: t.map(|t| t.align).unwrap_or_default(),
        })
    }

    /// 刚用菜单/命令改过样式的那个框:也算"选中",新建框时照抄它
    pub(super) fn remember_box_look(&mut self, i: usize) {
        if let Some(look) = self.look_of_box(i) {
            self.box_look = Some(look);
        }
    }

    /// 新建框该照抄的样式:光标下这个框(实时取),否则用最近改过/选中的那个框
    pub(super) fn box_look_now(&self) -> Option<BoxLook> {
        if self.view.sel.is_some() {
            return None;
        }
        self.view
            .pick
            .box_at()
            .and_then(|i| self.look_of_box(i))
            .or(self.box_look)
    }

    /// 新建边时该照抄的样式:最近选中的那条边;区块选区时为空(那不算选中一条边)
    pub(super) fn edge_look_now(&self) -> Option<EdgeLook> {
        if self.view.sel.is_some() {
            return None;
        }
        self.view
            .pick
            .edge()
            .filter(|&e| e < self.doc.edges.len())
            .map(|e| {
                let ed = &self.doc.edges[e];
                EdgeLook {
                    group: ed.group.clone(),
                    style: ed.style,
                    corner: ed.corner,
                    color: ed.color,
                    bg: ed.bg,
                    head: ed.head,
                }
            })
            .or_else(|| self.edge_look.clone())
    }

    /// 把照抄来的样式落到新建的边上(分组在 add_edge 时已经定了)
    pub(super) fn apply_edge_look(&mut self, e: usize, look: &EdgeLook) {
        if e >= self.doc.edges.len() {
            return;
        }
        let ed = &mut self.doc.edges[e];
        ed.style = look.style;
        ed.corner = look.corner;
        ed.color = look.color;
        ed.bg = look.bg;
        ed.head = look.head;
    }
}
