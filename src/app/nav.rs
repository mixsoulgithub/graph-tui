//! nav 相关实现.函数体从 app.rs 原样搬来.

use super::*;

impl App {
    /// 拖拽时靠近视口边缘就自动平移:离边 2 格以内,越靠边挪得越多(最多 3 格).
    /// 只在拖拽过程中调用,普通鼠标移动不挪视图
    pub(super) fn autopan(&mut self, col: u16, row: u16) {
        let p = render::panels(&self.doc, self.area, &self.view);
        if p.map.width == 0 || p.map.height == 0 {
            return;
        }
        let (x0, y0) = (p.map.x as i32, p.map.y as i32);
        let (x1, y1) = (x0 + p.map.width as i32 - 1, y0 + p.map.height as i32 - 1);
        let (c, r) = (col as i32, row as i32);
        let edge_dist = |v: i32, lo: i32, hi: i32| -> i32 {
            if v < lo + 2 {
                (lo + 2 - v).min(3)
            } else if v > hi - 2 {
                (v - (hi - 2)).min(3)
            } else {
                0
            }
        };
        // 正数 = 视图往左/上推(内容往右/下走)
        let dx = edge_dist(c, x0, x1);
        let dy = edge_dist(r, y0, y1);
        if dx == 0 && dy == 0 {
            return;
        }
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let proj = p.proj;
        self.view.center = Some(render::pan_center(&proj, center, dx, dy));
        self.view.target = None;
        self.view.pan = None;
    }

    /// 光标格当前的屏幕位置(缩放锚点)
    pub(super) fn cursor_screen_cell(&self) -> render::Cell {
        let p = render::panels(&self.doc, self.area, &self.view);
        p.proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64))
    }

    /// 以屏幕上的 `anchor` 格为锚点换缩放:缩放前后那一格下面的画布坐标不变.
    /// 指针/光标就不会在这次缩放里"跑掉"
    pub(super) fn zoom_at(&mut self, zoom: f64, anchor: render::Cell) {
        let zoom = snap_zoom(clamp_zoom(zoom));
        if (zoom - self.view.zoom).abs() < 1e-9 {
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let point = p.proj.canvas_f(anchor.0, anchor.1);
        let (cx, cy) = p.proj.center_for(point, anchor.0, anchor.1, zoom);
        self.view.zoom = zoom;
        self.view.center = Some((cx, cy));
        self.view.target = None;
        self.view.pan = None;
        self.note_zoom();
    }

    /// 光标格下面是不是一个框
    pub(super) fn cursor_box(&self) -> Option<usize> {
        self.view.pick.box_at()
    }

    /// 现在选中的边:没选,或者下标已经越界都算没选
    /// 真实终端光标该放哪:命令/搜索输入时在输入行末尾,否则在光标框的文字起点.
    /// 不在视口内就返回 None(让光标藏起来,别被终端夹到边上)
    pub fn cursor_cell(&self) -> Option<(u16, u16)> {
        let area = self.area;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if self.view.help || self.view.file_pick.is_some() {
            return None;
        }
        let status_y = area.y + area.height - 1;
        if let Some((what, typed)) = &self.view.prompt {
            let prefix = match what {
                Prompt::NewBox => "框: ",
                Prompt::Text(_) => "文字: ",
                Prompt::Link(_) => "连到: ",
            };
            let shown = render::prompt_display(typed, self.view.lang);
            // 光标在 `prompt_pos` 处:多行内容被折成"第一行 [+N 行]",光标只按第一行算
            let first_line: String = typed
                .split('\n')
                .next()
                .unwrap_or("")
                .chars()
                .take(self.view.prompt_pos)
                .collect();
            let _ = &shown;
            let before = render::raw_width(&first_line);
            let x = area.x + render::raw_width(prefix) as u16 + before as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(c) = &self.view.cmd {
            // 状态行画的是 `:w hell` 这串本身,光标落在这串的右边一格.
            // 这里必须按原样宽度算:路径里的 `/`、文字里的 `*` 都不是标记
            let before: String = c.chars().take(self.view.cmd_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(q) = &self.view.query {
            let before: String = q.chars().take(self.view.query_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if self.doc.boxes.is_empty() {
            return None;
        }
        let p = render::panels(&self.doc, area, &self.view);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let (x, y) = (cx as u16, cy as u16);
        let inside =
            x >= p.map.x && x < p.map.x + p.map.width && y >= p.map.y && y < p.map.y + p.map.height;
        inside.then_some((x, y))
    }

    /// 这次重排该不该转 90 度(以及转几份).
    /// auto 只在 force / layered 上生效:geo 与 relax 的输入形状本身有方向含义
    pub(super) fn layout_quarters(&self, l: Layout) -> i32 {
        if let Some(k) = self.view.orientation.quarters() {
            return k;
        }
        if !matches!(l, Layout::Force | Layout::Layered) {
            return 0;
        }
        let area = render::panels(&self.doc, self.area, &self.view).map;
        let (x0, y0, x1, y1) = self.doc.bbox();
        let (gw, gh) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
        if gw <= 0.0 || gh <= 0.0 || area.width == 0 || area.height == 0 {
            return 0;
        }
        let cols = area.width as f64;
        let rows = area.height as f64;
        let normal = (gw / cols).max(gh / rows);
        let rotated = (gh / cols).max(gw / rows);
        // 差距不到 5% 就别转,免得临界点上左右横跳
        if rotated < normal * 0.95 {
            1
        } else {
            0
        }
    }

    /// i:光标下/选区里有东西就改它们的文字,什么都没有才新建框
    /// `w` / `b`:从光标往外射一条直线,跳到第一个撞到的**别的**框或边.
    ///
    /// 方向跟图的形状走:图比高宽就横着(前=右,后=左),否则竖着(前=下,后=上).
    /// 射线一直扫到整幅图的**边缘**,不是当前画面:目标在屏幕外也照样跳过去,
    /// 落点由 `goto_cell` 把视图滚过去.同一条线上什么都没有时(表格状布局常见),
    /// `w` / `b`:沿光标这一行向右 / 向左射一条直线(整幅图,不是当前画面),
    /// 撞到第一个框或边就停,视图跟着滚过去.
    ///
    /// 这一行上什么都没有时(竖排布局、表格状布局都常见)退一步:取"前 / 后"半区里
    /// 离光标最近的框 —— 前 = 右边或下边,后 = 左边或上边,距离量到**框边**.
    /// 所以竖着排的章节能靠 w / b 上下走,挂在旁边的离群框也能一步到.
    pub(super) fn jump_ray(&mut self, back: bool) {
        let (bx0, _, bx1, _) = self.doc.bbox();
        let p = render::panels(&self.doc, self.area, &self.view);
        let start_box = render::box_at(&self.doc, self.view.cursor, &self.view);
        let start_screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let start_edge = self.hit.edge_at(start_screen);
        // 射线扫到图的左右边界,不跟着视口走
        let steps = if back {
            self.view.cursor.0 - bx0 + 1
        } else {
            bx1 - self.view.cursor.0 + 1
        }
        .max(0);
        let dx = if back { -1 } else { 1 };
        // 命中表只覆盖当前画面(边是从画出来的笔画上量的):画面外只认框
        let (mx0, my0) = (p.map.x as i32, p.map.y as i32);
        let (mx1, my1) = (
            (p.map.x + p.map.width) as i32,
            (p.map.y + p.map.height) as i32,
        );
        let mut cell = self.view.cursor;
        for _ in 0..steps {
            cell = (cell.0 + dx, cell.1);
            // 先看框(框盖住的东西不算),再看边
            if let Some(b) = render::box_at(&self.doc, cell, &self.view) {
                if Some(b) != start_box {
                    self.goto_cell(cell);
                    return;
                }
                continue;
            }
            let screen = p.proj.cell((cell.0 as f64, cell.1 as f64));
            if screen.0 < mx0 || screen.1 < my0 || screen.0 >= mx1 || screen.1 >= my1 {
                continue;
            }
            if let Some(e) = self.hit.edge_at(screen) {
                if Some(e) != start_edge {
                    self.goto_cell(cell);
                    return;
                }
            }
        }
        // 这一行空了:前 / 后半区里最近的框(距离到框边,平手时同行的优先)
        let ego = render::ego_visible(&self.doc, &self.view);
        let cur = self.view.cursor;
        let mut best: Option<(i64, u8, usize, render::Cell)> = None;
        for i in 0..self.doc.boxes.len() {
            if Some(i) == start_box || self.doc.box_hidden(i) || render::ego_hides(ego.as_ref(), i)
            {
                continue;
            }
            // 光标是画布格,框的矩形也用画布的(别拿屏幕坐标比,缩放一挪就错)
            let b = &self.doc.boxes[i];
            let (x0, y0) = (b.at.0, b.at.1);
            let (w, h) = (b.size.0 as i32, b.size.1 as i32);
            let (x1, y1) = (x0 + w.max(1) - 1, y0 + h.max(1) - 1);
            let gap = |a: i32, lo: i32, hi: i32| {
                if a < lo {
                    lo - a
                } else if a > hi {
                    a - hi
                } else {
                    0
                }
            };
            let (gx, gy) = (gap(cur.0, x0, x1), gap(cur.1, y0, y1));
            // 前 = 右边或下边,后 = 左边或上边
            let ahead = if back { x1 < cur.0 } else { x0 > cur.0 };
            let below = if back { y1 < cur.1 } else { y0 > cur.1 };
            if !ahead && !below {
                continue;
            }
            let dist = (gx as i64) * (gx as i64) + (gy as i64) * (gy as i64);
            // 同一行/列的优先(射线本来就该先撞到它),再比距离
            let tie = u8::from(!ahead);
            let key = (dist, tie, i, self.cell_of(i));
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
        match best {
            Some((_, _, _, c)) => self.goto_cell(c),
            None => {
                let note = if back {
                    self.view.lang.pick_owned(
                        "这个方向后面没有框了".to_string(),
                        "no box behind the cursor".to_string(),
                    )
                } else {
                    self.view.lang.pick_owned(
                        "这个方向前面没有框了".to_string(),
                        "no box ahead of the cursor".to_string(),
                    )
                };
                self.view.note = Some(note);
            }
        }
    }

    /// 跳到某个框:挪光标,记进历史,再保证它还在画面里
    pub fn jump_to(&mut self, i: usize) {
        if i >= self.doc.boxes.len() {
            return;
        }
        // 跳到隔离藏起来的框:先退出隔离,不然落点看不见
        if render::ego_hides(render::ego_visible(&self.doc, &self.view).as_ref(), i) {
            self.view.ego = None;
        }
        let cell = self.cell_of(i);
        if cell == self.view.cursor {
            // 光标没动,但选中的元素可能变了(比如刚从边上跳回来)
            self.view.pick = render::Pick::Box(i);
            return;
        }
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.note_here();
        self.follow_cursor_jump();
    }

    /// 框的光标格:带框的落在文字起点,点节点就是它自己
    pub(super) fn cell_of(&self, i: usize) -> render::Cell {
        match self.doc.text(i) {
            Some(t) => t.at,
            None => (self.doc.boxes[i].at.0 + 1, self.doc.boxes[i].at.1 + 1),
        }
    }

    /// v / Ctrl-q:从光标格开始区块选择
    pub(super) fn start_rect_select(&mut self) {
        self.view.sel = Some(RectSel {
            anchor: self.view.cursor,
            end: self.view.cursor,
            confirmed: false,
        });
        let (x, y) = self.view.cursor;
        let note = self.view.lang.pick_owned(
            format!("区块选择:锚点 ({x},{y})(移动光标扩大,esc 取消)"),
            format!("block select from ({x},{y}) (move to grow, esc to cancel)"),
        );
        self.view.note = Some(note);
    }

    /// 隔离下看得见的命中:隐藏的框不算
    pub(super) fn visible_hits(&self) -> Vec<usize> {
        let ego = render::ego_visible(&self.doc, &self.view);
        self.view
            .hits
            .iter()
            .copied()
            .filter(|i| !render::ego_hides(ego.as_ref(), *i))
            .collect()
    }

    /// n / N:在命中表里按 dir 走一格,跳过被隔离藏起来的.
    /// 一个可见的都没有时说清楚,别让人以为跳转坏了
    pub(super) fn step_hit(&mut self, vis: &[usize], dir: isize) -> Option<usize> {
        let n = self.view.hits.len();
        if vis.is_empty() {
            if n > 0 {
                let note = self.view.lang.pick_owned(
                    "命中的框都被隔离藏起来了(E 退出后再看)".to_string(),
                    "every match is hidden by isolate (E to exit)".to_string(),
                );
                self.view.note = Some(note);
            }
            return None;
        }
        let mut idx = (self.view.hit_at as isize + dir).rem_euclid(n as isize) as usize;
        for _ in 0..n {
            let hit = self.view.hits[idx];
            if vis.contains(&hit) {
                self.view.hit_at = idx;
                return Some(hit);
            }
            idx = (idx as isize + dir).rem_euclid(n as isize) as usize;
        }
        None
    }

    /// z / 空格:把光标那个框挪到画面正中
    pub(super) fn center_cursor(&mut self) {
        let p = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, p);
    }

    /// F:整幅铺满;隔离开着时只铺可见的那几个节点
    pub(super) fn fit_all(&mut self) {
        if let Some(ego) = render::ego_visible(&self.doc, &self.view) {
            let p = render::panels(&self.doc, self.area, &self.view);
            let pts: Vec<(f64, f64)> = (0..self.doc.boxes.len())
                .filter(|&i| !render::ego_hides(Some(&ego), i))
                .map(|i| self.doc.center(i))
                .collect();
            if let Some((z, c)) = fit_to(p.map, pts.into_iter()) {
                self.view.zoom = z;
                self.view.center = Some(c);
                self.view.target = None;
                return;
            }
        }
        let zoom = render::zoom_for_fit(
            &self.doc,
            self.area.width,
            self.area.height,
            self.view.notes,
        );
        self.view.zoom = clamp_zoom(zoom);
        self.view.center = None;
        self.view.target = None;
    }

    /// f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
    pub(super) fn fit_focus(&mut self) {
        let p = render::panels(&self.doc, self.area, &self.view);
        let groups = match self.cursor_box() {
            Some(c) if self.view.focus.is_empty() => self.doc.box_groups(c).to_vec(),
            _ => self.view.focus.clone(),
        };
        let ego = render::ego_visible(&self.doc, &self.view);
        let pts: Vec<(f64, f64)> = (0..self.doc.boxes.len())
            .filter(|&i| {
                self.doc.box_groups(i).iter().any(|g| groups.contains(g))
                    && !render::ego_hides(ego.as_ref(), i)
            })
            .map(|i| self.doc.center(i))
            .collect();
        if let Some((z, c)) = fit_to(p.map, pts.into_iter()) {
            self.view.zoom = z;
            self.view.center = Some(c);
            self.view.target = None;
        }
    }

    /// 键盘挪光标:贴到边距时把画面挪一小段,挪完光标离边距还有几格缓冲.
    /// 一次挪一块(而不是每挪一行就拉一次画面),也不把光标拽到正中间
    pub(super) fn follow_cursor(&mut self) {
        // 还没画过一帧就没有视口,无从判断光标在不在画面里
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // 缓动还没走到目标时按目标算:不然每按一下都还在边距外,会被一路推着走
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        // 留两格边距:光标贴着边界时就把画面挪过去,不要等它掉出去
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // 像 nvim 那样:贴到边距之后,光标在屏幕上就钉在那儿,只有逻辑位置在走,
        // 画面一格一格跟着滚 —— 所以这里只补"越出去的那一点",不多挪.
        // 而且**当场落位**,不走缓动:缓动的话光标会先跟着逻辑位置走一格,
        // 再被追上来的画面带回原处,看着就是"先动一下又弹回来"
        let zoom = self.view.zoom.max(ZOOM_MIN);
        let cur = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let mut want = cur;
        if cx < x0 {
            want.0 = cur.0 + (cx - x0) as f64 / zoom;
        } else if cx > x1 {
            want.0 = cur.0 + (cx - x1) as f64 / zoom;
        }
        if cy < y0 {
            want.1 = cur.1 + (cy - y0) as f64 / zoom;
        } else if cy > y1 {
            want.1 = cur.1 + (cy - y1) as f64 / zoom;
        }
        self.view.center = Some(want);
        self.view.target = None;
        self.view.pan = None;
    }

    /// 序列变了就同步到右下角(showcmd)
    pub(super) fn show_seq(&mut self) {
        self.view.showcmd = self.seq.clone();
    }

    /// 作废当前序列
    pub(super) fn clear_seq(&mut self) {
        if self.seq.is_empty() && self.view.showcmd.is_empty() {
            return;
        }
        self.seq.clear();
        self.pending = None;
        self.show_seq();
    }

    /// 吃"还没定下来的按键":数字 / 逗号 / g / G.返回 true = 这一下已经用掉了
    pub(super) fn seq_key(&mut self, c: char) -> bool {
        if c.is_ascii_digit() {
            // 开头的 0 不吞:0 是"回到 1:1"
            if c == '0' && self.seq.is_empty() {
                return false;
            }
            self.seq.push(c);
            if self.seq.contains(',') {
                self.pending = None; // 坐标里的第二段不是计数
            } else {
                count_digit(&mut self.pending, c);
            }
            self.show_seq();
            return true;
        }
        if c == ',' {
            if self.seq.is_empty() || !self.seq.chars().all(|x| x.is_ascii_digit()) {
                return false;
            }
            self.seq.push(c);
            self.pending = None;
            self.show_seq();
            return true;
        }
        if c == 'g' {
            if self.seq.is_empty() {
                self.seq.push('g');
                self.show_seq();
                return true;
            }
            if self.seq == "g" {
                self.seq.clear();
                self.pending = None;
                self.show_seq();
                self.goto_cell((0, 0)); // gg:回到原点
                return true;
            }
            self.clear_seq();
            return false;
        }
        if c == 'f' && self.seq == "g" {
            // gf:打开光标下的链接(不是"铺满画面"那个 f)
            self.clear_seq();
            self.open_link();
            return true;
        }
        if c == 'G' {
            if self.seq.is_empty() {
                return false;
            }
            let cell = parse_goto(&self.seq, self.view.cursor);
            self.clear_seq();
            if let Some(cell) = cell {
                self.goto_cell(cell);
            }
            return true;
        }
        // 别的键:序列作废,按键照常处理
        if !self.seq.is_empty() {
            self.seq.clear();
            self.show_seq();
        }
        false
    }

    /// 跳到画布上的某个格子(gg / a,bG):记进跳转表,视图跟过去
    pub(super) fn goto_cell(&mut self, cell: (i32, i32)) {
        if self.view.cursor == cell {
            return;
        }
        self.note_here();
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor_jump();
    }

    /// 把"现在这里"记进跳转历史(Ctrl-o / Ctrl-i 前后翻).记的是 tab + 格子,
    /// 所以在另一个 buffer 里也能退回来
    pub(super) fn note_here(&mut self) {
        let cur = Jump {
            tab: self.tab_at,
            cell: self.view.cursor,
        };
        if self.jumps.get(self.jump_at) == Some(&cur) {
            return;
        }
        if self.jumps.last() == Some(&cur) {
            self.jump_at = self.jumps.len() - 1;
            return;
        }
        self.jumps.truncate(self.jump_at + 1); // 从历史中间跳走,后面那段丢掉
        self.jumps.push(cur);
        if self.jumps.len() > 64 {
            self.jumps.remove(0);
        }
        self.jump_at = self.jumps.len() - 1;
    }

    /// 回到历史里的上一个 / 下一个跳点(可能换 tab).返回 false 表示到头了
    pub(super) fn jump_history(&mut self, back: bool) -> bool {
        if self.jumps.is_empty() {
            return false;
        }
        if back {
            // 站在一个没记过的地方(走过来的):先把自己记上,再往回退,
            // 这样 Ctrl-o 永远回到"上次动手/搜索的地方",不是再往前一格
            let here = Jump {
                tab: self.tab_at,
                cell: self.view.cursor,
            };
            if self.jumps.get(self.jump_at) != Some(&here) {
                self.note_here();
            }
            if self.jump_at == 0 {
                return false;
            }
            self.jump_at -= 1;
        } else {
            if self.jump_at + 1 >= self.jumps.len() {
                return false;
            }
            self.jump_at += 1;
        }
        let j = self.jumps[self.jump_at];
        if j.tab != self.tab_at {
            // 跨 buffer:先把那个 tab 切回来,再落光标
            self.switch_tab(j.tab);
        }
        self.view.cursor = j.cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.view.note = None;
        self.follow_cursor_jump();
        true
    }

    /// 跳到别的地方(搜索 / 鼠标点 / 跳转历史):把光标往画面里带一把
    pub(super) fn follow_cursor_jump(&mut self) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // 缓动还没走完就按目标算:不然会以为光标还在画面外,反复重设目标
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // 跳到画面外:直接把光标放到正中.老的 0.85 拉近只适合"刚好越界",
        // 从 1000,1000 跳到 10,10 时它只走到半路,看着像没动
        let want = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, want);
    }

    /// 半屏跳:从光标往 dir 方向挪半屏,落到离那儿最近的框上
    pub(super) fn leap(&self, dir: (i32, i32)) -> usize {
        let p = render::panels(&self.doc, self.area, &self.view);
        let c = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let dx = dir.0 * (p.map.width as i32 / 2).max(1);
        let dy = dir.1 * (p.map.height as i32 / 2).max(1);
        let (ax, ay) = (p.map.x as i32, p.map.y as i32);
        let x_max = ax + p.map.width as i32 - 1;
        let y_max = ay + p.map.height as i32 - 1;
        let x = (c.0 + dx).clamp(ax, x_max.max(ax));
        let y = (c.1 + dy).clamp(ay, y_max.max(ay));
        render::node_at(&self.doc, &p, self.area, x as u16, y as u16)
            .or_else(|| {
                // 那儿没有框:挑离它最近的框
                (0..self.doc.boxes.len()).min_by_key(|&i| {
                    let (bx, by) = p.proj.cell(self.doc.center(i));
                    (bx - x).abs() + (by - y).abs()
                })
            })
            .unwrap_or(0)
    }

    /// 方向键移动:走一步挑屏幕上最近的那个框,连着按计数前缀就走几步
    /// 光标移动:一格一步,数字前缀就是格数
    pub(super) fn move_cursor(&mut self, dir: (i32, i32), steps: u32) {
        let steps = steps.max(1) as i32;
        self.view.cursor = (
            self.view.cursor.0 + dir.0 * steps,
            self.view.cursor.1 + dir.1 * steps,
        );
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor();
    }
}
