//! 光栅化:把画布坐标画成"每格往哪几个方向连",再按方向位合成制表符.
//! 这一层只认识字符格,不认识 ratatui(上屏由 render 做).

use std::collections::{HashMap, HashSet};

use crate::model::{BoxStyle, Corner, EdgeStyle};

/// 走线代价权重(按"看起来多糟糕"排):同轴重叠最糟 —— 后画的边整段看不见
const C_BEND: i32 = 10;
const C_CROSS: i32 = 25;
/// 规划结果:一条边的路线和两端端口
#[derive(Clone, Default)]
pub(crate) struct Stroke {
    pub(crate) pts: Vec<(i32, i32)>,
    pub(crate) a_port: Option<(u8, (i32, i32))>,
    pub(crate) b_port: Option<(u8, (i32, i32))>,
}

const C_OVERLAP: i32 = 40;
/// 规划出来的笔画(还没落笔)的代价:比真墨轻,但足以让后规划的边主动错开
const C_USAGE: i32 = 18;
const C_OTHER_GROUP: i32 = 6;
/// 踩到别人的端口/箭头格:软障碍,能绕开就绕开
const C_FOREIGN_PORT: i32 = 100;

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
pub(crate) fn junction_glyph(style: BoxStyle, mask: u8) -> char {
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
    /// 端口能不能落在四个角(带框的不能,角上要留拐角字形).
    /// 只影响自环扫格;普通边的端口由 render 的 PortPlan 直接给
    pub corners: bool,
    /// 端口格已经由 PortPlan 定好:不再扫边框找空位
    pub fixed: bool,
}

/// 边框格被覆盖成什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BorderMark {
    /// 连接点(T):横向 + 线从哪一面出去 + 这条边的颜色
    Junction(u8, (u8, u8, u8)),
    /// 终点箭头:字形 + 边色
    Arrow(char, (u8, u8, u8)),
    /// 只占位不画:文字节点没有边框,但端口格照样要被占住,
    /// 这样同一侧再接边时会顺延到下一格,多条边不会挤在同一个进出点
    Port,
}

/// (箭头格, owner 边) -> 墨色
pub(crate) type OwnerInk = HashMap<((i32, i32), usize), (u8, u8, u8)>;

/// 端口格的角色:一格的端口只能是这两种之一,不会混
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortKind {
    /// 箭头端
    Arrow,
    /// 普通连接端(T 合并)
    Line,
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
    /// 整幅图的范围(左上,右下,含).走线只在这个范围里绕,不跟着视口变
    pub(crate) frame: (i32, i32, i32, i32),
    /// 走线用的每格状态,画布外也记:视图挪动时路线不能跟着变
    /// (`.0` = 这格上有哪些分组的笔画,`.1` = 这格被占住的笔画轴向)
    pub(crate) ink: HashMap<(i32, i32), (u64, u8)>,
    /// 规划阶段算出来、还没落笔的笔画:`ink` 的反面,让后规划的边提前躲开它们.
    /// `.0` = 有哪些分组要从这儿过,`.1` = 方向位
    pub(crate) usage: HashMap<(i32, i32), (u64, u8)>,
    /// 只算路、不落笔:规划那一遍用它
    pub(crate) planning: bool,
    /// 规划阶段每条边占过的格子:轮到画它时先把自己的记账撤掉(占位的是它自己)
    pub(crate) usage_edges: HashMap<usize, Vec<(i32, i32)>>,
    /// 规划出来的笔画,按调用顺序排好;落笔那一遍直接取,不再重算一遍路由
    pub(crate) plans: Vec<Stroke>,
    pub(crate) plan_cursor: usize,
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
    /// 这套字形(Unicode / ASCII)
    pub(crate) glyphs: crate::glyphs::Glyphs,
    /// 这一格有没有被实线占过(实线优先,虚线不在这里断)
    pub(crate) solid: Vec<bool>,
    /// 节点框的装饰层:在连线之后画,盖在线上面
    pub(crate) deco: Vec<Option<Deco>>,
    /// 有向边画在箭头那一格的字形(None = 这一格没有箭头;只给点节点用)
    pub(crate) arrow: Vec<Option<char>>,
    /// 箭头自己的墨色(格子可能被别的边占着,颜色不能跟着格子走)
    pub(crate) arrow_ink: Vec<Option<(u8, u8, u8)>>,
    /// 箭头格 -> 把这里当箭头端点的边(第一个是画出字形的那条,同方向的边共用)
    pub(crate) arrow_owners: HashMap<(i32, i32), Vec<usize>>,
    /// (箭头格, owner 边) -> 这条边自己的墨色:共享箭头按"当前激活的 owner"取色
    pub(crate) arrow_owner_ink: OwnerInk,
    /// 端口格的角色(Arrow / Line):一格只能是一种
    pub(crate) port_kind: HashMap<(i32, i32), PortKind>,
    /// 端口格 -> 占着它的边(第一个是可见的那条)
    pub(crate) port_owners: HashMap<(i32, i32), Vec<usize>>,
    /// 端口**外侧**格 -> 角色.`none` 节点的箭头/线真正画在外侧那一格,
    /// 两个格都要有角色,不然线端能和别人的箭头外侧格重合
    pub(crate) port_out_kind: HashMap<(i32, i32), PortKind>,
    /// 每条边的可见笔画(选中/overlay 按它画,不再用 edge_cells 猜)
    pub(crate) stroke_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// 每条边自己的箭头端点格(只来自两端,不来自路径中段)
    pub(crate) arrows_of: HashMap<usize, Vec<(i32, i32)>>,
    /// 每条边**实际**用到的端口外侧格(线真正起步/箭头真正落点的格).
    /// 以落笔这一遍为准:规划可能没有这一端(挤满时走 pick_port 兜底)
    pub(crate) edge_out_cells: HashMap<usize, Vec<(i32, i32)>>,
    /// 端口/箭头格 -> 规划到那里的边:路由把别人的端口当软障碍
    pub(crate) port_guest: HashMap<(i32, i32), usize>,
    /// 选中那条边的端口外侧格:这一格上别人的箭头要藏起来(否则看着像 <->)
    pub(crate) picked_port_out: Vec<(i32, i32)>,
    /// 边框覆盖表:T 与嵌入边框的箭头
    pub(crate) border: HashMap<(i32, i32), BorderMark>,
    /// 全局箭头字形风格(`--arrow`;每条边没写 head 时用它)
    pub(crate) arrow_style: crate::model::ArrowStyle,
    /// 正在画的那条边的箭头字形(画之前设,None 时退回全局值)
    pub(crate) pen_arrow: crate::model::ArrowStyle,
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
    /// 这一格**可见形状**属于哪条边(-1 = 没有).重叠时只有它画出来,其余断一格
    pub(crate) shape_edge: Vec<i32>,
    /// 要压到最上面的边(选中的那条、E 模式下亮着的那几条):格子归它们,箭头也归它们
    pub(crate) raised: HashSet<usize>,
    /// 用户选中的那条边:它压过所有别的边(包括同样抬起来的),这样选中时整条都亮
    pub(crate) picked: Option<usize>,
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
    pub(crate) fn new(
        area: (i32, i32, i32, i32),
        priority: Vec<bool>,
        focus: u64,
        glyphs: crate::glyphs::Glyphs,
    ) -> Canvas {
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
            ink: HashMap::new(),
            usage: HashMap::new(),
            planning: false,
            usage_edges: HashMap::new(),
            plans: Vec::new(),
            plan_cursor: 0,
            frame: (x0, y0, x0 + w.max(0) - 1, y0 + h.max(0) - 1),
            style: vec![EdgeStyle::Solid; n],
            corner: vec![Corner::Rounded; n],
            pen_corner: Corner::Rounded,
            pen_bg: None,
            edge_bg: vec![None; n],
            glyphs,
            solid: vec![false; n],
            deco: vec![None; n],
            arrow: vec![None; n],
            arrow_ink: vec![None; n],
            arrow_owners: HashMap::new(),
            arrow_owner_ink: HashMap::new(),
            port_kind: HashMap::new(),
            port_owners: HashMap::new(),
            port_out_kind: HashMap::new(),
            stroke_cells: HashMap::new(),
            arrows_of: HashMap::new(),
            edge_out_cells: HashMap::new(),
            port_guest: HashMap::new(),
            picked_port_out: Vec::new(),
            border: HashMap::new(),
            arrow_style: Default::default(),
            pen_arrow: Default::default(),
            preview: false,
            pen_edge: 0,
            edge_of: vec![-1; n],
            shape_edge: vec![-1; n],
            raised: HashSet::new(),
            picked: None,
            edge_cells: HashMap::new(),
            edge_cands: HashMap::new(),
            focus,
            priority,
            lit: vec![false; n],
            layer_lit: true,
            layer_filter: false,
        }
    }

    /// 把一条边抬到最上面(选中的边、E 模式下亮着的边)
    pub(crate) fn raise(&mut self, edge: Option<usize>) {
        if let Some(e) = edge {
            self.raised.insert(e);
        }
    }

    /// 一格只能有一种颜色,也只能有一种形状.
    /// 颜色谁说了算:光标所在节点的那些分组 > 先画的分组 —— 于是两组挤在同一格时,
    /// 颜色会跟着光标走,从当前节点就能顺着颜色找到自己那一组.
    /// 形状只按"先画的说了算",**不跟光标走**:不然挪一下光标,谁在这里断一格就翻到
    /// 另一组上,整段边的字形都跟着变,看着就是边在变形
    fn claim(&mut self, i: usize, group: usize, bit: u8, fg: (u8, u8, u8), style: EdgeStyle) {
        let cur = self.shape_edge[i];
        let solid = style == EdgeStyle::Solid;
        if self.layer_lit {
            // 这一格有当前层的线经过:哪怕形状归别的层,也不压暗
            self.lit[i] = true;
        }
        // 同一条边在同一格又走了一遍(拐弯/自环):方向位合并成拐角
        if cur == self.pen_edge as i32 {
            self.note_stroke(i);
            self.mask[i] |= bit;
            if solid {
                self.style[i] = EdgeStyle::Solid;
                self.solid[i] = true;
            }
            if self.priority[group] {
                self.fg[i] = fg;
            }
            return;
        }
        // 空着 / 这一格原来归选中的那条(它最高优先) / 这条边抬起来了
        let mine = self.picked == Some(self.pen_edge) || self.raised.contains(&self.pen_edge);
        let taken = self.picked.is_some_and(|p| p == cur as usize);
        if (cur < 0 || mine) && !taken {
            self.note_stroke(i);
            self.shape_edge[i] = self.pen_edge as i32;
            // 形状只按这条边自己的方向位画:两条线叠在一起不会变成 ┼ / ├
            self.mask[i] = bit;
            self.owner[i] = group as i16;
            self.owner_mask[i] = bit;
            self.fg[i] = fg;
            self.mine[i] = self.priority[group];
            self.style[i] = style;
            self.solid[i] = solid;
            self.corner[i] = self.pen_corner;
            return;
        }
        // 这一格已经被别的边占住:它从这儿"断一格",只记候选.
        // 形状和颜色都跟先画的边:光标所在分组不能只把这一格染成自己的颜色,
        // 那会让别人的一段线看着像断了. 要整条变色就走 raised(选中/E 模式).
        self.note_candidate(i);
        if solid {
            self.style[i] = EdgeStyle::Solid;
            self.solid[i] = true;
        }
    }

    /// 这一格最后由正在画的这条边说了算:只进命中表(候选/并集),不表示角色
    fn note_hit(&mut self, i: usize) {
        self.edge_of[i] = self.pen_edge as i32;
        if self.pen_bg.is_some() {
            self.edge_bg[i] = self.pen_bg;
        }
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        self.edge_cells.entry(self.pen_edge).or_default().push(cell);
        self.note_candidate(i);
    }

    /// 这一格是这条边的可见笔画
    fn note_stroke(&mut self, i: usize) {
        self.note_hit(i);
        let cell = (self.x0 + (i as i32 % self.w), self.y0 + (i as i32 / self.w));
        self.stroke_cells
            .entry(self.pen_edge)
            .or_default()
            .push(cell);
    }

    /// 端口格:角色唯一(Arrow / Line),同角色可以被多条边共用.
    /// 外侧格 `outside(cell, side)` 同时记角色:线的起点和箭头落点都在那一格
    fn note_port(&mut self, cell: (i32, i32), side: u8, kind: PortKind) {
        self.note_port_cell(cell, kind);
        self.note_out_role(Self::outside(cell, side), kind);
        let owners = self.port_owners.entry(cell).or_default();
        if !owners.contains(&self.pen_edge) {
            owners.push(self.pen_edge);
        }
        if let Some(i) = self.idx(cell.0, cell.1) {
            self.note_hit(i);
        }
    }

    /// 外侧格的角色.和端口格一样:箭头端优先(它要在那一格画字形),线端只记账
    fn note_out_role(&mut self, cell: (i32, i32), kind: PortKind) {
        match self.port_out_kind.get(&cell).copied() {
            None => {
                self.port_out_kind.insert(cell, kind);
            }
            Some(PortKind::Line) if kind == PortKind::Arrow => {
                self.port_out_kind.insert(cell, PortKind::Arrow);
            }
            Some(_) => {}
        }
    }

    /// 记端口格本身的角色(外侧格走 `note_out_role`)
    fn note_port_cell(&mut self, cell: (i32, i32), kind: PortKind) {
        // 一格的端口角色唯一:箭头端始终优先(它要在这一格/外面画出箭头).
        // 线端并到箭头格上只是记账,这一格画的仍然是箭头;
        // 箭头端接管线端占位时直接升级(文字节点的端口格本来什么都不画)
        match self.port_kind.get(&cell).copied() {
            None => {
                self.port_kind.insert(cell, kind);
            }
            Some(PortKind::Line) if kind == PortKind::Arrow => {
                self.port_kind.insert(cell, PortKind::Arrow);
            }
            // 线端并到箭头格上只记账:这一格画的仍然是箭头,不压 T
            Some(PortKind::Arrow) if kind == PortKind::Line => {}
            Some(_) => {}
        }
    }

    /// 记一条边实际用到的端口外侧格
    fn note_out(&mut self, cell: (i32, i32)) {
        let list = self.edge_out_cells.entry(self.pen_edge).or_default();
        if !list.contains(&cell) {
            list.push(cell);
        }
    }

    /// 箭头格:同方向已经画过就只加 owner,字形/颜色留在第一条边上
    fn note_arrow(&mut self, cell: (i32, i32), fg: (u8, u8, u8)) {
        // 每个 owner 记自己的墨色:共享箭头亮起来时要跟着那条边的颜色
        self.arrow_owner_ink
            .entry((cell, self.pen_edge))
            .or_insert(fg);
        let owners = self.arrow_owners.entry(cell).or_default();
        if !owners.contains(&self.pen_edge) {
            owners.push(self.pen_edge);
        }
        let mine = self.arrows_of.entry(self.pen_edge).or_default();
        if !mine.contains(&cell) {
            mine.push(cell);
        }
        if let Some(i) = self.idx(cell.0, cell.1) {
            self.note_hit(i);
        }
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
        // 画布外的格子也记一笔:走线要看"这一格有没有别的笔画",跟着视口变就会乱跳.
        // 轴向也不分"占没占住":并线看起来就是一条线,和谁赢这一格无关
        let axis = if bit & LR != 0 { LR } else { UD };
        let ink = self.ink.entry((x, y)).or_default();
        ink.0 |= 1 << group;
        ink.1 |= axis;
        let Some(i) = self.idx(x, y) else { return };
        self.groups_here[i] |= 1 << group;
        self.claim(i, group, bit, fg, style);
    }

    /// 只给这一格加一个方向位, 不碰相邻格(也不碰端口格).
    /// `none` 节点的端口到外侧那一格本来不画线(免得压到文字上), 但外侧格需要
    /// "朝向端口"的那一位, 路线在这里转弯时才会画出 ╭╮╰╯ 而不是断成 ─
    fn link_here(
        &mut self,
        cell: (i32, i32),
        bit: u8,
        group: usize,
        fg: (u8, u8, u8),
        style: EdgeStyle,
    ) {
        let axis = if bit & LR != 0 { LR } else { UD };
        let ink = self.ink.entry(cell).or_default();
        ink.0 |= 1 << group;
        ink.1 |= axis;
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
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
        let has = |p: (i32, i32)| self.ink.get(&p).is_some_and(|v| v.0 & bit != 0);
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

    /// 这一格已经有一条**同方向**的笔画了吗.交叉过一格无所谓(很常见),整段并上去
    /// 就分不清是几条边了 —— 后画的那条会整段看不见,箭头还留在别人的笔画上
    fn dup_run(&self, p: (i32, i32), axis: u8) -> bool {
        self.ink.get(&p).is_some_and(|v| v.1 & axis != 0)
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
            .any(|p| self.ink.get(&p).map(|v| v.0 & bit == 0).unwrap_or(true));
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
        let mine =
            |p: (i32, i32)| path.contains(&p) || self.ink.get(&p).is_some_and(|v| v.0 & bit != 0);
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
        // 横段和竖段分开算:同向并线比交叉过一格难看多了
        for (p0, p1) in [(a, corner), (corner, b)] {
            let axis = if p0.1 == p1.1 { LR } else { UD };
            for p in self.span(p0, p1) {
                if p == a || p == b {
                    continue;
                }
                if self.dup_run(p, axis) {
                    n += 8;
                }
            }
        }
        for p in self.span(a, corner).into_iter().chain(self.span(corner, b)) {
            if p == a || p == b {
                continue; // 两个端点本来就是节点
            }
            let groups = self.ink.get(&p).map(|v| v.0).unwrap_or(0);
            if groups != 0 && groups & bit == 0 {
                n += 1;
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
        let share_a = a_end.is_some_and(|e| e.corners);
        let share_b = b_end.is_some_and(|e| e.corners);
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
        // 规划那一遍:算路、记账、把结果存起来
        if self.planning {
            self.drop_own_usage();
            let pts = self.route(s, out, t, into, group);
            if pts.is_empty() && (a_end.is_some_and(|e| e.fixed) || b_end.is_some_and(|e| e.fixed))
            {
                // 计划给的端口一步都走不出去(对着别的框):换成"扫边框找空位"重试一次,
                // 让这条边还能画出来,而不是整条消失
                let a2 = a_end.map(|e| EndMark { fixed: false, ..e });
                let b2 = b_end.map(|e| EndMark { fixed: false, ..e });
                self.edge_full(a, b, group, fg, arrive_hor, heads, style, a2, b2);
                return;
            }
            let mut all = Vec::with_capacity(pts.len() + 2);
            if let Some((_, c)) = a_port {
                all.push(c);
            }
            all.extend_from_slice(&pts);
            if let Some((_, c)) = b_port {
                all.push(c);
            }
            self.note_usage(&all, group);
            self.plans.push(Stroke {
                pts,
                a_port,
                b_port,
            });
            return;
        }
        // 落笔这一遍照着规划好的路线走:重算一遍既慢,也可能和规划不一致
        let (pts, a_port, b_port) = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            (st.pts, st.a_port, st.b_port)
        } else {
            (self.route(s, out, t, into, group), a_port, b_port)
        };
        // 端口格子以最终采用的那一对为准:规划那一遍定的端口可能和这次重新挑的不同
        // (角色的占用在两次之间会变),箭头/短接必须落在真正用的那一格外面
        let s = a_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(a);
        let t = b_port.map(|(side, c)| Self::outside(c, side)).unwrap_or(b);
        // 实际端口外侧格(兜底路径也算):选中时靠它压住别人的箭头
        if a_port.is_some() {
            self.note_out(s);
        }
        if b_port.is_some() {
            self.note_out(t);
        }
        if pts.is_empty() {
            // 这一段找不到路:整条边一笔都不画,端口短接和箭头也不落
            return;
        }
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
                // 文字节点:端口格不画, 但外侧格要补"朝向端口"那一笔,
                // 否则路线在这儿转弯只剩一个方向位, 画出来是 ─ 而不是拐角
                let _ = c;
                self.link_here(s, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            } else {
                let _ = c;
                self.link_here(t, Self::into_side(side), group, fg, style);
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
            self.commit_end(side, c, at_b, fg, draw, share_b);
            if !draw && at_b {
                // 没有边框:箭头落在区域外面那一格
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(prev, pts[pts.len() - 1], group, fg, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw, share_a);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(next, pts[0], group, fg, None);
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
        // 规划那一遍只记路,不落笔:自环会挑端口、占边框格,
        // 提前画一遍会让落笔那一遍挑到别的口,环就断了
        if self.planning {
            return;
        }
        let (at_a, at_b) = heads;
        let side2 = match end.side {
            UP => RIGHT,
            RIGHT => DOWN,
            DOWN => LEFT,
            _ => UP,
        };
        // 没有边框的节点(文字节点/点节点):端口格就是节点自己那一行,
        // 那一行上早被别的边占住了也得用——自环再被挤掉就彻底看不见了,
        // 所以这一端直接取该侧的中心格,不去扫空位
        let (s1, c1) = if end.mark {
            match self.pick_port(end, at_a) {
                Some(x) => x,
                None => return,
            }
        } else {
            (end.side, Self::side_base(end.rect, end.side))
        };
        // 第二端优先挑离第一端最近的那一格:环才会贴着节点收紧,不会横跨半个框
        let nearest = Self::side_cells(end.rect, side2, end.corners)
            .into_iter()
            .min_by_key(|c| (c.0 - c1.0).abs() + (c.1 - c1.1).abs())
            .unwrap_or_else(|| Self::side_base(end.rect, side2));
        let probe2 = EndMark {
            fixed: false,
            rect: end.rect,
            side: side2,
            cell: nearest,
            mark: end.mark,
            corners: end.corners,
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
        self.note_out(s);
        self.note_out(t);
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
        } else {
            // 文字节点:端口格不画, 外侧格补"朝向端口"那一笔(转弯才有拐角)
            self.link_here(s, Self::into_side(s1), group, fg, style);
            self.link_here(t, Self::into_side(s2), group, fg, style);
        }
        self.commit_end(s1, c1, at_a, fg, end.mark, end.corners);
        self.commit_end(s2, c2, at_b, fg, end.mark, end.corners);
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
        if self.planning {
            // 分道的边也要记账(以前这里直接落笔,规划那一遍把墨弄脏了)
            self.note_usage(&pts, group);
            self.plans.push(Stroke {
                pts: pts.to_vec(),
                a_port: None,
                b_port: None,
            });
            return;
        }
        // 落笔这一遍必须把规划时排的这条也取走,否则后面的边会错位
        let pts = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            st.pts
        } else {
            pts.to_vec()
        };
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        if at_b {
            self.head_arrow(pts[2], b, group, fg, None);
        }
        if at_a {
            self.head_arrow(pts[1], a, group, fg, None);
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
        // 老规矩:同排/同列就直着连,否则按代价挑一个 L
        let hor_first = (b.0, a.1);
        let ver_first = (a.0, b.1);
        let prefer_hor = arrive_hor
            .map(|h| !h)
            .unwrap_or((b.0 - a.0).abs() >= (b.1 - a.1).abs());
        let corner = if a.1 == b.1 || a.0 == b.0 {
            None
        } else {
            let (c1, c2) = (
                self.cost(a, hor_first, b, group),
                self.cost(a, ver_first, b, group),
            );
            Some(if prefer_hor {
                if c1 <= c2 + 1 {
                    hor_first
                } else {
                    ver_first
                }
            } else if c2 <= c1 + 1 {
                ver_first
            } else {
                hor_first
            })
        };
        let mut simple: Vec<(i32, i32)> = vec![a];
        if let Some(c) = corner {
            simple.push(c);
        }
        simple.push(b);
        // 这个 L 被框挡住(墙比搜索窗口还高就是这种):升级成避障搜索
        let blocked = simple.windows(2).any(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            (1..steps).any(|k| {
                self.station_cells
                    .contains(&(w[0].0 + dx.signum() * k, w[0].1 + dy.signum() * k))
            })
        });
        if !self.planning {
            self.drop_own_usage();
        }
        let pts = if blocked {
            let into = arrive_hor.map(|h| {
                if h {
                    ((b.0 - a.0).signum(), 0)
                } else {
                    (0, (b.1 - a.1).signum())
                }
            });
            self.route(a, None, b, into, group)
        } else {
            simple
        };
        if self.planning {
            self.note_usage(&pts, group);
            self.plans.push(Stroke {
                pts,
                a_port: None,
                b_port: None,
            });
            return;
        }
        // 落笔这一遍用规划好的路线,不再重算
        let pts = if self.plan_cursor < self.plans.len() {
            let st = self.plans[self.plan_cursor].clone();
            self.plan_cursor += 1;
            st.pts
        } else {
            pts
        };
        if pts.is_empty() {
            return; // 找不到路:这条边整条不画
        }
        for w in pts.windows(2) {
            self.run(w[0], w[1], group, fg, style);
        }
        if at_b {
            let from = if pts.len() >= 2 {
                pts[pts.len() - 2]
            } else {
                a
            };
            self.head_arrow(from, b, group, fg, None);
        }
        if at_a {
            let from = if pts.len() >= 2 { pts[1] } else { b };
            self.head_arrow(from, a, group, fg, None);
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
        let share_a = a_end.is_some_and(|e| e.corners);
        let share_b = b_end.is_some_and(|e| e.corners);
        let s = a_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[0]);
        let t = b_port
            .map(|(side, c)| Self::outside(c, side))
            .unwrap_or(pts[pts.len() - 1]);
        // 手工折点:丢掉"折回来"的那种.相邻两个折点在同一列/行上,而走向在这一点反了
        // (先往下再往上,或先往左再往右),那个折点只会让线绕出去再回来,画面上多一截尾巴
        let vias: Vec<(i32, i32)> = pts[1..pts.len() - 1].to_vec();
        let mut kept: Vec<(i32, i32)> = Vec::with_capacity(vias.len());
        let mut cursor = s;
        for (i, &v) in vias.iter().enumerate() {
            let next = vias.get(i + 1).copied().unwrap_or(t);
            // 同一列上"先下去再上来",或同一行上"先过去再回来" -> 这是个回折点
            let back_x = v.1 == next.1
                && (v.0 - cursor.0).signum() != 0
                && (v.0 - cursor.0).signum() != (next.0 - v.0).signum();
            let back_y = v.0 == next.0
                && (v.1 - cursor.1).signum() != 0
                && (v.1 - cursor.1).signum() != (next.1 - v.1).signum();
            if back_x || back_y {
                continue;
            }
            kept.push(v);
            cursor = v;
        }
        let mut way: Vec<(i32, i32)> = vec![s];
        way.extend(kept);
        way.push(t);
        if self.planning {
            // 手工折点的边也算在规划里,别的边能躲开它
            self.note_usage(&way, group);
            return;
        }
        // 实际用到的端口外侧格(兜底路径也算)
        if a_port.is_some() {
            self.note_out(s);
        }
        if b_port.is_some() {
            self.note_out(t);
        }
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
        if let Some((side, c)) = a_port {
            if a_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, s, group, fg, style);
            } else {
                self.link_here(s, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            if b_end.map(|e| e.mark).unwrap_or(false) {
                self.run(c, t, group, fg, style);
            } else {
                self.link_here(t, Self::into_side(side), group, fg, style);
            }
        }
        if let Some((side, c)) = b_port {
            let draw = b_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_b, fg, draw, share_b);
            if !draw && at_b {
                self.outside_arrow(t, side, fg);
            }
        } else if at_b {
            self.head_arrow(way[n - 2], t, group, fg, None);
        }
        if let Some((side, c)) = a_port {
            let draw = a_end.map(|e| e.mark).unwrap_or(false);
            self.commit_end(side, c, at_a, fg, draw, share_a);
            if !draw && at_a {
                self.outside_arrow(s, side, fg);
            }
        } else if at_a {
            self.head_arrow(way[1], s, group, fg, None);
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

    /// 从外侧格指向端口的那一位(`none` 节点补隐藏方向位用)
    pub(crate) fn into_side(side: u8) -> u8 {
        match side {
            UP => DOWN,
            DOWN => UP,
            LEFT => RIGHT,
            _ => LEFT,
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

    /// 折线的代价:碰到框直接判死,其余按加权和算(见 `C_*`)。
    /// 端点自己不算重叠/交叉:固定端口本来就会落在这里
    fn route_cost(&self, pts: &[(i32, i32)], group: usize) -> Option<i32> {
        let bit = 1u64 << group;
        let last = pts.len() - 1;
        let mut cost = 0;
        for (k, w) in pts.windows(2).enumerate() {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            let axis = if dx != 0 { LR } else { UD };
            for step in 0..=steps {
                let p = (w[0].0 + (dx.signum()) * step, w[0].1 + (dy.signum()) * step);
                let is_end = (k == 0 && step == 0) || (k == last - 1 && step == steps);
                if self.station_cells.contains(&p) && !is_end {
                    return None; // 不穿框
                }
                cost += 1;
                if is_end {
                    continue;
                }
                if let Some((groups, axes)) = self.ink.get(&p) {
                    if *axes & axis != 0 {
                        // 同轴:并上去以后这一段就看不见了,最贵
                        cost += C_OVERLAP;
                    } else if *axes != 0 {
                        cost += C_CROSS;
                    }
                    if *groups != 0 && *groups & bit == 0 {
                        cost += C_OTHER_GROUP;
                    }
                } else if let Some((_, axes)) = self.usage.get(&p) {
                    // 规划里已经有边要走这儿:轻一点,但够让同一对框之间的两条线分开
                    cost += if *axes & axis != 0 {
                        C_USAGE
                    } else {
                        C_USAGE / 3
                    };
                }
            }
            if k > 0 {
                cost += C_BEND;
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
        // 实在绕不开:走一个 L,碰到框就不画(不留悬空线)
        self.route_clip(s, t)
    }

    /// 固定形状里挑总代价最小的那个.折弯不再绝对优先:
    /// 多一个折弯但少压一大段别人的线是划算的
    fn route_shaped(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
    ) -> Option<Vec<(i32, i32)>> {
        let manhattan = (s.0 - t.0).abs() + (s.1 - t.1).abs();
        // 绕太远的候选不要:宁可多压一点,也别把线拉成一个大圈
        let limit = manhattan * 3 + 40;
        let mut best: Option<(i32, Polyline)> = None;
        for cand in Self::shapes(s, out, t, into) {
            let Some(cost) = self.route_cost(&cand, group) else {
                continue;
            };
            let len: i32 = cand
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs())
                .sum();
            if len > limit {
                continue;
            }
            if best.as_ref().map(|(c, _)| cost < *c).unwrap_or(true) {
                best = Some((cost, cand));
            }
        }
        best.map(|b| b.1)
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
        // 小窗口先试(便宜),不够再往外放:墙比 6 格宽时也能绕过去.
        // 整幅 frame 太大(几千格见方)时不试第二档:那是给大图留的降级口子
        let (fx0, fy0, fx1, fy1) = self.frame;
        let area = (fx1 - fx0).max(0) as i64 * (fy1 - fy0).max(0) as i64;
        if let Some(pts) = self.route_search_in(s, out, t, into, group, 6) {
            return Some(pts);
        }
        if area > 250_000 {
            return None;
        }
        self.route_search_in(s, out, t, into, group, i32::MAX)
    }

    fn route_search_in(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
        pad: i32,
    ) -> Option<Vec<(i32, i32)>> {
        self.route_search_ext(s, out, t, into, group, pad, 40_000)
    }

    /// 同上,但可以给一个更大的搜索预算
    #[allow(clippy::too_many_arguments)]
    fn route_search_ext(
        &self,
        s: (i32, i32),
        out: Option<(i32, i32)>,
        t: (i32, i32),
        into: Option<(i32, i32)>,
        group: usize,
        pad: i32,
        budget: u64,
    ) -> Option<Vec<(i32, i32)>> {
        const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let dir_idx = |d: (i32, i32)| DIRS.iter().position(|x| *x == d);
        let turn = C_BEND;
        let bit = 1u64 << group;
        // 窗口:两端包围盒外扩几格,但优先留在画布里(绕到画面外的线看不见);
        // 两端本身一定要包含进来,哪怕它们在画面外
        // pad 可能是 i32::MAX("整幅图"),用饱和运算免得溢出
        let (lx, hx) = (
            s.0.min(t.0).saturating_sub(pad),
            s.0.max(t.0).saturating_add(pad),
        );
        let (ly, hy) = (
            s.1.min(t.1).saturating_sub(pad),
            s.1.max(t.1).saturating_add(pad),
        );
        // 窗口夹在整幅图里:跟着视口走的话路线会随视图变,但绕到图外也没意义
        let (fx0, fy0, fx1, fy1) = self.frame;
        let lx = lx.max(fx0).min(s.0.min(t.0));
        let hx = hx.min(fx1).max(s.0.max(t.0));
        let ly = ly.max(fy0).min(s.1.min(t.1));
        let hy = hy.min(fy1).max(s.1.max(t.1));
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
                    let axis = if dx != 0 { LR } else { UD };
                    let cost = 1 + self.step_cost(p, bit, axis);
                    heap.push(std::cmp::Reverse((cost + h(p), cost, p, d)));
                    best.insert(key(p, d), cost);
                    from.insert(key(p, d), (s, d));
                }
            }
        }
        let start_dir = out.and_then(dir_idx);
        let goal_dir = into.and_then(dir_idx);
        // 兜底上限:窗口再大也不至于把一帧的预算耗光
        let mut pops = 0u64;
        let budget: u64 = if pad <= 6 { 12_000 } else { budget };
        while let Some(std::cmp::Reverse((_, cost, p, d))) = heap.pop() {
            pops += 1;
            if pops > budget {
                return None;
            }
            // 过期状态先丢掉,再看是不是到了:反过来会把差的路径当成结果
            if best.get(&(p, d)).copied().unwrap_or(i32::MAX) < cost {
                continue;
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
            for (nd, (dx, dy)) in DIRS.iter().copied().enumerate() {
                let q = (p.0 + dx, p.1 + dy);
                if !free(q) {
                    continue;
                }
                if nd != d && start_dir.is_some() && p == s {
                    continue;
                }
                let axis = if dx != 0 { LR } else { UD };
                let nc = cost + 1 + self.step_cost(q, bit, axis) + if nd == d { 0 } else { turn };
                if nc < best.get(&key(q, nd)).copied().unwrap_or(i32::MAX) {
                    best.insert(key(q, nd), nc);
                    from.insert(key(q, nd), (p, d));
                    heap.push(std::cmp::Reverse((nc + h(q), nc, q, nd)));
                }
            }
        }
        None
    }

    /// 走这一格的额外代价:`axis` 是这一步的方向.
    /// 同轴重叠 = 并进别人的线(整段看不见),交叉只是打个结,别的分组是观感
    fn step_cost(&self, p: (i32, i32), bit: u64, axis: u8) -> i32 {
        // 别人的端口/箭头格:软障碍(能绕就绕,绕不过也不算错)
        let guest = if self.port_guest.get(&p).is_some_and(|e| *e != self.pen_edge) {
            C_FOREIGN_PORT
        } else {
            0
        };
        if let Some((groups, axes)) = self.ink.get(&p) {
            let mut cost = guest;
            if *axes & axis != 0 {
                cost += C_OVERLAP;
            } else if *axes != 0 {
                cost += C_CROSS;
            }
            if *groups != 0 && *groups & bit == 0 {
                cost += C_OTHER_GROUP;
            }
            return cost;
        }
        let usage = match self.usage.get(&p) {
            Some((_, axes)) if *axes & axis != 0 => C_USAGE,
            Some((_, axes)) if *axes != 0 => C_USAGE / 3,
            _ => 0,
        };
        guest + usage
    }

    /// 轮到画这条边了:先撤掉它自己那份"规划占位",免得自己的路线跟自己打架
    pub(crate) fn drop_own_usage(&mut self) {
        if let Some(cells) = self.usage_edges.get(&self.pen_edge) {
            for c in cells {
                self.usage.remove(c);
            }
        }
    }

    /// 规划阶段:把一条笔画占的格子记进 `usage`
    pub(crate) fn note_usage(&mut self, pts: &[(i32, i32)], group: usize) {
        let bit = 1u64 << group;
        let mut cells: Vec<(i32, i32)> = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let steps = dx.abs().max(dy.abs());
            let axis = if dx != 0 { LR } else { UD };
            for step in 0..=steps {
                let p = (w[0].0 + dx.signum() * step, w[0].1 + dy.signum() * step);
                let e = self.usage.entry(p).or_insert((0, 0));
                e.0 |= bit;
                e.1 |= axis;
                cells.push(p);
            }
        }
        self.usage_edges
            .entry(self.pen_edge)
            .or_default()
            .extend(cells);
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
                    // 走不过去:返回空路径,整条边都不画(不留半截线,也不落端口/箭头)
                    return Vec::new();
                }
                pts.push(next);
                cur = next;
            }
        }
        pts
    }

    /// 某一侧的连接格(按框的宽高奇偶取中)
    /// 某一侧的正中格.四个方向用同一套约定:`left + w/2` / `top + h/2`,
    /// 不再上下、左右各留一套奇偶规则(端口实际位置由 render 的 PortPlan 定,
    /// 这里只给自环/预览当起点)
    pub(crate) fn side_base(rect: (i32, i32, i32, i32), side: u8) -> (i32, i32) {
        let (left, top, w, h) = rect;
        match side {
            UP => (left + w / 2, top),
            DOWN => (left + w / 2, top + h - 1),
            LEFT => (left, top + h / 2),
            _ => (left + w - 1, top + h / 2),
        }
    }

    /// 这一侧除角以外的格子,按顺时针顺序
    /// 这一侧的格子,按顺时针顺序.
    /// `corners`:带框的节点不让线落在四个角上(角上要留拐角字形);
    /// 文字节点没有拐角,整条边都能接
    pub(crate) fn side_cells(
        rect: (i32, i32, i32, i32),
        side: u8,
        corners: bool,
    ) -> Vec<(i32, i32)> {
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
        let cells = Self::side_cells(end.rect, end.side, end.corners);
        if cells.is_empty() {
            return None;
        }
        let start = cells
            .iter()
            .position(|c| *c == end.cell || *c == Self::side_base(end.rect, end.side))
            .unwrap_or(0);
        // 端口角色:Arrow 格只给箭头端;Line 格只给普通连接,
        // 而且只有已经做成 T 的那一格才容得下第二条线(普通占位格仍然一格一条)
        let free = |c: &(i32, i32), s: &Self| match s.port_kind.get(c) {
            None => true,
            Some(PortKind::Arrow) => false,
            Some(PortKind::Line) => {
                !arrow && matches!(s.border.get(c), Some(BorderMark::Junction(..)))
            }
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
        // 普通边的端口是由 PortPlan 算好的固定格:不再看边框占用了哪几格.
        // 但这一格的角色要是被别的框抢成了另一类,退回扫边框(角色互斥是硬不变量)
        if end.fixed {
            let ok = match self.port_kind.get(&end.cell) {
                None => true,
                Some(k) => (*k == PortKind::Arrow) == arrow,
            };
            if ok {
                return Some((end.side, end.cell));
            }
        }
        let order = match end.side {
            UP => [UP, RIGHT, DOWN, LEFT],
            RIGHT => [RIGHT, DOWN, LEFT, UP],
            DOWN => [DOWN, LEFT, UP, RIGHT],
            _ => [LEFT, UP, RIGHT, DOWN],
        };
        for side in order {
            // 这一侧出去的第一格就是框:线一步也走不出去,换下一侧
            let base = if side == end.side {
                end.cell
            } else {
                Self::side_base(end.rect, side)
            };
            if self.station_cells.contains(&Self::outside(base, side)) {
                continue;
            }
            let probe = EndMark {
                fixed: false,
                rect: end.rect,
                side,
                // 首选格沿用调用方给的那一格:自环要靠它挑"离另一头最近"的位置
                cell: if side == end.side {
                    end.cell
                } else {
                    Self::side_base(end.rect, side)
                },
                mark: end.mark,
                corners: end.corners,
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
        share: bool,
    ) {
        if self.preview {
            return; // 预览不落 T/箭头,不然看着像已经建好了
        }
        if !draw {
            // 文字节点没有边框:端口格只记角色(同一侧默认"一格一条边",顺延到下一格);
            // 框比文字大的(换样式留下的白边)按带框的规矩来,多条边可以并到一个口上,
            // 这样换样式前后端口分配一致、走线不重排
            if share {
                let bits = match side {
                    UP | DOWN => LR | side,
                    _ => UD | side,
                };
                match self.border.get_mut(&cell) {
                    Some(BorderMark::Junction(m, c)) => {
                        *m |= bits;
                        *c = fg;
                    }
                    _ => {
                        self.border.insert(cell, BorderMark::Junction(bits, fg));
                    }
                }
            }
            let kind = if arrow {
                PortKind::Arrow
            } else {
                PortKind::Line
            };
            self.note_port(cell, side, kind);
            return;
        }
        if arrow {
            let ch = self.pen_arrow.arrow(side);
            // 同方向的箭头已经在这一格就共用:字形/颜色保持第一次落下的那条边,
            // 后来的边只进 owner 集合(选中/高亮任一条都会亮)
            let shared =
                matches!(self.border.get(&cell), Some(BorderMark::Arrow(a, _)) if *a == ch);
            if !shared {
                self.border.insert(cell, BorderMark::Arrow(ch, fg));
            }
            self.note_port(cell, side, PortKind::Arrow);
            self.note_arrow(cell, fg);
        } else {
            // 这一格已经被箭头占住(另一条边先落的):只记账,不往上压 T,
            // 一个格子的可见角色只能是其中一种
            if self.port_kind.get(&cell) == Some(&PortKind::Arrow) {
                self.note_port(cell, side, PortKind::Line);
                return;
            }
            // 上下两侧是"横线 + 竖着的线头",左右两侧是"竖线 + 横着的线头"
            let bits = match side {
                UP | DOWN => LR | side,
                _ => UD | side,
            };
            match self.border.get_mut(&cell) {
                Some(BorderMark::Junction(m, c)) => {
                    *m |= bits;
                    *c = fg;
                }
                _ => {
                    self.border.insert(cell, BorderMark::Junction(bits, fg));
                }
            }
            self.note_port(cell, side, PortKind::Line);
        }
    }

    /// 文字节点(没有边框)的箭头:落在区域外面那一格,方向仍然指向节点
    pub(crate) fn outside_arrow(&mut self, cell: (i32, i32), side: u8, fg: (u8, u8, u8)) {
        let Some(i) = self.idx(cell.0, cell.1) else {
            return;
        };
        let ch = self.pen_arrow.arrow(side);
        if self.node[i] != 0 {
            return;
        }
        // 这一格已经是一条普通连接的起步格:箭头落上去,那条线的起点就看着像 <->.
        // 同方向的箭头除外(共用同一个箭头位是允许的)
        if self.port_out_kind.get(&cell) == Some(&PortKind::Line) && self.arrow[i] != Some(ch) {
            return;
        }
        self.place_arrow(i, cell, ch, fg);
    }

    /// 落一个箭头:同字形就共用(只加 owner),否则看能不能顶掉别人的
    fn place_arrow(&mut self, i: usize, cell: (i32, i32), ch: char, fg: (u8, u8, u8)) {
        if self.arrow[i] == Some(ch) {
            self.note_arrow(cell, fg);
            return;
        }
        if !self.arrow_free(cell, ch) {
            return;
        }
        self.put_arrow(i, cell, ch, fg);
    }

    /// 这一格能不能落箭头:空着 / 同字形(共用) / 抬起来或选中的边可以顶掉别人的
    fn arrow_free(&self, cell: (i32, i32), ch: char) -> bool {
        let Some(i) = self.idx(cell.0, cell.1) else {
            return true;
        };
        let Some(a) = self.arrow[i] else { return true };
        if a == ch {
            return true;
        }
        let owners = self.arrow_owners.get(&cell);
        let taken_by_picked = owners.is_some_and(|o| self.picked.is_some_and(|p| o.contains(&p)));
        if self.picked == Some(self.pen_edge) {
            return !taken_by_picked;
        }
        if !self.raised.contains(&self.pen_edge) {
            return false;
        }
        let other_raised = owners.is_some_and(|o| o.iter().any(|e| self.raised.contains(e)));
        !taken_by_picked && !other_raised
    }

    /// 箭头画在最上面(盖在别人的笔画上),颜色跟着自己的边走;
    /// 同方向共用的箭头只由第一条边决定字形/颜色
    fn put_arrow(&mut self, i: usize, cell: (i32, i32), ch: char, fg: (u8, u8, u8)) {
        self.arrow[i] = Some(ch);
        self.arrow_ink[i] = Some(fg);
        self.arrow_owners.insert(cell, vec![self.pen_edge]);
        self.note_arrow(cell, fg);
    }

    /// 有向边的箭头:画在 to 紧挨着的那一格(从 from 往 to 走的方向).
    /// to 带框时按最后一段的走向落在框外紧挨着的那一格(`to_box` 是框的左上角与宽高).
    /// 一格最多一个箭头;撞上节点字形、或者这一格已经归别的分组,就跳过
    fn head_arrow(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
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
                    self.pen_arrow.arrow(LEFT)
                } else {
                    self.pen_arrow.arrow(RIGHT)
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
                    self.pen_arrow.arrow(UP)
                } else {
                    self.pen_arrow.arrow(DOWN)
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
        // 别的边从这一格过也照样落箭头:箭头在最上层,方向按这条边走线的最后一格算
        let _ = group;
        self.place_arrow(i, cell, ch, fg);
    }

    /// 把节点框占的格子提前登记下来(画边时避开,别从框里穿过去).
    /// `at` 是框的左上角
    pub(crate) fn reserve_box(&mut self, at: (i32, i32), size: (usize, usize)) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        for y in at.1..at.1 + h {
            for x in at.0..at.0 + w {
                // 屏幕外的框也要占位:不然视图一挪,线就会从那个框身上穿过去
                self.station_cells.insert((x, y));
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
        let [hz, vt, tl, tr, bl, br] = self.glyphs.box_chars(style);
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
                    Some(BorderMark::Junction(m, c)) => (self.glyphs.junction(style, *m), *c),
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

    /// 区域的边框:只写字形与颜色,不占节点/站点(边可以从框边穿过).
    /// 画在框之前,后画的框/边自然盖住它
    pub(crate) fn plate_at(
        &mut self,
        at: (i32, i32),
        size: (usize, usize),
        style: BoxStyle,
        fg: (u8, u8, u8),
    ) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let [hz, vt, tl, tr, bl, br] = self.glyphs.box_chars(style);
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
                    continue;
                };
                let (x, y) = (left + xx, top + yy);
                let Some(i) = self.idx(x, y) else { continue };
                self.lit[i] = self.layer_lit;
                self.deco[i] = Some(Deco {
                    ch,
                    fg,
                    bg: None,
                    groups: 0,
                    node: -1,
                });
            }
        }
    }

    /// 区域标题:直接铺在边框上(不占节点),后画的框会盖掉重叠部分
    pub(crate) fn stamp_at(&mut self, x: i32, y: i32, text: &str, fg: (u8, u8, u8)) {
        let mut cx = x;
        for ch in text.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as i32;
            let Some(i) = self.idx(cx, y) else { break };
            self.lit[i] = self.layer_lit;
            self.deco[i] = Some(Deco {
                ch,
                fg,
                bg: None,
                groups: 0,
                node: -1,
            });
            cx += w.max(1);
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
        self.ink.entry((x, y)).or_default().0 |= group_mask;
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
mod tests;
