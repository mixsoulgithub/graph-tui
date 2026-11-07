use super::*;
use crate::model::Corner;

/// 兜底路线被框挡住时,不要画半条悬空线:返回原地点,绘制那一层就什么都不画
#[test]
fn 兜底路线走不通就不画() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 39, 19);
    // 一堵从这一头到那一头的高墙,把两个端点完全隔开
    for y in 0..20 {
        cv.station_cells.insert((20, y));
    }
    let pts = cv.route_clip((2, 10), (36, 10));
    assert!(pts.is_empty(), "被挡住时该返回空路径:{pts:?}");
    // 没有遮挡时照旧能走到目标
    let pts = cv.route_clip((2, 10), (10, 16));
    assert_eq!(*pts.last().unwrap(), (10, 16));
}

/// 找不到路时整条边都不画:没有端口短接、没有箭头、也没有半截线
#[test]
fn 走线失败时连端口都不落() {
    let mut cv = Canvas::new(
        (0, 0, 60, 24),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 59, 23);
    // 一堵把目标整个围起来的墙,只在斜对角留不出路
    for y in 0..24 {
        cv.station_cells.insert((40, y));
    }
    let end_a = EndMark {
        fixed: true,
        rect: (4, 8, 6, 3),
        side: RIGHT,
        cell: (9, 9),
        mark: false,
        corners: false,
    };
    let end_b = EndMark {
        fixed: true,
        rect: (50, 8, 6, 3),
        side: LEFT,
        cell: (50, 9),
        mark: false,
        corners: false,
    };
    cv.edge_full(
        (9, 9),
        (50, 9),
        0,
        (255, 0, 0),
        None,
        (true, true),
        EdgeStyle::Solid,
        Some(end_a),
        Some(end_b),
        None,
    );
    assert!(
        cv.ink.is_empty(),
        "找不到路时不该留墨:{:?}",
        cv.ink.keys().take(5)
    );
    assert!(cv.edge_cells.is_empty(), "不该记成这条边的格子");
    assert!(cv.border.is_empty(), "不该在目标边框落箭头/端口");
}

/// 规划那一遍不许把自环画出来:提前画一遍会占掉端口/边框格,
/// 落笔那一遍挑到别的口,环就断了(examples/real/tcp.gtui 的 ESTABLISHED 自环就是这样丢的)
#[test]
fn 规划那一遍不画自环() {
    let mk = || {
        Canvas::new(
            (0, 0, 24, 12),
            vec![false; 1],
            0,
            crate::glyphs::Glyphs::UNICODE,
        )
    };
    let end = EndMark {
        fixed: false,
        rect: (4, 3, 8, 3),
        side: RIGHT,
        cell: (11, 4),
        mark: true,
        corners: true,
    };
    let mut once = mk();
    once.edge_loop(end, (true, false), 0, (255, 0, 0), EdgeStyle::Solid);
    let mut twice = mk();
    twice.planning = true;
    twice.edge_loop(end, (true, false), 0, (255, 0, 0), EdgeStyle::Solid);
    twice.planning = false;
    twice.edge_loop(end, (true, false), 0, (255, 0, 0), EdgeStyle::Solid);
    assert_eq!(once.ink, twice.ink, "规划那一遍把墨弄脏了");
    assert_eq!(once.border, twice.border, "规划那一遍把端口/边框占了");
}

/// 一堵比搜索窗口高的墙夹在两点之间:线要翻过去,不能停在墙边
#[test]
fn 翻得过高墙() {
    let mut cv = Canvas::new(
        (0, 0, 80, 60),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 墙:5 宽 41 高,正好盖住 a 和 b 之间
    cv.frame = (-10, -30, 50, 30);
    for y in -20..=20 {
        cv.station_cells.insert((20, y));
        cv.station_cells.insert((24, y));
    }
    for x in 20..=24 {
        cv.station_cells.insert((x, -20));
        cv.station_cells.insert((x, 20));
    }
    let pts = cv.route((0, 0), Some((1, 0)), (40, 0), Some((-1, 0)), 0);
    let last = *pts.last().unwrap();
    assert_eq!(last, (40, 0), "线要真的走到目标:{pts:?}");
    assert!(
        pts.iter().any(|p| p.1 < -20 || p.1 > 20),
        "该从墙的上/下翻过去:{pts:?}"
    );
    assert!(
        !pts.iter().any(|p| cv.station_cells.contains(p)),
        "不能穿墙:{pts:?}"
    );
}

/// 两条边叠在同一格:只有一条画出来(不并成 ┼),选中的那条压到上面
#[test]
fn 重叠的边一格只画一条() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 边 0:横着穿过 (5,5)
    cv.pen_edge = 0;
    cv.pen_corner = Corner::Rounded;
    cv.link(4, 5, RIGHT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(5, 5, LEFT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(5, 5, RIGHT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(6, 5, LEFT, 0, (10, 20, 30), EdgeStyle::Solid);
    let i = cv.idx(5, 5).unwrap();
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '─');
    assert_eq!(cv.edge_of[i], 0);
    // 边 1:竖着也从 (5,5) 过.它被压住,这一格还是横线,不变成 ┼
    cv.pen_edge = 1;
    cv.link(5, 5, UP, 1, (200, 10, 10), EdgeStyle::Solid);
    cv.link(5, 5, DOWN, 1, (200, 10, 10), EdgeStyle::Solid);
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '─', "叠上来不该并成 ┼");
    assert_eq!(cv.edge_of[i], 0, "形状还归先画的那条");
    // 这一格两条边都是候选:点两下能选到下面那条
    let cell = (5, 5);
    assert_eq!(
        cv.edge_cands.get(&cell).map(Vec::as_slice),
        Some(&[0usize, 1][..])
    );
    // 选中边 1:它压到上面,这一格变成竖线
    cv.raise(Some(1));
    cv.pen_edge = 1;
    cv.link(5, 5, UP, 1, (200, 10, 10), EdgeStyle::Solid);
    cv.link(5, 5, DOWN, 1, (200, 10, 10), EdgeStyle::Solid);
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '│', "选中的要盖上来");
    assert_eq!(cv.edge_of[i], 1);
}

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

/// 点节点连出来的 L 不要整段并到已有的边上:并上去就只剩一条线,
/// 后画的那条整段看不见,还会顶着别人的箭头,看着像双向
#[test]
fn 点节点的拐法不和已有的线并线() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 第一条:右上 -> 左下,竖着下来再横着走,占住 y=4 整行
    cv.pen_edge = 0;
    cv.edge((20, 0), (0, 4), 0, (255, 0, 0), Some(true), (false, true));
    assert!(cv.edge_cells[&0].contains(&(10, 4)), "第一条占住了 y=4");
    // 左下那条竖线先占上:这样第二条的两种拐法都算"贴着分组自己",平时打平
    cv.pen_edge = 1;
    cv.edge((0, 4), (0, 8), 0, (255, 0, 0), None, (false, false));
    // 第二条:左下 -> 右下.横着先走会整段压在第一条的 y=4 上
    cv.pen_edge = 2;
    cv.edge((0, 4), (20, 8), 0, (0, 255, 0), None, (false, true));
    let cands = cv.edge_cands.get(&(10, 4)).cloned().unwrap_or_default();
    assert!(!cands.contains(&2), "第二条不该走第一条那一行:{cands:?}");
    assert!(
        cv.edge_cells[&2].iter().any(|&c| c.1 == 8 && c.0 > 0),
        "改成先竖着下去再从 y=8 横着走"
    );
}

/// 端口走线不穿过别的框:障碍框内部不能有任何方向位,绕行的线在框外
#[test]
fn 走线绕开障碍框() {
    let mut cv = Canvas::new(
        (0, 0, 40, 30),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
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
            fixed: false,
            rect: (0, 3, 5, 3),
            side: DOWN,
            cell: (2, 5),
            mark: true,
            corners: true,
        }),
        Some(EndMark {
            fixed: false,
            rect: (0, 14, 5, 3),
            side: UP,
            cell: (2, 14),
            mark: true,
            corners: true,
        }),
        None,
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
        Some(&BorderMark::Junction(LR | DOWN, (255, 0, 0))),
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
        let mut cv = Canvas::new(
            (0, 0, 40, 40),
            vec![false; 1],
            0,
            crate::glyphs::Glyphs::UNICODE,
        );
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
    let mut cv = Canvas::new(
        (0, 0, 40, 40),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
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
    let mut cv = Canvas::new(
        (0, 0, 40, 40),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, false));
    cv.edge((5, 2), (5, 9), 0, (255, 0, 0), None, (false, false));
    let cross = cv.idx(5, 5).expect("在画布内");
    assert_eq!(cv.mask[cross], LEFT | RIGHT | UP | DOWN);
    assert_eq!(box_char(cv.mask[cross], Corner::Rounded), '┼');
    // 只从上面接过来:三通里的 ┬
    let mut cv = Canvas::new(
        (0, 0, 40, 40),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
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
    let mut cv = Canvas::new(
        (0, 0, 60, 30),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
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
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'));
    assert_eq!(cv.arrow[cv.idx(9, 5).unwrap()], None, "箭头不占终点本身");
    assert_eq!(cv.arrow[cv.idx(2, 5).unwrap()], None, "只在终点那一头画");
    // 往西:箭头跟着走,画在另一侧
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((9, 5), (2, 5), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(3, 5).unwrap()], Some('<'));
    // 往南(屏幕行号变大)
    let mut cv = Canvas::new(
        (0, 0, 20, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((5, 2), (5, 12), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(5, 11).unwrap()], Some('v'));
    // 往北
    let mut cv = Canvas::new(
        (0, 0, 20, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((5, 12), (5, 2), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(5, 3).unwrap()], Some('^'));
}

/// 拐弯的边:箭头按**最后那一段**的走向,不是两个端点的连线方向
#[test]
fn 拐弯的有向边按最后一段画箭头() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 横向比纵向长,先横后竖:最后一段是竖的,往下走到 (20,10)
    cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (false, true));
    assert_eq!(
        box_char(cv.mask[cv.idx(20, 2).unwrap()], Corner::Rounded),
        '╮',
        "先横后竖"
    );
    assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "竖着进节点");
    // 箭头画在 a 那一头时也一样:它指着箭头所在的那个节点,所以是反着的
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, false));
    assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "指回 (2,2)");
    assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], None, "另一头不画");
}

/// 箭头不盖节点字形
#[test]
fn 箭头不盖节点字形() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.node_at(5, 5, false, 1, Some(0), (0, 255, 0));
    cv.station_cells.insert((5, 5));
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(5, 5).unwrap()], None, "节点那一格不画箭头");
    assert_eq!(cv.arrow[cv.idx(4, 5).unwrap()], None, "箭头不会往前挪一格");
}

/// 一格已经归别的分组(两组挤在同一格)时不画箭头:方向说不清
#[test]
fn 箭头压在最上层() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 红的线和蓝的线交叉:红线的箭头照画,颜色还是红的
    cv.edge((0, 5), (9, 5), 1, (0, 0, 255), None, (false, false));
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
    let cell = cv.idx(8, 5).unwrap();
    assert_eq!(cv.arrow[cell], Some('>'), "箭头盖在别人的线上");
    assert_eq!(cv.arrow_ink[cell], Some((255, 0, 0)), "颜色跟着自己的边");
    assert_eq!(cv.groups_here[cell], 0b11, "两组都在这格上");
    // 一格最多一个箭头:后来的边顶不掉,抬起来的那条才能顶掉
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
    cv.pen_edge = 1;
    cv.edge((0, 5), (9, 5), 0, (0, 0, 255), None, (false, true));
    let cell = cv.idx(8, 5).unwrap();
    assert_eq!(cv.arrow[cell], Some('>'), "先到的那一个说了算");
    assert_eq!(cv.arrow_ink[cell], Some((255, 0, 0)));
    assert_eq!(cv.arrow_owners[&(8, 5)], vec![0, 1], "同方向的边共用一格");
    // 抬起来的边(选中 / E 高亮)压掉**方向不同**的箭头
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.outside_arrow((12, 5), 2, (255, 0, 0)); // '<'
    cv.raise(Some(1));
    cv.pen_edge = 1;
    cv.outside_arrow((12, 5), 1, (0, 0, 255)); // '>'
    let cell = cv.idx(12, 5).unwrap();
    assert_eq!(cv.arrow[cell], Some('>'), "抬起来的那条占住箭头");
    assert_eq!(cv.arrow_owners[&(12, 5)], vec![1], "归属也换掉了");
    assert_eq!(cv.arrow_ink[cell], Some((0, 0, 255)));
    // 没抬起来就不能顶掉别人的箭头
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.outside_arrow((12, 5), 2, (255, 0, 0));
    cv.pen_edge = 1;
    cv.outside_arrow((12, 5), 1, (0, 0, 255));
    let cell = cv.idx(12, 5).unwrap();
    assert_eq!(cv.arrow[cell], Some('<'), "方向不同也顶不掉");
    assert_eq!(cv.arrow_owners[&(12, 5)], vec![0]);
}

/// 同方向已经有一个箭头时,后来那条边只记账:格子进它的 edge_cells,
/// 字形/颜色/归属都留在第一条边上(几条边指同一个一行文字的目标)
#[test]
fn 同方向箭头共用一格() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.outside_arrow((12, 5), 2, (255, 0, 0));
    let cell = cv.idx(12, 5).unwrap();
    let first = cv.edge_cells.get(&0).cloned().unwrap_or_default();
    assert!(first.contains(&(12, 5)));
    // 第二条边同一个方向再来:字形/墨色不动,只是把这格记到它名下
    cv.pen_edge = 1;
    cv.outside_arrow((12, 5), 2, (0, 0, 255));
    assert_eq!(cv.arrow[cell], Some('<'), "字形只画一次");
    assert_eq!(cv.arrow_ink[cell], Some((255, 0, 0)), "颜色留在第一条边上");
    assert_eq!(cv.arrow_owners[&(12, 5)], vec![0, 1], "两条边都是 owner");
    assert!(
        cv.arrows_of.get(&1).is_some_and(|c| c.contains(&(12, 5))),
        "第二条边也要把自己的箭头端点记下来"
    );
    assert!(
        cv.edge_cells.get(&1).is_some_and(|c| c.contains(&(12, 5))),
        "第二条边的命中表里要有这一格"
    );
}

/// 文字节点(none)的端点:外侧格要补"朝向端口"那一位, 端口格(文字)不碰
#[test]
fn 文字节点端点补朝向端口的方向位() {
    let mut cv = Canvas::new(
        (0, 0, 40, 12),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    let rect = (10, 5, 8, 1); // 一行八列的文字节点
    let end = EndMark {
        fixed: false,
        rect,
        side: UP,
        cell: Canvas::side_base(rect, UP),
        mark: false,
        corners: false,
    };
    let port = end.cell;
    let out = Canvas::outside(port, UP);
    // 这一端的路线从外侧格往下走进去(原本只有 UP 一位)
    cv.edge_full(
        (20, 5),
        (18, 5),
        0,
        (255, 0, 0),
        None,
        (false, false),
        EdgeStyle::Solid,
        None,
        Some(end),
        None,
    );
    let oi = cv.idx(out.0, out.1).unwrap();
    // 外侧格 = 路线那一笔(UP) + 朝向端口那一笔(DOWN):补位前只有一个方向位
    assert_eq!(
        cv.mask[oi],
        UP | DOWN,
        "外侧格该有路线那一笔 + 朝向端口那一笔"
    );
    // 端口格(文字)什么都没被画上去
    let pi = cv.idx(port.0, port.1).unwrap();
    assert_eq!(cv.mask[pi], 0, "端口格不该有方向位");
    assert!(cv.deco[pi].is_none(), "端口格不该被占用");
}

/// 直线端点不强行拐弯:外侧格只有一个轴的两向位
#[test]
fn 直线端点不强行拐弯() {
    let mut cv = Canvas::new(
        (0, 0, 40, 12),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    let rect = (10, 5, 8, 1);
    let end = EndMark {
        fixed: false,
        rect,
        side: LEFT,
        cell: Canvas::side_base(rect, LEFT),
        mark: false,
        corners: false,
    };
    let out = Canvas::outside(end.cell, LEFT);
    // 目标就在左边同一行:路线继续向左
    cv.edge_full(
        (0, 5),
        (2, 5),
        0,
        (255, 0, 0),
        None,
        (false, false),
        EdgeStyle::Solid,
        None,
        Some(end),
        None,
    );
    let oi = cv.idx(out.0, out.1).unwrap();
    assert_eq!(cv.mask[oi], LEFT | RIGHT, "直线该只有左右两个方向位");
    assert!(
        matches!(cv.glyphs.line(cv.mask[oi], cv.corner[oi]), '─'),
        "直线不该变成拐角"
    );
}

/// link_here 只动这一格:相邻格不变
#[test]
fn link_here_只动这一格() {
    let mut cv = Canvas::new(
        (0, 0, 20, 8),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.run((5, 4), (9, 4), 0, (255, 0, 0), EdgeStyle::Solid);
    cv.pen_edge = 0;
    cv.link_here((9, 4), DOWN, 0, (255, 0, 0), EdgeStyle::Solid);
    let c = cv.idx(9, 4).unwrap();
    assert_eq!(cv.mask[c], LEFT | DOWN, "路线那一笔 + 朝向端口那一笔");
    assert!(
        !matches!(cv.glyphs.line(cv.mask[c], cv.corner[c]), '─' | '│'),
        "两笔方向不同时应该画成拐角"
    );
    let n = cv.idx(10, 4).unwrap();
    assert_eq!(cv.mask[n], 0, "相邻格不该被补位");
}

/// 共享箭头:字形只画一次,但每个 owner 都记下自己的墨色
#[test]
fn 共享箭头每个_owner_有墨色() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.pen_edge = 0;
    cv.outside_arrow((12, 5), 2, (255, 0, 0));
    cv.pen_edge = 1;
    cv.outside_arrow((12, 5), 2, (0, 0, 255));
    assert_eq!(cv.arrow_owners[&(12, 5)], vec![0, 1]);
    assert_eq!(
        cv.arrow_owner_ink[&((12, 5), 0)],
        (255, 0, 0),
        "第一条边的墨色"
    );
    assert_eq!(
        cv.arrow_owner_ink[&((12, 5), 1)],
        (0, 0, 255),
        "后来的边也要记自己的"
    );
    let cell = cv.idx(12, 5).unwrap();
    assert_eq!(cv.arrow_ink[cell], Some((255, 0, 0)), "可见色仍是第一条的");
}

/// 线端的外侧格(线真正起步的那一格)上不许再落箭头:
/// 否则选中这条线,起点会顶着别人的箭头,看着像 <->
#[test]
fn 线端的外侧格不画箭头() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 0 号边的线端落在 (10,5),外侧格是 (11,5)
    cv.pen_edge = 0;
    cv.commit_end(2, (10, 5), false, (255, 0, 0), false, false);
    assert_eq!(
        cv.port_out_kind.get(&(11, 5)),
        Some(&PortKind::Line),
        "外侧格该记成线端"
    );
    // 1 号边想把箭头画在 (11,5):不许
    cv.pen_edge = 1;
    cv.outside_arrow((11, 5), 2, (0, 0, 255));
    assert_eq!(
        cv.arrow[cv.idx(11, 5).unwrap()],
        None,
        "箭头不能压在线起点上"
    );
    // 端口格本身已经是箭头端时可以照常画(同一格的角色是箭头)
    cv.pen_edge = 2;
    cv.commit_end(2, (10, 5), true, (0, 0, 255), false, false);
    cv.outside_arrow((11, 5), 2, (0, 0, 255));
    assert_eq!(cv.port_kind.get(&(10, 5)), Some(&PortKind::Arrow));
}

/// 文字节点(none)没有边框可画,但端口照样按顺序占格:
/// 同一个节点上的多条边不会挤在同一个进出点
#[test]
fn 文字节点的端口顺延_兜底路径() {
    let mut cv = Canvas::new(
        (0, 0, 40, 12),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    let rect = (10, 5, 8, 1); // 一行八列的文字节点
    let end = EndMark {
        fixed: false,
        rect,
        side: UP,
        cell: Canvas::side_base(rect, UP),
        mark: false,    // 没有边框
        corners: false, // 整条边都能接
    };
    // 普通边的端口现在由 render 的 PortPlan 定;这条兜底路径只剩下预览用
    for k in 0..2 {
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
            None,
        );
    }
    // 文字节点的端口只记角色(不再往 border 里放占位标记)
    let ports: Vec<(i32, i32)> = cv
        .port_kind
        .iter()
        .filter(|(_, k)| **k == PortKind::Line)
        .map(|(c, _)| *c)
        .collect();
    assert_eq!(ports.len(), 2, "两条边该占两个不同的端口格:{ports:?}");
    // 占的都在这个节点自己的那一行里
    assert!(ports
        .iter()
        .all(|c| c.1 == rect.1 && c.0 >= rect.0 && c.0 < rect.0 + rect.2));
}

/// 同一个分组里 a -> b 与 b -> a 是两条边,合到一段上就是两端各一个箭头
#[test]
fn 双向边两端都有箭头() {
    // 直线:去程那一头朝东,回程那一头朝西
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (true, true));
    assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'), "箭头对着 (9,5)");
    assert_eq!(cv.arrow[cv.idx(1, 5).unwrap()], Some('<'), "箭头对着 (0,5)");
    // 拐弯的也一样:两端各自照自己那一端的走向画
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, true));
    assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "b 端竖着进来");
    assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "a 端横着回去");
    // 两个节点挨着(中间没有空格)时箭头没地方放,节点字形优先,谁都不画
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    for x in [4, 5] {
        cv.node_at(x, 5, false, 1, Some(0), (0, 255, 0));
        cv.station_cells.insert((x, 5));
    }
    cv.edge((4, 5), (5, 5), 0, (255, 0, 0), None, (true, true));
    assert_eq!(cv.arrow.iter().filter(|a| a.is_some()).count(), 0);
}

/// 折回来的 via(先往下跑再原路往上)只是多出一截尾巴:丢掉它,线直着走
#[test]
fn 折回来的_via_不画() {
    let mut cv = Canvas::new(
        (0, 0, 40, 24),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 源在 (5,6),目标在 (25,6);via 先跑到 (20,18) 再折回 (20,6)
    cv.path(
        &[(5, 6), (20, 18), (20, 6), (25, 6)],
        0,
        (255, 0, 0),
        EdgeStyle::Solid,
        (false, false),
        None,
        None,
    );
    let far = cv.idx(20, 18).unwrap();
    assert_eq!(cv.mask[far], 0, "折回来的 via 不该留下笔画");
    let near = cv.idx(20, 6).unwrap();
    assert_ne!(cv.mask[near], 0, "该走的那一段还要画");
    let end = cv.idx(25, 6).unwrap();
    assert_ne!(cv.mask[end], 0, "线要画到目标");
}

/// 正常的折点(一路往目标走)照旧生效
#[test]
fn 前进的_via_照旧生效() {
    let mut cv = Canvas::new(
        (0, 0, 40, 24),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // 绕一个 L:先下到 (20,18),再右到 (30,18),再上到目标
    cv.path(
        &[(5, 6), (5, 18), (30, 18), (30, 6)],
        0,
        (255, 0, 0),
        EdgeStyle::Solid,
        (false, false),
        None,
        None,
    );
    let corner = cv.idx(5, 18).unwrap();
    assert_ne!(cv.mask[corner], 0, "往目标方向的折点要保留");
    let far = cv.idx(30, 18).unwrap();
    assert_ne!(cv.mask[far], 0, "第二个折点也要保留");
}
