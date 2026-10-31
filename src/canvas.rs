//! 光栅化:把画布坐标画成"每格往哪几个方向连",再按方向位合成制表符.
//! 这一层只认识字符格,不认识 ratatui(上屏由 render 做).

use std::collections::{HashMap, HashSet};

use crate::model::{BoxStyle, Corner, EdgeStyle};

/// 节点框的一格:字形、颜色、框内填充、这个节点属于哪几个分组、节点下标(光标高亮用)
#[derive(Clone, Copy)]
pub(crate) struct Deco {
    pub(crate) ch: char,
    pub(crate) fg: (u8, u8, u8),
    /// 框内背景填充(压暗时由 blit 丢掉)
    pub(crate) bg: Option<(u8, u8, u8)>,
    pub(crate) groups: u64,
    pub(crate) node: i32,
}

/// 格子上的方向位:一格会和上下左右四个方向连
pub(crate) const LEFT: u8 = 1;
pub(crate) const RIGHT: u8 = 2;
pub(crate) const UP: u8 = 4;
pub(crate) const DOWN: u8 = 8;

// 方向位的组合(模式匹配里要当作一个整体,不能现算)
const LR: u8 = LEFT | RIGHT;
const UD: u8 = UP | DOWN;
const LRD: u8 = LEFT | RIGHT | DOWN;
const LRU: u8 = LEFT | RIGHT | UP;
const UDR: u8 = UP | DOWN | RIGHT;
const UDL: u8 = UP | DOWN | LEFT;
const LRUD: u8 = LEFT | RIGHT | UP | DOWN;
const DR: u8 = DOWN | RIGHT;
const DL: u8 = DOWN | LEFT;
const UR: u8 = UP | RIGHT;
const UL: u8 = UP | LEFT;

/// 边框上的连接点:横向 + 线从哪一面出去.双线/粗线各有一套字形
fn junction_glyph(style: BoxStyle, mask: u8) -> char {
    let double = style == BoxStyle::Double;
    let heavy = style == BoxStyle::Heavy;
    match mask {
        LRUD => {
            if double {
                '╬'
            } else if heavy {
                '╋'
            } else {
                '┼'
            }
        }
        LRD => {
            if double {
                '╦'
            } else if heavy {
                '┳'
            } else {
                '┬'
            }
        }
        LRU => {
            if double {
                '╩'
            } else if heavy {
                '┻'
            } else {
                '┴'
            }
        }
        UDR => {
            if double {
                '╠'
            } else if heavy {
                '┣'
            } else {
                '├'
            }
        }
        UDL => {
            if double {
                '╣'
            } else if heavy {
                '┫'
            } else {
                '┤'
            }
        }
        other => box_char(other, Corner::Rounded),
    }
}

/// 方向位 -> 制表符(横竖 + 三通四通 + 拐角).
/// 拐角可以是圆角 `╭╮╯╰` 或锐角 `┌┐└┘`,只有四个拐角模式受影响.
/// 单个方向不成笔画(路径至少有一步),落到兜底那档给个 '│'
pub(crate) fn box_char(mask: u8, corner: Corner) -> char {
    let sharp = corner == Corner::Sharp;
    match mask {
        0 => ' ',
        LRUD => '┼',
        LRD => '┬',
        LRU => '┴',
        UDR => '├',
        UDL => '┤',
        LR => '─',
        UD => '│',
        DR => {
            if sharp {
                '┌'
            } else {
                '╭'
            }
        }
        DL => {
            if sharp {
                '┐'
            } else {
                '╮'
            }
        }
        UR => {
            if sharp {
                '└'
            } else {
                '╰'
            }
        }
        UL => {
            if sharp {
                '┘'
            } else {
                '╯'
            }
        }
        // 只剩一个方向位的格子(线的端点):横着就是横线,竖着就是竖线
        LEFT | RIGHT => '─',
        _ => '│',
    }
}

/// a 和 c 相对 b 在同一侧(都偏北/南/东/西)时,边在 b 这里应该是"竖着进"或"横着进";
/// 一进一出(正常穿过)就交给主方向自己定.返回 true 表示这一段要求横着进节点
pub(crate) fn arrive_hor(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> Option<bool> {
    let (da, dc) = (a.1 - b.1, c.1 - b.1);
    if da != 0 && dc != 0 && da.signum() == dc.signum() {
        return Some(false); // 两头都在上边或都在下边:竖着进
    }
    let (xa, xc) = (a.0 - b.0, c.0 - b.0);
    if xa != 0 && xc != 0 && xa.signum() == xc.signum() {
        return Some(true); // 两头都在左边或都在右边:横着进
    }
    None
}

/// 画布:边只记"这一格往哪几个方向连",最后按方向位合成制表符,
/// 这样拐弯处自然就是 ╭╮╯╰,交叉处自然就是 ┼,不用逐段猜字符
/// 一条边在某一端的落点:节点框的矩形、线从哪一面进出、以及那一格
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EndMark {
    pub rect: (i32, i32, i32, i32),
    pub side: u8,
    pub cell: (i32, i32),
    /// 这一端有没有边框可以落 T/箭头(文字节点没有,箭头改落在区域外面那一格)
    pub mark: bool,
}

/// 边框格被覆盖成什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BorderMark {
    /// 连接点(T):横向 + 线从哪一面出去
    Junction(u8),
    /// 终点箭头:字形 + 边色
    Arrow(char, (u8, u8, u8)),
    /// 只占位不画:文字节点没有边框,但端口格照样要被占住,
    /// 这样同一侧再接边时会顺延到下一格,多条边不会挤在同一个进出点
    Port,
}

/// 走线用的一个点(画布格)
pub(crate) type Point = (i32, i32);
/// 一条折线
pub(crate) type Polyline = Vec<Point>;
/// 搜索里的状态:位置 + 进入方向
pub(crate) type State = (Point, usize);

pub(crate) struct Canvas {
    pub(crate) x0: i32,
    pub(crate) y0: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) mask: Vec<u8>,
    pub(crate) fg: Vec<(u8, u8, u8)>,
    /// 这一格的颜色是第几个分组占的(-1 = 还没人占)
    pub(crate) owner: Vec<i16>,
    /// 占住这一格的那个分组在这一格的方向位(别的分组从这儿过就"断一格")
    pub(crate) owner_mask: Vec<u8>,
    /// 这一格的颜色是不是"光标那条分组"的
    pub(crate) mine: Vec<bool>,
    /// 节点:0 没有,1 普通节点,2 枢纽
    pub(crate) node: Vec<u8>,
    /// 这一格上有哪几个分组(位掩码,最多 64 个够用)
    pub(crate) groups_here: Vec<u64>,
    /// 作为节点,这一个节点属于哪几个分组(只是想从这儿穿过的分组不算)
    pub(crate) node_groups: Vec<u64>,
    /// 所有节点的格子(有框的节点是整个框):画边时先避开它们,别从节点身上穿过去
    pub(crate) station_cells: HashSet<(i32, i32)>,
    /// 每格占住它的那条边的线型
    pub(crate) style: Vec<EdgeStyle>,
    /// 每格占住它的那条边的拐角样式(先占的说了算)
    pub(crate) corner: Vec<Corner>,
    /// 当前正在画的这条边用什么拐角;claim 第一次占住格子时抄进去
    pub(crate) pen_corner: Corner,
    /// 当前这条边自己的底色;占住格子时抄进 edge_bg
    pub(crate) pen_bg: Option<crate::document::Rgb>,
    /// 每格上那条边的底色(和框的填充互不影响)
    pub(crate) edge_bg: Vec<Option<crate::document::Rgb>>,
    /// 这一格有没有被实线占过(实线优先,虚线不在这里断)
    pub(crate) solid: Vec<bool>,
    /// 节点框的装饰层:在连线之后画,盖在线上面
    pub(crate) deco: Vec<Option<Deco>>,
    /// 有向边画在箭头那一格的字形(None = 这一格没有箭头;只给点节点用)
    pub(crate) arrow: Vec<Option<char>>,
    /// 边框覆盖表:T 与嵌入边框的箭头
    pub(crate) border: HashMap<(i32, i32), BorderMark>,
    /// 箭头字形风格
    pub(crate) arrow_style: crate::model::ArrowStyle,
    /// 预览模式:只画线,不落 T/箭头,也不画端口短接(连线预览用)
    pub(crate) preview: bool,
    /// 聚焦的分组(位掩码;0 = 不聚焦).没聚焦时所有分组都正常画
    pub(crate) focus: u64,
    /// 光标所在节点的那几个分组:颜色冲突时它们说了算
    pub(crate) priority: Vec<bool>,
    /// 这一格的内容在不在"正在看的那一层"上:压暗只看它,跟分组聚焦互不干扰
    pub(crate) lit: Vec<bool>,
    /// 正在画的那条边的下标(画之前设,claim 时抄进 edge_of)
    pub(crate) pen_edge: usize,
    /// 这一格最后显示的是哪条边(-1 = 没有边)
    pub(crate) edge_of: Vec<i32>,
    /// 每条边最后可见的格子(取格子时用)
    pub(crate) edge_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// 同一格上有多条边时的候选(按画的先后)
    pub(crate) edge_cands: HashMap<(i32, i32), Vec<usize>>,
    /// 正在画的东西是不是当前层(画的时候临时设,画完读 lit 判断压不压暗)
    pub(crate) layer_lit: bool,
    /// 有没有在按图层看.没按图层看时 lit 一律不参与判断
    pub(crate) layer_filter: bool,
}

impl Canvas {
    pub(crate) fn new(area: (i32, i32, i32, i32), priority: Vec<bool>, focus: u64) -> Canvas {
        let (x0, y0, w, h) = area;
        let n = (w.max(0) * h.max(0)) as usize;
        Canvas {
            x0,
            y0,
            w: w.max(0),
            h: h.max(0),
            mask: vec![0; n],
            fg: vec![(0, 0, 0); n],
            owner: vec![-1; n],
            owner_mask: vec![0; n],
            mine: vec![false; n],
            node: vec![0; n],
            groups_here: vec![0; n],
            node_groups: vec![0; n],
            station_cells: HashSet::new(),
            style: vec![EdgeStyle::Solid; n],
            corner: vec![Corner::Rounded; n],
            pen_corner: Corner::Rounded,
            pen_bg: None,
            edge_bg: vec![None; n],
            solid: vec![false; n],
            deco: vec![None; n],
            arrow: vec![None; n],
            border: HashMap::new(),
            arrow_style: Default::default(),
            preview: false,
            pen_edge: 0,
            edge_of: vec![-1; n],
            edge_cells: HashMap::new(),
            edge_cands: HashMap::new(),
            focus,
            priority,
            lit: vec![false; n],
            layer_lit: true,
            layer_filter: false,
        }
    }

    /// 一格只能有一种颜色,也只能有一种形状.
    /// 颜色谁说了算:光标所在节点的那些分组 > 先画的分组 —— 于是两组挤在同一格时,
    /// 颜色会跟着光标走,从当前节点就能顺着颜色找到自己那一组.
    /// 形状只按"先画的说了算",**不跟光标走**:不然挪一下光标,谁在这里断一格就翻到
    /// 另一组上,整段边的字形都跟着变,看着就是边在变形
    fn claim(&mut self, i: usize, group: usize, bit: u8, fg: (u8, u8, u8), style: EdgeStyle) {
        let cur = self.owner[i];
        let solid = style == EdgeStyle::Solid;
        if self.layer_lit {
            // 这一格有当前层的线经过:哪怕形状归别的层,也不压暗
            self.lit[i] = true;
        }
        if cur == group as i16 {
            self.note_edge(i);
            self.owner_mask[i] |= bit;
            if solid {
                // 这一格被实线占过就按实线画,别让虚线把它断掉
                self.style[i] = EdgeStyle::Solid;
                self.solid[i] = true;
            }
            if self.priority[group] {
                self.fg[i] = fg; // 这一格本来就是自己占的,颜色归自己
            }
            return;
        }
        if cur < 0 {
            self.note_edge(i);
            self.owner[i] = group as i16;
            self.owner_mask[i] = bit;
            self.fg[i] = fg;
            self.mine[i] = self.priority[group];
            self.style[i] = style;
            self.solid[i] = solid;
            // 拐角样式跟形状一样:这一格被谁先占住,就用谁的
            self.corner[i] = self.pen_corner;
        } else {
            // 这一格被别的分组占着:形状归它,但这条边仍然是候选
            self.note_candidate(i);
            if solid {
                self.style[i] = EdgeStyle::Solid;
                self.solid[i] = true;
            }
            if self.priority[group] && !self.priority[cur as usize] {
                self.fg[i] = fg; // 只让颜色,形状留给先画的那一组
            }
        }
    }

    /// 这一格最后由正在画的这条边说了算
    fn note_edge(&mut self, i: usize) {
        self.edge_of[i] = self.pen_edge as i32;
        if self.pen_bg.is_some() {
            self.edge_bg[i] = self.pen_bg;
        }
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        self.edge_cells.entry(self.pen_edge).or_default().push(cell);
        self.note_candidate(i);
    }

    /// 同一格上的边按画的先后排进候选(不重复记)
    fn note_candidate(&mut self, i: usize) {
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        let list = self.edge_cands.entry(cell).or_default();
        if !list.contains(&self.pen_edge) {
            list.push(self.pen_edge);
        }
    }

    pub(crate) fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        Some(((y - self.y0) * self.w + (x - self.x0)) as usize)
    }

    /// 这一格有没有东西(节点名标注只往空位上写)
    pub(crate) fn at(&self, x: i32, y: i32) -> u8 {
        match self.idx(x, y) {
            Some(i) => {
                let mut k = self.node[i];
                if self.mask[i] != 0 || self.deco[i].is_some() {
                    k = k.max(1);
                }
                k
            }
            None => u8::MAX,
        }
    }

    /// 在 (x, y) 记上"这里和某个方向连着"
    fn link(&mut self, x: i32, y: i32, bit: u8, group: usize, fg: (u8, u8, u8), style: EdgeStyle) {
        let Some(i) = self.idx(x, y) else { return };
        self.mask[i] |= bit;
        self.groups_here[i] |= 1 << group;
        self.claim(i, group, bit, fg, style);
    }

    /// 一段直线(同一行或同一列)
    fn run(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        if a.1 == b.1 {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            for x in x0..x1 {
                self.link(x, a.1, RIGHT, group, fg, style);
                self.link(x + 1, a.1, LEFT, group, fg, style);
            }
        } else if a.0 == b.0 {
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            for y in y0..y1 {
                self.link(a.0, y, DOWN, group, fg, style);
                self.link(a.0, y + 1, UP, group, fg, style);
            }
        }
    }

    /// 一段直线上的格子(含首尾)
    fn span(&self, a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        if a.1 == b.1 {
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                out.push((x, a.1));
            }
        } else if a.0 == b.0 {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                out.push((a.0, y));
            }
        }
        out
    }

    /// 从 from 出发、只走这个分组已经画过的格子,能不能走到 to
    fn joined(&self, group: usize, from: (i32, i32), to: (i32, i32)) -> bool {
        let bit = 1u64 << group;
        let has = |p: (i32, i32)| {
            self.idx(p.0, p.1)
                .map(|i| self.groups_here[i] & bit != 0)
                .unwrap_or(false)
        };
        if !has(from) || !has(to) {
            return false;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        seen.insert(from);
        while let Some(p) = stack.pop() {
            if p == to {
                return true;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let q = (p.0 + dx, p.1 + dy);
                if has(q) && seen.insert(q) {
                    stack.push(q);
                }
            }
        }
        false
    }

    /// 这条折线会踩到多少**别的分组**已经画过的格子(复用自己的走廊不算代价,
    /// 否则"下去再上来"会因为想省格子而绕到旁边去,围出一个方框)
    fn cost(&self, a: (i32, i32), corner: (i32, i32), b: (i32, i32), group: usize) -> i32 {
        let bit = 1u64 << group;
        let mut n = 0;
        // 这一笔里有没有"这个分组还没占过"的格子
        let fresh = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .any(|p| {
                self.idx(p.0, p.1)
                    .map(|i| self.groups_here[i] & bit == 0)
                    .unwrap_or(false)
            });
        // 两端在这个分组已经画过的地方是通的,而这一笔又要踩新格子:
        // 那就等于把自己绕成一个环(图上一个封闭的圈),尽量别这么拐
        if fresh && (self.joined(group, a, corner) || self.joined(group, corner, b)) {
            n += 1000;
        }
        // 再把"封口"也算进去:这一笔落下之后,如果某一格的四条边都成了这个分组的笔画,
        // 那一格就被自己围住了(视觉上就是个方框),这种拐法也要尽量躲开
        let path: HashSet<(i32, i32)> = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .collect();
        let mine = |p: (i32, i32)| {
            path.contains(&p)
                || self
                    .idx(p.0, p.1)
                    .map(|i| self.groups_here[i] & bit != 0)
                    .unwrap_or(false)
        };
        let mut encloses = false;
        'outer: for &(x, y) in &path {
            for (dx, dy) in [(-1i32, -1i32), (-1, 0), (0, -1), (0, 0)] {
                let c = (x + dx, y + dy);
                if mine(c)
                    && mine((c.0 + 1, c.1))
                    && mine((c.0, c.1 + 1))
                    && mine((c.0 + 1, c.1 + 1))
                {
                    encloses = true;
                    break 'outer;
                }
            }
        }
        if encloses {
            n += 1000;
        }
        for p in self.span(a, corner).into_iter().chain(self.span(corner, b)) {
            if p == a || p == b {
                continue; // 两个端点本来就是节点
            }
            if let Some(i) = self.idx(p.0, p.1) {
                if self.groups_here[i] != 0 && self.groups_here[i] & bit == 0 {
                    n += 1;
                }
            }
            // 从节点(或它的框)身上穿过去很难看:边会画进那个字形里,还容易和别的段围出方框
            if self.station_cells.contains(&p) {
                n += 100;
            }
        }
        n
    }

    /// 一条实线边(不需要框的信息时用这个;单测里方便)
    #[cfg(test)]
    pub(crate) fn edge(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
    ) {
        self.edge_full(
            a,
            b,
            group,
            fg,
            arrive_hor,
            heads,
            EdgeStyle::Solid,
            None,
            None,
        );
    }

    /// 一条边:同排或同列直接连,否则拐一个直角.
    ///
    /// 拐法有两种(先横后竖 / 先竖后横).`arrive_hor` 是这一端要求的"进节点方向"
    /// (由前后两个节点的位置定,见 arrive_hor):要求竖着进,就先横着走再竖着进,
    /// 于是同一个分组"下去再上来"会走同一列,不会绕着节点围出一个方框.
    /// 实在会多踩别人很多格子,才让位给"踩得少"的那种.
    ///
    /// `heads` 是有向边的箭头画在哪几端:(画在 a 端, 画在 b 端).
    /// 同一个分组里 a -> b 与 b -> a 是两条独立的边,合到这一段上就是两端各一个箭头.
    /// `a_box`/`b_box` 是两端节点的框(左上角与宽高):目标带框时箭头落在框外紧挨着的那一格
    /// 一条边:两端有框时先定端口,再把端口外侧一格当自由段端点走最小折弯折线.
    ///
    /// `a`/`b` 是两端的中心格(点节点就直接画到这儿);`a_end`/`b_end` 带框时由这里
    /// 定端口格:线只走到端口外侧,端口格本身交给 T/箭头覆盖.
    // 走线原语:端点、分组、颜色、拐法、两端箭头、两端框,少一个都画不出来 —— 打包成结构体只是换个地方堆
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn edge_full(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
        style: EdgeStyle,
        a_end: Option<EndMark>,
        b_end: Option<EndMark>,
    ) {
        let (at_a, at_b) = heads;
        if a == b {
            return;
        }
        // 端口:先定边框格,线的端点取它外侧一格
        let a_port = a_end.and_then(|e| self.pick_port(e, at_a));
        let b_port = b_end.and_then(|e| self.pick_port(e, at_b));
        if a_port.is_none() && b_port.is_none() {
            // 两头都是点节点:按老办法走一个 L(拐法按前后节点定,避免绕出方框)
            self.edge_point(a, b, group, fg, arrive_hor, heads, style);
            return;
        }
        let s = a_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(a);
        let t = b_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(b);
        let out = a_port.map(|(side, _)| Self::outward(side));
        // 最后一段是"朝框里走",方向是目标侧外法线的反向
        let into = b_port.map(|(side, _)| {
            let n = Self::outward(side);
            (-n.0, -n.1)
        });
        let pts = self.route(s, out, t, into, group);
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        // 端口到外侧的一格:让线和边框接上,T/箭头随后盖住端口格
        // (预览也要这一段:不然线两端只剩一个方向位,画出来是个竖杠).
        // 文字节点没有边框:这一格不画,免得把线画到字上面
        if let Some((side, c)) = a_port {
            if a_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, s, group, fg, style);
            } else {
                let _ = (side, c);
            }
        }
        if let Some((side, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            } else {
                let _ = (side, c);
            }
        }
        // 折点表只有一格时(两端外侧格重合)没有相邻点可用,退回端点本身
        let prev = if pts.len() >= 2 {
            pts[pts.len() - 2]
        } else {
            b
        };
        let next = if pts.len() >= 2 { pts[1] } else { a };
        if let Some((side, c)) = b_port {
            let draw = b_end.map(|e| e.mark).unwrap_or(false);
            // 文字节点没有边框格可画,但端口照样要占住(多条边才不会抢同一个进出点)
            self.commit_end(side, c, at_b, fg, draw);
            if !draw && at_b {
                // 没有边框:箭头落在区域外面那一格
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(prev, pts[pts.len() - 1], group, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(next, pts[0], group, None);
        }
    }

    /// 自环:两端都落在同一个节点上.先从 `end` 那一侧出去,再拐到顺时针的下一侧回来.
    /// 两端若挤在同一条边上,走线会成一根直线,看不出是个环,所以第二端换一侧.
    pub(crate) fn edge_loop(
        &mut self,
        end: EndMark,
        heads: (bool, bool),
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        let (at_a, at_b) = heads;
        let side2 = match end.side {
            UP => RIGHT,
            RIGHT => DOWN,
            DOWN => LEFT,
            _ => UP,
        };
        let Some((s1, c1)) = self.pick_port(end, at_a) else {
            return;
        };
        // 第二端优先挑离第一端最近的那一格:环才会贴着节点收紧,不会横跨半个框
        let nearest = Self::side_cells(end.rect, side2, end.mark)
            .into_iter()
            .min_by_key(|c| (c.0 - c1.0).abs() + (c.1 - c1.1).abs())
            .unwrap_or_else(|| Self::side_base(end.rect, side2));
        let probe2 = EndMark {
            rect: end.rect,
            side: side2,
            cell: nearest,
            mark: end.mark,
        };
        // 先把第一端占住,第二端才不会选到同一格.
        // 只有带边框的节点才需要:一格的点节点/文字节点两侧本来就是同一格,占了就再也挑不出来
        if !self.preview && end.mark {
            self.border.insert(c1, BorderMark::Port);
        }
        let Some((s2, c2)) = self.pick_port(probe2, at_b) else {
            return;
        };
        let s = Self::outside(c1, s1);
        let t = Self::outside(c2, s2);
        let out = Some(Self::outward(s1));
        // 末段朝框里走:方向是第二侧外法线的反向
        let into = Some({
            let n = Self::outward(s2);
            (-n.0, -n.1)
        });
        let pts = self.route(s, out, t, into, group);
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        // 端口到外侧那一格(文字节点没有边框,这一格不画)
        if end.mark {
            self.run(c1, s, group, fg, style);
            self.run(c2, t, group, fg, style);
        }
        self.commit_end(s1, c1, at_a, fg, end.mark);
        self.commit_end(s2, c2, at_b, fg, end.mark);
        if !end.mark && at_a {
            self.outside_arrow(s, s1, fg);
        }
        if !end.mark && at_b {
            self.outside_arrow(t, s2, fg);
        }
    }

    /// 同一段上的第 `lane` 条边:整条直段垂直于自己平移几格,两端各拐一格接回节点.
    /// `lane = 0` 就是走中间那条直线.只用于"两个点节点之间有多条边"的情形:
    /// 没有边框可以顺延,不给偏移就只能一条盖住另一条
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn edge_lane(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        lane: i32,
        group: usize,
        fg: (u8, u8, u8),
        heads: (bool, bool),
        style: EdgeStyle,
    ) {
        let (at_a, at_b) = heads;
        if a == b {
            return;
        }
        if lane == 0 || (a.0 != b.0 && a.1 != b.1) {
            self.edge_point(a, b, group, fg, None, heads, style);
            return;
        }
        let pts: [(i32, i32); 4] = if a.1 == b.1 {
            // 横着的一段:上下平移
            [a, (a.0, a.1 + lane), (b.0, b.1 + lane), b]
        } else {
            // 竖着的一段:左右平移
            [a, (a.0 + lane, a.1), (b.0 + lane, b.1), b]
        };
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        if at_b {
            self.head_arrow(pts[2], b, group, None);
        }
        if at_a {
            self.head_arrow(pts[1], a, group, None);
        }
    }

    /// 两端都是点节点时的走线:同排同列直接连,否则拐一个直角
    #[allow(clippy::too_many_arguments)]
    fn edge_point(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
        style: EdgeStyle,
    ) {
        let (at_a, at_b) = heads;
        if a.1 == b.1 || a.0 == b.0 {
            self.run(a, b, group, fg, style);
            if at_b {
                self.head_arrow(a, b, group, None);
            }
            if at_a {
                self.head_arrow(b, a, group, None);
            }
            return;
        }
        let hor_first = (b.0, a.1);
        let ver_first = (a.0, b.1);
        let (c1, c2) = (
            self.cost(a, hor_first, b, group),
            self.cost(a, ver_first, b, group),
        );
        // 要求竖着进节点(进节点那一段是竖的)就先横后竖
        let prefer_hor = arrive_hor
            .map(|h| !h)
            .unwrap_or((b.0 - a.0).abs() >= (b.1 - a.1).abs());
        let corner = if prefer_hor {
            if c1 <= c2 + 1 {
                hor_first
            } else {
                ver_first
            }
        } else if c2 <= c1 + 1 {
            ver_first
        } else {
            hor_first
        };
        self.run(a, corner, group, fg, style);
        self.run(corner, b, group, fg, style);
        if at_b {
            self.head_arrow(corner, b, group, None);
        }
        if at_a {
            self.head_arrow(corner, a, group, None);
        }
    }

    /// 一条带手工折点的边:源外侧 -> 各 via -> 目标外侧,逐段走最小折弯.
    /// 第一段沿源侧外法线、最后一段沿目标侧内法线;via 点本身固定不动
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn path(
        &mut self,
        pts: &[(i32, i32)],
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
        heads: (bool, bool),
        a_end: Option<EndMark>,
        b_end: Option<EndMark>,
    ) {
        if pts.len() < 2 {
            return;
        }
        let (at_a, at_b) = heads;
        let a_port = a_end.and_then(|e| self.pick_port(e, at_a));
        let b_port = b_end.and_then(|e| self.pick_port(e, at_b));
        let s = a_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[0]);
        let t = b_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[pts.len() - 1]);
        let mut way: Vec<(i32, i32)> = vec![s];
        way.extend_from_slice(&pts[1..pts.len() - 1]);
        way.push(t);
        let n = way.len();
        let out = a_port.map(|(side, _)| Self::outward(side));
        let into = b_port.map(|(side, _)| {
            let o = Self::outward(side);
            (-o.0, -o.1)
        });
        for k in 0..n.saturating_sub(1) {
            if way[k] == way[k + 1] {
                continue;
            }
            let d1 = if k == 0 { out } else { None };
            let d2 = if k + 2 == n { into } else { None };
            let seg = self.route(way[k], d1, way[k + 1], d2, group);
            for w in seg.windows(2) {
                self.run(w[0], w[1], group, fg, style);
            }
        }
        if let Some((_, c)) = a_port {
            if a_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, s, group, fg, style);
            }
        }
        if let Some((_, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            let draw = b_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_b, fg, draw);
            if !draw && at_b {
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(way[n - 2], t, group, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(way[1], s, group, None);
        }
    }

    /// 某一侧的外法线(离开框的方向)
    pub(crate) fn outward(side: u8) -> (i32, i32) {
        match side {
            UP => (0, -1),
            DOWN => (0, 1),
            LEFT => (-1, 0),
            _ => (1, 0),
        }
    }

    /// 端口格外侧紧挨着的那一格:线画到这里,端口格交给 T/箭头
    pub(crate) fn outside(cell: (i32, i32), side: u8) -> (i32, i32) {
        let n = Self::outward(side);
        (cell.0 + n.0, cell.1 + n.1)
    }

    /// 一端一格(有向单位向量);不是正交相邻就返回 None
    fn seg_dir(a: (i32, i32), b: (i32, i32)) -> Option<(i32, i32)> {
        if a.1 == b.1 && a.0 != b.0 {
            Some(((b.0 - a.0).signum(), 0))
        } else if a.0 == b.0 && a.1 != b.1 {
            Some((0, (b.1 - a.1).signum()))
        } else {
            None
        }
    }

    /// 折线是否合法:每段正交、首段沿 out、末段沿 into(给了就要求)
    fn fits(pts: &[(i32, i32)], out: Option<(i32, i32)>, into: Option<(i32, i32)>) -> bool {
        if pts.len() < 2 {
            return false;
        }
        let mut dirs = Vec::with_capacity(pts.len() - 1);
        for w in pts.windows(2) {
            match Self::seg_dir(w[0], w[1]) {
                Some(d) => dirs.push(d),
                None => return false,
            }
        }
        if let Some(d) = out {
            if dirs[0] != d {
                return false;
            }
        }
        if let Some(d) = into {
            if *dirs.last().unwrap() != d {
                return false;
            }
        }
        true
    }

    /// 走线候选,按折弯数从少到多:0 折直线、1 折 L、2 折小 jog、3 折绕行
    fn shapes(
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
    ) -> Vec<Vec<(i32, i32)>> {
        let mut cands: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut push = |pts: Vec<(i32, i32)>| {
            let ok = Self::fits(&pts, out, into) && !cands.contains(&pts);
            if ok {
                cands.push(pts);
            }
        };
        if s == t {
            return cands;
        }
        // 0 折
        push(vec![s, t]);
        // 1 折
        if s.0 != t.0 && s.1 != t.1 {
            push(vec![s, (t.0, s.1), t]);
            push(vec![s, (s.0, t.1), t]);
        }
        // 2 折:先贴目标端错一格,再把折点往源端挪
        let mut rows: Vec<i32> = Vec::new();
        if let Some(i) = into {
            rows.push(t.1 - i.1);
        }
        if let Some(o) = out {
            rows.push(s.1 + o.1);
        }
        rows.push((s.1 + t.1) / 2);
        if s.1 != t.1 {
            rows.push(s.1 + (t.1 - s.1).signum());
        }
        rows.dedup();
        for jy in &rows {
            if s.0 != t.0 {
                push(vec![s, (s.0, *jy), (t.0, *jy), t]);
            }
        }
        let mut cols: Vec<i32> = Vec::new();
        if let Some(i) = into {
            cols.push(t.0 - i.0);
        }
        if let Some(o) = out {
            cols.push(s.0 + o.0);
        }
        cols.push((s.0 + t.0) / 2);
        if s.0 != t.0 {
            cols.push(s.0 + (t.0 - s.0).signum());
        }
        cols.dedup();
        for jx in &cols {
            if s.1 != t.1 {
                push(vec![s, (*jx, s.1), (*jx, t.1), t]);
            }
        }
        // 3 折:先按出点法线走一格,再绕到目标行的前一行
        if let (Some(o), Some(i)) = (out, into) {
            if o.0 != 0 && i.1 != 0 {
                for k in 1..=2 {
                    let x1 = s.0 + o.0 * k;
                    if x1 == t.0 {
                        continue;
                    }
                    let y2 = t.1 - i.1;
                    push(vec![s, (x1, s.1), (x1, y2), (t.0, y2), t]);
                }
            } else if o.1 != 0 && i.0 != 0 {
                for k in 1..=2 {
                    let y1 = s.1 + o.1 * k;
                    if y1 == t.1 {
                        continue;
                    }
                    let x2 = t.0 - i.0;
                    push(vec![s, (s.0, y1), (x2, y1), (x2, t.1), t]);
                }
            }
        }
        cands
    }

    /// 折线的代价:碰到框直接判死,踩到别的分组加价,越长越贵
    fn route_cost(&self, pts: &[(i32, i32)], group: usize) -> Option<i32> {
        let bit = 1u64 << group;
        let last = pts.len() - 1;
        let mut cost = 0;
        for (k, w) in pts.windows(2).enumerate() {
            // 段内每一格(端点留给两端处理)
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            for step in 0..=steps {
                let p = (w[0].0 + (dx.signum()) * step, w[0].1 + (dy.signum()) * step);
                let is_end = (k == 0 && step == 0) || (k == last - 1 && step == steps);
                if self.station_cells.contains(&p) && !is_end {
                    return None; // 不穿框
                }
                if let Some(i) = self.idx(p.0, p.1) {
                    if self.groups_here[i] & bit == 0 && self.groups_here[i] != 0 {
                        cost += 30;
                    }
                    if self.station_cells.contains(&p) && !is_end {
                        return None;
                    }
                }
                cost += 1;
            }
        }
        Some(cost)
    }

    /// 最小折弯走线:从 s 沿 out 出发,最后沿 into 进入 t;返回含首尾的折点表.
    /// 先试固定形状(它们天然把 jog 放在靠目标端),再放开方向约束,最后才搜索绕行
    fn route(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Vec<(i32, i32)> {
        if s == t {
            // 两个端口的外侧格重合(间隙刚好一格):两段短接直接对上
            return vec![s];
        }
        for (o, i) in [(out, into), (None, None)] {
            if let Some(pts) = self.route_shaped(s, o, t, i, group) {
                return pts;
            }
        }
        for (o, i) in [(out, into), (None, None)] {
            if let Some(pts) = self.route_search(s, o, t, i, group) {
                return pts;
            }
        }
        // 实在绕不开:走一个 L,但碰到框就停在最后一格,别穿过去
        self.route_clip(s, t)
    }

    /// 固定形状里挑最好的:折弯少优先,其次代价
    fn route_shaped(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Option<Vec<(i32, i32)>> {
        // (折弯数, 代价, 折点表)
        let mut best: Option<(i32, i32, Polyline)> = None;
        for cand in Self::shapes(s, out, t, into) {
            let bends = cand.len() as i32 - 2;
            let Some(cost) = self.route_cost(&cand, group) else {
                continue;
            };
            let better = match &best {
                None => true,
                Some((b, c, _)) => (bends, cost) < (*b, *c),
            };
            if better {
                best = Some((bends, cost, cand));
            }
        }
        best.map(|b| b.2)
    }

    /// 搜索绕行:折弯按代价算,不穿框,尽量不踩别的分组
    fn route_search(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Option<Vec<(i32, i32)>> {
        const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let dir_idx = |d: (i32, i32)| DIRS.iter().position(|x| *x == d);
        let turn = 40;
        let bit = 1u64 << group;
        let pad = 6;
        // 窗口:两端包围盒外扩几格,但优先留在画布里(绕到画面外的线看不见);
        // 两端本身一定要包含进来,哪怕它们在画面外
        let (lx, hx) = (s.0.min(t.0) - pad, s.0.max(t.0) + pad);
        let (ly, hy) = (s.1.min(t.1) - pad, s.1.max(t.1) + pad);
        // 画布范围内的格子才画得出来;两端在画布外时才把窗口撑过去
        let (cx0, cy0) = (self.x0, self.y0);
        let (cx1, cy1) = (self.x0 + self.w - 1, self.y0 + self.h - 1);
        let lx = lx.max(cx0).min(s.0.min(t.0));
        let hx = hx.min(cx1).max(s.0.max(t.0));
        let ly = ly.max(cy0).min(s.1.min(t.1));
        let hy = hy.min(cy1).max(s.1.max(t.1));
        if t.0 < lx || t.0 > hx || t.1 < ly || t.1 > hy {
            return None;
        }
        let key = |p: (i32, i32), d: usize| (p, d);
        let free = |p: (i32, i32)| -> bool {
            if p == s || p == t {
                return true;
            }
            if p.0 < lx || p.0 > hx || p.1 < ly || p.1 > hy {
                return false;
            }
            !self.station_cells.contains(&p)
        };
        // 堆里放 (估价值, 已走代价, 点, 方向)
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(i32, i32, Point, usize)>> =
            std::collections::BinaryHeap::new();
        let mut best: HashMap<State, i32> = HashMap::new();
        let mut from: HashMap<State, State> = HashMap::new();
        let h = |p: (i32, i32)| (p.0 - t.0).abs() + (p.1 - t.1).abs();
        match out.and_then(dir_idx) {
            Some(d) => {
                heap.push(std::cmp::Reverse((h(s), 0, s, d)));
                best.insert(key(s, d), 0);
            }
            None => {
                for (d, (dx, dy)) in DIRS.iter().copied().enumerate() {
                    // 起点方向还没定:四个方向都试,但第一步得真的能走
                    let p = (s.0 + dx, s.1 + dy);
                    if !free(p) {
                        continue;
                    }
                    let cost = 1 + self.step_cost(p, bit);
                    heap.push(std::cmp::Reverse((cost + h(p), cost, p, d)));
                    best.insert(key(p, d), cost);
                    from.insert(key(p, d), (s, d));
                }
            }
        }
        let start_dir = out.and_then(dir_idx);
        let goal_dir = into.and_then(dir_idx);
        // 兜底上限:窗口再大也不至于把一帧的预算耗光
        let mut pops = 0;
        while let Some(std::cmp::Reverse((_, cost, p, d))) = heap.pop() {
            pops += 1;
            if pops > 20000 {
                return None;
            }
            if p == t && goal_dir.map(|g| g == d).unwrap_or(true) {
                let mut pts = vec![t];
                let mut cur = (p, d);
                while let Some(&(prev, pd)) = from.get(&cur) {
                    if prev == s {
                        break;
                    }
                    pts.push(prev);
                    cur = (prev, pd);
                }
                pts.push(s);
                pts.reverse();
                return Some(pts);
            }
            if best.get(&(p, d)).copied().unwrap_or(i32::MAX) < cost {
                continue;
            }
            for (nd, (dx, dy)) in DIRS.iter().copied().enumerate() {
                let q = (p.0 + dx, p.1 + dy);
                if !free(q) {
                    continue;
                }
                if nd != d && start_dir.is_some() && p == s {
                    continue;
                }
                let nc = cost + 1 + self.step_cost(q, bit) + if nd == d { 0 } else { turn };
                if nc < best.get(&key(q, nd)).copied().unwrap_or(i32::MAX) {
                    best.insert(key(q, nd), nc);
                    from.insert(key(q, nd), (p, d));
                    heap.push(std::cmp::Reverse((nc + h(q), nc, q, nd)));
                }
            }
        }
        None
    }

    /// 走这一格的额外代价:踩到别的分组(不那么致命,只是不好看)
    fn step_cost(&self, p: (i32, i32), bit: u64) -> i32 {
        match self.idx(p.0, p.1) {
            Some(i) if self.groups_here[i] != 0 && self.groups_here[i] & bit == 0 => 6,
            _ => 0,
        }
    }

    /// 兜底:一个 L,遇到框就停在最后一格之前
    fn route_clip(&self, s: (i32, i32), t: (i32, i32)) -> Vec<(i32, i32)> {
        let mut pts = vec![s];
        let mut cur = s;
        let mut legs = Vec::new();
        if s.0 != t.0 {
            legs.push((t.0, s.1));
        }
        if s.1 != t.1 {
            let last = *pts.last().unwrap();
            legs.push((last.0.max(t.0).min(t.0), t.1));
        }
        for corner in legs {
            let (dx, dy) = ((corner.0 - cur.0).signum(), (corner.1 - cur.1).signum());
            let steps = (corner.0 - cur.0).abs().max((corner.1 - cur.1).abs());
            for _ in 0..steps {
                let next = (cur.0 + dx, cur.1 + dy);
                if self.station_cells.contains(&next) && next != t {
                    return pts;
                }
                pts.push(next);
                cur = next;
            }
        }
        pts
    }

    /// 某一侧的连接格(按框的宽高奇偶取中)
    pub(crate) fn side_base(rect: (i32, i32, i32, i32), side: u8) -> (i32, i32) {
        let (left, top, w, h) = rect;
        match side {
            UP => (left + w / 2, top),
            DOWN => (left + (w - 1) / 2, top + h - 1),
            LEFT => (left, top + (h - 1) / 2),
            _ => (left + w - 1, top + h / 2),
        }
    }

    /// 这一侧除角以外的格子,按顺时针顺序
    /// 这一侧的格子,按顺时针顺序.
    /// `corners`:带框的节点不让线落在四个角上(角上要留拐角字形);
    /// 文字节点没有拐角,整条边都能接
    fn side_cells(rect: (i32, i32, i32, i32), side: u8, corners: bool) -> Vec<(i32, i32)> {
        let (left, top, w, h) = rect;
        let (right, bottom) = (left + w - 1, top + h - 1);
        // x1 / y1 是开区间上界
        let (x0, x1) = if corners {
            (left + 1, right)
        } else {
            (left, right + 1)
        };
        let (y0, y1) = if corners {
            (top + 1, bottom)
        } else {
            (top, bottom + 1)
        };
        match side {
            UP => (x0..x1).map(|x| (x, top)).collect(),
            RIGHT => (y0..y1).map(|y| (right, y)).collect(),
            DOWN => (x0..x1).rev().map(|x| (x, bottom)).collect(),
            _ => (y0..y1).rev().map(|y| (left, y)).collect(),
        }
    }

    /// 找一个能用的边框格:优先首选格,被占就顺时针、再逆时针.
    /// 连接点可以并到已有连接点上(合并方向),箭头必须独占一格
    fn border_slot(&self, end: EndMark, arrow: bool) -> Option<(i32, i32)> {
        let cells = Self::side_cells(end.rect, end.side, end.mark);
        if cells.is_empty() {
            return None;
        }
        let start = cells
            .iter()
            .position(|c| *c == end.cell || *c == Self::side_base(end.rect, end.side))
            .unwrap_or(0);
        let free = |c: &(i32, i32), s: &Self| match s.border.get(c) {
            None => true,
            Some(BorderMark::Junction(_)) => !arrow,
            Some(BorderMark::Arrow(..)) | Some(BorderMark::Port) => false,
        };
        let n = cells.len();
        for k in 0..n {
            let c = cells[(start + k) % n];
            if free(&c, self) {
                return Some(c);
            }
        }
        for k in 1..n {
            let c = cells[(start + n - k) % n];
            if free(&c, self) {
                return Some(c);
            }
        }
        None
    }

    /// 定端口:先按选侧的取格规则,本侧被占满就顺时针换相邻侧,对面侧最后
    fn pick_port(&self, end: EndMark, arrow: bool) -> Option<(u8, (i32, i32))> {
        let order = match end.side {
            UP => [UP, RIGHT, DOWN, LEFT],
            RIGHT => [RIGHT, DOWN, LEFT, UP],
            DOWN => [DOWN, LEFT, UP, RIGHT],
            _ => [LEFT, UP, RIGHT, DOWN],
        };
        for side in order {
            let probe = EndMark {
                rect: end.rect,
                side,
                // 首选格沿用调用方给的那一格:自环要靠它挑"离另一头最近"的位置
                cell: if side == end.side {
                    end.cell
                } else {
                    Self::side_base(end.rect, side)
                },
                mark: end.mark,
            };
            if let Some(c) = self.border_slot(probe, arrow) {
                return Some((side, c));
            }
        }
        None
    }

    /// 在已经选好的端口格上落标记:箭头端嵌入箭头,另一端画 T.
    /// `draw = false`(文字节点)时只占位,不往那一格画东西
    fn commit_end(
        &mut self,
        side: u8,
        cell: (i32, i32),
        arrow: bool,
        fg: (u8, u8, u8),
        draw: bool,
    ) {
        if self.preview {
            return; // 预览不落 T/箭头,不然看着像已经建好了
        }
        if !draw {
            self.border.insert(cell, BorderMark::Port);
            if let Some(i) = self.idx(cell.0, cell.1) {
                self.note_edge(i);
            }
            return;
        }
        if arrow {
            let ch = self.arrow_style.arrow(side);
            self.border.insert(cell, BorderMark::Arrow(ch, fg));
            if let Some(i) = self.idx(cell.0, cell.1) {
                self.note_edge(i);
            }
        } else {
            // 上下两侧是"横线 + 竖着的线头",左右两侧是"竖线 + 横着的线头"
            let bits = match side {
                UP | DOWN => LR | side,
                _ => UD | side,
            };
            match self.border.get_mut(&cell) {
                Some(BorderMark::Junction(m)) => *m |= bits,
                _ => {
                    self.border.insert(cell, BorderMark::Junction(bits));
                }
            }
        }
    }

    /// 文字节点(没有边框)的箭头:落在区域外面那一格,方向仍然指向节点
    fn outside_arrow(&mut self, cell: (i32, i32), side: u8, fg: (u8, u8, u8)) {
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        if self.node[i] != 0 || self.arrow[i].is_some() {
            return;
        }
        self.arrow[i] = Some(self.arrow_style.arrow(side));
        self.fg[i] = fg;
        self.note_edge(i);
    }

    /// 有向边的箭头:画在 to 紧挨着的那一格(从 from 往 to 走的方向).
    /// to 带框时按最后一段的走向落在框外紧挨着的那一格(`to_box` 是框的左上角与宽高).
    /// 一格最多一个箭头;撞上节点字形、或者这一格已经归别的分组,就跳过
    fn head_arrow(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        group: usize,
        to_box: Option<(i32, i32, i32, i32)>,
    ) {
        let (dx, dy) = ((to.0 - from.0).signum(), (to.1 - from.1).signum());
        let (cell, ch) = if dx != 0 {
            let cell = match to_box {
                // 框就在 to 那儿:箭头落在左边界外一格 / 右边界外一格,走在边进来的那一行
                Some((left, _top, w, _h)) => {
                    if dx > 0 {
                        (left - 1, to.1)
                    } else {
                        (left + w, to.1)
                    }
                }
                None => (to.0 - dx, to.1),
            };
            (
                cell,
                // 往东走 = 从节点左边进来
                if dx > 0 {
                    self.arrow_style.arrow(LEFT)
                } else {
                    self.arrow_style.arrow(RIGHT)
                },
            )
        } else if dy != 0 {
            let cell = match to_box {
                Some((_left, top, _w, h)) => {
                    if dy > 0 {
                        (to.0, top - 1)
                    } else {
                        (to.0, top + h)
                    }
                }
                None => (to.0, to.1 - dy),
            };
            (
                cell,
                // 往下走 = 从节点上边进来
                if dy > 0 {
                    self.arrow_style.arrow(UP)
                } else {
                    self.arrow_style.arrow(DOWN)
                },
            )
        } else {
            return;
        };
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        if self.node[i] != 0 {
            return; // 节点字形优先
        }
        if self.arrow[i].is_some() {
            return;
        }
        // 这一格得是这个分组自己占的:和别的分组挤在一起时方向说不清
        if self.owner[i] != group as i16 || self.groups_here[i] != 1u64 << group {
            return;
        }
        self.arrow[i] = Some(ch);
        self.note_edge(i);
    }

    /// 把节点框占的格子提前登记下来(画边时避开,别从框里穿过去).
    /// `at` 是框的左上角
    pub(crate) fn reserve_box(&mut self, at: (i32, i32), size: (usize, usize)) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        for y in at.1..at.1 + h {
            for x in at.0..at.0 + w {
                if self.idx(x, y).is_some() {
                    self.station_cells.insert((x, y));
                }
            }
        }
    }

    /// 画一个节点框(在连线之后调用,盖在线上面).
    /// `at` 是框的左上角,`size` 是屏幕格上的宽高.文字由 render 用 text::put 画.
    /// `fill` 是框内背景(整个矩形,含边框格与文字格);压暗时由 blit 丢掉
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn box_at(
        &mut self,
        node: usize,
        at: (i32, i32),
        size: (usize, usize),
        style: BoxStyle,
        groups: u64,
        fg: (u8, u8, u8),
        fill: Option<(u8, u8, u8)>,
    ) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let (hz, vt, tl, tr, bl, br) = style.glyphs();
        let (left, top) = at;
        for yy in 0..h {
            for xx in 0..w {
                let ch = if yy == 0 {
                    if xx == 0 {
                        tl
                    } else if xx == w - 1 {
                        tr
                    } else {
                        hz
                    }
                } else if yy == h - 1 {
                    if xx == 0 {
                        bl
                    } else if xx == w - 1 {
                        br
                    } else {
                        hz
                    }
                } else if xx == 0 || xx == w - 1 {
                    vt
                } else {
                    ' '
                };
                let (x, y) = (left + xx, top + yy);
                let Some(i) = self.idx(x, y) else { continue };
                // 边框被覆盖成 T 或嵌入箭头时,用覆盖后的字形与颜色
                let (ch, fg) = match self.border.get(&(x, y)) {
                    Some(BorderMark::Junction(m)) => (junction_glyph(style, *m), fg),
                    Some(BorderMark::Arrow(c, col)) => (*c, *col),
                    // 只占位的端口:边框照原样画
                    Some(BorderMark::Port) | None => (ch, fg),
                };
                self.station_cells.insert((x, y));
                // 节点框盖在边上:这一格算不算当前层,由框自己的层说了算
                self.lit[i] = self.layer_lit;
                self.deco[i] = Some(Deco {
                    ch,
                    fg,
                    bg: fill,
                    groups,
                    node: node as i32,
                });
            }
        }
    }

    /// group_mask:这一个节点经过的所有分组;group:用哪个颜色(fg 也是它的).
    /// 孤立节点不属于任何分组,给 None:格子照画,颜色用中性色
    pub(crate) fn node_at(
        &mut self,
        x: i32,
        y: i32,
        hub: bool,
        group_mask: u64,
        group: Option<usize>,
        fg: (u8, u8, u8),
    ) {
        let Some(i) = self.idx(x, y) else { return };
        self.node[i] = if hub { 2 } else { 1 };
        // 节点格压不压暗看节点自己的层
        self.lit[i] = self.layer_lit;
        self.groups_here[i] |= group_mask;
        self.node_groups[i] = group_mask;
        match group {
            Some(g) => {
                self.owner[i] = g as i16;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = self.priority[g];
            }
            None => {
                self.owner[i] = -1;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = false;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;
    use crate::model::Corner;

    #[test]
    fn 方向位合成制表符() {
        assert_eq!(box_char(LEFT | RIGHT, Corner::Rounded), '─');
        assert_eq!(box_char(UP | DOWN, Corner::Rounded), '│');
        assert_eq!(box_char(DOWN | RIGHT, Corner::Rounded), '╭');
        assert_eq!(box_char(DOWN | LEFT, Corner::Rounded), '╮');
        assert_eq!(box_char(UP | RIGHT, Corner::Rounded), '╰');
        assert_eq!(box_char(UP | LEFT, Corner::Rounded), '╯');
        assert_eq!(box_char(UP | DOWN | RIGHT, Corner::Rounded), '├');
        assert_eq!(box_char(UP | DOWN | LEFT, Corner::Rounded), '┤');
        assert_eq!(box_char(LEFT | RIGHT | DOWN, Corner::Rounded), '┬');
        assert_eq!(box_char(LEFT | RIGHT | UP, Corner::Rounded), '┴');
        assert_eq!(box_char(LEFT | RIGHT | UP | DOWN, Corner::Rounded), '┼');
        assert_eq!(box_char(0, Corner::Rounded), ' ');
    }

    /// 端口走线不穿过别的框:障碍框内部不能有任何方向位,绕行的线在框外
    #[test]
    fn 走线绕开障碍框() {
        let mut cv = Canvas::new((0, 0, 40, 30), vec![false; 1], 0);
        // a 在 (0,3,5,3),b 在 (0,14,5,3),c 挡在中间
        cv.reserve_box((0, 3), (5, 3));
        cv.reserve_box((0, 14), (5, 3));
        cv.reserve_box((0, 8), (5, 3));
        cv.edge_full(
            (2, 4),
            (2, 15),
            0,
            (255, 0, 0),
            None,
            (false, true),
            EdgeStyle::Solid,
            Some(EndMark {
                rect: (0, 3, 5, 3),
                side: DOWN,
                cell: (2, 5),
                mark: true,
            }),
            Some(EndMark {
                rect: (0, 14, 5, 3),
                side: UP,
                cell: (2, 14),
                mark: true,
            }),
        );
        for y in 8..=10 {
            for x in 0..=4 {
                assert_eq!(
                    cv.mask[cv.idx(x, y).expect("在画布内")],
                    0,
                    "({x},{y}) 在障碍框里,不该有线"
                );
            }
        }
        // 绕行:障碍框右侧那一列有竖线
        let detour = (8..=11)
            .filter(|y| cv.mask[cv.idx(5, *y).unwrap()] != 0)
            .count();
        assert_eq!(detour, 4, "该从框右边那一列整列绕过去");
        // 端口格上落 T 与箭头
        assert_eq!(
            cv.border.get(&(2, 5)),
            Some(&BorderMark::Junction(LR | DOWN)),
            "起点是 ┬"
        );
        assert_eq!(
            cv.border.get(&(2, 14)),
            Some(&BorderMark::Arrow('v', (255, 0, 0))),
            "终点是 v"
        );
    }

    /// 直角拐弯:两笔的方向位一合,自然就是四个圆角之一
    #[test]
    fn 拐角两笔合成圆角() {
        let corner_of = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| {
            let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
            cv.edge(a, b, 0, (255, 0, 0), None, (false, false));
            box_char(
                cv.mask[cv.idx(c.0, c.1).expect("在画布内")],
                Corner::Rounded,
            )
        };
        // 先横后竖:拐点在右上角是 ╮、在左上角是 ╭
        assert_eq!(corner_of((2, 2), (7, 7), (7, 2)), '╮');
        assert_eq!(corner_of((12, 2), (7, 7), (7, 2)), '╭');
        // 先竖后横:拐点在左下角是 ╰、在右下角是 ╯
        assert_eq!(corner_of((2, 2), (7, 12), (2, 12)), '╰');
        assert_eq!(corner_of((12, 2), (7, 12), (12, 12)), '╯');
    }

    /// 锐角拐角:同样的方向位换成 ┌┐└┘,直笔与三通四通不受影响
    #[test]
    fn 锐角拐角只影响四个拐角() {
        assert_eq!(box_char(DR, Corner::Sharp), '┌');
        assert_eq!(box_char(DL, Corner::Sharp), '┐');
        assert_eq!(box_char(UR, Corner::Sharp), '└');
        assert_eq!(box_char(UL, Corner::Sharp), '┘');
        assert_eq!(box_char(DR, Corner::Rounded), '╭');
        assert_eq!(box_char(LR, Corner::Sharp), '─');
        assert_eq!(box_char(LRUD, Corner::Sharp), '┼');
        // 画的时候按边自己的拐角样式落格
        let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
        cv.pen_corner = Corner::Sharp;
        cv.edge((2, 2), (7, 7), 0, (255, 0, 0), None, (false, false));
        let i = cv.idx(7, 2).expect("在画布内");
        assert_eq!(box_char(cv.mask[i], cv.corner[i]), '┐');
        // 先占的那条边说了算:后面同一格再画圆角也不改
        cv.pen_corner = Corner::Rounded;
        cv.edge((7, 2), (7, 7), 0, (255, 0, 0), None, (false, false));
        assert_eq!(cv.corner[i], Corner::Sharp);
    }

    /// 三通四通:横竖两笔在**同一个分组**的同一格上接起来就是 ┼ / ┬
    #[test]
    fn 三通四通按方向位合成() {
        let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, false));
        cv.edge((5, 2), (5, 9), 0, (255, 0, 0), None, (false, false));
        let cross = cv.idx(5, 5).expect("在画布内");
        assert_eq!(cv.mask[cross], LEFT | RIGHT | UP | DOWN);
        assert_eq!(box_char(cv.mask[cross], Corner::Rounded), '┼');
        // 只从上面接过来:三通里的 ┬
        let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, false));
        cv.edge((5, 5), (5, 9), 0, (255, 0, 0), None, (false, false));
        let tee = cv.idx(5, 5).expect("在画布内");
        assert_eq!(cv.mask[tee], LEFT | RIGHT | DOWN);
        assert_eq!(box_char(cv.mask[tee], Corner::Rounded), '┬');
    }

    /// "下去再上来"的两条边必须共用同一列:
    /// a 与 c 都在 b 的北边,于是进 b 与出 b 都走竖的,两条边落在同一列,
    /// 不会围出一个方框
    #[test]
    fn 下去又上来要走同一列() {
        let mut cv = Canvas::new((0, 0, 60, 30), vec![false; 2], 0);
        let (a, b, c) = ((10, 5), (13, 9), (12, 5));
        let arrive = arrive_hor(a, b, c);
        assert_eq!(arrive, Some(false), "两头都在北边,该竖着进节点");
        cv.edge(a, b, 0, (255, 0, 0), arrive, (false, false));
        cv.edge(b, c, 0, (255, 0, 0), None, (false, false));
        // b 所在那一列:从 a 的行到 b 的行,每一格都该有笔画
        for y in a.1..=b.1 {
            let i = cv.idx(b.0, y).expect("在画布内");
            assert_ne!(cv.mask[i], 0, "({}, {y}) 该有笔画", b.0);
        }
        // 同一个纵向区间里,别的列不该再出现笔画(那就是绕出来的方框)
        for x in a.0..b.0 {
            for y in a.1 + 1..b.1 {
                let i = cv.idx(x, y).expect("在画布内");
                assert_eq!(cv.mask[i], 0, "({x}, {y}) 不该有笔画:边绕出去了");
            }
        }
    }

    /// 有向边的箭头:画在终点紧挨着的那一格,方向按最后那一段的走向
    #[test]
    fn 有向边箭头画在终点那一格() {
        // 往东
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'));
        assert_eq!(cv.arrow[cv.idx(9, 5).unwrap()], None, "箭头不占终点本身");
        assert_eq!(cv.arrow[cv.idx(2, 5).unwrap()], None, "只在终点那一头画");
        // 往西:箭头跟着走,画在另一侧
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((9, 5), (2, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(3, 5).unwrap()], Some('<'));
        // 往南(屏幕行号变大)
        let mut cv = Canvas::new((0, 0, 20, 20), vec![false; 1], 0);
        cv.edge((5, 2), (5, 12), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 11).unwrap()], Some('v'));
        // 往北
        let mut cv = Canvas::new((0, 0, 20, 20), vec![false; 1], 0);
        cv.edge((5, 12), (5, 2), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 3).unwrap()], Some('^'));
    }

    /// 拐弯的边:箭头按**最后那一段**的走向,不是两个端点的连线方向
    #[test]
    fn 拐弯的有向边按最后一段画箭头() {
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        // 横向比纵向长,先横后竖:最后一段是竖的,往下走到 (20,10)
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (false, true));
        assert_eq!(
            box_char(cv.mask[cv.idx(20, 2).unwrap()], Corner::Rounded),
            '╮',
            "先横后竖"
        );
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "竖着进节点");
        // 箭头画在 a 那一头时也一样:它指着箭头所在的那个节点,所以是反着的
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, false));
        assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "指回 (2,2)");
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], None, "另一头不画");
    }

    /// 箭头不盖节点字形
    #[test]
    fn 箭头不盖节点字形() {
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.node_at(5, 5, false, 1, Some(0), (0, 255, 0));
        cv.station_cells.insert((5, 5));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 5).unwrap()], None, "节点那一格不画箭头");
        assert_eq!(cv.arrow[cv.idx(4, 5).unwrap()], None, "箭头不会往前挪一格");
    }

    /// 一格已经归别的分组(两组挤在同一格)时不画箭头:方向说不清
    #[test]
    fn 箭头不进别人占的格子() {
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 2], 0);
        cv.edge((0, 5), (9, 5), 1, (0, 0, 255), None, (false, false));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], None);
        assert_eq!(
            cv.groups_here[cv.idx(8, 5).unwrap()],
            0b11,
            "两组都在这格上"
        );
        // 一格最多一个箭头:同一笔画再走一遍也不重复画
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 2], 0);
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        let n = cv.arrow.iter().filter(|a| a.is_some()).count();
        assert_eq!(n, 1, "两笔画同一格时只留一个箭头");
        assert_eq!(
            cv.arrow[cv.idx(8, 5).unwrap()],
            Some('>'),
            "先到的那一个说了算"
        );
    }

    /// 文字节点(none)没有边框可画,但端口照样按顺序占格:
    /// 同一个节点上的多条边不会挤在同一个进出点
    #[test]
    fn 文字节点的端口也会顺延() {
        let mut cv = Canvas::new((0, 0, 40, 12), vec![false; 1], 0);
        let rect = (10, 5, 8, 1); // 一行八列的文字节点
        let end = EndMark {
            rect,
            side: UP,
            cell: Canvas::side_base(rect, UP),
            mark: false, // 没有边框
        };
        for k in 0..4 {
            cv.pen_edge = k;
            cv.edge_full(
                (20, 0),
                (18, 5),
                0,
                (255, 0, 0),
                None,
                (false, false),
                EdgeStyle::Solid,
                None,
                Some(end),
            );
        }
        let ports: Vec<(i32, i32)> = cv
            .border
            .iter()
            .filter(|(_, m)| matches!(m, BorderMark::Port))
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(ports.len(), 4, "四条边该占四个不同的端口格:{ports:?}");
        // 占的都在这个节点自己的那一行里
        assert!(ports
            .iter()
            .all(|c| c.1 == rect.1 && c.0 >= rect.0 && c.0 < rect.0 + rect.2));
    }

    /// 同一个分组里 a -> b 与 b -> a 是两条边,合到一段上就是两端各一个箭头
    #[test]
    fn 双向边两端都有箭头() {
        // 直线:去程那一头朝东,回程那一头朝西
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'), "箭头对着 (9,5)");
        assert_eq!(cv.arrow[cv.idx(1, 5).unwrap()], Some('<'), "箭头对着 (0,5)");
        // 拐弯的也一样:两端各自照自己那一端的走向画
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "b 端竖着进来");
        assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "a 端横着回去");
        // 两个节点挨着(中间没有空格)时箭头没地方放,节点字形优先,谁都不画
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        for x in [4, 5] {
            cv.node_at(x, 5, false, 1, Some(0), (0, 255, 0));
            cv.station_cells.insert((x, 5));
        }
        cv.edge((4, 5), (5, 5), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow.iter().filter(|a| a.is_some()).count(), 0);
    }
}
