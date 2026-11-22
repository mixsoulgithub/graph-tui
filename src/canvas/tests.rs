use super::*;
use crate::model::Corner;

/// Blocked fallback route: return the start cell and draw nothing half-way; a dangling
/// half-line is never drawn.
#[test]
fn fallback_route_blocked_draws_nothing() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 39, 19);
    // A tall wall from one end to the other fully separates the two endpoints
    for y in 0..20 {
        cv.station_cells.insert((20, y));
    }
    let pts = cv.route_clip((2, 10), (36, 10));
    assert!(pts.is_empty(), "被挡住时该返回空路径:{pts:?}");
    // Without an obstacle the route reaches the target
    let pts = cv.route_clip((2, 10), (10, 16));
    assert_eq!(*pts.last().unwrap(), (10, 16));
}

/// Failed routing draws none of the edge: no port short-circuit, no arrow, no partial line
#[test]
fn failed_route_leaves_no_ports() {
    let mut cv = Canvas::new(
        (0, 0, 60, 24),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.frame = (0, 0, 59, 23);
    // A wall enclosing the target, leaving a gap only on the diagonal
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

/// Planning must not draw self-loops: an early draw occupies port/border cells, so the
/// drawing pass picks another port and the loop breaks (see examples/real/tcp.gtui).
#[test]
fn planning_pass_skips_self_loop() {
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

/// A wall taller than the search window between two points: climb over it, never stop at it
#[test]
fn climbs_over_tall_wall() {
    let mut cv = Canvas::new(
        (0, 0, 80, 60),
        vec![false],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Wall: 5 wide, 41 tall, covering the span between a and b
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

/// Two edges on the same cell: only one is drawn (never merged into ┼), the selected on top
#[test]
fn overlapping_edges_draw_one_glyph() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Edge 0: horizontal through (5,5)
    cv.pen_edge = 0;
    cv.pen_corner = Corner::Rounded;
    cv.link(4, 5, RIGHT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(5, 5, LEFT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(5, 5, RIGHT, 0, (10, 20, 30), EdgeStyle::Solid);
    cv.link(6, 5, LEFT, 0, (10, 20, 30), EdgeStyle::Solid);
    let i = cv.idx(5, 5).unwrap();
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '─');
    assert_eq!(cv.edge_of[i], 0);
    // Edge 1: vertical through (5,5) too. It is buried, so the cell stays horizontal, not ┼
    cv.pen_edge = 1;
    cv.link(5, 5, UP, 1, (200, 10, 10), EdgeStyle::Solid);
    cv.link(5, 5, DOWN, 1, (200, 10, 10), EdgeStyle::Solid);
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '─', "叠上来不该并成 ┼");
    assert_eq!(cv.edge_of[i], 0, "形状还归先画的那条");
    // Both edges are candidates here: two clicks reach the one underneath
    let cell = (5, 5);
    assert_eq!(
        cv.edge_cands.get(&cell).map(Vec::as_slice),
        Some(&[0usize, 1][..])
    );
    // Select edge 1: it moves on top and the cell becomes vertical
    cv.raise(Some(1));
    cv.pen_edge = 1;
    cv.link(5, 5, UP, 1, (200, 10, 10), EdgeStyle::Solid);
    cv.link(5, 5, DOWN, 1, (200, 10, 10), EdgeStyle::Solid);
    assert_eq!(box_char(cv.mask[i], cv.corner[i]), '│', "选中的要盖上来");
    assert_eq!(cv.edge_of[i], 1);
}

#[test]
fn direction_bits_compose_glyph() {
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

/// An L from a point node must not merge into an existing edge: merged, only one line
/// survives, the later edge is invisible and wears someone else's arrow (looks bidirectional).
#[test]
fn point_node_corner_does_not_merge() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // First: top-right -> bottom-left, down then across, taking the whole row y=4
    cv.pen_edge = 0;
    cv.edge((20, 0), (0, 4), 0, (255, 0, 0), Some(true), (false, true));
    assert!(cv.edge_cells[&0].contains(&(10, 4)), "第一条占住了 y=4");
    // Claiming the lower vertical first makes both corner styles of the second edge tie
    cv.pen_edge = 1;
    cv.edge((0, 4), (0, 8), 0, (255, 0, 0), None, (false, false));
    // Second: bottom-left -> bottom-right. Going horizontal first would sit on the whole y=4
    cv.pen_edge = 2;
    cv.edge((0, 4), (20, 8), 0, (0, 255, 0), None, (false, true));
    let cands = cv.edge_cands.get(&(10, 4)).cloned().unwrap_or_default();
    assert!(!cands.contains(&2), "第二条不该走第一条那一行:{cands:?}");
    assert!(
        cv.edge_cells[&2].iter().any(|&c| c.1 == 8 && c.0 > 0),
        "改成先竖着下去再从 y=8 横着走"
    );
}

/// Port routes never cross a box: no direction bits inside an obstacle, detours stay outside
#[test]
fn route_avoids_obstacle_box() {
    let mut cv = Canvas::new(
        (0, 0, 40, 30),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // a at (0,3,5,3), b at (0,14,5,3), c blocking in between
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
    // Detour: the column right of the obstacle box carries a vertical line
    let detour = (8..=11)
        .filter(|y| cv.mask[cv.idx(5, *y).unwrap()] != 0)
        .count();
    assert_eq!(detour, 4, "该从框右边那一列整列绕过去");
    // Port cells get a T and an arrow
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

/// Right angles: combining the two strokes' direction bits yields one of four rounded corners
#[test]
fn two_strokes_make_rounded_corner() {
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
    // Horizontal then vertical: the corner is ╮ at the top-right, ╭ at the top-left
    assert_eq!(corner_of((2, 2), (7, 7), (7, 2)), '╮');
    assert_eq!(corner_of((12, 2), (7, 7), (7, 2)), '╭');
    // Vertical then horizontal: ╰ at the bottom-left, ╯ at the bottom-right
    assert_eq!(corner_of((2, 2), (7, 12), (2, 12)), '╰');
    assert_eq!(corner_of((12, 2), (7, 12), (12, 12)), '╯');
}

/// Sharp corners: the same bits give ┌┐└┘; straight runs and junctions are unaffected
#[test]
fn sharp_corner_only_changes_corners() {
    assert_eq!(box_char(DR, Corner::Sharp), '┌');
    assert_eq!(box_char(DL, Corner::Sharp), '┐');
    assert_eq!(box_char(UR, Corner::Sharp), '└');
    assert_eq!(box_char(UL, Corner::Sharp), '┘');
    assert_eq!(box_char(DR, Corner::Rounded), '╭');
    assert_eq!(box_char(LR, Corner::Sharp), '─');
    assert_eq!(box_char(LRUD, Corner::Sharp), '┼');
    // Drawing uses the edge's own corner style
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
    // The edge that claimed the cell wins: a later rounded corner does not change it
    cv.pen_corner = Corner::Rounded;
    cv.edge((7, 2), (7, 7), 0, (255, 0, 0), None, (false, false));
    assert_eq!(cv.corner[i], Corner::Sharp);
}

/// T/cross junctions: two strokes meeting on one cell of the **same group** form ┼ / ┬
#[test]
fn junction_glyphs_from_direction_bits() {
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
    // Only joined from above: the ┬ of a T junction
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

/// "Down then up again" edges must share one column:
/// a and c are both north of b, so entering and leaving b both go vertical, the two
/// edges land in the same column and do not enclose a box
#[test]
fn down_and_up_share_one_column() {
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
    // b's column: every cell from a's row to b's row carries a stroke
    for y in a.1..=b.1 {
        let i = cv.idx(b.0, y).expect("在画布内");
        assert_ne!(cv.mask[i], 0, "({}, {y}) 该有笔画", b.0);
    }
    // Elsewhere in that span no other column may carry strokes (that would be a box)
    for x in a.0..b.0 {
        for y in a.1 + 1..b.1 {
            let i = cv.idx(x, y).expect("在画布内");
            assert_eq!(cv.mask[i], 0, "({x}, {y}) 不该有笔画:边绕出去了");
        }
    }
}

/// Directed edge arrow: the cell next to the target, direction taken from the last segment
#[test]
fn directed_arrow_cell_next_to_target() {
    // East
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
    // West: the arrow follows, drawn on the other side
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((9, 5), (2, 5), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(3, 5).unwrap()], Some('<'));
    // South (screen rows grow)
    let mut cv = Canvas::new(
        (0, 0, 20, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((5, 2), (5, 12), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(5, 11).unwrap()], Some('v'));
    // North
    let mut cv = Canvas::new(
        (0, 0, 20, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((5, 12), (5, 2), 0, (255, 0, 0), None, (false, true));
    assert_eq!(cv.arrow[cv.idx(5, 3).unwrap()], Some('^'));
}

/// A bent edge: the arrow follows the **last segment**, not the line between the endpoints
#[test]
fn bent_edge_arrow_follows_last_segment() {
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Wider than tall, so horizontal first: the last segment is vertical, down to (20,10)
    cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (false, true));
    assert_eq!(
        box_char(cv.mask[cv.idx(20, 2).unwrap()], Corner::Rounded),
        '╮',
        "先横后竖"
    );
    assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "竖着进节点");
    // Same when the arrow sits at the a end: it points at the node it touches, so reversed
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

/// Arrows do not cover node glyphs
#[test]
fn arrow_does_not_cover_node_glyph() {
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

/// No arrow when the cell already belongs to another group: the direction would be unclear
#[test]
fn arrow_cell_ownership_and_raise() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Red and blue lines cross: the red arrow is drawn, in red
    cv.edge((0, 5), (9, 5), 1, (0, 0, 255), None, (false, false));
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
    let cell = cv.idx(8, 5).unwrap();
    assert_eq!(cv.arrow[cell], Some('>'), "箭头盖在别人的线上");
    assert_eq!(cv.arrow_ink[cell], Some((255, 0, 0)), "颜色跟着自己的边");
    assert_eq!(cv.groups_here[cell], 0b11, "两组都在这格上");
    // One arrow per cell: a later edge cannot steal it, only a raised one can
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
    // A raised edge (selected / E highlight) overrides arrows of **different direction**
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
    // Unraised, it cannot steal another edge's arrow
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

/// When an arrow already exists in the same direction, a later edge only records the cell in
/// its edge_cells; glyph, colour and owner stay with the first edge (shared text target)
#[test]
fn same_direction_arrows_share_cell() {
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
    // A second edge in the same direction: glyph and ink stay, only the cell is recorded
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

/// Text node (none) end: the outside cell gains the "towards port" bit; the text cell stays clear
#[test]
fn text_node_end_fills_towards_port_bit() {
    let mut cv = Canvas::new(
        (0, 0, 40, 12),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    let rect = (10, 5, 8, 1); // text node, one row of eight columns
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
    // The route reaches this end from the outside cell below (only UP was set)
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
    // Outside cell = route stroke (UP) + towards-port stroke (DOWN): one bit before
    assert_eq!(
        cv.mask[oi],
        UP | DOWN,
        "外侧格该有路线那一笔 + 朝向端口那一笔"
    );
    // Nothing is drawn on the port cell (text)
    let pi = cv.idx(port.0, port.1).unwrap();
    assert_eq!(cv.mask[pi], 0, "端口格不该有方向位");
    assert!(cv.deco[pi].is_none(), "端口格不该被占用");
}

/// Straight ends do not bend: the outside cell carries one axis only (two opposite bits)
#[test]
fn straight_end_does_not_bend() {
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
    // Target is on the same row to the left: the route continues left
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

/// link_here touches only its cell: neighbours are unchanged
#[test]
fn link_here_touches_only_its_cell() {
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

/// Shared arrow: the glyph is drawn once, but every owner records its own ink
#[test]
fn shared_arrow_keeps_owner_ink() {
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

/// No arrow on the outside cell where a line actually starts:
/// otherwise selecting that line puts the start under someone else's arrow, looking like <->
#[test]
fn line_start_outside_cell_takes_no_arrow() {
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 2],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Edge 0's line start lands at (10,5); its outside cell is (11,5)
    cv.pen_edge = 0;
    cv.commit_end(2, (10, 5), false, (255, 0, 0), false, false);
    assert_eq!(
        cv.port_out_kind.get(&(11, 5)),
        Some(&PortKind::Line),
        "外侧格该记成线端"
    );
    // Edge 1 wants to draw an arrow at (11,5): not allowed
    cv.pen_edge = 1;
    cv.outside_arrow((11, 5), 2, (0, 0, 255));
    assert_eq!(
        cv.arrow[cv.idx(11, 5).unwrap()],
        None,
        "箭头不能压在线起点上"
    );
    // A port cell that is already an arrow end draws normally (the cell's role is arrow)
    cv.pen_edge = 2;
    cv.commit_end(2, (10, 5), true, (0, 0, 255), false, false);
    cv.outside_arrow((11, 5), 2, (0, 0, 255));
    assert_eq!(cv.port_kind.get(&(10, 5)), Some(&PortKind::Arrow));
}

/// Text node (none) has no border to draw, but ports take cells in order:
/// several edges on one node never crowd into the same entry/exit point
#[test]
fn text_node_ports_advance_fallback() {
    let mut cv = Canvas::new(
        (0, 0, 40, 12),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    let rect = (10, 5, 8, 1); // text node, one row of eight columns
    let end = EndMark {
        fixed: false,
        rect,
        side: UP,
        cell: Canvas::side_base(rect, UP),
        mark: false,    // no border
        corners: false, // every cell of the line can connect
    };
    // Ordinary edges take their ports from render's PortPlan; this fallback is preview-only
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
    // Text node ports record only a role; border holds no placeholder marks
    let ports: Vec<(i32, i32)> = cv
        .port_kind
        .iter()
        .filter(|(_, k)| **k == PortKind::Line)
        .map(|(c, _)| *c)
        .collect();
    assert_eq!(ports.len(), 2, "两条边该占两个不同的端口格:{ports:?}");
    // All occupied cells lie on the node's own row
    assert!(ports
        .iter()
        .all(|c| c.1 == rect.1 && c.0 >= rect.0 && c.0 < rect.0 + rect.2));
}

/// In one group a -> b and b -> a are two edges; on one segment each end gets an arrow
#[test]
fn bidirectional_edge_has_arrows_both_ends() {
    // Straight line: the forward end faces east, the return end faces west
    let mut cv = Canvas::new(
        (0, 0, 20, 10),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (true, true));
    assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'), "箭头对着 (9,5)");
    assert_eq!(cv.arrow[cv.idx(1, 5).unwrap()], Some('<'), "箭头对着 (0,5)");
    // Same for a bent edge: each end is drawn along its own direction
    let mut cv = Canvas::new(
        (0, 0, 40, 20),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, true));
    assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "b 端竖着进来");
    assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "a 端横着回去");
    // Adjacent nodes (no cell between): no room for arrows, node glyphs win, none drawn
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

/// A backtracking via (down, then back up the same way) only adds a tail: drop it, go straight
#[test]
fn backtracking_via_is_dropped() {
    let mut cv = Canvas::new(
        (0, 0, 40, 24),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Source (5,6), target (25,6); via runs to (20,18) then back to (20,6)
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

/// A backtrack need not share a row/column with the next point: going out and back in that
/// direction is a dead tail (hongloumeng has one on 薛姨妈->薛宝琴, adding a sideways leg)
#[test]
fn diagonal_backtracking_via_dropped() {
    let mut cv = Canvas::new(
        (0, 0, 60, 24),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // Source (5,6) -> via (30,6) -> target (20,10): dash across then come back
    cv.path(
        &[(5, 6), (30, 6), (20, 10)],
        0,
        (255, 0, 0),
        EdgeStyle::Solid,
        (false, false),
        None,
        None,
    );
    let far = cv.idx(30, 6).unwrap();
    assert_eq!(cv.mask[far], 0, "折返 via 不该把线拉出去");
    let end = cv.idx(20, 10).unwrap();
    assert_ne!(cv.mask[end], 0, "线要画到目标");
}

/// Regular via points, always heading towards the target, are kept on the path
#[test]
fn forward_via_is_kept() {
    let mut cv = Canvas::new(
        (0, 0, 40, 24),
        vec![false; 1],
        0,
        crate::glyphs::Glyphs::UNICODE,
    );
    // An L: down to (20,18), right to (30,18), then up to the target
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
