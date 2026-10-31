//! 布局:把文档里的框摆到整数单元格上(单位是字符格,x 向右、y 向下).
//!
//! 四套算法,对输入的要求不一样:
//!   geo      坐标已经是单元格,原样保留(只处理重叠)
//!   relax    从现有位置出发做弹簧放松,把框距按尺寸拉匀、走向贴到横竖 45 度
//!   force    力导(Fruchterman-Reingold),用现有位置当起点,没有就确定性地撒点
//!   layered  分层(Sugiyama-lite),适合有向的依赖图/流程图
//!
//! 每套算法内部算的都是框的中心点(f64);收尾统一换成左上角整数单元格:
//!   at = (round(cx) - (w - 1) / 2, round(cy) - (h - 1) / 2)
//! 然后用 `Document::place_box` 写回(它会把文字位置一起同步),最后 `rebuild`.
//!
//! 尺寸参与间距:格距按最宽的框算,框与框之间至少留一格.所有布局最后都要过
//! `settle`:按确定的顺序把每个框放到离目标最近的空位,矩形之间至少隔一格,
//! 所以四套布局都不会叠框,也不会有两个框贴在一起.
//!
//! 规模与复杂度(框数 n、边数 m、规模系数 k = sqrt(n / 262)):
//!   relax   每轮 O(n + m + repel 的网格邻域),轮数 = min(600 k, 迭代预算),见 Cfg::iters
//!   force   同上一套轮数;每轮 O(n + m + 网格邻域里的点对)
//!   layered DFS 破环 + 拓扑分层 + 几轮重心,都是 O(n + m),收尾同样走 polish
//!   polish  取整 O(n * 环数);每轮 separate / spread_all / octilinear 走候选格的
//!           (2r+1)^2 邻域,迭代里没人动就提前退出
//!   settle  每个框 O(自己的面积 + 冲突时的有限环搜索),环数有上限
//! 同心环数、搜索半径、迭代轮数与后处理轮数全都有上限(见 Cfg 与下面几个常量),
//! 真的找不到空位就放到已有包围盒下方的新一行,所以不会卡死.
//!
//! 确定性:哈希表只用"按 key 查",从不按哈希顺序遍历;凡是要排序的地方都用框索引兜底,
//! 没有位置时的起点用黄金角撒点而不是随机数.同一份文档跑几次结果完全一样.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::document::Document;
use crate::lang::Lang;

/// 相邻框的目标间距下限(格)
const STEP: f64 = 2.6;
/// 放松迭代轮数基准
const ITERS: usize = 600;
/// 迭代总量的封顶:轮数 x 框数不超过这个量级,框多了就少迭代几轮.
/// 一轮是一遍网格 + 邻域里的点对,总量封在这里,几千个框也就几秒
const ITER_BUDGET: usize = 240_000;
/// 力导的工作画布:方画布,边长 = max(2 格, FORCE_SPAN * sqrt(框数)).
/// 按框数开方是"每个框分到差不多格子"的意思;乘得比 STEP 大一点,
/// 给弹簧留出余地(画布太紧,框会被挤到边框上,收尾又得一个个搬开)
const FORCE_SPAN: f64 = STEP * 1.6;
/// 力导的期望间距上限(格):小图不能按画布摊到几十格远,那样中间全是空白
const FORCE_K_MAX: f64 = 4.0;
/// 力导的重力:往画布中心轻轻拽一把.
/// 只靠斥力和弹簧,框会被推到画布边框上挤成一圈(边框上到处是挨着的框,
/// 收尾就得把它们一个个搬开,整幅图会胀大好几倍);有了重力密度就摊得均匀
const GRAVITY: f64 = 0.06;
/// 后处理里"最多磨几轮"的轮数上限:每轮都要扫一遍边,框多了宁可少磨几轮
const ROUND_CAP: usize = 64;
/// 掰直(octilinear)的轮数上限:这一步最贵(每条边要在 8 个方向上试格子),
/// 框再多也不加轮数 —— 磨到后面基本在原地打转,不如把时间留给别的
const OCTI_CAP: usize = 24;
/// 再大的图也至少迭代这么多轮
const MIN_ITERS: usize = 40;
/// 拉直力:让一组上连着三节点尽量共线,画出来才有长长的直走廊
const K_STRAIGHT: f64 = 0.06;
/// 锚的力度:越小越不理会原有位置的疏密,越往框距均匀那边靠
const ANCHOR_W: f64 = 0.030;
/// 分层布局的层间距下限(格):>= 3 才保证上下两层不贴在一起
const LAYER_GAP: f64 = 3.0;
/// 分层布局同一层里相邻两框至少隔几格
const LANE_GAP: f64 = 3.0;
/// 重心扫描轮数
const BARY_ROUNDS: usize = 4;
/// 黄金角(弧度):没有位置时用它撒点,不用随机数
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

/// 规模参数:框一多,搜索半径、迭代轮数都要跟着放大 ——
/// 不然后处理在密集区找不到空位,框会叠在一起(262 个框的图是基准,k = 1)
struct Cfg {
    n: usize,
    k: f64,
}

impl Cfg {
    fn of(n: usize) -> Cfg {
        Cfg {
            n,
            k: (n as f64 / 262.0).sqrt().max(1.0),
        }
    }
    /// 同心环往外找空位的上限(基准 64,k 大了也只能到 96 环,再往外找不如让框挤一点)
    fn rings(&self) -> i32 {
        ((64.0 * self.k) as i32).min(96)
    }
    /// best_cell 的搜索半径(基准 3,也就是 7x7;封顶 5)
    fn radius(&self) -> i32 {
        ((3.0 * self.k).round() as i32).clamp(2, 5)
    }
    /// clean_cell 允许挪动的格数(基准 2;封顶 4)
    fn nudge(&self) -> i32 {
        ((2.0 * self.k).round() as i32).clamp(1, 4)
    }
    /// 后处理里"最多磨几轮"的轮数:基准 x k,再按图的大小收紧.
    /// 基准值是杭州那种规模(262 个框)调出来的,小图完全按原样走;
    /// 大图在密集区磨几十轮也理不清,不如把时间留给别的(最后的 settle 兜底)
    fn rounds(&self, base: usize) -> usize {
        let cap = if self.n > 2000 {
            6
        } else if self.n > 600 {
            16
        } else {
            ROUND_CAP
        };
        ((base as f64 * self.k).round() as usize).min(cap)
    }
    /// 主迭代轮数:基准 x k,再按 ITER_BUDGET 封顶,但不少于 MIN_ITERS
    fn iters(&self, n: usize, base: usize) -> usize {
        let want = (base as f64 * self.k).round() as usize;
        want.min((ITER_BUDGET / n.max(1)).max(MIN_ITERS))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Geo,
    Relax,
    Force,
    Layered,
}

impl Layout {
    /// 四套算法,按状态栏/R 键循环的顺序
    pub fn all() -> [Layout; 4] {
        [Layout::Geo, Layout::Relax, Layout::Force, Layout::Layered]
    }

    /// 命令行 / ":" 里认的名字(大小写与前后空格都无所谓)
    pub fn parse(s: &str) -> Option<Layout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "geo" | "real" | "geographic" => Some(Layout::Geo),
            "relax" | "spring" | "relaxed" => Some(Layout::Relax),
            "force" | "fr" => Some(Layout::Force),
            "layered" | "dag" | "sugiyama" => Some(Layout::Layered),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Layout::Geo => "geo",
            Layout::Relax => "relax",
            Layout::Force => "force",
            Layout::Layered => "layered",
        }
    }

    /// 状态栏里的中文/英文名
    pub fn label(&self, lang: Lang) -> &'static str {
        match self {
            Layout::Geo => lang.pick("真实坐标", "geographic"),
            Layout::Relax => lang.pick("松弛", "relaxed"),
            Layout::Force => lang.pick("力导", "force"),
            Layout::Layered => lang.pick("分层", "layered"),
        }
    }

    /// r 键循环到下一套
    pub fn next(&self) -> Layout {
        match self {
            Layout::Geo => Layout::Relax,
            Layout::Relax => Layout::Force,
            Layout::Force => Layout::Layered,
            Layout::Layered => Layout::Geo,
        }
    }
}

/// 跑一套布局,把结果写回文档的框位置(文字位置由 place_box 同步).
/// 结束时所有坐标都是整数,任意两个框的矩形至少隔一格.
/// 布局方向:auto 按窗口挑,其余是固定角度
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Orientation {
    #[default]
    Auto,
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Orientation {
    pub fn parse(s: &str) -> Option<Orientation> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Orientation::Auto),
            "0" => Some(Orientation::Deg0),
            "90" => Some(Orientation::Deg90),
            "180" => Some(Orientation::Deg180),
            "270" => Some(Orientation::Deg270),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Orientation::Auto => "auto",
            Orientation::Deg0 => "0",
            Orientation::Deg90 => "90",
            Orientation::Deg180 => "180",
            Orientation::Deg270 => "270",
        }
    }

    /// 固定角度换成 90 度的份数;auto 没有固定值
    pub fn quarters(&self) -> Option<i32> {
        match self {
            Orientation::Auto => None,
            Orientation::Deg0 => Some(0),
            Orientation::Deg90 => Some(1),
            Orientation::Deg180 => Some(2),
            Orientation::Deg270 => Some(3),
        }
    }

    pub fn degrees(&self) -> i32 {
        self.quarters().map(|k| k * 90).unwrap_or(0)
    }
}

pub fn apply(doc: &mut Document, l: Layout) -> Result<(), String> {
    let n = doc.boxes.len();
    if n == 0 {
        return Ok(());
    }
    let sizes: Vec<(i32, i32)> = doc
        .boxes
        .iter()
        .map(|b| (b.size.0 as i32, b.size.1 as i32))
        .collect();
    if let Some(i) = sizes.iter().position(|s| s.0 <= 0 || s.1 <= 0) {
        return Err(format!("box {:?}: empty size", doc.boxes[i].id));
    }
    let cfg = Cfg::of(n);
    // geo 的位置本来就是单元格:原样用 at,不要从中心点反推(偶数宽会差一格)
    let desired: Vec<(i32, i32)> = if l == Layout::Geo {
        doc.boxes.iter().map(|b| b.at).collect()
    } else {
        let centers = match l {
            Layout::Relax => relax(doc, &sizes),
            Layout::Force => force(doc, &sizes),
            Layout::Layered => layered(doc, &sizes),
            // geo 在上面已经分出去了,这里给个空表只是为了 match 完整
            Layout::Geo => Vec::new(),
        };
        to_top_left(&centers, &sizes)
    };
    // 注释框"钉住":布局不挪它,但别的框会绕开它
    let pinned: Vec<bool> = (0..n).map(|i| doc.is_note(i)).collect();
    let orig: Vec<(i32, i32)> = doc.boxes.iter().map(|b| b.at).collect();
    let placed = settle(&desired, &sizes, &cfg, &pinned, &orig);
    for (i, at) in placed.into_iter().enumerate() {
        doc.place_box(i, at);
    }
    doc.rebuild();
    Ok(())
}

/// 没指定布局时自动挑一套:边有方向就按有没有环选(分层 / 力导),
/// 没有方向的边(或没有边)一律分层.不再看坐标.
pub fn pick(doc: &Document) -> Layout {
    let arcs = directed_arcs(doc);
    if !arcs.is_empty() && has_cycle(doc.boxes.len(), &arcs) {
        Layout::Force
    } else {
        Layout::Layered
    }
}

/// 有向边(按 dir 判断),端点是框下标
fn directed_arcs(doc: &Document) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for e in &doc.edges {
        if !e.dir.is_arrow() {
            continue;
        }
        let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
            continue;
        };
        if a == b {
            // 自环没有"方向",不参与分层
            continue;
        }
        out.push((a, b));
    }
    out
}

/// 全部边,不分分组:后处理要保证的是"任意一条边两端都隔开"
fn edge_pairs(doc: &Document) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(doc.edges.len());
    for (i, _) in doc.edges.iter().enumerate() {
        if let Some((a, b)) = doc.edge_ends(i) {
            if a != b {
                // 自环不需要和谁隔开
                out.push((a, b));
            }
        }
    }
    out
}

/// 最宽的框、最高的框(至少 1x1)
fn max_size(sizes: &[(i32, i32)]) -> (i32, i32) {
    sizes
        .iter()
        .fold((1, 1), |(w, h), &(a, b)| (w.max(a), h.max(b)))
}

/// 层内 / 层间的间距:有框时按最大框留一格空,没框时就是老的分层间距.
/// (lane, layer)
fn pitch(sizes: &[(i32, i32)]) -> (f64, f64) {
    let (w, h) = max_size(sizes);
    (
        ((w + 1) as f64).max(LANE_GAP),
        ((h + 1) as f64).max(LAYER_GAP),
    )
}

/// 各向同性布局(放松 / 力导)的格距:按最大的框留一格空
fn unit(sizes: &[(i32, i32)]) -> f64 {
    let (w, h) = max_size(sizes);
    ((w + 1).max(h + 1) as f64).max(STEP)
}

/// 中心点 -> 左上角整数单元格
fn to_top_left(centers: &[(f64, f64)], sizes: &[(i32, i32)]) -> Vec<(i32, i32)> {
    centers
        .iter()
        .zip(sizes)
        .map(|(&(cx, cy), &(w, h))| (round_i32(cx) - (w - 1) / 2, round_i32(cy) - (h - 1) / 2))
        .collect()
}

/// 取整并夹到安全范围,避免异常值把后面的加法溢出
fn round_i32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    v.round()
        .clamp(i32::MIN as f64 / 4.0, i32::MAX as f64 / 4.0) as i32
}

/// 位置是不是退化成一团(所有框都在同一个点附近):这种输入等于没有位置
fn flat(p: &[(f64, f64)]) -> bool {
    if p.len() < 2 {
        return true;
    }
    let (x0, y0, x1, y1) = bbox(p);
    (x1 - x0) < 0.5 && (y1 - y0) < 0.5
}

/// 把一段中心点等比铺进 w x h 的方框:只缩放形状,不翻转 y(单元格 y 向下)
fn fit_centers(p: &[(f64, f64)], w: f64, h: f64) -> Vec<(f64, f64)> {
    let (x0, y0, x1, y1) = bbox(p);
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    let s = ((w - 6.0).max(1.0) / gw).min((h - 4.0).max(1.0) / gh);
    let ox = (w - gw * s) / 2.0;
    let oy = (h - gh * s) / 2.0;
    p.iter()
        .map(|q| ((q.0 - x0) * s + ox, (q.1 - y0) * s + oy))
        .collect()
}

fn bbox(p: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in p {
        x0 = x0.min(q.0);
        y0 = y0.min(q.1);
        x1 = x1.max(q.0);
        y1 = y1.max(q.1);
    }
    (x0, y0, x1, y1)
}

/// 边的目标方向(格坐标下的单位向量):水平、竖直或 45 度
fn snap_dir(dx: f64, dy: f64) -> (f64, f64) {
    let (ax, ay) = (dx.abs(), dy.abs());
    if ay < ax * 0.4 {
        (dx.signum(), 0.0)
    } else if ax < ay * 0.4 {
        (0.0, dy.signum())
    } else {
        // 45 度:格坐标里就是这个斜率,画出来正好一个直角拐弯
        let k = std::f64::consts::FRAC_1_SQRT_2;
        (dx.signum() * k, dy.signum() * k)
    }
}

// ---- 收尾:保证不叠框 ----

/// 候选矩形(左上角 at、尺寸 w x h)能不能放:自己的格子连同周围一圈
/// 都不能被占.要求周围一圈也空,就是"两个框之间至少隔一格".
fn fits(at: (i32, i32), (w, h): (i32, i32), taken: &HashSet<(i32, i32)>) -> bool {
    for dy in -1..=h {
        for dx in -1..=w {
            if taken.contains(&(at.0 + dx, at.1 + dy)) {
                return false;
            }
        }
    }
    true
}

/// 把矩形自己占的格子记下来(周围那圈不记,由 fits 的扩张来保证)
fn mark(at: (i32, i32), (w, h): (i32, i32), taken: &mut HashSet<(i32, i32)>, bottom: &mut i32) {
    for dy in 0..h {
        for dx in 0..w {
            taken.insert((at.0 + dx, at.1 + dy));
        }
    }
    *bottom = (*bottom).max(at.1 + h - 1);
}

/// 按目标位置铺开,保证任意两个框的矩形至少隔一格.
///
/// 顺序按(目标 y, 目标 x, 下标)排,先放的优先占原位,所以布局原来的形状
/// 不会被后来的框挤乱.目标位被占时按同心环往外找最近的空位(环数有上限);
/// 真的找不到就放到已有内容下方的新一行 —— 那里一定是空的,所以一定能放下.
fn settle(
    desired: &[(i32, i32)],
    sizes: &[(i32, i32)],
    cfg: &Cfg,
    pinned: &[bool],
    orig: &[(i32, i32)],
) -> Vec<(i32, i32)> {
    let n = desired.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        desired[a]
            .1
            .cmp(&desired[b].1)
            .then(desired[a].0.cmp(&desired[b].0))
            .then(a.cmp(&b))
    });
    let mut taken: HashSet<(i32, i32)> = HashSet::with_capacity(n.saturating_mul(4));
    let mut out = vec![(0, 0); n];
    let mut bottom = i32::MIN / 4;
    // 钉住的先占位:它们不动,别的框把它们的矩形当障碍
    for i in 0..n {
        if pinned.get(i).copied().unwrap_or(false) {
            mark(orig[i], sizes[i], &mut taken, &mut bottom);
        }
    }
    for i in order {
        let size = sizes[i];
        if pinned.get(i).copied().unwrap_or(false) {
            out[i] = orig[i];
            continue;
        }
        let (x0, y0) = desired[i];
        let mut spot = if fits((x0, y0), size, &taken) {
            Some((x0, y0))
        } else {
            None
        };
        if spot.is_none() {
            let cap = cfg.rings();
            // 只走环上的格子(不是整个方块),不然密集区里一圈要空扫 O(r^2)
            'rings: for r in 1..=cap {
                for dy in -r..=r {
                    if dy == -r || dy == r {
                        for dx in -r..=r {
                            let c = (x0 + dx, y0 + dy);
                            if fits(c, size, &taken) {
                                spot = Some(c);
                                break 'rings;
                            }
                        }
                    } else {
                        for dx in [-r, r] {
                            let c = (x0 + dx, y0 + dy);
                            if fits(c, size, &taken) {
                                spot = Some(c);
                                break 'rings;
                            }
                        }
                    }
                }
            }
        }
        let at = spot.unwrap_or((x0, bottom + 2));
        mark(at, size, &mut taken, &mut bottom);
        out[i] = at;
    }
    out
}

// ---- relax ----

/// 斥力:两框贴太近就推开(用网格找候选,免得 O(n^2)).
/// gap 是目标格距,已经按框的尺寸放大过
fn repel(p: &[(f64, f64)], f: &mut [(f64, f64)], k: f64, gap: f64) {
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / gap).floor() as i32, (q.1 / gap).floor() as i32);
        grid.entry(cell).or_default().push(i);
    }
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / gap).floor() as i32, (q.1 / gap).floor() as i32);
        for di in -1..=1 {
            for dj in -1..=1 {
                for &j in grid
                    .get(&(cell.0 + di, cell.1 + dj))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if j <= i {
                        continue;
                    }
                    let (dx, dy) = (p[j].0 - q.0, p[j].1 - q.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    if len >= gap {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 {
                        (0.0, 1.0)
                    } else {
                        (dx / len, dy / len)
                    };
                    let push = (gap - len) * k;
                    f[i].0 -= ux * push;
                    f[i].1 -= uy * push;
                    f[j].0 += ux * push;
                    f[j].1 += uy * push;
                }
            }
        }
    }
}

/// 取整去重叠:一格一框,占了就按同心环往外找最近的空格.
/// 这里的坐标是"格",调用方已经把真实坐标除过 cell(见 polish)
fn snap_to_grid(p: &[(f64, f64)], cfg: &Cfg) -> Vec<(f64, f64)> {
    let mut taken: HashSet<(i32, i32)> = HashSet::with_capacity(p.len());
    let mut out = vec![(0.0, 0.0); p.len()];
    let mut order: Vec<usize> = (0..p.len()).collect();
    order.sort_by(|&a, &b| {
        p[a].1
            .total_cmp(&p[b].1)
            .then(p[a].0.total_cmp(&p[b].0))
            .then(a.cmp(&b))
    });
    for i in order {
        let (x0, y0) = (round_i32(p[i].0), round_i32(p[i].1));
        let mut spot = (x0, y0);
        // 只走环上的格子(不是整个方块),不然密集区里一圈要空扫 O(r^2)
        'grow: for r in 0..cfg.rings() {
            for dy in -r..=r {
                if dy == -r || dy == r {
                    for dx in -r..=r {
                        if !taken.contains(&(x0 + dx, y0 + dy)) {
                            spot = (x0 + dx, y0 + dy);
                            break 'grow;
                        }
                    }
                } else {
                    for dx in [-r, r] {
                        if !taken.contains(&(x0 + dx, y0 + dy)) {
                            spot = (x0 + dx, y0 + dy);
                            break 'grow;
                        }
                    }
                }
            }
        }
        taken.insert(spot);
        out[i] = (spot.0 as f64, spot.1 as f64);
    }
    out
}

/// 八个方向:横竖与两条对角线
const DIRS8: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// 候选格有没有跑出这个框的活动范围(收尾只许局部微调,见 polish 上的说明)
fn within(leash: (&[(i32, i32)], i32), who: usize, c: (i32, i32)) -> bool {
    let (origin, drift) = leash;
    let o = origin[who];
    (c.0 - o.0).abs().max((c.1 - o.1).abs()) <= drift
}

/// 把边的走向整成横、竖或 45 度:让端点挪到同一个方向上最近的空格.
/// 这样画出来才是干净的一笔,而不是一格一格爬的阶梯.
fn octilinear(
    p: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    cfg: &Cfg,
    leash: (&[(i32, i32)], i32),
) {
    for _ in 0..cfg.rounds(24).min(OCTI_CAP) {
        let mut taken: HashSet<(i32, i32)> = p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (ax, ay) = (p[a].0 as i32, p[a].1 as i32);
            let (bx, by) = (p[b].0 as i32, p[b].1 as i32);
            let (dx, dy) = (bx - ax, by - ay);
            if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
                continue;
            }
            // 先试挪 b;b 周围满了就把 a 挪到以 b 为中心的同方向格子上
            let spot = clean_cell((ax, ay), (bx, by), &taken, cfg.nudge(), leash, b)
                .map(|c| (b, c))
                .or_else(|| {
                    clean_cell((bx, by), (ax, ay), &taken, cfg.nudge(), leash, a).map(|c| (a, c))
                });
            let Some((who, c)) = spot else { continue };
            let old = (p[who].0 as i32, p[who].1 as i32);
            taken.remove(&old);
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// 以 base 为圆心、在 8 个方向上找一个空格,使它到 want 最近(用来把边掰直).
/// 超出 who 活动范围的格子先筛掉(见 best_cell 上的说明)
fn clean_cell(
    base: (i32, i32),
    want: (i32, i32),
    taken: &HashSet<(i32, i32)>,
    nudge: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let l = (want.0 - base.0).abs().max((want.1 - base.1).abs());
    let mut best: Option<(i64, (i32, i32))> = None;
    for len in [l - 1, l, l + 1, l + 2] {
        if len < 1 {
            continue;
        }
        for (ux, uy) in DIRS8 {
            let c = (base.0 + ux * len, base.1 + uy * len);
            // 只许就近挪几格:不然框会被越推越远,整幅图炸开
            if (c.0 - want.0).abs().max((c.1 - want.1).abs()) > nudge {
                continue;
            }
            if !within(leash, who, c) {
                continue;
            }
            if !room_at(c, taken, want) {
                continue;
            }
            let d = ((c.0 - want.0) as i64).pow(2) + ((c.1 - want.1) as i64).pow(2);
            if best.map(|(s, _)| d < s).unwrap_or(true) {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// 任意两框都不能挨在一起(不只是边上的相邻框):
/// 挨着画出来就是两个框糊在一块,密集的地方最容易这样
fn spread_all(p: &mut [(f64, f64)], cfg: &Cfg, leash: (&[(i32, i32)], i32)) {
    for _ in 0..cfg.rounds(40) {
        // 一格可能摞了不止一个框(前面的收尾偶尔会撞上),所以这里用多重映射
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, q) in p.iter().enumerate() {
            grid.entry((q.0 as i32, q.1 as i32)).or_default().push(i);
        }
        let mut taken: HashSet<(i32, i32)> = grid.keys().copied().collect();
        let mut moved = false;
        for i in 0..p.len() {
            let (x, y) = (p[i].0 as i32, p[i].1 as i32);
            let mut close: Option<usize> = None;
            // 同一格里的另一个框也算"贴在一起"
            if let Some(cell) = grid.get(&(x, y)) {
                if let Some(&j) = cell.iter().find(|&&j| j > i) {
                    close = Some(j);
                }
            }
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    if let Some(cell) = grid.get(&(x + dx, y + dy)) {
                        if let Some(&j) = cell.iter().find(|&&j| j > i) {
                            close = Some(j);
                        }
                    }
                }
            }
            let Some(j) = close else { continue };
            // 谁挪都行:先试 i,再试 j;每种先找"周围八格也空着"的位子,
            // 实在没有才退一步只要求这一格空
            let r = cfg.radius();
            let spot = best_cell(p[j], p[i], &taken, true, r, leash, i)
                .map(|c| (i, c))
                .or_else(|| best_cell(p[i], p[j], &taken, true, r, leash, j).map(|c| (j, c)))
                .or_else(|| best_cell(p[j], p[i], &taken, false, r, leash, i).map(|c| (i, c)))
                .or_else(|| best_cell(p[i], p[j], &taken, false, r, leash, j).map(|c| (j, c)));
            if let Some((who, c)) = spot {
                let old = (p[who].0 as i32, p[who].1 as i32);
                if let Some(cell) = grid.get_mut(&old) {
                    cell.retain(|&k| k != who);
                }
                grid.entry(c).or_default().push(who);
                // taken 也要跟着改:不然它还以为旧格占着、新格空着,
                // 后面就会把别的框搬进刚占上的这一格(格子少的时候必撞)
                taken.remove(&old);
                taken.insert(c);
                p[who] = (c.0 as f64, c.1 as f64);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// 边上的相邻框至少要隔一格:不然中间那截线会被两个框吃掉,线看起来是断的
fn separate(
    p: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    cfg: &Cfg,
    leash: (&[(i32, i32)], i32),
) {
    for _ in 0..cfg.rounds(60) {
        let mut taken: HashSet<(i32, i32)> = p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            if dx.abs().max(dy.abs()) >= 2.0 {
                continue;
            }
            // 先挪 b:在附近找一个离 a 更远的空格,挪不动就反过来挪 a.
            // 先找"周围也空着"的位子,实在没有才退而求其次只要求这一格空
            let r = cfg.radius();
            let spot = best_cell(p[a], p[b], &taken, true, r, leash, b)
                .map(|c| (b, c))
                .or_else(|| best_cell(p[b], p[a], &taken, true, r, leash, a).map(|c| (a, c)))
                .or_else(|| best_cell(p[a], p[b], &taken, false, r, leash, b).map(|c| (b, c)))
                .or_else(|| best_cell(p[b], p[a], &taken, false, r, leash, a).map(|c| (a, c)));
            let Some((who, c)) = spot else { continue };
            taken.remove(&(p[who].0 as i32, p[who].1 as i32));
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// 候选格能不能站:自己空着,周围 8 格也不能有别的框.
/// 只有"要挪走的那一个框"自己现在占的那一格可以不算 —— 那个位子马上就空出来了.
/// (别的框占的格子一律算占着,不然会把两个框摞到同一格上)
fn room_at(c: (i32, i32), taken: &HashSet<(i32, i32)>, own: (i32, i32)) -> bool {
    let blocked = |q: (i32, i32)| taken.contains(&q) && q != own;
    if blocked(c) {
        return false;
    }
    !DIRS8.iter().any(|(ux, uy)| blocked((c.0 + ux, c.1 + uy)))
}

/// 在 keep 周围 (2r+1)^2 里挑一个空格:离 from 越远越好,同时别离 keep 太远.
/// 超出这个框活动范围的格子先筛掉:那种格子再多也用不上,
/// 而"能不能站"的检查要点 9 次哈希,先比一下格坐标便宜得多
fn best_cell(
    from: (f64, f64),
    keep: (f64, f64),
    taken: &HashSet<(i32, i32)>,
    strict: bool,
    radius: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let (kx, ky) = (keep.0 as i32, keep.1 as i32);
    let own = (kx, ky);
    let mut best: Option<(f64, (i32, i32))> = None;
    // 基准是 7x7:挤成一坨的地方(枢纽扎堆)近处常常一个空位都没有,框多了还要更大
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let c = (kx + dx, ky + dy);
            if !within(leash, who, c) {
                continue;
            }
            if strict && !room_at(c, taken, own) {
                continue;
            }
            if !strict && taken.contains(&c) {
                continue;
            }
            let d_from = ((c.0 as f64 - from.0).powi(2) + (c.1 as f64 - from.1).powi(2)).sqrt();
            let score = d_from * 3.0 - (dx * dx + dy * dy) as f64;
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// 后处理流水线:先把中心点按 cell 换算成"格",一格一框地对到整数格上,
/// 再让"分开(separate) / 拉开(spread_all) / 拉直(octilinear)"三件事来回磨几轮 ——
/// 它们互相会打断对方,挑位子时都尽量选"周围也空着"的格子,磨到互相让步;
/// 最后兜一次底,把边上相邻的框再拉开一遍.力导与分层也走这一套.
///
/// cell 是格距,按最大的框算:不同格子的两个框,左/上边至少差 cell,
/// 也就是中间至少空一格.没有框(全是点)时 cell = 1,行为和以前一致.
///
/// 每个框只许在"取整后的原位"周围 drift(3 倍搜索半径)格里活动.
/// 没有这个限制,密集处会连锁反应 —— 这一轮把框挪开、下一轮它又跟别的邻居贴上了,
/// 几十轮下来框一路漂出去,整幅图胀成画布的几倍大.
/// 限住之后,收尾最多把包围盒撑大 2 * drift 格;代价是极密的地方可能留一对挨着的框
/// (最后的 settle 会补上这一点).
fn polish(p: Vec<(f64, f64)>, edges: &[(usize, usize)], cfg: &Cfg, cell: f64) -> Vec<(f64, f64)> {
    let cell = if cell.is_finite() && cell > 1.0 {
        cell
    } else {
        1.0
    };
    let scaled: Vec<(f64, f64)> = p.iter().map(|q| (q.0 / cell, q.1 / cell)).collect();
    let mut out = snap_to_grid(&scaled, cfg);
    let origin: Vec<(i32, i32)> = out.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
    let leash = (origin.as_slice(), cfg.radius() * 3);
    // 大图少磨几轮:密集区里来回挪动几乎不会收敛,而最后的 settle 一定会分开
    let passes = if out.len() > 2000 {
        1
    } else if out.len() > 600 {
        3
    } else {
        6
    };
    for _ in 0..passes {
        separate(&mut out, edges, cfg, leash);
        spread_all(&mut out, cfg, leash);
        octilinear(&mut out, edges, cfg, leash);
    }
    separate(&mut out, edges, cfg, leash);
    out.iter().map(|q| (q.0 * cell, q.1 * cell)).collect()
}

/// 每组里"夹在中间"的框:(前一框, 中间框, 后一框).
/// 按分组算度,不是按整图度数 —— 枢纽也是某组路径上的中间框,同样要拉直
fn bends(doc: &Document) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for gi in 0..doc.groups.len() {
        let mut nb: HashMap<usize, Vec<usize>> = HashMap::new();
        for e in doc
            .edges
            .iter()
            .filter(|e| doc.group_idx(&e.group) == Some(gi))
        {
            let (Some(a), Some(b)) = (doc.box_idx(&e.from), doc.box_idx(&e.to)) else {
                continue;
            };
            if a == b {
                // 自环不构成"夹在中间",跳过
                continue;
            }
            nb.entry(a).or_default().push(b);
            nb.entry(b).or_default().push(a);
        }
        // 按框索引升序走一遍,结果与哈希顺序无关
        let mut nodes: Vec<usize> = nb.keys().copied().collect();
        nodes.sort_unstable();
        for i in nodes {
            let v = &nb[&i];
            // 这一组里正好两个邻居才算中间框:端点、分叉口的方向本来就不定
            if v.len() == 2 {
                out.push((v[0], i, v[1]));
            }
        }
    }
    out
}

/// 松弛:从现有位置出发做弹簧放松,再走 polish.
///
/// 长度弹簧让每条边上相邻的框保持按尺寸算出的格距,方向吸附把边的走向掰到
/// 横竖 45 度,三框拉直力抹平一站一站的小折角,另外有一个很弱的锚把整幅图
/// 拽在原位附近 —— 结果是相对方位跟原来一致,但框距按尺寸拉匀了.
fn relax(doc: &Document, sizes: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let gap = unit(sizes);
    let mut p: Vec<(f64, f64)> = (0..n).map(|i| doc.center(i)).collect();
    if flat(&p) {
        // 所有框都在同一处:没有位置可用,按黄金角撒一圈
        let span = (n as f64).sqrt() * gap;
        p = spiral(n, span.max(gap * 2.0), span.max(gap * 2.0));
    }
    let edges = edge_pairs(doc);
    let bends = bends(doc);
    // 锚点跟着当前布局一起缩放:只保留"相对方位",不让输入里稀疏的地方把整幅图撑大
    let anchor_raw = p.clone();
    let raw = bbox(&anchor_raw);
    let raw_c = ((raw.0 + raw.2) / 2.0, (raw.1 + raw.3) / 2.0);
    let raw_size = ((raw.2 - raw.0).max(1.0), (raw.3 - raw.1).max(1.0));

    let mut anchor = anchor_raw.clone();
    let iters = cfg.iters(n, ITERS);
    for it in 0..iters {
        if it % 20 == 0 {
            let b = bbox(&p);
            let c = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
            let s = 0.5 * ((b.2 - b.0) / raw_size.0 + (b.3 - b.1) / raw_size.1);
            for i in 0..n {
                anchor[i] = (
                    c.0 + (anchor_raw[i].0 - raw_c.0) * s,
                    c.1 + (anchor_raw[i].1 - raw_c.1) * s,
                );
            }
        }
        let mut f = vec![(0.0f64, 0.0f64); n];
        for &(a, b) in &edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let (ux, uy) = (dx / len, dy / len);
            // 长度弹簧:相邻框保持在 gap 附近
            let fl = (len - gap) * 0.30;
            f[a].0 += ux * fl;
            f[a].1 += uy * fl;
            f[b].0 -= ux * fl;
            f[b].1 -= uy * fl;
            // 方向吸附:把边往横 / 竖 / 45 度上掰
            let (tx, ty) = snap_dir(dx, dy);
            let (px, py) = (-ty, tx);
            let c = (dx * px + dy * py) * 0.30;
            f[a].0 += px * c;
            f[a].1 += py * c;
            f[b].0 -= px * c;
            f[b].1 -= py * c;
        }
        for &(a, b, c) in &bends {
            // 中间框往两边连线的中点靠,一站一站的小折角就会被抹平
            f[b].0 += ((p[a].0 + p[c].0) / 2.0 - p[b].0) * K_STRAIGHT;
            f[b].1 += ((p[a].1 + p[c].1) / 2.0 - p[b].1) * K_STRAIGHT;
        }
        repel(&p, &mut f, 0.7, gap);
        for i in 0..n {
            // 锚:整幅图别跑偏
            f[i].0 += (anchor[i].0 - p[i].0) * ANCHOR_W;
            f[i].1 += (anchor[i].1 - p[i].1) * ANCHOR_W;
            p[i].0 += f[i].0.clamp(-1.2, 1.2);
            p[i].1 += f[i].1.clamp(-1.2, 1.2);
        }
    }
    polish(p, &edges, &cfg, gap)
}

/// 没有位置时的起点:按黄金角(太阳花)撒点 —— 面积均匀、不用随机数,
/// 同一张图每次跑出来都一样;撒成画布形状的椭圆,免得方图挤在正中间
fn spiral(n: usize, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = ((w - 6.0).max(1.0) / 2.0, (h - 4.0).max(1.0) / 2.0);
    (0..n)
        .map(|i| {
            let r = ((i as f64 + 0.5) / n as f64).sqrt();
            let a = i as f64 * GOLDEN_ANGLE;
            (cx + rx * r * a.cos(), cy + ry * r * a.sin())
        })
        .collect()
}

/// 力导:没有位置也能画.
///
/// Fruchterman-Reingold —— 斥力让框散开、边上的弹簧把相邻框拉到理想间距 k,
/// 每个框再被一个很弱的重力往画布中心拽一把(只靠前两个力,整幅图会被推到
/// 画布边框上挤成一圈,收尾阶段就得把那些框一个个搬开,图会胀大好几倍),
/// 每轮位移按线性退火的温度封顶(先生猛后细微,免得来回弹),并夹在画布框里不许飞出去.
/// 有位置就从位置出发(等比铺到画布),没有位置就按黄金角撒一圈点.
/// 斥力只算一格网格邻域内的框(格边长就是斥力截断半径),所以一轮是 O(n + m).
///
/// 工作画布按图自己定(见 FORCE_SPAN):按框数开方,给每个框分到差不多的格子.
/// 框大时画布至少是格距的两倍,免得一开始就挤在一起
fn force(doc: &Document, sizes: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let gap = unit(sizes);
    let span = (FORCE_SPAN * (n as f64).sqrt()).max(gap * 2.0);
    let (w, h) = (span, span);
    let cur: Vec<(f64, f64)> = (0..n).map(|i| doc.center(i)).collect();
    let mut p = if flat(&cur) {
        spiral(n, w, h)
    } else {
        fit_centers(&cur, w, h)
    };
    let edges = edge_pairs(doc);
    // 期望间距:紧凑画布均摊下来每个框分到的边长,至少要有框的格距
    let k = (w * h / n as f64)
        .sqrt()
        .clamp(gap, (gap * 2.0).max(FORCE_K_MAX));
    // 超过这个距离就不算斥力了,同时也是网格的格边长(3x3 邻域正好盖住它)
    let cutoff = k * 2.5;
    let iters = cfg.iters(n, ITERS);
    let t0 = (w / 10.0).min(k * 2.0);
    let center = (w / 2.0, h / 2.0);
    let mut disp = vec![(0.0f64, 0.0f64); n];
    for it in 0..iters {
        // 线性退火:最后一轮几乎不动,只做微调
        let t = t0 * (1.0 - it as f64 / iters as f64);
        fr_forces(&p, &mut disp, &edges, k, cutoff, center, GRAVITY);
        for i in 0..n {
            let (dx, dy) = disp[i];
            let l = (dx * dx + dy * dy).sqrt();
            if l <= 1e-9 {
                continue;
            }
            let s = l.min(t) / l;
            p[i].0 = (p[i].0 + dx * s).clamp(1.0, w - 1.0);
            p[i].1 = (p[i].1 + dy * s).clamp(1.0, h - 1.0);
        }
    }
    // 力导的疏密是相对的,绝对尺度没有意义:把结果等比铺满工作画布.
    // 力导的平衡点是"边上相邻框差不多 k 格",但整幅图占多大是自由的;
    // 不铺满的话框会缩在中间或贴上边框,均摊下来每个框的格子数偏少,
    // 收尾在密集处就找不到空位.缩放不改变形状,只让均摊格子数和画布对齐
    let b = bbox(&p);
    // 退化成一维(一条横线/竖线)或一个点时不做等比缩放:除零换来的巨大倍数
    // 只会把坐标推到 i32 边缘,形状没有任何改善
    let s = if b.2 - b.0 < 1e-6 || b.3 - b.1 < 1e-6 {
        1.0
    } else {
        ((w - 6.0) / (b.2 - b.0)).min((h - 4.0) / (b.3 - b.1))
    };
    let (bcx, bcy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    for q in p.iter_mut() {
        *q = (w / 2.0 + (q.0 - bcx) * s, h / 2.0 + (q.1 - bcy) * s);
    }
    polish(p, &edges, &cfg, gap)
}

/// 点落在哪个网格格子里(格边长 cell)
fn cell_of(p: (f64, f64), cell: f64) -> (i32, i32) {
    ((p.0 / cell).floor() as i32, (p.1 / cell).floor() as i32)
}

/// 一轮力:斥力(只算 cutoff 内的,网格找候选)+ 边上的弹簧力 + 往画布中心的重力,
/// 都累加到 disp.每一对只算一次(i < j 时算,两边同时受力),
/// 所以一轮是 O(n + m + 邻域里的对数)
fn fr_forces(
    p: &[(f64, f64)],
    disp: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    k: f64,
    cutoff: f64,
    center: (f64, f64),
    gravity: f64,
) {
    for d in disp.iter_mut() {
        *d = (0.0, 0.0);
    }
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        grid.entry(cell_of(*q, cutoff)).or_default().push(i);
    }
    for i in 0..p.len() {
        let (cx, cy) = cell_of(p[i], cutoff);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &j in bucket {
                    if j <= i {
                        continue;
                    }
                    let (ex, ey) = (p[j].0 - p[i].0, p[j].1 - p[i].1);
                    let len = (ex * ex + ey * ey).sqrt();
                    if len >= cutoff {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 {
                        (0.0, 1.0)
                    } else {
                        (ex / len, ey / len)
                    };
                    // 斥力 k^2 / d:越近推得越狠
                    let f = k * k / len.max(0.01);
                    disp[i].0 -= ux * f;
                    disp[i].1 -= uy * f;
                    disp[j].0 += ux * f;
                    disp[j].1 += uy * f;
                }
            }
        }
    }
    for &(a, b) in edges {
        let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len, dy / len);
        // 弹簧力 d^2 / k:离得越远拉得越狠
        let f = len * len / k;
        disp[a].0 += ux * f;
        disp[a].1 += uy * f;
        disp[b].0 -= ux * f;
        disp[b].1 -= uy * f;
    }
    for i in 0..p.len() {
        disp[i].0 += (center.0 - p[i].0) * gravity;
        disp[i].1 += (center.1 - p[i].1) * gravity;
    }
}

// ---- layered ----

/// 分层:先把边定向,再破环、定层号
fn ranks(doc: &Document) -> (Vec<usize>, Vec<(usize, usize)>) {
    let n = doc.boxes.len();
    let dirs = directed_arcs(doc);
    if dirs.is_empty() {
        // 一条有向边都没有:按无向处理,连通块的 BFS 深度就是层号
        let r = ranks_bfs(doc);
        // 顺手把每条边都朝下摆(层号小的当上端),后面扫重心要用
        let arcs = edge_pairs(doc)
            .into_iter()
            .map(|(a, b)| if r[a] <= r[b] { (a, b) } else { (b, a) })
            .collect();
        (r, arcs)
    } else {
        let (kept, _) = break_cycles(n, &dirs);
        (longest_path(n, &kept), kept)
    }
}

/// 有向图里找环:DFS 三色标记,指向"当前路径上"(灰色)节点的边就是回边,去掉它环就断了.
/// 返回(去掉回边后剩下的边, 有没有去掉过东西)
fn break_cycles(n: usize, arcs: &[(usize, usize)]) -> (Vec<(usize, usize)>, bool) {
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &(a, _)) in arcs.iter().enumerate() {
        out_of[a].push(i);
    }
    let mut color = vec![0u8; n]; // 0 没碰过 1 在当前路径上 2 走完了
    let mut cursor = vec![0usize; n];
    let mut kept = vec![true; arcs.len()];
    let mut dropped = false;
    for s in 0..n {
        if color[s] != 0 {
            continue;
        }
        color[s] = 1;
        let mut stack = vec![s];
        while let Some(&v) = stack.last() {
            if cursor[v] < out_of[v].len() {
                let ei = out_of[v][cursor[v]];
                cursor[v] += 1;
                let w = arcs[ei].1;
                match color[w] {
                    0 => {
                        color[w] = 1;
                        stack.push(w);
                    }
                    1 => {
                        kept[ei] = false;
                        dropped = true;
                    }
                    _ => {}
                }
            } else {
                color[v] = 2;
                stack.pop();
            }
        }
    }
    let kept_arcs: Vec<(usize, usize)> = arcs
        .iter()
        .zip(kept.iter())
        .filter(|(_, &k)| k)
        .map(|(&e, _)| e)
        .collect();
    (kept_arcs, dropped)
}

/// 最长路径当层号:入度为零的先入队,松弛出边.队列按索引升序起步,拓扑序是确定的
fn longest_path(n: usize, arcs: &[(usize, usize)]) -> Vec<usize> {
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for &(a, b) in arcs {
        succ[a].push(b);
        indeg[b] += 1;
    }
    let mut rank = vec![0usize; n];
    let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    while let Some(u) = q.pop_front() {
        for &v in &succ[u] {
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    rank
}

/// 无向图的层号:每个连通块从索引最小的框开始 BFS,深度就是层号
fn ranks_bfs(doc: &Document) -> Vec<usize> {
    let n = doc.boxes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, b) in edge_pairs(doc) {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut rank = vec![usize::MAX; n];
    let mut q: VecDeque<usize> = VecDeque::new();
    for s in 0..n {
        if rank[s] != usize::MAX {
            continue;
        }
        rank[s] = 0;
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if rank[v] == usize::MAX {
                    rank[v] = rank[u] + 1;
                    q.push_back(v);
                }
            }
        }
    }
    rank
}

/// 层内排序:每个框往相邻层里邻居的平均位置靠,来回扫几轮把交叉的边理顺.
/// 排序键用 (重心, 框索引),一样的时候按索引定,结果稳定
fn barycenter(layers: &mut [Vec<usize>], arcs: &[(usize, usize)], rank: &[usize]) {
    let n = rank.len();
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in arcs {
        if rank[a] < rank[b] {
            pred[b].push(a);
            succ[a].push(b);
        }
    }
    let depth = layers.len();
    // 相邻层里"框 -> 它排第几"的缓冲,每层都复用:只写这一层的框,
    // 别的框的旧值不会被读到(读之前先按 rank 过滤)
    let mut slot = vec![0.0f64; n];
    for round in 0..BARY_ROUNDS {
        // 一轮下行(按上一层排)、一轮上行(按下一层排)交替
        let down = round % 2 == 0;
        let order: Vec<usize> = if down {
            (1..depth).collect()
        } else {
            (0..depth.saturating_sub(1)).rev().collect()
        };
        for r in order {
            let adjacent = if down { r - 1 } else { r + 1 };
            let nb = if down { &pred } else { &succ };
            // 相邻层里"框 -> 它排第几"
            for (j, &v) in layers[adjacent].iter().enumerate() {
                slot[v] = j as f64;
            }
            let mut key: Vec<(f64, usize)> = Vec::with_capacity(layers[r].len());
            for (j, &v) in layers[r].iter().enumerate() {
                let mut sum = 0.0;
                let mut cnt = 0usize;
                for &u in &nb[v] {
                    if rank[u] == adjacent {
                        sum += slot[u];
                        cnt += 1;
                    }
                }
                // 相邻层里一个邻居都没有:留在原地
                let bary = if cnt == 0 { j as f64 } else { sum / cnt as f64 };
                key.push((bary, v));
            }
            key.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            layers[r] = key.into_iter().map(|(_, v)| v).collect();
        }
    }
}

/// polish 之后把框收回到各自的层带里:层号决定行(y = 层号 * 层间距),
/// 同一层按当前 x 从左到右排,相邻两个至少隔 lane_gap.
/// 于是任意两框不同格、同层不挤,有向边(层号大的在下)的上下次序一定保得住
fn relayer(p: &mut [(f64, f64)], rank: &[usize], (lane_gap, layer_gap): (f64, f64)) {
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..p.len() {
        layers[rank[i]].push(i);
    }
    for (r, layer) in layers.iter_mut().enumerate() {
        layer.sort_by(|&a, &b| p[a].0.total_cmp(&p[b].0).then(a.cmp(&b)));
        let y = r as f64 * layer_gap;
        let mut prev = f64::NEG_INFINITY;
        for &i in layer.iter() {
            let want = p[i].0.round();
            let x = if prev.is_finite() {
                want.max(prev + lane_gap)
            } else {
                want
            };
            p[i] = (x, y);
            prev = x;
        }
    }
}

/// 分层布局(Sugiyama-lite,够小够明确):
///   1. 有向边当约束,DFS 找回边先去掉 —— 有环的图也画得出来
///   2. 按最长路径定层号(层号小的在上);一条有向边都没有时按无向处理,
///      连通块的 BFS 深度当层号
///   3. 层内按重心扫几轮定顺序,排序用框索引兜底,结果稳定
///   4. y = 层号 * 层间距,x = 层内顺序 * 层间格距,先摆成这样
///   5. 走同一套 polish 把边理顺,再按层收回层带(见 relayer),
///      保证有向边一定自上而下,框与框之间留够尺寸的空
fn layered(doc: &Document, sizes: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = doc.boxes.len();
    let cfg = Cfg::of(n);
    let (lane, layer) = pitch(sizes);
    let (rank, arcs) = ranks(doc);
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..n {
        layers[rank[i]].push(i);
    }
    let mut p = vec![(0.0, 0.0); n];
    let place = |p: &mut [(f64, f64)], layers: &[Vec<usize>]| {
        for (r, row) in layers.iter().enumerate() {
            for (j, &i) in row.iter().enumerate() {
                p[i] = (j as f64 * lane, r as f64 * layer);
            }
        }
    };
    place(&mut p, &layers);
    barycenter(&mut layers, &arcs, &rank);
    // 重心扫完会改层内顺序,要按新顺序重新摆一遍 x
    place(&mut p, &layers);
    let edges = edge_pairs(doc);
    let mut out = polish(p, &edges, &cfg, 1.0);
    relayer(&mut out, &rank, (lane, layer));
    out
}

/// 图里有没有环:有向边当约束,回边就是环.
/// pick 用它决定有方向的图走分层(无环)还是力导(有环)
fn has_cycle(n: usize, arcs: &[(usize, usize)]) -> bool {
    break_cycles(n, arcs).1
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;
    use crate::document::{Box, DocEdge, Group, Layer, Text, ViewState};
    use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

    /// 按文字算框尺寸:`Document::fit_box` 的规则(点节点 1x1)
    fn size_of(label: &str, style: BoxStyle) -> (u16, u16) {
        if !style.is_boxed() {
            return (1, 1);
        }
        let w = (crate::text::plain_width(label).max(2) + 2).max(4);
        (w as u16, 3)
    }

    /// 造一份测试文档:框按顺序横着摆,给一些边
    fn doc_of(labels: &[(&str, BoxStyle)], edges: &[(usize, usize, bool)]) -> Document {
        let groups = vec![Group {
            id: "g".to_string(),
            label: "g".to_string(),
            color: (200, 200, 200),
        }];
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let mut boxes = Vec::new();
        let mut texts = Vec::new();
        let mut x = 0;
        for (i, &(label, style)) in labels.iter().enumerate() {
            let size = size_of(label, style);
            let at = (x, 0);
            boxes.push(Box {
                id: format!("b{i}"),
                at,
                size,
                style,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            });
            texts.push(Text {
                id: format!("t{i}"),
                text: label.to_string(),
                at: if style.is_boxed() {
                    (at.0 + 1, at.1 + 1)
                } else {
                    at
                },
                box_id: format!("b{i}"),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            });
            x += size.0 as i32 + 1;
        }
        let edges = edges
            .iter()
            .enumerate()
            .map(|(i, &(a, b, directed))| DocEdge {
                id: format!("e{i}"),
                from: format!("b{a}"),
                to: format!("b{b}"),
                dir: if directed { Dir::Forward } else { Dir::None },
                group: "g".to_string(),
                style: EdgeStyle::Solid,
                color: None,
                bg: None,
                via: Vec::new(),
                corner: Corner::Rounded,
                layer: String::new(),
            })
            .collect();
        Document::from_parts(
            "test",
            groups,
            layers,
            boxes,
            texts,
            edges,
            ViewState::default(),
            crate::document::Legend::default(),
        )
        .expect("测试文档要能通过校验")
    }

    /// 两个矩形至少隔一格
    fn clear(a: (i32, i32), asz: (u16, u16), b: (i32, i32), bsz: (u16, u16)) -> bool {
        let (ax1, ay1) = (a.0 as i64 + asz.0 as i64 - 1, a.1 as i64 + asz.1 as i64 - 1);
        let (bx1, by1) = (b.0 as i64 + bsz.0 as i64 - 1, b.1 as i64 + bsz.1 as i64 - 1);
        let (ax0, ay0) = (a.0 as i64, a.1 as i64);
        let (bx0, by0) = (b.0 as i64, b.1 as i64);
        ax1 + 1 < bx0 || bx1 + 1 < ax0 || ay1 + 1 < by0 || by1 + 1 < ay0
    }

    fn no_overlap(doc: &Document) -> bool {
        for i in 0..doc.boxes.len() {
            for j in i + 1..doc.boxes.len() {
                if !clear(
                    doc.boxes[i].at,
                    doc.boxes[i].size,
                    doc.boxes[j].at,
                    doc.boxes[j].size,
                ) {
                    return false;
                }
            }
        }
        true
    }

    fn ats(doc: &Document) -> Vec<(i32, i32)> {
        doc.boxes.iter().map(|b| b.at).collect()
    }

    #[test]
    fn 四套布局都是整数且文字跟随() {
        for l in Layout::all() {
            let mut doc = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Single),
                    ("c", BoxStyle::Rounded),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            apply(&mut doc, l).expect("布局");
            for i in 0..doc.boxes.len() {
                let b = &doc.boxes[i];
                let t = doc.text(i).expect("每个框都有文字");
                let want = if b.style.is_boxed() {
                    (b.at.0 + 1, b.at.1 + 1)
                } else {
                    b.at
                };
                assert_eq!(t.at, want, "{} 的文字位置没跟着框", l.name());
            }
            assert!(no_overlap(&doc), "{} 让两个框贴在一起或重叠了", l.name());
        }
    }

    #[test]
    fn 布局是确定的() {
        for l in Layout::all() {
            let mut a = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Double),
                    ("c", BoxStyle::Point),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            let mut b = doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("bbbb", BoxStyle::Double),
                    ("c", BoxStyle::Point),
                ],
                &[(0, 1, true), (1, 2, true)],
            );
            apply(&mut a, l).expect("布局");
            apply(&mut b, l).expect("布局");
            assert_eq!(ats(&a), ats(&b), "{} 两次结果不一样", l.name());
        }
    }

    #[test]
    fn geo_不动已经摆好的框() {
        let mut doc = doc_of(
            &[("a", BoxStyle::Point), ("bbbb", BoxStyle::Single)],
            &[(0, 1, true)],
        );
        let before = ats(&doc);
        apply(&mut doc, Layout::Geo).expect("布局");
        assert_eq!(ats(&doc), before);
    }

    #[test]
    fn 有环图也画得出来() {
        let mut cyclic = doc_of(
            &[
                ("a", BoxStyle::Point),
                ("b", BoxStyle::Point),
                ("c", BoxStyle::Point),
            ],
            &[(0, 1, true), (1, 2, true), (2, 0, true)],
        );
        for l in [Layout::Force, Layout::Layered] {
            apply(&mut cyclic, l).expect("布局");
            assert_eq!(cyclic.boxes.len(), 3);
            assert!(no_overlap(&cyclic), "{} 在环上叠了框", l.name());
        }
        let mut plain = doc_of(
            &[
                ("a", BoxStyle::Point),
                ("b", BoxStyle::Point),
                ("c", BoxStyle::Point),
                ("d", BoxStyle::Point),
            ],
            &[(0, 1, false), (1, 2, false), (2, 3, false), (3, 0, false)],
        );
        apply(&mut plain, Layout::Layered).expect("布局");
        assert!(no_overlap(&plain));
    }

    #[test]
    fn pick_只看方向与环() {
        assert_eq!(
            pick(&doc_of(
                &[("a", BoxStyle::Point), ("b", BoxStyle::Point)],
                &[(0, 1, true)]
            )),
            Layout::Layered
        );
        assert_eq!(
            pick(&doc_of(
                &[
                    ("a", BoxStyle::Point),
                    ("b", BoxStyle::Point),
                    ("c", BoxStyle::Point)
                ],
                &[(0, 1, true), (1, 2, true), (2, 0, true)]
            )),
            Layout::Force
        );
        assert_eq!(
            pick(&doc_of(
                &[("a", BoxStyle::Point), ("b", BoxStyle::Point)],
                &[(0, 1, false)]
            )),
            Layout::Layered
        );
        assert_eq!(
            pick(&doc_of(
                &[("a", BoxStyle::Point), ("b", BoxStyle::Point)],
                &[]
            )),
            Layout::Layered
        );
    }

    #[test]
    fn 名字别名与循环() {
        for l in Layout::all() {
            assert_eq!(Layout::parse(l.name()), Some(l));
            assert_eq!(l.next().next().next().next(), l);
            assert!(!l.label(Lang::Zh).is_empty() && !l.label(Lang::En).is_empty());
        }
        assert_eq!(Layout::parse(" real "), Some(Layout::Geo));
        assert_eq!(Layout::parse("SPRING"), Some(Layout::Relax));
        assert_eq!(Layout::parse("fr"), Some(Layout::Force));
        assert_eq!(Layout::parse("dag"), Some(Layout::Layered));
        assert_eq!(Layout::parse("sugiyama"), Some(Layout::Layered));
        assert_eq!(Layout::parse("nope"), None);
    }

    #[test]
    fn 大图不会卡死() {
        let n = 6000;
        let labels: Vec<(&str, BoxStyle)> = (0..n).map(|_| ("x", BoxStyle::Point)).collect();
        let edges: Vec<(usize, usize, bool)> = (0..n - 1).map(|i| (i, i + 1, true)).collect();
        let mut doc = doc_of(&labels, &edges);
        apply(&mut doc, Layout::Layered).expect("布局");
        assert_eq!(doc.boxes.len(), n);
        assert!(no_overlap(&doc));
    }
}
