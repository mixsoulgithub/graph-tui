//! state 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    pub fn new(doc: Document, opts: Options) -> Result<App, String> {
        let mut app = App {
            doc,
            view: View::default(),
            source: None,
            area: Rect::default(),
            hit: render::FrameHit::default(),
            first_frame: true,
            quit: false,
            undo: History::new(200),
            previewing: false,
            clipboard: None,
            pending: None,
            jumps: Vec::new(),
            jump_at: 0,
            seq: String::new(),
            open_request: None,
            opener: opts.opener.clone(),
            links: opts.links.clone(),
            menu_memory: std::collections::HashMap::new(),
            last_click: None,
            note_draft: Vec::new(),
            edit_request: None,
            drag: None,
            search_from: (0, 0),
            search_focus: Vec::new(),
            src_stamp: None,
            rev: 0,
            src_rev: 0,
            reload_armed: None,
            tabs: vec![None],
            tab_at: 0,
            keys: opts.keys.clone(),
            mouse: opts.mouse,
            spacing: opts.spacing,
            glyphs: opts.glyphs,
            cwd: std::env::current_dir()
                .map(|p| crate::paths::tildify(&p.to_string_lossy()))
                .unwrap_or_default(),
            box_look: None,
            edge_look: None,
        };
        app.view_from_doc();
        app.view.glyphs = app.glyphs;
        // 配置里的默认墨色:没写颜色的边/文字用它
        app.view.edge_ink = opts.edge_ink;
        app.view.text_ink = opts.text_ink;
        // 打开就 1:1 并且整幅居中(文件里存的视角只管导出,交互打开不恢复)
        app.reset_view();
        // 命令行显式给的视角优先于文件里的视图状态.
        // layout 只改显示与 r 的循环顺序:打开文件不重算坐标.
        if let Some(l) = opts.layout {
            app.view.layout = l;
        }
        if opts.labels {
            app.view.labels = true;
        }
        app.view.notes = opts.notes.apply(app.view.notes);
        app.view.drawn = opts.drawn;
        if let Some(n) = opts.history {
            app.undo = History::new(n);
        }
        if let Some(a) = opts.arrow {
            app.view.arrow_style = a;
        }
        // Options.lang 没有"没给"的表示:只有显式给了非默认语言才盖过文件
        if opts.lang != Lang::default() {
            app.view.lang = opts.lang;
        }
        // --at:按名字/别名找,找不着就把话说在提示行上,不拦着开局
        if let Some(name) = &opts.at {
            match find_box(&app.doc, name) {
                Some(i) => app.jump_to(i),
                None => {
                    let note = app.view.lang.pick_owned(
                        format!("没有叫 {name} 的框"),
                        format!("no box named {name}"),
                    );
                    app.view.note = Some(note);
                }
            }
        }
        // --find:等于按 / 输入再回车,命中留着可以按 n 跳
        if let Some(text) = &opts.find {
            if !text.trim().is_empty() {
                app.view.query = Some(text.clone());
                refresh(&app.doc, &mut app.view);
                app.view.query = None;
                if app.view.hits.is_empty() {
                    let note = app
                        .view
                        .lang
                        .pick_owned(format!("没搜到 {text}"), format!("nothing matched {text}"));
                    app.view.note = Some(note);
                }
            }
        }
        app.search_from = app.view.cursor;
        if !app.doc.boxes.is_empty() {
            app.note_here();
        }
        Ok(app)
    }

    /// 打开文件后把视角归位:和按 `0` 完全一样(1:1、居中到整幅).
    /// 文档里存的 zoom/center 仍然会被 `--dump` / 导出用到,但交互打开时不恢复
    pub fn reset_view(&mut self) {
        self.view.zoom = 1.0;
        self.view.center = None;
        self.view.target = None;
        self.view.pan = None;
    }

    /// 把文档里的 view 状态映射进 View,并清掉不属于文档的临时状态
    pub(super) fn view_from_doc(&mut self) {
        let z = self.doc.view.zoom;
        self.view.zoom = if z.is_finite() && z > 0.0 {
            clamp_zoom(z)
        } else {
            1.0
        };
        let c = self.doc.view.center;
        self.view.center = if c.0.is_finite() && c.1.is_finite() {
            Some(c)
        } else {
            None
        };
        let layout =
            Layout::parse(&self.doc.view.layout).unwrap_or_else(|| crate::layout::pick(&self.doc));
        self.view.layout = layout;
        self.view.labels = self.doc.view.labels;
        self.view.notes = self.doc.view.notes;
        if let Some(l) = Lang::parse(&self.doc.view.lang) {
            self.view.lang = l;
        }
        self.view.layer = self.doc.view.layer.clone();
        self.view.side = if self.doc.view.side.is_finite() {
            self.doc.view.side
        } else {
            2.0
        };
        self.view.cycle = self.doc.view.cycle.filter(|c| c.is_finite());
        self.view.orientation = layout::Orientation::parse(&self.doc.view.orientation)
            .unwrap_or(layout::Orientation::Auto);
        self.view.angle = self.view.orientation.degrees();
        let focus: Vec<usize> = self
            .doc
            .view
            .focus
            .iter()
            .filter_map(|id| self.doc.group_idx(id))
            .collect();
        self.view.focus = focus;
        // 文档里存的是光标格(v1 存的是框 id,这里都换成格子)
        self.view.cursor = self.doc.view.cursor_cell(&self.doc);
        self.refresh_pick();
        // 打开文档不再自动"铺满":缩放是文档状态(新文档是 1:1),字符格放大缩小都不会改变字形,
        // 铺满只会把图压成看不清的方块.想看全图按 F.
        // 这些不属于文档:换文档时一律清空
        self.view.query = None;
        self.view.hits.clear();
        self.view.hit_at = 0;
        self.view.sel = None;
        self.view.cmd = None;
        self.view.help = false;
        self.view.target = None;
        self.view.pan = None;
        self.view.pulse = 0.0;
        self.view.started = Instant::now();
        self.pending = None;
        self.clear_seq();
        self.drag = None;
        self.search_from = self.view.cursor;
        self.search_focus = self.view.focus.clone();
    }

    /// 重新算"光标格下面是什么";候选边按下标挑
    pub fn refresh_pick(&mut self) {
        self.sync_hit();
        // 隔离的中心跟着光标走:挪到另一个**可见**节点就换中心(隐藏的框当空白).
        // 光标压在注释框上时,亮的是它绑的那条边的"两头"(按边看)
        if self.view.ego.is_some() {
            if let Some(n) = render::box_at(&self.doc, self.view.cursor, &self.view) {
                let n = match self.doc.edge_of_box(n) {
                    Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
                    None => n,
                };
                if self.view.ego != Some(n) {
                    self.view.ego = Some(n);
                    self.view.ego_id = Some(self.doc.boxes[n].id.clone());
                }
            }
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        self.view.pick =
            render::pick_at(&self.doc, &self.hit, self.view.cursor, screen, &self.view);
        if let render::Pick::Edge(_) = self.view.pick {
            // 候选表按屏幕格记;这一格压着好几条边时用 cand 轮.
            // E 模式只认"高亮中心连着的那几条边":别的边从这儿路过也不闪,
            // 一组边挤在同一个时候才能先看到中心自己的那些
            let cands: Vec<usize> = self
                .hit
                .candidates_at(screen)
                .iter()
                .copied()
                .filter(|e| self.ego_lit_edge(*e))
                .collect();
            if cands.is_empty() {
                self.view.pick = render::Pick::None;
            } else if cands.len() > 1 {
                let e = cands[self.view.cand % cands.len()];
                self.view.pick = render::Pick::Edge(e);
            }
        }
        self.remember_look();
    }

    /// 命中表只对生成它的那一版文档有效(里面存的是数组下标);
    /// 文档改过就整张作废,别拿旧下标去选边
    pub(super) fn sync_hit(&mut self) {
        if self.hit.rev != self.doc.rev() {
            self.hit = render::FrameHit::default();
            self.view.cand = 0;
        }
    }

    /// 当前这一版可用的命中表(对不上就当没有)
    pub(super) fn usable_hit(&self) -> &render::FrameHit {
        static EMPTY: std::sync::OnceLock<render::FrameHit> = std::sync::OnceLock::new();
        if self.hit.rev == self.doc.rev() {
            &self.hit
        } else {
            EMPTY.get_or_init(render::FrameHit::default)
        }
    }

    /// 有没有动画在跑(没有就可以少画几帧)
    pub fn busy(&self) -> bool {
        // 有选中元素就要一直推进呼吸动画;平移缓动同理
        self.view.target.is_some()
            || self.view.pan.is_some()
            || self.view.pick != render::Pick::None
            || self.view.sel.is_some()
    }

    /// 按时间推进动画(平移缓动,光标呼吸).返回"这一帧和上一帧可能不一样吗":
    /// 呼吸量化成有限档,缓动也要真的动了才需要重画 —— 否则每秒几十次全屏 diff
    /// 全是在往终端写同样的字节.
    pub fn tick(&mut self) -> bool {
        let before = self.view.pulse;
        let phase = self.view.started.elapsed().as_secs_f64() / BEAT;
        let q = (pulse_at(phase) * PULSE_STEPS).round() / PULSE_STEPS;
        self.view.pulse = q;
        let (center, zoom) = (self.view.center, self.view.zoom);
        let cell = render::panels(&self.doc, self.area, &self.view).proj.scale;
        animate(&mut self.view, &self.doc, cell);
        (q - before).abs() > f64::EPSILON || self.view.center != center || self.view.zoom != zoom
    }

    /// 画一帧(顺手把画面区域记下来,鼠标换算要用)
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        self.area = area;
        // 状态栏要显示 [n/m] 与脏标记
        self.view.tab = (self.tabs.len() > 1).then_some((self.tab_at + 1, self.tabs.len()));
        self.view.tab_name = self.tab_name();
        self.view.tab_dirty = self.dirty();
        // 上行的文件路径与工作目录:`~` 缩写
        self.view.file_display = self
            .source
            .as_deref()
            .map(|p| crate::paths::tildify(&crate::paths::absolute_display(p)));
        self.view.cwd = self.cwd.clone();
        // 删掉/撤销之后下标会漂移:按 id 把中心认回来,认不回就退出隔离
        if let Some(n) = self.view.ego {
            let id = self.view.ego_id.clone().unwrap_or_default();
            let same = n < self.doc.boxes.len() && self.doc.boxes[n].id == id;
            if !same {
                match self.doc.box_idx(&id) {
                    Some(k) => self.view.ego = Some(k),
                    None => {
                        self.view.ego = None;
                        self.view.ego_id = None;
                    }
                }
            }
        }
        // 选区里已经什么都没有了(比如把选中的框删了):自动取消,
        // 效果和"点选区外面"一样,不给编辑留一块空选区
        if self.view.sel.is_some_and(|s| s.confirmed) {
            let p = render::panels(&self.doc, area, &self.view);
            let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, &self.hit);
            if boxes.is_empty() && edges.is_empty() {
                self.view.sel = None;
            }
        }
        // 打开文件时光标可能不在视口里:首帧把它居中一次
        if self.first_frame {
            self.first_frame = false;
            if self.cursor_cell().is_none() && !self.doc.boxes.is_empty() {
                self.view.center = Some((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
                self.view.target = None;
            }
        }
        // 上一帧的命中表借出去给这一帧用;挪走再放回,省掉每帧一次深拷贝
        let prev = std::mem::take(&mut self.hit);
        let (_, hit) = render::render_hit(buf, area, &self.doc, &self.view, &prev);
        // 状态栏要提示"有名字没画下":用上一帧的统计,一帧延迟看不出来
        self.view.labels_hidden = hit.labels_skipped.len();
        self.hit = hit;
        if self.view.help {
            render::render_help(buf, area, render::help_keys(self.view.lang), self.view.lang);
        }
        if self.view.file_pick.is_some() {
            render::render_pick(buf, area, &self.view);
        }
    }

    /// 缩放变化后用回显位说一句(状态行只剩节点名了)
    pub(super) fn note_zoom(&mut self) {
        let pct = (self.view.zoom * 100.0).round() as i64;
        let note = self
            .view
            .lang
            .pick_owned(format!("缩放 {pct}%"), format!("zoom {pct}%"));
        self.view.note = Some(note);
    }

    /// 把坐标转到绝对角度 deg(要求 1:1)
    pub(super) fn rotate_to(&mut self, o: layout::Orientation, deg: i32) {
        self.require_edit_scale();
        self.begin_edit("转方向");
        let k = (deg - self.view.angle).rem_euclid(360) / 90;
        if k != 0 {
            self.doc.rotate_quarters(k);
        }
        self.view.orientation = o;
        self.view.angle = deg.rem_euclid(360);
        let note = self.view.lang.pick_owned(
            format!("方向 = {}(坐标已转)", self.view.angle),
            format!("orientation = {} (coordinates rotated)", self.view.angle),
        );
        self.view.note = Some(note);
        self.end_edit();
    }

    /// 想退出时给一句:退出只认 :q
    pub(super) fn note_quit_hint(&mut self) {
        self.view.note = Some(
            self.view
                .lang
                .pick_owned("退出用 :q".to_string(), "quit with :q".to_string()),
        );
    }

    /// 把 View 里属于文档的状态写回 doc.view(存盘前必须做)
    pub(super) fn sync_view(&mut self) {
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let cursor = Some(crate::document::Cursor::Cell(self.view.cursor));
        let focus: Vec<String> = self
            .view
            .focus
            .iter()
            .filter_map(|&g| self.doc.groups.get(g).map(|x| x.id.clone()))
            .collect();
        // 文件里的 view.layer 必须引用已存在的图层,否则下次打不开
        let layer = self
            .view
            .layer
            .clone()
            .filter(|l| self.doc.layers.iter().any(|x| &x.name == l));
        let view = ViewState {
            center,
            zoom: self.view.zoom,
            layout: self.view.layout.name().to_string(),
            cursor,
            focus,
            labels: self.view.labels,
            lang: self.view.lang.code().to_string(),
            layer,
            side: self.view.side,
            cycle: self.view.cycle,
            orientation: self.view.orientation.name().to_string(),
            notes: self.view.notes,
        };
        self.doc.view = view;
    }

    /// 换布局:调 layout::apply 写回整数单元格坐标.任意缩放下都可执行;
    /// 一直保持 1:1.
    pub(super) fn apply_layout(&mut self, l: Layout) {
        self.begin_edit("换布局");
        match layout::apply_with(&mut self.doc, l, self.spacing) {
            Ok(report) => {
                let k = self.layout_quarters(l);
                if k != 0 {
                    self.doc.rotate_quarters(k);
                }
                self.view.angle = self.view.orientation.degrees();
                self.view.layout = l;
                self.view.pan = None;
                self.view.target = None;
                self.note_layout(l, &report);
            }
            Err(e) => self.view.note = Some(e),
        }
        self.end_edit();
    }

    /// 换布局后把名字说在提示行上;布局报告里有告警就把前三条接在后面
    pub(super) fn note_layout(&mut self, l: Layout, report: &layout::LayoutReport) {
        // auto:把实际选中的布局一起报出来(auto(tree) / auto(layered))
        let n = if l == Layout::Auto && !report.profile.is_empty() {
            format!("auto({})", report.profile)
        } else {
            l.name().to_string()
        };
        let mut note = self
            .view
            .lang
            .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
        let _ = &n;
        if !report.warnings.is_empty() {
            note.push_str("  (");
            note.push_str(&report.summary(3));
            note.push(')');
        }
        self.view.note = Some(note);
    }

    /// r:轮到下一套布局
    pub(super) fn cycle_layout(&mut self) {
        let l = self.view.layout.next();
        self.apply_layout(l);
    }
}
