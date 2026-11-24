use super::*;
use crate::document::{Box, Dir, DocEdge, Group, Text};

fn sample_document() -> Document {
    let g0 = Group {
        id: "g0".into(),
        label: "1".into(),
        color: (0xff, 0, 0),
    };
    let g1 = Group {
        id: "g1".into(),
        label: "2".into(),
        color: (0, 0xff, 0),
    };
    let layers = vec![Layer {
        name: String::new(),
        order: 0,
    }];
    let boxes = vec![
        Box {
            id: "b0".into(),
            at: (0, 0),
            size: (6, 3),
            style: BoxStyle::Rounded,
            color: None,
            fill: None,
            layer: String::new(),
            terms: vec!["jia".into()],
            edge: None,
        },
        Box {
            id: "b1".into(),
            at: (10, 0),
            size: (6, 3),
            style: BoxStyle::Rounded,
            color: None,
            fill: None,
            layer: String::new(),
            terms: vec!["yi".into()],
            edge: None,
        },
        Box {
            id: "b2".into(),
            at: (20, 0),
            size: (6, 3),
            style: BoxStyle::Rounded,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        },
    ];
    let texts = vec![
        Text {
            id: "t0".into(),
            text: "甲".into(),
            at: (1, 1),
            box_id: "b0".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        },
        Text {
            id: "t1".into(),
            text: "乙".into(),
            at: (11, 1),
            box_id: "b1".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        },
        Text {
            id: "t2".into(),
            text: "丙".into(),
            at: (21, 1),
            box_id: "b2".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        },
    ];
    let edges = vec![
        DocEdge {
            id: "e0".into(),
            from: "b0".into(),
            to: "b1".into(),
            dir: Dir::Forward,
            group: "g0".into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            layer: String::new(),
            corner: Corner::Rounded,
            from_side: None,
            to_side: None,
            head: None,
        },
        DocEdge {
            id: "e1".into(),
            from: "b2".into(),
            to: "b1".into(),
            dir: Dir::None,
            group: "g1".into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            layer: String::new(),
            corner: Corner::Rounded,
            from_side: None,
            to_side: None,
            head: None,
        },
    ];
    let view = ViewState {
        cursor: Some(crate::document::Cursor::Box("b0".into())),
        ..ViewState::default()
    };
    Document::from_parts(
        "graph-tui 0.2.0",
        vec![g0, g1],
        layers,
        boxes,
        texts,
        edges,
        view,
    )
    .expect("样例文档")
}

fn temp_path(tag: &str, ext: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!(
            "graph-tui-app-{tag}-{}-{nanos}.{ext}",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

/// Tests use the Chinese UI (the default is English; Chinese copy has its own case)
fn zh_opts() -> Options {
    Options {
        lang: Some(Lang::Zh),
        ..Options::default()
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn shift_by_one_syncs_text_and_round_trips() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.view.zoom = 1.0;
    app.jump_to(app.doc.box_idx("b0").unwrap());
    app.handle(ctrl(KeyCode::Char('l')));
    assert_eq!(app.doc.boxes[0].at, (1, 0), "框该右移一格");
    assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字跟着框走");
    let path = temp_path("roundtrip", "gtui");
    app.save_document(&path).expect("存盘");
    let body = std::fs::read_to_string(&path).expect("读回");
    let back = Document::load(&body).expect("解析");
    assert_eq!(back.boxes, app.doc.boxes);
    assert_eq!(back.texts, app.doc.texts);
    assert_eq!(back.edges, app.doc.edges);
    assert_eq!(back.groups, app.doc.groups);
    assert_eq!(back.layers, app.doc.layers);
    assert_eq!(back.view, app.doc.view);
    let _ = std::fs::remove_file(&path);
}

/// A click puts the cursor on the clicked cell (no snap to a node corner) and moves no box
#[test]
fn click_lands_cursor() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // Inside the box (middle row), not the border and not the text start
    let (x, y) = p.proj.cell((4.0, 1.0));
    let before = app.doc.boxes[0].at;
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    assert_eq!(app.view.cursor, (4, 1), "光标就是点到的那一格");
    assert_eq!(app.view.pick.box_at(), Some(0), "点到的还是这个框");
    assert_eq!(app.doc.boxes[0].at, before, "单击不改几何");
    assert!(app.view.sel.is_none(), "单击不起选区");
}

/// Once a selection is confirmed: another drag replaces
/// it; a click outside cancels, inside keeps it
#[test]
fn confirmed_selection_retargets_or_cancels() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));

    // First block: start from empty space and cover b0 (pressing on a box moves it)
    let (ax, ay) = at((-2, 8));
    let (bx, by) = at((5, 2));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    let first = app.view.sel.expect("第一块选区");
    assert!(first.confirmed);

    // Click inside: the selection stays (a cell inside the rect that is not on a box)
    let (cx, cy) = at((2, 4));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        cx as u16,
        cy as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        cx as u16,
        cy as u16,
    ));
    assert!(app.view.sel.is_some(), "点选区里面不该取消");

    // Click outside: cancels
    // Inside the map but far from the selection (do not hit the status line)
    let (ox, oy) = at((30, 9));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        ox as u16,
        oy as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        ox as u16,
        oy as u16,
    ));
    assert!(app.view.sel.is_none(), "点选区外面该取消");

    // Drag another block: it replaces the selection (the old one must not get in the way)
    let (dx, dy) = at((10, 8));
    let (ex, ey) = at((16, 2));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        dx as u16,
        dy as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        ex as u16,
        ey as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        ex as u16,
        ey as u16,
    ));
    let second = app.view.sel.expect("第二块选区");
    assert!(second.confirmed, "松手还是即确认");
    assert_eq!(second.anchor, (10, 8), "锚点换成了新按下的地方");
    assert_ne!(second.anchor, first.anchor, "不是旧的那一块");
}

/// A selection dragged from empty space is confirmed
/// on release and can be edited right away (no Enter)
#[test]
fn drag_from_empty_confirms_immediately() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // Start from empty space (pressing on a box moves it)
    let (ax, ay) = p.proj.cell((-2.0, 8.0));
    let (bx, by) = p.proj.cell((5.0, 2.0));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    let sel = app.view.sel.expect("该有选区");
    assert!(sel.confirmed, "松手即确认");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("已确认"), "回显要说一声:{note}");
    // Editing works straight away, not blocked by "press Enter to confirm first"
    app.cmd_apply("box double");
    assert!(
        app.doc.boxes[0].style == BoxStyle::Double,
        "拖完的选区该能直接改:{}",
        app.view.note.clone().unwrap_or_default()
    );
}

/// Two edges on one cell: repeated clicks there cycle between them (otherwise one is unreachable)
#[test]
fn edges_stacked_on_one_cell_cycle() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    // Give both edges the same manual via point: they share a path and overlap
    app.doc.edges[0].via = vec![(8, 8)];
    app.doc.edges[1].via = vec![(8, 8)];
    app.doc.rebuild();
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // Find a cell that holds two edges and is not on a box
    let cell = app
        .hit
        .candidates
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(c, _)| *c)
        .find(|c| render::node_at(&app.doc, &p, app.area, c.0 as u16, c.1 as u16).is_none())
        .expect("该有一格压着两条边");
    let (cx, cy) = cell;
    let cands = app.hit.candidates_at(cell).to_vec();
    assert!(cands.len() > 1, "这一格该压着两条边:{cands:?}");
    let at = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: cx as u16,
            row: cy as u16,
            modifiers: KeyModifiers::NONE,
        })
    };
    app.handle(at(MouseEventKind::Down(MouseButton::Left)));
    let first = app.view.pick.edge().expect("先选到一条");
    app.handle(at(MouseEventKind::Up(MouseButton::Left)));
    app.handle(at(MouseEventKind::Down(MouseButton::Left)));
    let second = app.view.pick.edge().expect("再点一下选到下一条");
    assert_ne!(first, second, "连点同一格该在两条边之间轮");
}

/// Double click on empty space = `i`: opens the new node prompt; on a node = edit text
#[test]
fn double_click_on_empty_creates_node() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // Empty space: the sample boxes sit in the top two
    // rows, the rest is empty (avoid the status line)
    let (x, y) = p.proj.cell((8.0, 8.0));
    assert!(y < (p.map.y + p.map.height) as i32, "测试点该在地图里");
    let click = |app: &mut App| {
        app.handle(mouse(
            MouseEventKind::Down(MouseButton::Left),
            x as u16,
            y as u16,
        ));
        app.handle(mouse(
            MouseEventKind::Up(MouseButton::Left),
            x as u16,
            y as u16,
        ));
    };
    click(&mut app);
    assert!(app.view.prompt.is_none(), "单击不该弹输入框");
    click(&mut app);
    assert!(
        matches!(app.view.prompt, Some((Prompt::NewBox, _))),
        "双击空处该和按 i 一样弹新建:{:?}",
        app.view.prompt.is_some()
    );
    assert_eq!(app.view.cursor, p.proj.canvas(x, y), "光标落在双击那一格");
}

/// After everything in the selection is deleted the empty
/// selection cancels itself (same as a click outside)
#[test]
fn selection_emptied_after_delete_cancels() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
    // Drag a block from empty space that covers b0
    let (ax, ay) = at((-2, 8));
    let (bx, by) = at((5, 2));
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    assert!(app.view.sel.is_some(), "先有一块选区");

    // Delete the boxes in the selection: the selection disappears
    app.handle(key(KeyCode::Char('d')));
    app.draw(&mut buf, app.area);
    assert!(app.view.sel.is_none(), "选区空了就该自己取消");
    assert_eq!(app.doc.boxes.len(), 2, "只删掉被选中的那个框");
}

/// Drag on a node moves the box (the whole drag is one undo step); empty space starts a selection
#[test]
fn dragging_node_moves_box() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // Start the drag inside the box: still a box move (only a border drag starts a link)
    let (x, y) = p.proj.cell((4.0, 1.0));
    let before = app.doc.boxes[0].at;
    let undo_before = app.undo.counts();
    app.handle(mouse(
        MouseEventKind::Down(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    assert!(app.view.sel.is_none(), "按下不该起选区");
    app.handle(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        (x + 2) as u16,
        (y + 1) as u16,
    ));
    app.handle(mouse(
        MouseEventKind::Up(MouseButton::Left),
        (x + 2) as u16,
        (y + 1) as u16,
    ));
    assert_eq!(
        app.doc.boxes[0].at,
        (before.0 + 2, before.1 + 1),
        "拖了两格就该挪两格"
    );
    assert!(app.view.sel.is_none(), "拖框不该留下选区");
    assert_eq!(app.undo.counts().0, undo_before.0 + 1, "整段拖动算一步");
    app.undo_step();
    assert_eq!(app.doc.boxes[0].at, before, "一步撤销回到原位");
}

/// `:e` with no argument: opens the file picker, filter then Enter opens
#[test]
fn colon_e_without_arg_picks_file() {
    let one = temp_path("pick-one", "gtui");
    let two = temp_path("pick-two", "gtui");
    let body = sample_document().save();
    std::fs::write(&one, &body).expect("写 gtui");
    std::fs::write(&two, &body).expect("写 gtui");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = Some(one.clone());
    // :e with no argument
    for c in ":e".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(key(KeyCode::Enter));
    let pick = app.view.file_pick.as_ref().expect("该弹出选择浮层");
    assert!(pick.items.len() >= 2, "该列出候选:{:?}", pick.items);
    // Type a filter (the file name itself) to narrow down to one candidate
    let stem = std::path::Path::new(&two)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    for c in stem.chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.file_pick.as_ref().unwrap().shown().len(), 1);
    // Enter opens
    app.handle(key(KeyCode::Enter));
    assert!(app.view.file_pick.is_none(), "打开后该关掉浮层");
    assert_eq!(app.source.as_deref(), Some(two.as_str()));
    assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
    let _ = std::fs::remove_file(&one);
    let _ = std::fs::remove_file(&two);
}

/// Spec: at a zoom other than 1:1 the verb switches to 1:1 and then runs, it is not refused
#[test]
fn edit_verbs_return_to_unity_zoom_first() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 2.0;
    app.handle(ctrl(KeyCode::Char('l')));
    assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
    assert_eq!(app.doc.boxes[0].at, (1, 0), "切回 1:1 之后这一下就生效");
    assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字同量平移");
    assert!(app.view.note.is_some(), "该给一句回显");
}

/// The ray stops on an edge it hits (an edge is an item; no pass-through to the box behind it)
#[test]
fn w_stops_on_edge() {
    let mut d = row_document(3);
    // A vertical edge between box 0 and box 1 crossing the text row y=1
    d.boxes.push(crate::document::Box {
        id: "v1".into(),
        at: (4, -6),
        size: (1, 1),
        style: BoxStyle::Point,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    });
    d.boxes.push(crate::document::Box {
        id: "v2".into(),
        at: (4, 6),
        size: (1, 1),
        style: BoxStyle::Point,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    });
    d.texts.push(crate::document::Text {
        id: "tv1".into(),
        text: "a".into(),
        at: (4, -6),
        box_id: "v1".into(),
        color: None,
        bg: None,
        align: crate::model::Align::Left,
        layer: String::new(),
    });
    d.texts.push(crate::document::Text {
        id: "tv2".into(),
        text: "b".into(),
        at: (4, 6),
        box_id: "v2".into(),
        color: None,
        bg: None,
        align: crate::model::Align::Left,
        layer: String::new(),
    });
    let g = d.groups[0].id.clone();
    d.edges.push(crate::document::DocEdge {
        id: "ev".into(),
        from: "v1".into(),
        to: "v2".into(),
        dir: Dir::None,
        group: g,
        style: EdgeStyle::Solid,
        color: None,
        bg: None,
        via: Vec::new(),
        layer: String::new(),
        corner: Corner::Rounded,
        from_side: None,
        to_side: None,
        head: None,
    });
    d.rebuild();
    let mut app = app_with_links(d);
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.handle(key(KeyCode::Char('w')));
    assert!(
        app.view.cursor.0 < 8,
        "该停在中间的边上,而不是穿到下一个框:{:?}",
        app.view.cursor
    );
    assert!(app.view.pick.edge().is_some(), "停在边上就该选中那条边");
}

/// Nothing on that line (table layout): step aside and take the nearest box in that direction
#[test]
fn w_falls_back_to_nearest_box_on_empty_row() {
    let mut d = row_document(3);
    // Move the third box to the next row: scanning y=1 hits nothing
    d.boxes[2].at = (40, 8);
    for t in d.texts.iter_mut() {
        if t.box_id == "b2" {
            t.at = (41, 9);
        }
    }
    d.rebuild();
    let mut app = app_with_links(d);
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.handle(key(KeyCode::Char('w')));
    assert_ne!(app.view.cursor, app.doc.text(0).unwrap().at, "得往前走");
    // The second w starts from box 1, which has no box to its right: take the nearest one (40,8)
    app.handle(key(KeyCode::Char('w')));
    assert_eq!(
        render::box_at(&app.doc, app.view.cursor, &app.view),
        Some(2),
        "射线没撞到东西时取最近的框:{:?}",
        app.view.cursor
    );
}

/// Cursor in empty space: when the main axis is empty the cross axis direction still hops
/// (the tutorial's hello box hanging on the left is
/// like this; the main axis alone never reaches it)
#[test]
fn b_jumps_to_box_on_cross_axis() {
    let mut d = row_document(3);
    // Stack the three boxes vertically and hang an isolated box on the left
    for (k, b) in d.boxes.iter_mut().enumerate() {
        b.at = (0, (k as i32) * 12);
    }
    for t in d.texts.iter_mut() {
        if let Some(k) = t
            .box_id
            .strip_prefix('b')
            .and_then(|s| s.parse::<i32>().ok())
        {
            t.at = (1, k * 12 + 1);
        }
    }
    d.boxes.push(crate::document::Box {
        id: "hello".into(),
        at: (-30, 12),
        size: (7, 3),
        style: BoxStyle::Rounded,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    });
    d.texts.push(crate::document::Text {
        id: "th".into(),
        text: "hello".into(),
        at: (-29, 13),
        box_id: "hello".into(),
        color: None,
        bg: None,
        align: crate::model::Align::Left,
        layer: String::new(),
    });
    d.rebuild();
    let mut app = app_with_links(d);
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Cursor right of hello, below the top row boxes
    app.view.cursor = (-15, 10);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('b')));
    assert_eq!(
        render::box_at(&app.doc, app.view.cursor, &app.view).map(|i| app.doc.boxes[i].id.as_str()),
        Some("hello"),
        "该跳到左边的 hello 框:{:?}",
        app.view.cursor
    );
}

/// Cursor in a box: shoot along the row first, so w is not stolen by a nearer box below
#[test]
fn w_scans_current_row_first() {
    let mut d = row_document(2);
    d.boxes[1].at = (8, 0);
    // A nearer box below (no horizontal gap, 4 rows down)
    d.boxes.push(crate::document::Box {
        id: "down".into(),
        at: (0, 4),
        size: (6, 3),
        style: BoxStyle::Rounded,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    });
    d.texts.push(crate::document::Text {
        id: "td".into(),
        text: "下".into(),
        at: (1, 5),
        box_id: "down".into(),
        color: None,
        bg: None,
        align: crate::model::Align::Left,
        layer: String::new(),
    });
    d.rebuild();
    let mut app = app_with_links(d);
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.handle(key(KeyCode::Char('w')));
    assert_eq!(
        render::box_at(&app.doc, app.view.cursor, &app.view).map(|i| app.doc.boxes[i].id.as_str()),
        Some("b1"),
        "该沿这一行往右走,不该拐到下面:{:?}",
        app.view.cursor
    );
}

/// w is not limited to the viewport: an off-screen target is still jumped to and the view is pushed
#[test]
fn w_jumps_to_box_off_screen() {
    let mut d = row_document(3);
    d.boxes[2].at = (200, 0);
    for t in d.texts.iter_mut() {
        if t.box_id == "b2" {
            t.at = (201, 1);
        }
    }
    d.rebuild();
    let mut app = app_with_links(d);
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    let p = render::panels(&app.doc, app.area, &app.view);
    // The third box is off screen
    let cell = app.doc.text(2).unwrap().at;
    let screen = p.proj.cell((cell.0 as f64, cell.1 as f64));
    assert!(
        screen.0 < p.map.x as i32 || screen.0 >= (p.map.x + p.map.width) as i32,
        "这个框该在画面外:{screen:?}"
    );
    let center0 = app.view.center;
    // First w: to the second box
    app.handle(key(KeyCode::Char('w')));
    assert!(app.view.cursor.0 < 100, "先到可见的下一个框");
    // Second w: jump to the off-screen third box, the view follows
    app.handle(key(KeyCode::Char('w')));
    assert_eq!(
        render::box_at(&app.doc, app.view.cursor, &app.view),
        Some(2),
        "该跳到画面外那个框上:{:?}",
        app.view.cursor
    );
    assert!(
        app.view.target.is_some() || app.view.center != center0,
        "视图要跟着推过去"
    );
    let note = app.view.note.clone().unwrap_or_default();
    assert!(!note.contains("没有"), "不该报没东西:{note}");
}

/// i: with a selection edit text in bulk; only empty space creates
/// w / b: on a row layout w goes right, b left, hopping
/// from item to item; at the end a note, no move
#[test]
fn w_b_ray_hops_to_items() {
    let mut app = app_with_links(row_document(3));
    app.jump_to(0);
    let start = app.view.cursor;
    // Box 0 spans (0,0)-(5,2), the next item is to the right
    app.handle(key(KeyCode::Char('w')));
    assert!(
        app.view.cursor.0 > start.0,
        "w 该往右走:{start:?} -> {:?}",
        app.view.cursor
    );
    let first = app.view.cursor;
    // Press again to continue right, never staying put
    app.handle(key(KeyCode::Char('w')));
    assert!(app.view.cursor.0 > first.0, "第二次 w 还要往前走");
    // At the end: a note, the cursor stays
    let at_end = app.view.cursor;
    app.handle(key(KeyCode::Char('w')));
    assert_eq!(app.view.cursor, at_end, "没东西可跳时光标不动");
    assert!(
        app.view
            .note
            .clone()
            .unwrap_or_default()
            .contains("前面没有"),
        "{:?}",
        app.view.note
    );
    // b walks back
    app.handle(key(KeyCode::Char('b')));
    assert!(app.view.cursor.0 < at_end.0, "b 该往左走");
    // Keep pressing b to the far left, then a note
    for _ in 0..20 {
        app.handle(key(KeyCode::Char('b')));
    }
    assert!(
        app.view
            .note
            .clone()
            .unwrap_or_default()
            .contains("后面没有"),
        "{:?}",
        app.view.note
    );
}

/// S: split the box between the cursor row and the next
/// row; last row / point node / annotation box refuse
#[test]
fn split_below_cursor_row() {
    let mut app = app_with_links(row_document(1));
    let i = 0usize;
    app.doc.set_text(i, "a\nb\nc");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let at = app.doc.text(i).unwrap().at;
    app.view.cursor = at;
    app.refresh_pick();
    let n0 = app.doc.boxes.len();
    let undo = app.undo.counts().0;
    app.handle(key(KeyCode::Char('S')));
    assert_eq!(app.doc.boxes.len(), n0 + 1, "拆出一个新框");
    assert_eq!(app.doc.text(i).unwrap().text, "a", "第 0 行留在原框");
    let new = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.text(new).unwrap().text, "b\nc", "其余进新框");
    assert!(
        app.doc.boxes[new].at.0 > app.doc.boxes[i].at.0,
        "新框在右边"
    );
    assert_eq!(app.undo.counts().0, undo + 1, "一步撤销");
    assert!(app.doc.edges.is_empty(), "新框不连边");

    // Cursor on the last row: a note, no undo step
    let undo = app.undo.counts().0;
    let n0 = app.doc.boxes.len();
    app.view.cursor = (at.0, at.1);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('S')));
    assert_eq!(app.undo.counts().0, undo, "没东西可拆就不压撤销");
    assert_eq!(app.doc.boxes.len(), n0);
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("最后一行"));
}

/// M: merge the selected boxes: text joined in reading
/// order, internal edges dropped, boundary edges relinked
#[test]
fn merge_selected_boxes() {
    let mut app = app_with_links(row_document(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("内部边");
    app.doc
        .add_edge("b1", "b2", "g0", Dir::Forward)
        .expect("边界边");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Select the first two boxes
    app.view.cursor = (0, 0);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(key(KeyCode::Enter));
    let undo = app.undo.counts().0;
    app.handle(key(KeyCode::Char('M')));
    assert_eq!(app.doc.boxes.len(), 2, "两个框并成一个");
    let merged = app
        .doc
        .texts
        .iter()
        .find(|t| t.text == "n0\nn1")
        .expect("文字按阅读顺序接起来");
    assert_eq!(merged.box_id, "b1", "目标框 = 光标所在的选中框(b1)");
    assert_eq!(app.doc.edges.len(), 1, "内部边没了,边界边还在");
    assert_eq!(app.doc.edges[0].from, "b1", "边界边接在目标框上");
    assert_eq!(app.doc.edges[0].to, "b2");
    assert_eq!(app.undo.counts().0, undo + 1, "一步撤销");
    app.undo_step();
    assert_eq!(app.doc.boxes.len(), 3, "撤销能整个还原");
    assert_eq!(app.doc.edges.len(), 2);

    // Fewer than two boxes: only a note
    app.view.sel = None;
    app.jump_to(0);
    let undo = app.undo.counts().0;
    app.handle(key(KeyCode::Char('M')));
    assert_eq!(app.undo.counts().0, undo);
    assert!(app.view.note.clone().unwrap_or_default().contains("至少"));
}

/// y: copies without touching the document; boundary edges stay out of the clipboard
#[test]
fn copy_keeps_document_and_skips_boundary_edges() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    let (boxes, edges) = (app.doc.boxes.len(), app.doc.edges.len());
    let undo = app.undo.counts().0;
    app.handle(key(KeyCode::Char('y')));
    assert_eq!((app.doc.boxes.len(), app.doc.edges.len()), (boxes, edges));
    assert_eq!(app.undo.counts().0, undo, "y 不压撤销");
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 1);
    assert!(clip.edges.is_empty(), "边界边不进剪贴板");
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .starts_with("yanked 1 boxes"));

    // Cursor in empty space: a note
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('y')));
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有可复制"));
}

/// i only touches the box under the cursor: with a block selection it only notes and edits no text
#[test]
fn i_edits_only_box_under_cursor() {
    let mut app = app_with_links(row_document(2));
    app.view.sel = Some(RectSel {
        anchor: (-2, -2),
        end: (14, 5),
        confirmed: true,
    });
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    app.handle(key(KeyCode::Char('i')));
    assert!(app.view.edit.is_none(), "区块选择时不进编辑");
    assert!(app.view.note.clone().unwrap_or_default().contains("一个框"));
    let after: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    assert_eq!(before, after, "一个字都不改");

    // Without a selection: i edits the box under the cursor, other boxes stay put
    app.view.sel = None;
    app.jump_to(0);
    app.handle(key(KeyCode::Char('i')));
    let edit = app.view.edit.clone().expect("该进就地编辑");
    assert_eq!(edit.box_idx, 0);
    let _ = app;
}

/// Press i on a glyph cell: the cursor lands on that glyph, not on another glyph of the cell count
#[test]
fn i_targets_the_clicked_glyph() {
    let mut d = sample_document();
    d.texts[0].text = "贾a政\n一二三".into();
    let mut app = app_with_links(d);
    let t = app.doc.text(0).expect("有文字").clone();
    let mut at = 0usize;
    for (row, line) in t.text.split('\n').enumerate() {
        let mut col = 0usize;
        for (i, ch) in line.chars().enumerate() {
            let w = unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1);
            for k in 0..w {
                app.view.cursor = (t.at.0 + (col + k) as i32, t.at.1 + row as i32);
                assert_eq!(
                    app.caret_from_cursor(0),
                    Some(at + i),
                    "{ch:?} 的第 {k} 格(第 {row} 行第 {col} 列)该落到字符 {i}"
                );
            }
            col += w;
        }
        at += line.chars().count() + 1;
    }
}

/// Wide glyphs take two cells: the draft caret column and vertical moves count cells (like nvim)
#[test]
fn wide_glyph_caret_counts_cells() {
    // "贾a政": caret 2 = 贾 (2 cells) + a (1 cell)
    assert_eq!(App::caret_row_col("贾a政", 2), (0, 3));
    assert_eq!(App::caret_row_col("贾政", 1), (0, 2), "一个汉字走两格");
    assert_eq!(App::caret_row_col("贾政\nab", 5), (1, 2), "换行后归零");
    // Vertical move: back to the glyph boundary whose start does not exceed that cell
    assert_eq!(
        App::caret_vertical("一二\nabcd", 2, 1),
        7,
        "第一行 4 格 -> 第二行 4 格"
    );
    assert_eq!(
        App::caret_vertical("一二\nabcd", 1, 1),
        5,
        "落在宽字中间就退到字头"
    );
    assert_eq!(
        App::caret_vertical("一二\nabcd", 2, -1),
        2,
        "往上回到行尾位置"
    );
}

#[test]
fn i_edits_text_inline() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let n0 = app.doc.boxes.len();
    // Cursor on the first glyph: i edits in place with the cursor on that glyph (not at the end)
    let at = app.doc.text(0).unwrap().at;
    app.view.cursor = at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('i')));
    let edit = app.view.edit.clone().expect("该进就地编辑");
    assert_eq!(edit.box_idx, 0);
    assert_eq!(edit.text, "甲", "草稿从原文开始");
    assert_eq!(edit.caret, 0, "光标落在画布光标指到的那个字上");
    assert!(app.view.prompt.is_none(), "不弹底行输入框");
    // End to the end of the line, then type two characters
    app.handle(key(KeyCode::End));
    assert_eq!(app.view.edit.as_ref().map(|e| e.caret), Some(1));
    for c in "XY".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲XY")
    );
    assert_eq!(app.doc.text(0).unwrap().text, "甲", "提交前文档不变");
    app.handle(key(KeyCode::Esc));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.text(0).unwrap().text, "甲XY");
    assert_eq!(app.doc.boxes.len(), n0, "不该新建框");
    assert_eq!(app.undo.counts().0, 1, "改文字一步撤销");
    app.undo_step();
    assert_eq!(app.doc.text(0).unwrap().text, "甲");

    // Alt+Backspace: delete the word before the cursor, same as the command line / search bar
    app.handle(key(KeyCode::Char('i')));
    app.handle(key(KeyCode::End));
    for c in " alpha beta".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲 alpha ")
    );
    assert_eq!(app.view.edit.as_ref().map(|e| e.caret), Some(8));
    // At the start of the line nothing is deleted
    app.handle(key(KeyCode::Home));
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲 alpha ")
    );

    // Ctrl-C cancels: the document is unchanged
    app.handle(key(KeyCode::Char('i')));
    app.handle(key(KeyCode::Char('X')));
    app.handle(ctrl(KeyCode::Char('c')));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.text(0).unwrap().text, "甲", "取消不写回");
    assert!(app.view.note.clone().unwrap_or_default().contains("取消"));

    // Newline: Enter inserts one, the box grows after the commit
    app.handle(key(KeyCode::Char('i')));
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Char('乙')));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.doc.text(0).unwrap().text, "甲\n乙");
    assert!(app.doc.boxes[0].size.1 >= 2, "框跟着长高");

    // i on empty space: create the box and enter editing; Ctrl-C undoes the whole box
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    let n1 = app.doc.boxes.len();
    app.handle(key(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "空白处也进就地编辑");
    assert_eq!(app.doc.boxes.len(), n1 + 1, "先建了一个空框");
    app.handle(ctrl(KeyCode::Char('c')));
    assert_eq!(app.doc.boxes.len(), n1, "取消时新建的框一起撤掉");

    // i on empty space, type a few characters then Esc: the box holds those characters
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('i')));
    for c in "hello".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(key(KeyCode::Esc));
    assert_eq!(
        app.doc.texts.last().map(|t| t.text.as_str()),
        Some("hello"),
        "新框里是刚打的字"
    );
}

/// Moving the cursor onto an edge stroke selects that edge (it survives sync_edge_sel)
#[test]
fn cursor_on_edge_selects_edge() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Find a visible cell of an edge that maps back into no box
    let p = render::panels(&app.doc, app.area, &app.view);
    let mut target = None;
    for (e, cells) in app.hit.edge_cells.iter() {
        for c in cells {
            let canvas = p.proj.canvas(c.0, c.1);
            if app.doc.box_at_cell(canvas).is_none() {
                target = Some((*e, canvas));
                break;
            }
        }
        if target.is_some() {
            break;
        }
    }
    let (e, cell) = target.expect("样例该有边笔画");
    app.view.cursor = cell;
    app.refresh_pick();
    assert!(
        app.view.pick.edge().is_some(),
        "光标在边上该选中边(不一定是 {e},同格可能有多条)"
    );
    let picked = app.view.pick;
    // Press a key that does not move the cursor: the selection must survive
    app.handle(key(KeyCode::Char('B')));
    assert_eq!(app.view.pick, picked, "按键后仍选中同一条边");
    // One cell away is no longer that edge
    app.handle(key(KeyCode::Left));
    app.handle(key(KeyCode::Left));
    app.handle(key(KeyCode::Up));
    app.handle(key(KeyCode::Up));
    assert_ne!(app.view.pick, picked, "移开就该换选中项");
}

/// n boxes in a row, one group, no edges
fn row_document(n: usize) -> Document {
    let groups = vec![Group {
        id: "g0".into(),
        label: "线".into(),
        color: (0x4c, 0x8e, 0xf8),
    }];
    let layers = vec![Layer {
        name: String::new(),
        order: 0,
    }];
    let boxes: Vec<Box> = (0..n)
        .map(|i| Box {
            id: format!("b{i}"),
            at: ((i as i32) * 8, 0),
            size: (6, 3),
            style: BoxStyle::Rounded,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        })
        .collect();
    let texts: Vec<Text> = (0..n)
        .map(|i| Text {
            id: format!("t{i}"),
            text: format!("n{i}"),
            at: ((i as i32) * 8 + 1, 1),
            box_id: format!("b{i}"),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        })
        .collect();
    Document::from_parts(
        "graph-tui 0.2.0",
        groups,
        layers,
        boxes,
        texts,
        Vec::new(),
        ViewState::default(),
    )
    .expect("横排文档")
}

fn app_with_links(doc: Document) -> App {
    let mut app = App::new(doc, zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 30);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app
}

/// Sources and targets set directly, then link_commit (the unit-test entry for the batch rules)
fn connect(app: &mut App, sources: Vec<usize>, targets: Vec<usize>) {
    app.view.link = Some(LinkState {
        sources,
        targets,
        confirming: false,
    });
    app.link_commit();
}

fn edge_names(app: &App) -> Vec<(String, String)> {
    app.doc
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect()
}

#[test]
fn link_one_to_one() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    assert!(app.view.link.is_some(), "该进入连边模式");
    app.jump_to(1);
    app.handle(key(KeyCode::Enter));
    assert_eq!(edge_names(&app), vec![("b0".into(), "b1".into())]);
    assert!(app.view.link.is_none(), "建完退出连边模式");
    assert_eq!(app.doc.edges[0].dir, Dir::Forward);
}

/// Space equals Enter: confirm an unconfirmed selection; z centres
#[test]
fn space_confirms_selection_and_z_centres() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    // Drag an unconfirmed selection
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(2);
    app.handle(key(KeyCode::Char('v')));
    let sel = app.view.sel.expect("该有选区");
    assert!(!sel.confirmed, "v 拉出来的选区还没确认");
    app.handle(key(KeyCode::Char(' ')));
    assert!(
        app.view.sel.map(|s| s.confirmed).unwrap_or(false),
        "空格该把选区确认掉(和 Enter 一样)"
    );
    // z centres: the view centre moves to the cursor cell
    app.view.sel = None;
    app.view.center = None;
    app.handle(key(KeyCode::Char('z')));
    assert!(app.view.center.is_some(), "z 该把视图中心挪到光标上");
}

/// In link mode Space behaves like Enter: confirm and build the edge
#[test]
fn link_space_confirms_too() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    assert!(app.view.link.is_some());
    app.jump_to(1);
    app.handle(key(KeyCode::Char(' ')));
    assert_eq!(edge_names(&app), vec![("b0".into(), "b1".into())]);
    assert!(app.view.link.is_none(), "建完退出连边模式");
}

#[test]
fn link_one_to_many() {
    let mut app = app_with_links(row_document(4));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    // Target: v over 1, 2, 3
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(3);
    app.handle(key(KeyCode::Enter));
    let made = edge_names(&app);
    // v from b0; the selection contains the source, so the self loop b0->b0 is built too
    assert_eq!(made.len(), 4);
    for (from, to) in made {
        assert_eq!(from, "b0");
        assert!(to == "b0" || to == "b1" || to == "b2" || to == "b3", "{to}");
    }
}

#[test]
fn link_many_to_one() {
    let mut app = app_with_links(row_document(4));
    // Sources: the selection 0, 1, 2
    app.view.cursor = app.cell_of(0);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(2);
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Char('e')));
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1, 2]);
    app.jump_to(3);
    app.handle(key(KeyCode::Enter));
    assert_eq!(edge_names(&app).len(), 3);
    assert!(app.doc.edges.iter().all(|e| e.to == "b3"));
}

#[test]
fn link_many_to_many() {
    let mut app = app_with_links(row_document(5));
    connect(&mut app, vec![0, 1], vec![2, 3, 4]);
    assert_eq!(edge_names(&app).len(), 6, "2x3 该建 6 条");
}

#[test]
fn link_self_loops_and_duplicates() {
    let mut app = app_with_links(row_document(2));
    connect(&mut app, vec![0, 1], vec![0, 1]);
    // Four pairs: two self loops (b1->b1, b2->b2) plus 0->1 and 1->0
    assert_eq!(edge_names(&app).len(), 4, "自环也该建出来");
    assert_eq!(
        app.doc.edges.iter().filter(|e| e.from == e.to).count(),
        2,
        "两条自环"
    );
    // Again: all duplicates
    let undo0 = app.undo.counts().0;
    connect(&mut app, vec![0], vec![1]);
    assert_eq!(edge_names(&app).len(), 4, "重复的不再加");
    assert_eq!(app.undo.counts().0, undo0, "一条没建不压撤销");
}

#[test]
fn link_batch_is_one_undo_step() {
    let mut app = app_with_links(row_document(5));
    let undo0 = app.undo.counts().0;
    connect(&mut app, vec![0, 1], vec![2, 3, 4]);
    assert_eq!(app.doc.edges.len(), 6);
    assert_eq!(app.undo.counts().0, undo0 + 1, "整批只算一步");
    app.undo_step();
    assert!(app.doc.edges.is_empty(), "撤一次全部消失");
}

#[test]
fn link_esc_cancels_without_change() {
    let mut app = app_with_links(row_document(3));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    app.jump_to(2);
    app.handle(key(KeyCode::Esc));
    assert!(app.view.link.is_none());
    assert!(app.doc.edges.is_empty(), "取消不该建边");
}

#[test]
fn link_confirms_unconfirmed_source_selection() {
    let mut app = app_with_links(row_document(3));
    app.view.cursor = app.cell_of(0);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(1);
    // e pressed without Enter
    app.handle(key(KeyCode::Char('e')));
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1]);
    assert!(app.view.sel.is_none(), "源选区交给目标用");
}

#[test]
fn link_builds_with_unconfirmed_target_selection() {
    let mut app = app_with_links(row_document(4));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(2);
    // Target selection unconfirmed; Enter confirms and
    // links (selection contains the source: one extra loop)
    app.handle(key(KeyCode::Enter));
    assert_eq!(edge_names(&app).len(), 3);
}

#[test]
fn link_target_stage_v_keeps_source() {
    let mut app = app_with_links(row_document(4));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(3);
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0]);
}

#[test]
fn link_large_count_needs_second_confirm() {
    let mut app = app_with_links(row_document(20));
    // 8 sources x 9 targets = 72: the first Enter only enters the confirm state
    connect(&mut app, (0..8).collect(), (9..18).collect());
    assert!(app.doc.edges.is_empty(), "第一次 Enter 不建");
    let link = app.view.link.clone().expect("还在连边模式");
    assert!(link.confirming);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("再按 Enter"), "{note}");
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.edges.len(), 72);
}

#[test]
fn link_over_limit_is_refused() {
    let mut app = app_with_links(row_document(60));
    connect(&mut app, (0..30).collect(), (30..50).collect());
    assert!(app.doc.edges.is_empty(), "600 条超上限,一条都不建");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("512"), "{note}");
    assert!(app.view.link.is_some(), "还在连边模式等缩小选区");
}

#[test]
fn link_group_follows_focus_or_source() {
    // In the sample b0 and b2 belong to g0; focusing g1 makes everything use g1
    let mut app = app_with_links(sample_document());
    app.view.focus = vec![1];
    connect(&mut app, vec![0], vec![2]);
    assert_eq!(app.doc.edges.last().unwrap().group, "g1");
    // No focus: use the source box's own first group
    let mut app = app_with_links(sample_document());
    connect(&mut app, vec![0], vec![2]);
    assert_eq!(app.doc.edges.last().unwrap().group, "g0");
}

#[test]
fn link_shift_click_adds_target() {
    let mut app = app_with_links(row_document(4));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    let p = render::panels(&app.doc, app.area, &app.view);
    let at: Vec<(u16, u16)> = (0..app.doc.boxes.len())
        .map(|i| {
            let (x, y) = p.proj.cell(app.doc.center(i));
            (x as u16, y as u16)
        })
        .collect();
    let click = |app: &mut App, i: usize, shift: bool| {
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: at[i].0,
            row: at[i].1,
            modifiers: if shift {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            },
        }));
    };
    // Plain click: the target becomes this one
    click(&mut app, 2, false);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
    // Shift click: adds it
    click(&mut app, 3, true);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2, 3]);
    // Shift click again: removes it
    click(&mut app, 3, true);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
}

/// With edges in the clipboard the paste lands where it would without them (edges must not drag it)
#[test]
fn paste_position_ignores_edges() {
    // b top-left, a bottom-right, an edge between them;
    // deleting a cuts that edge into the clipboard
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加一条边");
    app.doc.move_box(1, -20, 10);
    app.doc.move_box(0, 20, 0);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let a = app.doc.box_idx("b0").unwrap();
    app.jump_to(a);
    // d = cut with edges: the edge leaves the document, but it is a boundary edge (other end alive)
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(1),
        0,
        "边界边不进剪贴板"
    );
    app.view.cursor = (20, 0);
    app.handle(key(KeyCode::Char('p')));
    let pasted = app
        .doc
        .boxes
        .iter()
        .find(|b| b.id != "b0" && b.id != "b1")
        .expect("该粘回来一个框");
    assert_eq!(pasted.at, (20, 0), "粘贴落点该是光标处,和边无关");
}

#[test]
fn link_relocates_source_by_id_after_delete() {
    let mut app = app_with_links(row_document(3));
    app.jump_to(2);
    app.handle(key(KeyCode::Char('e'))); // source = b2
    let src = app.doc.boxes[2].id.clone();
    app.view.cursor = app.cell_of(0);
    app.refresh_pick();
    // Source box deleted: link mode must exit on its own
    app.doc.remove_box(&src).expect("删源");
    app.doc.rebuild();
    app.handle(key(KeyCode::Enter));
    assert!(app.view.link.is_none(), "源都没了,连边该退出");
}

/// d puts related edges in the clipboard, x only boxes
/// and text; both remove the edges from the document
#[test]
fn d_cut_carries_related_edges() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.edges.len(), 0, "关联边要一起删,不能留悬空");
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 1);
    assert_eq!(clip.texts.len(), 1);
    assert!(clip.edges.is_empty(), "边界边(另一头还在文档里)不进剪贴板");
    // x is no longer a binding
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('x')));
    assert!(app.clipboard.is_none(), "x 不该再剪切任何东西");
}

/// x pastes without edges; d pastes with edges
#[test]
fn paste_drops_boundary_edges() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    app.handle(key(KeyCode::Char('p')));
    assert_eq!(app.doc.edges.len(), 0, "边界边粘不出来(另一端不在片段里)");
}

/// Repeated p: three copies never overlap exactly and all ids differ
#[test]
fn repeated_paste_offsets_and_keeps_ids_unique() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    let mut seen: Vec<(i32, i32)> = Vec::new();
    for _ in 0..3 {
        app.handle(key(KeyCode::Char('p')));
        let b = app.doc.boxes.last().expect("粘出来的框");
        assert!(
            !seen.contains(&b.at),
            "第 {} 份和前面重叠了",
            seen.len() + 1
        );
        seen.push(b.at);
    }
    let ids: std::collections::HashSet<&str> =
        app.doc.boxes.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids.len(), app.doc.boxes.len(), "id 该各不相同");
}

/// The first copy lands exactly on the cursor and the selection points at it
#[test]
fn first_paste_lands_on_cursor_and_is_selected() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    app.view.cursor = (40, 12);
    app.handle(key(KeyCode::Char('p')));
    let i = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[i].at, (40, 12), "第一份该精确落在光标处");
    assert_eq!(app.view.pick, render::Pick::Box(i), "该选中刚粘的那份");
    assert_eq!(
        app.view.cursor,
        app.cell_of(i),
        "光标该落到刚粘的那份上(下一次命令默认动它)"
    );
}

/// Deep copy: editing one does not affect the others, via points are per copy
#[test]
fn paste_is_a_deep_copy() {
    // Three boxes, cut the first two: b0-b1 is internal (clipboard), b1-b2 boundary (document only)
    let mut app = app_with_links(row_document(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.doc
        .add_edge("b1", "b2", "g0", Dir::Forward)
        .expect("再加一条");
    app.view.sel = Some(RectSel {
        anchor: (-2, -2),
        end: (14, 5),
        confirmed: true,
    });
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), 1, "只剩第三个框");
    assert_eq!(app.doc.edges.len(), 0, "边界边一起删掉");
    app.handle(key(KeyCode::Char('p')));
    app.handle(key(KeyCode::Char('p')));
    assert_eq!(app.doc.edges.len(), 2, "两次粘贴各带一条边");
    // Move the via point of the first edge, the second is unaffected
    let via = vec![(99, 99)];
    app.doc.edges[0].via = via.clone();
    app.doc.rebuild();
    let other = app
        .doc
        .edges
        .iter()
        .enumerate()
        .find(|(i, _)| *i != 0)
        .map(|(_, e)| e.via.clone())
        .unwrap();
    assert_ne!(other, via, "via 该各自独立");
    // The clipboard is unaffected by later edits
    let clip = app.clipboard.clone().unwrap();
    assert!(clip.edges[0].via.is_empty(), "剪贴板保持删除时的样子");
}

/// Main regression: delete the third copy's edge, the first two keep theirs (no wrong index)
#[test]
fn deleting_one_copy_keeps_other_edges() {
    // Two boxes plus the edge between them: internal, so it goes to the clipboard
    let mut app = app_with_links(row_document(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.view.sel = Some(RectSel {
        anchor: (-2, -2),
        end: (14, 5),
        confirmed: true,
    });
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.view.sel, None);
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(0),
        1,
        "内部边进剪贴板"
    );
    for _ in 0..3 {
        app.handle(key(KeyCode::Char('p')));
    }
    assert_eq!(app.doc.edges.len(), 3, "三份各带一条边(边界边已经剪掉)");
    // Delete the copy pasted last:
    let last = app.doc.boxes.len() - 1;
    app.view.cursor = app.cell_of(last);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.edges.len(), 2, "只删掉那一份的边");
}

/// After the document changed, edge indices from an old hit table can no longer drive selection
#[test]
fn stale_hit_table_cannot_select_across_edits() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area); // build a hit table
    assert_eq!(app.hit.rev, app.doc.rev(), "命中表和文档同版");
    // change the document (paste a box)
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    app.handle(key(KeyCode::Char('p')));
    assert_ne!(app.hit.rev, app.doc.rev(), "旧命中表该作废");
    assert!(app.usable_hit().candidates.is_empty(), "作废后不返回候选");
}

/// Overlapping boxes: the topmost wins and E cycles between them
#[test]
fn overlapping_boxes_pick_topmost_and_cycle() {
    let mut app = app_with_links(row_document(2));
    // Move the second box to exactly cover the first
    let at0 = app.doc.boxes[0].at;
    app.doc.move_box(
        1,
        at0.0 - app.doc.boxes[1].at.0,
        at0.1 - app.doc.boxes[1].at.1,
    );
    let cell = (at0.0 + 1, at0.1 + 1);
    assert_eq!(app.doc.box_at_cell(cell), Some(1), "重叠时该取后画的那个");
    let stack = app.doc.boxes_at_cell(cell);
    assert_eq!(stack, vec![1, 0], "从上到下");
    app.view.cursor = cell;
    app.refresh_pick();
    app.handle(ctrl(KeyCode::Char('e')));
    let first = app.view.pick.box_at().expect("该选中一个框");
    app.handle(ctrl(KeyCode::Char('e')));
    let second = app.view.pick.box_at().expect("该选中另一个框");
    assert_ne!(first, second, "E 该在重叠的框之间轮");
}

/// Delete two edges in a row without a redraw: the second must not hit a shifted index
#[test]
fn consecutive_edge_deletes_keep_indices() {
    let mut app = app_with_links(row_document(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("e1");
    app.doc
        .add_edge("b1", "b2", "g0", Dir::Forward)
        .expect("e2");
    app.jump_to(1); // stop on the middle box, which has one edge on each side
    let mut gone = Vec::new();
    for _ in 0..2 {
        app.handle(ctrl(KeyCode::Char('e')));
        let e = app.view.pick.edge().expect("该选到一条边");
        gone.push(app.doc.edges[e].id.clone());
        app.handle(key(KeyCode::Char('d')));
    }
    assert!(app.doc.edges.is_empty(), "两条边该都删掉");
    assert_ne!(gone[0], gone[1], "两次删的该是两条不同的边");
}

/// Paste and delete are one undo step each; undo/redo leave the clipboard alone
#[test]
fn paste_undoes_in_one_step_and_keeps_clipboard() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    let clip_after_cut = app.clipboard.clone().unwrap();
    let undo_after_cut = app.undo.counts().0;
    app.handle(key(KeyCode::Char('p')));
    assert_eq!(app.undo.counts().0, undo_after_cut + 1, "粘贴一步撤销");
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.boxes.len()),
        Some(clip_after_cut.boxes.len()),
        "粘贴不改剪贴板"
    );
    let boxes_before = app.doc.boxes.len();
    app.handle(key(KeyCode::Char('u')));
    assert_eq!(app.doc.boxes.len(), boxes_before - 1, "撤销掉这次粘贴");
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.boxes.len()),
        Some(clip_after_cut.boxes.len()),
        "撤销不改剪贴板"
    );
    app.handle(ctrl(KeyCode::Char('r')));
    assert_eq!(app.doc.boxes.len(), boxes_before, "重做回来");
}

/// Edges inside the selection: d copies them to the clipboard, x deletes without copying
#[test]
fn selected_edges_keep_internal_ones_in_clipboard() {
    let mut app = app_with_links(row_document(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    // Cover the first two boxes (their edge is internal)
    app.view.cursor = (0, 0);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Char('d')));
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 2, "两个框");
    assert_eq!(clip.edges.len(), 1, "内部边进剪贴板");

    // Boundary edge: select one box only, the edge leaves the document but not into the clipboard
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(key(KeyCode::Char('d')));
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 1);
    assert!(clip.edges.is_empty(), "边界边不进剪贴板");
    assert_eq!(app.doc.edges.len(), 0, "但文档里的边已经删掉");
}

/// After a delete the cursor stays put (it does not jump to the nearest box)
#[test]
fn delete_keeps_cursor_in_place() {
    let mut app = app_with_links(row_document(3));
    app.jump_to(1);
    let before = app.view.cursor;
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.view.cursor, before, "光标该留在原地");
    assert!(
        app.doc.box_at_cell(before).is_none(),
        "那个框已经删掉了,光标格上应该空了"
    );
}

#[test]
fn link_menu_confirms_and_cancels() {
    // confirm the link
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    app.jump_to(1);
    app.open_menu(40, 10);
    let menu = app.view.menu.clone().expect("该弹出菜单");
    let titles: Vec<&str> = menu.level().iter().map(|i| i.title.as_str()).collect();
    assert!(titles.contains(&"确认连边"), "{titles:?}");
    app.menu_choose();
    assert_eq!(edge_names(&app), vec![("b0".into(), "b1".into())]);
    // cancel the link
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    app.open_menu(40, 10);
    app.view.menu.as_mut().unwrap().sel = 1;
    app.menu_choose();
    assert!(app.view.link.is_none());
    assert!(app.doc.edges.is_empty());
}

#[test]
fn link_requires_unity_zoom() {
    let mut app = app_with_links(row_document(2));
    app.view.zoom = 2.0;
    app.jump_to(0);
    app.handle(key(KeyCode::Char('e')));
    assert!((app.view.zoom - 1.0).abs() < 1e-9, "连边前先切回 1:1");
}

/// s: box menu on a box, edge menu on an edge, a note on empty space
#[test]
fn s_opens_style_menu_by_type() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(key(KeyCode::Char('s')));
    let menu = app.view.menu.clone().expect("框上该开菜单");
    assert_eq!(menu.level()[0].title, "框型");
    assert!(matches!(menu.level()[0].action, MenuAction::Sub(_)));
    app.handle(key(KeyCode::Esc));

    // On an edge: select the edge first
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
    app.handle(key(KeyCode::Char('s')));
    let menu = app.view.menu.clone().expect("边上该开菜单");
    assert_eq!(menu.level()[0].title, "线型");
    app.handle(key(KeyCode::Esc));

    // Empty space: no menu, only a note
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('s')));
    assert!(app.view.menu.is_none());
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("没有可改样式"), "{note}");
}

/// A new edge copies group / line style / corner / colour from the last selected edge
#[test]
fn new_edge_copies_last_selected_edge() {
    let mut app = app_with_links(row_document(3));
    let e0 = app
        .doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.doc.edges[e0].style = crate::model::EdgeStyle::Dashed;
    app.doc.edges[e0].corner = crate::model::Corner::Sharp;
    app.doc.edges[e0].color = Some((9, 8, 7));
    app.doc.edges[e0].bg = Some((1, 1, 1));
    app.doc.rebuild();
    // Note that edge's style, then create a new edge
    app.view.pick = render::Pick::Edge(e0);
    app.remember_look();
    assert!(app.edge_look.is_some(), "选中边该记下样式");

    connect(&mut app, vec![1], vec![2]);
    let e1 = app
        .doc
        .edges
        .iter()
        .position(|e| e.from == "b1" && e.to == "b2")
        .expect("建了新边");
    assert_eq!(app.doc.edges[e1].style, crate::model::EdgeStyle::Dashed);
    assert_eq!(app.doc.edges[e1].corner, crate::model::Corner::Sharp);
    assert_eq!(app.doc.edges[e1].color, Some((9, 8, 7)));
    assert_eq!(app.doc.edges[e1].bg, Some((1, 1, 1)));
    assert_eq!(app.doc.edges[e1].group, "g0", "分组也照抄");

    // With a block selection, fall back to the default style
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (3, 3),
        confirmed: true,
    });
    connect(&mut app, vec![2], vec![0]);
    let e2 = app
        .doc
        .edges
        .iter()
        .position(|e| e.from == "b2" && e.to == "b0")
        .expect("又建了一条");
    assert_eq!(app.doc.edges[e2].style, crate::model::EdgeStyle::Solid);
    assert_eq!(app.doc.edges[e2].color, None);
}

/// A new box copies style / colour / align from the
/// last selected box; a block selection does not copy
#[test]
fn new_box_copies_last_selected_box() {
    let mut app = app_with_links(row_document(2));
    app.doc.boxes[0].style = BoxStyle::Double;
    app.doc.boxes[0].color = Some((10, 20, 30));
    app.doc.boxes[0].fill = Some((1, 2, 3));
    let ti = app.doc.text_of(0).unwrap();
    app.doc.texts[ti].color = Some((200, 100, 50));
    app.doc.texts[ti].bg = Some((4, 5, 6));
    app.doc.texts[ti].align = Align::Right;
    app.doc.rebuild();
    app.view.cursor = app.cell_of(0);
    app.refresh_pick();
    assert_eq!(app.view.pick, render::Pick::Box(0));

    app.cmd_apply("new 下一个");
    let i = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[i].style, BoxStyle::Double, "照抄框型");
    assert_eq!(app.doc.boxes[i].color, Some((10, 20, 30)));
    assert_eq!(app.doc.boxes[i].fill, Some((1, 2, 3)));
    let t = app.doc.text(i).unwrap();
    assert_eq!(t.color, Some((200, 100, 50)));
    assert_eq!(t.bg, Some((4, 5, 6)));
    assert_eq!(t.align, Align::Right);

    // Block selection: no copy, back to the default (text only)
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (2, 2),
        confirmed: true,
    });
    app.cmd_apply("new 选区里的");
    let j = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[j].style, BoxStyle::None, "选一片时不照抄");
    assert_eq!(app.doc.boxes[j].color, None);
    assert_eq!(app.doc.text(j).unwrap().align, Align::Left);
}

/// Prompt line: i prefills the text with the cursor
/// at the end; Alt-B/F and Alt-Left/Right jump by word
#[test]
fn prompt_prefills_text_and_jumps_by_word() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    // Prompt line opens directly (no default binding opens it any more; use :text or the menu)
    app.open_prompt(Prompt::Text(vec![0]), app.common_text(&[0]));
    match app.view.prompt.clone() {
        Some((Prompt::Text(_), text)) => assert_eq!(text, "甲", "预填原文"),
        other => panic!("该是改文字:{other:?}"),
    }
    assert_eq!(app.view.prompt_pos, 1, "光标在末尾");

    // Ctrl-U clears, type three words
    app.handle(ctrl(KeyCode::Char('u')));
    for c in "one two three".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    let text = |app: &App| app.view.prompt.as_ref().map(|(_, t)| t.clone()).unwrap();
    assert_eq!(text(&app), "one two three");
    assert_eq!(app.view.prompt_pos, 13);

    // Alt-Left: back before "three"; twice more to before "one"
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.prompt_pos, 8);
    app.handle(alt(KeyCode::Char('b')));
    assert_eq!(app.view.prompt_pos, 4);
    app.handle(alt(KeyCode::Char('b')));
    assert_eq!(app.view.prompt_pos, 0);
    // Alt-Right: forward by word end
    app.handle(alt(KeyCode::Right));
    assert_eq!(app.view.prompt_pos, 3);
    app.handle(alt(KeyCode::Char('f')));
    assert_eq!(app.view.prompt_pos, 7);

    // Ctrl-K deletes to end of line, Ctrl-A/E go to start/end, insertion happens at the cursor
    app.handle(ctrl(KeyCode::Char('k')));
    assert_eq!(text(&app), "one two");
    app.handle(ctrl(KeyCode::Char('a')));
    app.handle(key(KeyCode::Char('>')));
    assert_eq!(text(&app), ">one two");
    // Ctrl-W deletes the previous word ("one" before the cursor)
    app.handle(ctrl(KeyCode::Char('e')));
    app.handle(ctrl(KeyCode::Char('w')));
    assert_eq!(text(&app), ">one ");
    // Alt+Backspace: delete the previous word (same as Ctrl-W)
    app.handle(ctrl(KeyCode::Char('u')));
    for c in "alpha beta gamma".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(key(KeyCode::End));
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(text(&app), "alpha beta ");

    // Delete removes the character at the cursor; Backspace the one before
    app.handle(ctrl(KeyCode::Char('a')));
    app.handle(key(KeyCode::Delete));
    assert_eq!(text(&app), "lpha beta ");
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Backspace));
    assert_eq!(text(&app), "lpha beta");
    app.handle(key(KeyCode::Esc));
    assert!(app.view.prompt.is_none());
}

/// The command line (:) uses the same line editing:
/// the cursor moves in and out, word jumps, Home/End
#[test]
fn command_line_jumps_by_word() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.handle(key(KeyCode::Char(':')));
    for c in "w out/x.gtui".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));
    assert_eq!(app.view.cmd_pos, 12);
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.cmd_pos, 2, "跳到 out/x.gtui 前面");
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.cmd_pos, 0);
    app.handle(key(KeyCode::Home));
    assert_eq!(app.view.cmd_pos, 0);
    app.handle(key(KeyCode::Char('#')));
    assert_eq!(app.view.cmd.as_deref(), Some("#w out/x.gtui"));
    // Cursor after '#': Ctrl-K deletes to end of line
    // and keeps the typed '#'; Ctrl-U clears the start
    app.handle(ctrl(KeyCode::Char('k')));
    assert_eq!(app.view.cmd.as_deref(), Some("#"));
    app.handle(ctrl(KeyCode::Char('u')));
    assert_eq!(app.view.cmd.as_deref(), Some(""));
    app.handle(key(KeyCode::Esc));
}

/// Split marker: one box becomes several, the boxes to the right line up after them, one undo step
#[test]
fn split_marker_splits_box() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let n0 = app.doc.boxes.len();
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    set_box_text(&mut app, "hello,<|new box|>world!");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"hello,"), "{texts:?}");
    assert!(texts.contains(&"world!"), "{texts:?}");
    assert_eq!(app.doc.boxes.len(), n0 + 1, "多出一个框");
    // the cursor stays on the original box
    assert_eq!(app.view.pick, render::Pick::Box(i));
    assert!(app
        .view
        .note
        .as_deref()
        .unwrap_or("")
        .contains("拆成 2 个框"));
    // The new box sits to the right of the original, top aligned
    let new = app.doc.boxes.len() - 1;
    assert!(app.doc.boxes[new].at.0 > app.doc.boxes[i].at.0, "放在右边");
    assert_eq!(app.doc.boxes[new].at.1, app.doc.boxes[i].at.1, "顶对齐");
    // One undo: the text returns to the pre-split state and the new boxes disappear
    app.undo_step();
    let back: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    assert_eq!(back, before, "撤销该恢复拆分前的文字");
    assert_eq!(app.doc.boxes.len(), n0, "新框一起撤销");
}

/// Set the text of the cursor box through the `:text` command (the prompt submit path)
fn set_box_text(app: &mut App, text: &str) {
    // t is not a default key any more: go through :text (same entry as prompt submit)
    let mut ids = app.targets().boxes;
    if ids.is_empty() {
        if let Some(i) = app.cursor_box() {
            ids.push(i);
        }
    }
    app.cmd_set_text(&ids, text);
}

/// Several markers: laid out left to right
#[test]
fn split_marker_makes_three_boxes() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "a<|new box|>b<|new box|>c");
    assert_eq!(app.doc.boxes.len(), n0 + 2);
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    for t in ["a", "b", "c"] {
        assert!(texts.contains(&t), "{t} 没切出来:{texts:?}");
    }
    // b, c follow to the right of the original and never overlap
    let mut xs: Vec<(i32, u16)> = (0..app.doc.boxes.len())
        .filter(|&k| k == i || k >= n0)
        .map(|k| (app.doc.boxes[k].at.0, app.doc.boxes[k].size.0))
        .collect();
    xs.sort();
    for w in xs.windows(2) {
        assert!(w[0].0 + w[0].1 as i32 <= w[1].0, "框互相压住:{xs:?}");
    }
}

/// Empty segments dropped: leading / trailing / consecutive
/// markers create no box; one segment does not split
#[test]
fn split_drops_empty_segments() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "<|new box|>a<|new box|><|new box|>b<|new box|>");
    assert_eq!(app.doc.boxes.len(), n0 + 1, "只有 a / b 两个非空段");
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "<|new box|><|new box|>");
    assert_eq!(app.doc.boxes.len(), n0, "没有内容就不拆");
    assert_eq!(
        app.doc.text(0).map(|t| t.text.clone()).unwrap_or_default(),
        "<|new box|><|new box|>",
        "拆不了就按文字写进去,不能把整段吞掉(否则这个框以后怎么改都不生效)"
    );
    assert!(app
        .view
        .note
        .as_deref()
        .unwrap_or("")
        .contains("nothing to split"));
    let _ = before;
}

/// The box already holds an unescaped marker: after `i` and Esc the text must be written back
#[test]
fn edit_commits_marker_in_existing_box() {
    let mut app = app_with_links(row_document(1));
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.doc.set_text(0, "a <|new box|> b".to_string());
    app.doc.rebuild();
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "该进就地编辑");
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Char('X')));
    app.handle(key(KeyCode::Esc));
    assert_eq!(
        app.doc.text(0).map(|t| t.text.clone()).unwrap_or_default(),
        "a <|new box|> bX",
        "改动要写回,标记按文字留着"
    );
    assert_eq!(app.doc.boxes.len(), 1, "拆不出来就别多建框");
}

/// Escaping: a backslash keeps the marker literal, no split
#[test]
fn escaped_marker_is_plain_text() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "x\\<|new box|>y");
    assert_eq!(app.doc.boxes.len(), n0, "转义不拆");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"x\\<|new box|>y"), "{texts:?}");
}

/// A box on the right gets out of the way, never overlaps
#[test]
fn split_offsets_past_right_neighbour() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let before: Vec<(i32, i32, u16, u16)> = app
        .doc
        .boxes
        .iter()
        .map(|b| (b.at.0, b.at.1, b.size.0, b.size.1))
        .collect();
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "long text here<|new box|>z");
    assert_eq!(app.doc.boxes.len(), n0 + 1);
    let new = &app.doc.boxes[n0];
    let (nx, ny, nw, nh) = (new.at.0, new.at.1, new.size.0 as i32, new.size.1 as i32);
    for (k, b) in app.doc.boxes.iter().enumerate() {
        if k == n0 || k == i {
            continue;
        }
        let (bx, by, bw, bh) = (b.at.0, b.at.1, b.size.0 as i32, b.size.1 as i32);
        assert!(
            !(nx < bx + bw && bx < nx + nw && ny < by + bh && by < ny + nh),
            "新框压住了第 {k} 个框"
        );
    }
    assert_eq!(before.len(), n0, "旧框数量没变");
}

/// Not splittable: multi-select / annotation box / point node keep the marker as plain text
#[test]
fn unsplittable_boxes_keep_marker_as_text() {
    // multi-select
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.view.sel = Some(RectSel {
        anchor: (-5, -5),
        end: (40, 20),
        confirmed: true,
    });
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "多选不拆");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(
        texts.contains(&"a<|new box|>b"),
        "标记按普通文字写:{texts:?}"
    );
    // point node
    let mut doc = row_document(2);
    doc.boxes[0].style = crate::model::BoxStyle::Point;
    let mut app = app_with_links(doc);
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "点节点不拆");
    assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
    // annotation box
    let mut app = app_with_links(row_document(2));
    let _ = app.doc.add_edge("b0", "b1", "g0", Dir::Forward);
    app.doc.set_note(0, "note");
    app.refresh_pick();
    let nb = app
        .doc
        .boxes
        .iter()
        .position(|b| b.edge.is_some())
        .expect("注释框");
    app.jump_to(nb);
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "注释框不拆");
    assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
}

/// The new box copies the original's style, colour, fill, align and layer
#[test]
fn split_copies_box_style() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    app.doc.boxes[i].style = crate::model::BoxStyle::Double;
    app.doc.boxes[i].color = Some((1, 2, 3));
    app.doc.boxes[i].fill = Some((4, 5, 6));
    let ti = app.doc.text_of(i).expect("文字");
    app.doc.texts[ti].color = Some((7, 8, 9));
    app.doc.texts[ti].bg = Some((10, 11, 12));
    app.doc.texts[ti].align = crate::model::Align::Center;
    let n0 = app.doc.boxes.len();
    set_box_text(&mut app, "p<|new box|>q");
    assert_eq!(app.doc.boxes.len(), n0 + 1);
    let b = &app.doc.boxes[n0];
    assert_eq!(b.style, crate::model::BoxStyle::Double, "框型");
    assert_eq!(b.color, Some((1, 2, 3)), "边框色");
    assert_eq!(b.fill, Some((4, 5, 6)), "填充");
    let t = app.doc.text(n0).expect("新框文字");
    assert_eq!(t.color, Some((7, 8, 9)), "文字色");
    assert_eq!(t.bg, Some((10, 11, 12)), "文字底色");
    assert_eq!(t.align, crate::model::Align::Center, "对齐");
    assert_eq!(t.text, "q");
}

/// Edges stay on the original box; new boxes get none; zoom does not affect splitting
#[test]
fn split_keeps_edges_on_original_box() {
    let mut app = app_with_links(row_document(3));
    let _ = app.doc.add_edge("b0", "b1", "g0", Dir::Forward);
    app.refresh_pick();
    app.jump_to(0);
    app.view.zoom = 2.0;
    app.view.center = Some((app.view.center.unwrap_or((0.0, 0.0)).0 + 1.0, 0.0));
    let i = app.cursor_box().unwrap();
    set_box_text(&mut app, "p<|new box|>q");
    assert_eq!(app.doc.edges.len(), 1, "边数不变");
    assert_eq!(app.doc.edges[0].from, app.doc.boxes[i].id, "边还挂在原框");
    assert!(
        app.doc.boxes.last().unwrap().edge.is_none(),
        "新框没有注释关系"
    );
}

/// `:text` does not split: the marker is plain text
#[test]
fn command_line_text_does_not_split() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    app.cmd_apply("text q<|new box|>r");
    assert_eq!(app.doc.boxes.len(), n0, ":text 不拆");
}

/// English UI: the style menu titles (box/edge) must not contain CJK; the Chinese UI is unchanged
#[test]
fn english_menu_has_no_cjk() {
    fn has_cjk(s: &str) -> bool {
        s.chars()
            .any(|c| matches!(c as u32, 0x3000..=0x303f | 0x4e00..=0x9fff | 0xff00..=0xffef))
    }
    for on_edge in [false, true] {
        let mut doc = row_document(2);
        doc.add_edge("b0", "b1", "g0", Dir::Forward).expect("加边");
        let mut app = app_with_links(doc);
        app.view.lang = Lang::En;
        if on_edge {
            app.view.pick = render::Pick::Edge(0);
        } else {
            app.view.pick = render::Pick::Box(0);
        }
        let titles: Vec<String> = app.menu_for().iter().map(|i| i.title.clone()).collect();
        assert!(!titles.is_empty());
        for title in &titles {
            assert!(!has_cjk(title), "英文菜单里有中文:{title}");
        }
    }
}

/// Default UI is English; `--lang zh` (Options.lang) and the document view.lang switch to Chinese
#[test]
fn default_english_and_two_ways_to_chinese() {
    assert_eq!(Lang::default(), Lang::En);
    assert_eq!(crate::document::ViewState::default().lang, "en");

    // default: English
    let app = App::new(Document::scratch(), Options::default()).expect("开局");
    assert_eq!(app.view.lang, Lang::En);

    // command line --lang zh
    let app = App::new(Document::scratch(), zh_opts()).expect("开局");
    assert_eq!(app.view.lang, Lang::Zh);

    // view.lang = zh in the document: a new tab stays
    // English, opening the document switches to Chinese
    let mut doc = Document::scratch();
    doc.view.lang = "zh".to_string();
    let app = App::new(doc, Options::default()).expect("开局");
    assert_eq!(app.view.lang, Lang::Zh, "文档里的语言要生效");

    // 显式的 en 也能压过文档(以前 En 与“没给”无法区分)
    let mut doc = Document::scratch();
    doc.view.lang = "zh".to_string();
    let en = Options {
        lang: Some(Lang::En),
        ..Options::default()
    };
    let app = App::new(doc, en).expect("开局");
    assert_eq!(app.view.lang, Lang::En, "显式 en 要压过 view.lang");
}

/// 真实光标用渲染那一份提示前缀:en 的 `box: ` 比 zh 的 `框: ` 宽一格,不能各算一套
#[test]
fn prompt_cursor_uses_the_language_prefix() {
    let mut app = app_with_links(row_document(2));
    app.area = Rect::new(0, 0, 60, 12);
    app.view.prompt = Some((Prompt::NewBox, String::new()));
    app.view.prompt_pos = 0;
    app.view.lang = Lang::En;
    assert_eq!(app.cursor_cell(), Some((5, 11)), "en: box: 占 5 格");
    app.view.lang = Lang::Zh;
    assert_eq!(app.cursor_cell(), Some((4, 11)), "zh: 框: 占 4 格");
}

/// `cursor = drawn` 画进画面:终端光标必须收起来,否则同一个位置有两个光标
#[test]
fn drawn_cursor_hides_the_terminal_cursor() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert!(app.cursor_cell().is_some(), "默认用终端光标");
    app.view.drawn = true;
    assert_eq!(app.cursor_cell(), None, "画进画面时不再设终端光标");
    // 输入行仍然要终端光标(画面里的光标不跟着输入走)
    app.view.prompt = Some((Prompt::NewBox, String::new()));
    app.view.prompt_pos = 0;
    assert_eq!(app.cursor_cell(), Some((4, 23)), "zh: 框: 宽 4");
}

/// A tab opened from a file keeps the cursor mode: a fresh tab used to paint the cursor into
/// the picture, so the terminal cursor vanished after Ctrl-P
#[test]
fn new_tab_keeps_the_cursor_mode() {
    let path = temp_path("tab-cursor", "gtui");
    std::fs::write(&path, Document::scratch().save()).unwrap();
    let mut app = App::new(sample_document(), Options::default()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert!(!app.view.drawn, "默认用终端光标");
    app.new_tab(Some(&path));
    assert!(!app.view.drawn, "新 tab 不该切到画进画面的光标");
    assert!(app.cursor_cell().is_some(), "终端光标要还在");

    // 启动时显式要求画进画面,新 tab 也跟着
    let opts = Options {
        drawn: true,
        ..Options::default()
    };
    let mut drawn = App::new(sample_document(), opts).expect("开局");
    drawn.area = Rect::new(0, 0, 80, 24);
    drawn.new_tab(Some(&path));
    assert!(drawn.view.drawn, "画进画面的模式要带过去");
    let _ = std::fs::remove_file(&path);
}

/// `:export-canvas` writes JSON Canvas with the boxes and edges of the document
#[test]
fn export_canvas_verb_writes_json_canvas() {
    let mut app = app_with_links(row_document(2));
    let path = temp_path("export-canvas", "canvas");
    app.cmd_apply(&format!("export-canvas {path}"));
    let text = std::fs::read_to_string(&path).expect("写出的文件");
    let v = crate::json::parse(&text).expect("合法 JSON");
    let nodes = v.get("nodes").and_then(|n| n.as_arr()).expect("nodes");
    let edges = v.get("edges").and_then(|e| e.as_arr()).expect("edges");
    assert_eq!(nodes.len(), app.doc.boxes.len());
    assert_eq!(edges.len(), app.doc.edges.len());
    let note = app.view.note.clone().unwrap_or_default();
    assert!(
        note.contains("exported to") || note.contains("已导出"),
        "{note}"
    );
    let _ = std::fs::remove_file(&path);
}

/// A box restyled through the `s` menu counts as
/// selected: move to empty space, :new still copies it
#[test]
fn restyled_box_counts_as_selected() {
    let mut app = app_with_links(row_document(2));
    app.view.cursor = app.cell_of(0);
    app.refresh_pick();
    // restyle through the menu: style => double
    app.handle(key(KeyCode::Char('s')));
    app.handle(key(KeyCode::Char('l')));
    let want = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|i| i.title == "double")
        .expect("有 double");
    for _ in 0..want {
        app.handle(key(KeyCode::Char('j')));
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Double);

    // Move the cursor to empty space (no box there), then create
    app.view.cursor = (60, 20);
    app.view.pick = render::Pick::None;
    app.cmd_apply("new 远方");
    let i = app.doc.boxes.len() - 1;
    assert_eq!(
        app.doc.boxes[i].style,
        BoxStyle::Double,
        "刚用菜单改过样式的框该被照抄"
    );
}

/// Edge menu port: start / end each get auto plus the four sides, via :fromside / :toside
#[test]
fn port_menu_sets_both_sides() {
    let mut app = app_with_links(row_document(2));
    let e = app
        .doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
    app.handle(key(KeyCode::Char('s')));
    let menu = app.view.menu.clone().expect("边上该开菜单");
    let port = menu
        .level()
        .iter()
        .find(|i| i.title == "端口")
        .expect("边菜单该有端口");
    let MenuAction::Sub(ends) = &port.action else {
        panic!("端口该是子菜单")
    };
    let titles: Vec<&str> = ends.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["起点", "终点"]);
    let MenuAction::Sub(sides) = &ends[0].action else {
        panic!("起点该是子菜单")
    };
    let cmds: Vec<&str> = sides
        .iter()
        .map(|i| match &i.action {
            MenuAction::Cmd(c) => c.as_str(),
            _ => panic!("每一侧都是一条命令"),
        })
        .collect();
    assert_eq!(
        cmds,
        [
            "fromside auto",
            "fromside left",
            "fromside right",
            "fromside up",
            "fromside down"
        ]
    );
    app.handle(key(KeyCode::Esc));
    // Closing the menu hands pick back to the element under the cursor; select the edge again
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "重新选到边");

    // Go through the command behind the menu: start left, end up, one undo step each
    app.cmd_apply("fromside left");
    app.cmd_apply("toside up");
    assert_eq!(app.doc.edges[e].from_side, Some(Side::Left));
    assert_eq!(app.doc.edges[e].to_side, Some(Side::Up));

    app.handle(key(KeyCode::Char('u')));
    assert_eq!(app.doc.edges[e].from_side, Some(Side::Left), "只回退一步");
    assert_eq!(app.doc.edges[e].to_side, None);
}

/// c: colour menu = auto + 12 colours, the current value marked with *; stroke colour only
#[test]
fn c_colour_menu_sets_stroke_colour() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('c')));
    let menu = app.view.menu.clone().expect("该开颜色菜单");
    let items = menu.level().to_vec();
    assert_eq!(items.len(), 13, "auto + 12 色");
    assert_eq!(items[0].title, "auto");
    assert!(items[0].checked, "还没设过颜色,auto 该打 *");
    app.handle(key(KeyCode::Esc));
    // Set a colour and a fill, then check the marker
    app.cmd_apply("nodecolor #ef4b4b");
    app.cmd_apply("fill #22b46a");
    app.handle(key(KeyCode::Char('c')));
    // Open again: the current value is in the palette now
    app.handle(key(KeyCode::Char('c')));
    let menu = app.view.menu.clone().expect("再开");
    let hit = menu
        .level()
        .iter()
        .position(|i| i.checked)
        .expect("该有打 * 的项");
    assert_eq!(hit, 2, "12 色里第二个 #ef4b4b");
    // Pick the third colour (index 3) and press Enter: only the stroke colour changes
    app.handle(key(KeyCode::Char('j')));
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].color, Some((0x22, 0xb4, 0x6a)), "边框色");
    assert_eq!(app.doc.boxes[0].fill, Some((0x22, 0xb4, 0x6a)), "填充不变");
}

/// Previews push no undo, Esc restores
#[test]
fn menu_preview_pushes_no_undo_esc_reverts() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let undo0 = app.undo.counts().0;
    let before = app.doc.boxes[0].style;
    app.handle(key(KeyCode::Char('s'))); // style
    app.handle(key(KeyCode::Char('l'))); // enter the submenu (previews the first item, point)
    app.handle(key(KeyCode::Char('j'))); // preview single
    assert_ne!(app.doc.boxes[0].style, before, "预览该实时改文档");
    assert_eq!(app.undo.counts().0, undo0, "预览不压撤销");
    app.handle(key(KeyCode::Esc)); // back one level
    app.handle(key(KeyCode::Esc)); // close
    assert_eq!(app.doc.boxes[0].style, before, "Esc 该还原");
    assert_eq!(app.undo.counts().0, undo0);
}

/// One multi-select edit changes several boxes in one step
#[test]
fn menu_edits_whole_selection_in_one_step() {
    let mut app = app_with_links(row_document(3));
    app.view.cursor = (0, 0);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(key(KeyCode::Enter));
    // Cursor back on a box, the menu follows the box type
    app.jump_to(0);
    let undo0 = app.undo.counts().0;
    app.handle(key(KeyCode::Char('s')));
    app.handle(key(KeyCode::Char('l')));
    // Find rounded in the submenu by title (indices shift once none is inserted)
    let want = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|i| i.title == "rounded")
        .expect("有 rounded");
    for _ in 0..want {
        app.handle(key(KeyCode::Char('j')));
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Rounded);
    assert_eq!(app.doc.boxes[1].style, BoxStyle::Rounded, "两个都改");
    assert_eq!(app.undo.counts().0, undo0 + 1, "整批一步");
    assert!(app.view.sel.is_some(), "菜单不该动选区");
}

/// Edge line style menu: one undo step after applying
#[test]
fn edge_menu_sets_line_style() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(ctrl(KeyCode::Char('e')));
    let undo0 = app.undo.counts().0;
    app.handle(key(KeyCode::Char('s')));
    app.handle(key(KeyCode::Char('l')));
    app.handle(key(KeyCode::Char('j')));
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.edges[0].style, EdgeStyle::Dashed);
    assert_eq!(app.undo.counts().0, undo0 + 1);
}

/// Choosing the current value pushes no undo
#[test]
fn same_value_choice_pushes_no_undo() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.cmd_apply("box single");
    let undo0 = app.undo.counts().0;
    app.handle(key(KeyCode::Char('s')));
    app.handle(key(KeyCode::Char('l')));
    // find single in the submenu by title
    let single = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|i| i.title == "single")
        .expect("有 single");
    for _ in 0..single {
        app.handle(key(KeyCode::Char('j')));
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Single);
    assert_eq!(app.undo.counts().0, undo0, "同值不该压撤销");
}

/// The edge menu's group item runs the group verb
#[test]
fn edge_menu_sets_group() {
    let mut app = app_with_links(sample_document());
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(1); // b1 has two edges
    app.handle(ctrl(KeyCode::Char('e')));
    let e = app.view.pick.edge().expect("选到边");
    let before = app.doc.edges[e].group.clone();
    app.handle(key(KeyCode::Char('s')));
    // Find the group item, enter it, pick another group
    let menu = app.view.menu.clone().expect("菜单");
    let pos = menu
        .level()
        .iter()
        .position(|i| i.title == "分组")
        .expect("边菜单该有分组项");
    for _ in 0..pos {
        app.handle(key(KeyCode::Char('j')));
    }
    app.handle(key(KeyCode::Char('l')));
    let sub = app.view.menu.clone().expect("子菜单");
    let other = (0..sub.level().len())
        .find(|i| !sub.level()[*i].checked)
        .expect("该有别的分组");
    for _ in 0..other {
        app.handle(key(KeyCode::Char('j')));
    }
    app.handle(key(KeyCode::Enter));
    assert_ne!(app.doc.edges[e].group, before, "分组该换掉");
}

/// textbg / edgebg change text background and edge background independently, and can clear them
#[test]
fn textbg_and_edgebg_verbs() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.cmd_apply("textbg #334455");
    assert_eq!(app.doc.text(0).unwrap().bg, Some((0x33, 0x44, 0x55)));
    assert_eq!(app.doc.boxes[0].fill, None, "文字底色不碰填充");
    // edge: select first, then change
    app.handle(ctrl(KeyCode::Char('e')));
    app.cmd_apply("edgebg #552211");
    let e = app.view.pick.edge().expect("选到边");
    assert_eq!(app.doc.edges[e].bg, Some((0x55, 0x22, 0x11)));
    assert_eq!(app.doc.edges[e].color, None, "边底色不该改边的前景色");
    // clear
    app.cmd_apply("edgebg none");
    assert_eq!(app.doc.edges[e].bg, None);
    app.jump_to(0);
    app.cmd_apply("textbg none");
    assert_eq!(app.doc.text(0).unwrap().bg, None);
}

/// In the menu h / l only navigate: l drills down, h goes up, a leaf item is not run by l
#[test]
fn h_and_l_navigate_menu_levels() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.handle(key(KeyCode::Char('s')));
    assert_eq!(app.view.menu.as_ref().unwrap().levels.len(), 1);
    // l enters box style
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        2,
        "l 该进子菜单"
    );
    assert_eq!(app.view.menu.as_ref().unwrap().parents.len(), 1);
    // h goes back one level
    app.handle(key(KeyCode::Char('h')));
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        1,
        "h 该回上一层"
    );
    assert!(app.view.menu.as_ref().unwrap().parents.is_empty());
    // h again: the top level closes
    app.handle(key(KeyCode::Char('h')));
    assert!(app.view.menu.is_none(), "顶层 h 该关掉菜单");
    // l on a leaf item does not run it
    let undo0 = app.undo.counts().0;
    let before = app.doc.boxes[0].style;
    app.handle(key(KeyCode::Char('s')));
    app.handle(key(KeyCode::Char('l'))); // enter box style
    app.handle(key(KeyCode::Char('j'))); // preview single
    app.handle(key(KeyCode::Char('l'))); // leaf: must not run
    assert!(app.view.menu.is_some(), "l 在叶子上不该关菜单");
    assert_eq!(app.undo.counts().0, undo0, "l 在叶子上不该压撤销");
    // Enter runs it
    app.handle(key(KeyCode::Enter));
    assert!(app.view.menu.is_none());
    assert_ne!(app.doc.boxes[0].style, before, "Enter 该应用");
    assert_eq!(app.undo.counts().0, undo0 + 1);
}

/// Menu rendering: swatches use the background colour, the
/// current value is marked *, status reports count and value
#[test]
fn menu_swatches_and_status_line() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.cmd_apply("nodecolor #ef4b4b");
    app.handle(key(KeyCode::Char('c')));
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let mut swatch = 0;
    let mut star = 0;
    for y in 0..app.area.height {
        for x in 0..app.area.width {
            let cell = &buf[(x, y)];
            if cell.bg == ratatui::style::Color::Rgb(0x22, 0xb4, 0x6a) {
                swatch += 1;
            }
            if cell.symbol() == "*" {
                star += 1;
            }
        }
    }
    assert!(swatch >= 2, "色块该用底色画出来,只找到 {swatch}");
    assert_eq!(star, 1, "当前值只有一个 *");
    // The bottom two rows read together (menu info on the lower row, status above)
    let rows: String = (app.area.height.saturating_sub(2)..app.area.height)
        .flat_map(|y| (0..app.area.width).map(move |x| (x, y)))
        .flat_map(|(x, y)| buf[(x, y)].symbol().chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(rows.contains("改1个框"), "状态栏该报作用数量:{rows}");
    assert!(rows.contains("边框色"), "状态栏该报属性:{rows}");
}

/// The right click menu matches the keyboard menu
#[test]
fn right_click_menu_matches_keyboard_menu() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    let keyboard = app.menu_for();
    let p = render::panels(&app.doc, app.area, &app.view);
    let (cx, cy) = p.proj.cell(app.doc.center(0));
    app.open_menu(cx as u16, cy as u16);
    let menu = app.view.menu.clone().expect("右键菜单");
    let a: Vec<&str> = keyboard.iter().map(|i| i.title.as_str()).collect();
    let b: Vec<&str> = menu.level().iter().map(|i| i.title.as_str()).collect();
    assert_eq!(a, b);
}

#[test]
fn link_verb_links_single_target() {
    let mut app = app_with_links(row_document(2));
    app.jump_to(0);
    app.cmd_apply("link b1");
    assert_eq!(edge_names(&app), vec![("b0".into(), "b1".into())]);
    // c is no longer link: it opens the colour menu
    app.handle(key(KeyCode::Char('c')));
    assert!(app.view.menu.is_some(), "c 该打开颜色菜单");
    assert!(app.view.prompt.is_none(), "c 不该再进连边提示");
}

#[test]
fn w_writes_gtui_next_to_dot_source() {
    let dot = temp_path("dot-src", "dot");
    std::fs::write(&dot, "// sentinel\n").expect("写 .dot");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = Some(dot.clone());
    app.cmd_apply("w");
    let native = Path::new(&dot).with_extension("gtui");
    assert!(native.exists(), "该写出同目录的 .gtui");
    assert_eq!(std::fs::read_to_string(&dot).unwrap(), "// sentinel\n");
    let native_text = native.to_string_lossy().into_owned();
    assert_eq!(app.source.as_deref(), Some(native_text.as_str()));
    let _ = std::fs::remove_file(&dot);
    let _ = std::fs::remove_file(&native);
}

/// :wq quits only after a successful save; with no file name it stays
#[test]
fn wq_saves_and_quits() {
    let path = temp_path("wq", "gtui");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = Some(path.clone());
    app.cmd_apply("wq");
    assert!(app.quit, "存成功该退出");
    assert!(Path::new(&path).exists(), "该写出来");
    let body = std::fs::read_to_string(&path).expect("读回来");
    assert!(body.contains("\"graph-tui\""));
    let _ = std::fs::remove_file(&path);

    // No file name: ask for a path and do not quit
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = None;
    app.cmd_apply("wq");
    assert!(!app.quit, "没存成不该退出");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("文件名"), "{note}");

    // :wq <path> works too
    let p2 = temp_path("wq-path", "gtui");
    app.cmd_apply(&format!("wq {p2}"));
    assert!(app.quit, "带路径存成功该退出");
    assert!(Path::new(&p2).exists());
    let _ = std::fs::remove_file(&p2);
}

/// :wq does not quit when the save fails (e.g. given a .dot path)
#[test]
fn wq_keeps_running_on_save_failure() {
    let dot = temp_path("wq-refuse", "dot");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("wq {dot}"));
    assert!(!app.quit, "存失败要留在原地");
    assert!(!Path::new(&dot).exists());
}

#[test]
fn w_writes_back_to_gtui_source() {
    let path = temp_path("gtui-src", "gtui");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = Some(path.clone());
    app.cmd_apply("w");
    let body = std::fs::read_to_string(&path).expect("该写出来");
    assert!(body.contains("\"graph-tui\""), "该是 native 文档:{body}");
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn stdin_source_save_needs_explicit_path() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.source = None;
    app.cmd_apply("w");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("文件名"), "该提示要显式路径:{note}");
    // :save with a path writes
    let path = temp_path("stdin-save", "gtui");
    app.cmd_apply(&format!("save {path}"));
    assert!(Path::new(&path).exists());
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_writes_gtui_only() {
    let dot = temp_path("refuse", "dot");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("save {dot}"));
    assert!(!Path::new(&dot).exists(), "绝不写回 .dot");
    assert!(app.view.note.as_deref().unwrap_or("").contains(".gtui"));
}

#[test]
fn export_dot_keeps_document_and_echoes_warning() {
    let dot = temp_path("export", "dot");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    let before = app.doc.boxes.clone();
    app.cmd_apply(&format!("export {dot}"));
    assert_eq!(app.doc.boxes, before, "导出不该改图");
    let body = std::fs::read_to_string(&dot).expect("该导出");
    assert!(
        body.contains("digraph") || body.contains("graph"),
        "该是 dot:{body}"
    );
    assert!(app.view.note.is_some(), "导出要有回显");
    let _ = std::fs::remove_file(&dot);
}

#[test]
fn r_relayout_keeps_integer_coords() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 2.5;
    let want = app.view.layout.next();
    app.handle(key(KeyCode::Char('r')));
    assert_eq!(app.view.layout, want);
    for (i, b) in app.doc.boxes.iter().enumerate() {
        let t = app.doc.text(i).expect("每个框都要有文字");
        if b.style.is_boxed() {
            assert_eq!(b.size.1, 3);
            assert_eq!(t.at, (b.at.0 + 1, b.at.1 + 1));
        } else {
            assert_eq!(b.size, (1, 1));
        }
    }
}

#[test]
fn view_state_round_trips_into_view() {
    let mut doc = sample_document();
    doc.view = ViewState {
        center: (12.5, -3.0),
        zoom: 2.0,
        layout: "force".into(),
        cursor: Some(crate::document::Cursor::Box("b1".into())),
        focus: vec!["g1".into()],
        node_labels: true,
        lang: "en".into(),
        layer: Some(String::new()),
        side: 3.5,
        cycle: Some(1.25),
        orientation: "90".to_string(),
        edge_labels: false,
    };
    let body = doc.save();
    let back = Document::load(&body).expect("读回");
    // The cursor is written out as cells, compare the parsed cells
    assert_eq!(back.view.cursor_cell(&back), doc.view.cursor_cell(&doc));
    assert_eq!(
        ViewState {
            cursor: None,
            ..back.view.clone()
        },
        ViewState {
            cursor: None,
            ..doc.view.clone()
        }
    );
    // The default option does not override the document language: a document written as en stays en
    let app = App::new(back, Options::default()).expect("开局");
    assert_eq!(
        app.view.cursor,
        app.doc.text(1).unwrap().at,
        "cursor 按 id 映射成文字起点格"
    );
    assert_eq!(app.view.focus, vec![1], "focus 按 id 映射成下标");
    assert!(app.view.node_labels);
    assert_eq!(app.view.lang, Lang::En);
    // Opening behaves like pressing 0 once: zoom/center stored in the file are not restored
    assert_eq!(app.view.zoom, 1.0, "打开该回到 1:1");
    assert!(app.view.center.is_none(), "打开该居中到整幅");
    assert_eq!(app.view.side, 3.5);
    assert_eq!(app.view.cycle, Some(1.25));
    assert_eq!(app.view.layout, Layout::Force);
    assert_eq!(app.view.layer.as_deref(), Some(""));
}

/// Opening behaves like pressing `0`: 1:1, fit the whole picture (`:e` to another file too)
#[test]
fn open_behaves_like_pressing_zero() {
    let mut doc = sample_document();
    doc.view.zoom = 3.0;
    doc.view.center = (40.0, 12.0);
    let mut app = App::new(doc.clone(), zh_opts()).expect("开局");
    assert_eq!(app.view.zoom, 1.0);
    assert!(app.view.center.is_none());
    assert!(app.view.pan.is_none() && app.view.target.is_none());

    // :e opens another file: same reset
    let path = temp_path("open-zero", "gtui");
    let mut other = sample_document();
    other.view.zoom = 2.5;
    other.view.center = (5.0, 5.0);
    std::fs::write(&path, other.save()).expect("写");
    app.view.zoom = 2.0;
    app.view.center = Some((7.0, 7.0));
    app.cmd_apply(&format!("e {path}"));
    assert_eq!(app.view.zoom, 1.0, ":e 之后也回到 1:1");
    assert!(app.view.center.is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn colon_e_opens_gtui_and_resets_view() {
    let src = temp_path("open-src", "gtui");
    let mut doc = sample_document();
    doc.view.cursor = Some(crate::document::Cursor::Box("b2".into()));
    doc.view.zoom = 1.0;
    std::fs::write(&src, doc.save()).expect("写源文件");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (0, 0),
        confirmed: false,
    });
    app.cmd_apply(&format!("e {src}"));
    let b2 = app.doc.box_idx("b2").unwrap();
    assert_eq!(app.view.cursor, app.doc.text(b2).unwrap().at);
    assert!(app.view.sel.is_none(), "换文档要清选区");
    assert_eq!(app.source.as_deref(), Some(src.as_str()));
    // Opened an en document: the UI turns English
    assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
    let _ = std::fs::remove_file(&src);
}

#[test]
fn colon_w_writes_path_and_switches_source() {
    let path = temp_path("colon-w", "gtui");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("w {path}"));
    assert!(Path::new(&path).exists());
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn count_prefix_and_command_parsing_unchanged() {
    let mut p: Option<u32> = None;
    assert!(count_digit(&mut p, '5'));
    assert_eq!(p, Some(5));
    assert!(count_digit(&mut p, '0'));
    assert_eq!(p, Some(50));
    let mut q: Option<u32> = None;
    assert!(!count_digit(&mut q, '0'));
    assert_eq!(cmd_rest("text  两个空格", "text"), " 两个空格");
    assert_eq!(parse_via("1,2 3,4"), Some(vec![(1, 2), (3, 4)]));
    assert_eq!(parse_via("1,2,z"), None);
    assert_eq!(parse_color("#4c8ef8"), Some((0x4c, 0x8e, 0xf8)));
    assert_eq!(parse_color("#12"), None);
}

#[test]
fn search_refreshes_hits() {
    let doc = sample_document();
    // number: focus that group
    let mut view = View::default();
    view.query = Some("1".into());
    refresh(&doc, &mut view);
    assert_eq!(view.focus, vec![0]);
    assert_eq!(view.hits, vec![0, 1]);
    // text: only highlight matching boxes, focus untouched (other boxes of the group stay dark)
    let mut view = View::default();
    view.query = Some("丙".into());
    refresh(&doc, &mut view);
    assert_eq!(view.cursor, doc.text(2).unwrap().at);
    assert_eq!(view.hits, vec![2]);
    assert!(view.focus.is_empty(), "文字搜索不改 focus");
    view.query = Some("zzzz".into());
    refresh(&doc, &mut view);
    assert!(view.hits.is_empty());
    assert!(view.focus.is_empty());
}

/// A search hit puts the cursor on the matching glyph, not at the text start
/// (otherwise the terminal cursor sits on the first glyph and looks highlighted too)
#[test]
fn search_puts_cursor_on_the_word() {
    let mut doc = Document::scratch();
    doc.add_box("xx target", (10, 4), BoxStyle::None).unwrap();
    doc.add_box("一二三匹配", (0, 10), BoxStyle::None).unwrap();
    doc.boxes[0].terms = vec!["jia".into()];
    let mut view = View::default();
    view.query = Some("target".into());
    refresh(&doc, &mut view);
    assert_eq!(view.hits, vec![1]);
    // The three characters "xx " -> text start (10,4) plus 3 cells
    assert_eq!(view.cursor, (13, 4), "光标该停在 target 的第一个字上");
    // Wide glyphs count by column, not by glyph index
    let mut view = View::default();
    view.query = Some("匹配".into());
    refresh(&doc, &mut view);
    assert_eq!(view.hits, vec![2]);
    assert_eq!(view.cursor, (6, 10), "三个汉字占六列");
    // Alias hit (the word is not in the text): fall back to the text start
    let mut view = View::default();
    view.query = Some("jia".into());
    refresh(&doc, &mut view);
    assert_eq!(view.hits, vec![0]);
    assert_eq!(view.cursor, doc.text(0).unwrap().at);
}

#[test]
fn edge_cycle_keys_clear_on_cursor_move() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(1);
    for want in [0usize, 1, 0] {
        app.handle(ctrl(KeyCode::Char('e')));
        assert_eq!(app.view.pick.edge(), Some(want));
    }
    app.view.pick = render::Pick::Edge(0);
    app.jump_to(2);
    assert!(app.view.pick.edge().is_none(), "换框后无关的选边要清掉");
}

/// Clicking an edge in empty space selects that edge and starts no drag
#[test]
fn mouse_click_on_edge_selects() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let area = app.area;
    let mut buf = ratatui::buffer::Buffer::empty(area);
    app.draw(&mut buf, area);
    // Find a screen cell that belongs to an edge and maps back into no box
    let p = render::panels(&app.doc, area, &app.view);
    let (x, y) = app
        .hit
        .edge_cells
        .values()
        .flat_map(|v| v.iter())
        .find(|c| app.doc.box_at_cell(p.proj.canvas(c.0, c.1)).is_none())
        .copied()
        .expect("样例该有边笔画");
    app.handle(crossterm::event::Event::Mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x as u16,
            row: y as u16,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
    ));
    assert!(app.view.pick.edge().is_some(), "点边该选中它");
    assert!(
        matches!(app.drag, Some(Drag::Select { .. })),
        "按下去就该准备好拉选区(拖起来才落)"
    );
}

/// Dragging in empty space starts a block selection
/// (a click does not); Ctrl+Left or middle drag pans
#[test]
fn mouse_drag_makes_block_selection() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let at = |x: u16, y: u16, kind: MouseEventKind| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    // Find an empty row: near the bottom right of the map
    let (sx, sy) = (p.map.x + 2, p.map.y + p.map.height - 3);
    app.handle(Event::Mouse(at(
        sx,
        sy,
        MouseEventKind::Down(MouseButton::Left),
    )));
    assert!(app.view.sel.is_none(), "只按一下不该成选区");
    app.handle(Event::Mouse(at(
        sx + 6,
        sy - 3,
        MouseEventKind::Drag(MouseButton::Left),
    )));
    let anchor = app.view.sel.expect("拖起来该有选区").anchor;
    assert_eq!(
        anchor,
        p.proj.canvas(sx as i32, sy as i32),
        "锚点是按下那一格"
    );
    assert_eq!(
        app.view.cursor,
        p.proj.canvas((sx + 6) as i32, (sy - 3) as i32)
    );
    app.handle(Event::Mouse(at(
        sx + 6,
        sy - 3,
        MouseEventKind::Up(MouseButton::Left),
    )));
    // This block is empty: releasing leaves no selection (it would block later editing)
    assert!(app.view.sel.is_none(), "空选区该被丢掉");

    // Drag a block covering the whole picture: every intersecting box is selected
    app.view.sel = None;
    let (x0, y0, x1, y1) = app.doc.bbox();
    let cell_to_screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
    let (ax, ay) = cell_to_screen((x0 - 1, y0 - 1));
    let (bx, by) = cell_to_screen((x1 + 1, y1 + 1));
    app.handle(Event::Mouse(at(
        ax as u16,
        ay as u16,
        MouseEventKind::Down(MouseButton::Left),
    )));
    app.handle(Event::Mouse(at(
        bx as u16,
        by as u16,
        MouseEventKind::Drag(MouseButton::Left),
    )));
    app.handle(Event::Mouse(at(
        bx as u16,
        by as u16,
        MouseEventKind::Up(MouseButton::Left),
    )));
    let p2 = render::panels(&app.doc, app.area, &app.view);
    let got = render::selected(&app.doc, &p2, &app.view);
    assert_eq!(got.len(), app.doc.boxes.len(), "整幅图都该被选中");
    // Releasing the left button means done: confirmed directly, no Enter
    assert!(
        app.view.sel.expect("选区还在").confirmed,
        "拖完松手该已确认"
    );
    // Ctrl + left drag pans and starts no selection
    app.view.sel = None;
    let center = app.view.center;
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: sx,
        row: sy,
        modifiers: KeyModifiers::CONTROL,
    }));
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: sx + 4,
        row: sy,
        modifiers: KeyModifiers::CONTROL,
    }));
    assert!(app.view.sel.is_none(), "Ctrl+左键是平移,不该拉选区");
    assert_ne!(app.view.center, center, "画面该跟着挪");
}

/// Jump list: searches are recorded too, Ctrl-o goes back, Tab forward
#[test]
fn jump_list_back_and_forward() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let start = app.view.cursor;
    app.jump_to(2);
    let a = app.view.cursor;
    app.jump_to(1);
    let b = app.view.cursor;
    app.handle(key(KeyCode::Char('u'))); // undo does not touch the jump list
    app.cmd_apply("undo");
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, a, "Ctrl-o 退回上一个跳点");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.view.cursor, b, "Tab(即 Ctrl-i)前进");
    app.handle(ctrl(KeyCode::Char('o')));
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, start, "连续退回到起点");
}

/// `a,bG` jumps to that cell, `gg` back to the origin; an unfinished sequence shows on showcmd
#[test]
fn coord_jump_and_gg() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    for c in "100,100".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.showcmd, "100,100", "没走完的序列该显示出来");
    app.handle(key(KeyCode::Char('G')));
    assert_eq!(app.view.cursor, (100, 100), "跳到那一格");
    assert!(app.view.showcmd.is_empty(), "按完就清掉");
    // gg back to the origin
    app.handle(key(KeyCode::Char('g')));
    assert_eq!(app.view.showcmd, "g");
    app.handle(key(KeyCode::Char('g')));
    assert_eq!(app.view.cursor, (0, 0));
    assert!(app.view.showcmd.is_empty());
    // A single number means vim's "line n", the column stays
    app.view.cursor = (7, 0);
    app.refresh_pick();
    for c in "40G".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (7, 40));
    // A bare numeric prefix still drives the arrow keys
    app.view.cursor = (0, 0);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('5')));
    assert_eq!(app.view.showcmd, "5");
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(app.view.cursor, (5, 0));
    assert!(app.view.showcmd.is_empty(), "动作做完序列就没了");
    // Esc drops the unfinished sequence
    app.handle(key(KeyCode::Char('1')));
    app.handle(key(KeyCode::Esc));
    assert!(app.view.showcmd.is_empty());
}

/// `0` only resets the zoom to 1:1 without moving the picture (use gg for the origin)
#[test]
fn zero_only_resets_zoom() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.zoom = 2.5;
    app.view.center = Some((40.0, 12.0));
    app.view.target = None;
    app.view.pan = None;
    app.handle(key(KeyCode::Char('0')));
    assert_eq!(app.view.zoom, 1.0, "回到 1:1");
    assert_eq!(app.view.center, Some((40.0, 12.0)), "画面不该挪");
    assert_eq!(app.view.target, None);
}

/// A long jump: the view must really follow and land centred
#[test]
fn long_jump_centres_cursor() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    for c in "1000,1000G".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (1000, 1000));
    assert_eq!(app.view.target, Some((1000.0, 1000.0)), "第一次跳转就居中");
    // simulate the easing to completion
    app.view.center = Some((1000.0, 1000.0));
    app.view.target = None;
    // Jump back: the target must be exactly (10,10) centred, not 85% of the way
    for c in "10,10G".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (10, 10));
    assert_eq!(app.view.target, Some((10.0, 10.0)), "跳回来该直接居中");
    app.view.center = app.view.target;
    let p = render::panels(&app.doc, app.area, &app.view);
    let (cx, cy) = p.proj.cell((10.0, 10.0));
    assert_eq!(
        (cx, cy),
        (
            p.map.x as i32 + p.map.width as i32 / 2,
            p.map.y as i32 + p.map.height as i32 / 2
        ),
        "光标落在画面正中"
    );
}

/// A sample with link text: box a's text is `[site](https://example.com) x`
fn link_document() -> Document {
    link_document_with_target("https://example.com")
}

fn link_document_with_target(target: &str) -> Document {
    let groups = vec![Group {
        id: "g0".into(),
        label: "线".into(),
        color: (0x4c, 0x8e, 0xf8),
    }];
    let layers = vec![Layer {
        name: String::new(),
        order: 0,
    }];
    let boxes = vec![
        Box {
            id: "a".into(),
            at: (0, 0),
            size: (24, 3),
            style: BoxStyle::Single,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        },
        Box {
            id: "b".into(),
            at: (0, 8),
            size: (1, 1),
            style: BoxStyle::Point,
            color: None,
            fill: None,
            layer: String::new(),
            terms: Vec::new(),
            edge: None,
        },
    ];
    let texts = vec![
        Text {
            id: "ta".into(),
            text: format!("[site]({target}) x"),
            at: (1, 1),
            box_id: "a".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        },
        Text {
            id: "tb".into(),
            text: "b".into(),
            at: (0, 9),
            box_id: "b".into(),
            color: None,
            bg: None,
            align: Align::Left,
            layer: String::new(),
        },
    ];
    let edges = vec![DocEdge {
        id: "e1".into(),
        from: "a".into(),
        to: "b".into(),
        dir: Dir::Forward,
        group: "g0".into(),
        style: EdgeStyle::Solid,
        color: None,
        bg: None,
        via: Vec::new(),
        corner: Corner::Rounded,
        layer: String::new(),
        from_side: None,
        to_side: None,
        head: None,
    }];
    Document::from_parts(
        "test",
        groups,
        layers,
        boxes,
        texts,
        edges,
        ViewState::default(),
    )
    .expect("样例")
}

fn target_graph(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("graph-tui-target-app");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{tag}.gtui"));
    std::fs::write(&p, link_document().save()).unwrap();
    p
}

/// Put the cursor on the link (the hit table is rebuilt on every use in tests)
fn click_on_link(app: &mut App) {
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    assert!(app.link_at_cursor().is_some(), "光标该在链接上");
}

/// `self:x,y`: jump inside the current tab without a new one, and record it in the jump list
#[test]
fn self_link_jumps_in_place() {
    let mut app = app_with_links(link_document_with_target("self:10,10"));
    click_on_link(&mut app);
    let tabs = app.tabs.len();
    let jumps = app.jumps.len();
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.tabs.len(), tabs, "self 不该新开 tab");
    assert_eq!(app.view.cursor, (10, 10));
    assert!(
        app.take_open_request().is_none(),
        "跳转由 App 做完,不甩给 main"
    );
    assert_eq!(app.jumps.len(), jumps + 1, "该记一个跳点");
    // renderable (out-of-range coordinates do not panic)
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
}

/// The document's own path plus `:x,y`: also a local jump
#[test]
fn own_path_link_jumps_locally() {
    let path = target_graph("selfdoc");
    let mut app = app_with_links(link_document_with_target(&format!(
        "{}:4,5",
        path.display()
    )));
    app.source = Some(path.display().to_string());
    click_on_link(&mut app);
    let tabs = app.tabs.len();
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.tabs.len(), tabs, "同一份文档不该新开 tab");
    assert_eq!(app.view.cursor, (4, 5));
}

/// The target is already open in another tab: switch there and jump
#[test]
fn link_to_open_document_switches_tab() {
    let other = target_graph("open-other");
    let mut app = app_with_links(link_document_with_target(&format!(
        "{}:7,8",
        other.display()
    )));
    app.new_tab(Some(&other.display().to_string()));
    assert_eq!(app.tab_at, 1, "先开成第二个 tab");
    app.switch_tab(0);
    click_on_link(&mut app);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.tab_at, 1, "该切回那个 tab");
    assert_eq!(app.view.cursor, (7, 8));
    assert_eq!(app.tabs.len(), 2, "不该再多开一个");
}

/// The target is not open: open a new tab and jump
#[test]
fn link_to_unopened_document_opens_tab() {
    let other = target_graph("fresh-other");
    let mut app = app_with_links(link_document_with_target(&format!(
        "{}:3,4",
        other.display()
    )));
    click_on_link(&mut app);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.tabs.len(), 2, "该新开一个 tab");
    assert_eq!(app.tab_at, 1);
    assert_eq!(app.view.cursor, (3, 4));
    // After the jump Ctrl-o must return to the previous tab and cell
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.tab_at, 0, "Ctrl-o 该回到原来的 tab");
    assert_eq!(app.view.cursor, (1, 1), "回到点链接之前的位置");
}

/// A local text link with a position becomes an editor plan in main, not a local jump
#[test]
fn text_link_with_position_plans_editor() {
    let dir = std::env::temp_dir().join("graph-tui-target-app");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("note.md");
    std::fs::write(&f, "hello\n").unwrap();
    let mut app = app_with_links(link_document_with_target(&format!("{}:12,4", f.display())));
    click_on_link(&mut app);
    app.handle(key(KeyCode::Enter));
    let plan = app.take_open_request().expect("该有打开计划");
    assert_eq!(
        plan,
        crate::target::OpenPlan::Editor {
            path: f,
            position: Some(crate::target::Position { a: 12, b: 4 })
        }
    );
}

/// Enter / gf open the link under the cursor; with an unconfirmed selection Enter only confirms it
#[test]
fn enter_opens_link_under_cursor() {
    let mut app = app_with_links(link_document());
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // cursor on the link glyphs
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    assert_eq!(app.link_at_cursor().as_deref(), Some("https://example.com"));
    app.handle(key(KeyCode::Enter));
    let req = app.take_open_request().expect("Enter 该请求打开链接");
    assert_eq!(
        req,
        crate::target::OpenPlan::Spawn {
            program: app.opener_command(),
            args: vec!["https://example.com".to_string()]
        }
    );
    // gf: the same
    app.handle(key(KeyCode::Char('g')));
    app.handle(key(KeyCode::Char('f')));
    assert!(app.take_open_request().is_some(), "gf 也该请求打开");
    // No link: a note, and no fallback to "fit the whole picture"
    app.view.cursor = (20, 12);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    let zoom = app.view.zoom;
    app.handle(key(KeyCode::Char('g')));
    app.handle(key(KeyCode::Char('f')));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.view.cursor, (20, 12), "不能跳走");
    assert_eq!(app.view.zoom, zoom, "不能变成铺满画面");
    assert!(app.view.note.as_deref().unwrap_or("").contains("没有链接"));
    // The unconfirmed selection wins: Enter only confirms it
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (5, 2),
        confirmed: false,
    });
    app.handle(key(KeyCode::Enter));
    assert!(app.take_open_request().is_none(), "选区优先,不打开链接");
    assert!(app.view.sel.expect("选区还在").confirmed);
}

/// With Enter rebound to confirm_selection, Enter no longer opens links
#[test]
fn rebound_enter_does_not_open_link() {
    let mut opts = zh_opts();
    opts.keys.bind(
        crate::keys::Key::parse("enter").expect("解析"),
        Some(crate::keys::Action::ConfirmSelection),
    );
    let mut app = App::new(link_document(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    app.handle(key(KeyCode::Enter));
    assert!(app.take_open_request().is_none(), "改绑后 Enter 只确认选区");
}

/// The menu remembers the last choice: reopening lands on that item (a changed colour stays lit)
#[test]
fn menu_remembers_last_choice() {
    let mut app = app_with_links(row_document(2));
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let i = app.cursor_box().expect("在框上");
    // First time: enter box style and pick double
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    let top = menu.levels.len();
    assert_eq!(top, 1, "第一次开是顶层");
    // Find the box style item and enter it
    let box_style = menu
        .level()
        .iter()
        .position(|it| it.title.contains("框型") || it.title.contains("box style"))
        .expect("有框型");
    app.view.menu.as_mut().unwrap().sel = box_style;
    app.menu_choose();
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        2,
        "进了子菜单"
    );
    let dbl = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|it| it.title == "double")
        .expect("有 double");
    app.view.menu.as_mut().unwrap().sel = dbl;
    app.menu_choose();
    assert!(app.view.menu.is_none(), "选完就关");
    assert_eq!(app.doc.boxes[i].style, crate::model::BoxStyle::Double);
    // Second time: land directly on double inside box style
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    assert_eq!(menu.levels.len(), 2, "记得上次进过子菜单");
    assert_eq!(menu.sel, dbl, "停在同一个选项上");
    assert!(menu.level()[menu.sel].checked, "就是当前值");
    app.menu_cancel();
}

/// Menu memory is a stack: the whole path through nested
/// submenus is remembered, going back up lands on the same item
#[test]
fn menu_remembers_whole_path() {
    let mut app = app_with_links(row_document(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("连边");
    app.refresh_pick();
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.view.pick = render::Pick::Edge(0);
    // Edge menu: port -> start -> right
    app.open_style_menu();
    let find = |app: &App, t: &str| -> usize {
        app.view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == t)
            .unwrap_or_else(|| panic!("没有 {t}"))
    };
    let ports = find(&app, "端口");
    app.view.menu.as_mut().unwrap().sel = ports;
    app.menu_choose();
    let from = find(&app, "起点");
    app.view.menu.as_mut().unwrap().sel = from;
    app.menu_choose();
    let right = find(&app, "right");
    app.view.menu.as_mut().unwrap().sel = right;
    app.menu_choose();
    assert!(app.view.menu.is_none(), "选完就关");
    assert_eq!(app.doc.edges[0].from_side, Some(crate::model::Side::Right));
    // Reopen: all three levels restored
    app.open_style_menu();
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 3, "端口 -> 起点 -> right 三层都记着");
        assert_eq!(m.sel, right, "停在 right");
        assert_eq!(m.parents, vec!["端口".to_string(), "起点".to_string()]);
    }
    // Back one level: lands on start, not the first item
    app.handle(key(KeyCode::Char('h')));
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 2);
        assert_eq!(m.sel, from, "回到进来时那一项");
    }
    app.handle(key(KeyCode::Char('h')));
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 1);
        assert_eq!(m.sel, ports, "再退回到端口那一项");
    }
    app.menu_cancel();
}

/// After changing a box colour, the next s lands directly on the colour submenu cell
#[test]
fn menu_remembers_last_colour() {
    let mut app = app_with_links(row_document(2));
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let i = app.cursor_box().expect("在框上");
    // First time: stroke colour -> 4th colour
    app.open_style_menu();
    let colour = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|it| it.title.contains("边框色") || it.title.contains("border colour"))
        .expect("有边框色");
    app.view.menu.as_mut().unwrap().sel = colour;
    app.menu_choose();
    let want = 4usize;
    app.view.menu.as_mut().unwrap().sel = want;
    let hex = app.view.menu.as_ref().unwrap().level()[want].title.clone();
    app.menu_choose();
    let c = crate::model::PALETTE[want - 1];
    assert_eq!(app.doc.boxes[i].color, Some(c), "颜色改成了 {hex}");
    // Second time: s enters the colour submenu, same cell, and it is the current value
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    assert_eq!(menu.levels.len(), 2, "记得上次进过颜色子菜单");
    assert_eq!(menu.sel, want, "停在同一个颜色上");
    assert!(menu.level()[menu.sel].checked, "就是当前颜色");
    app.menu_cancel();
}

/// The box style menu no longer has edit text, but delete is still there
#[test]
fn box_style_menu_has_delete_but_no_text_edit() {
    let mut app = app_with_links(row_document(2));
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    for it in menu.level() {
        assert!(
            !it.title.contains("改文字") && !it.title.contains("edit text"),
            "还有改文字:{}",
            it.title
        );
    }
    let del = menu
        .level()
        .iter()
        .position(|it| it.title == "删除")
        .expect("该有删除");
    assert_eq!(del, menu.level().len() - 1, "删除在最后一项");
    app.menu_cancel();
}

/// The jump list spans tabs: Ctrl-o returns from another buffer
#[test]
fn jump_list_spans_tabs() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(1); // record a position in the first tab
    let a = app.view.cursor;
    assert_eq!(app.tab_at, 0);
    // open a second tab (same as gf opening another file)
    app.new_tab(None);
    assert_eq!(app.tab_at, 1);
    app.draw(&mut buf, app.area);
    // Ctrl-o: back to the cell in the first tab
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.tab_at, 0, "该切回原来的 tab");
    assert_eq!(app.view.cursor, a, "格子也要回去");
    // Ctrl-i: back to the second tab
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.tab_at, 1, "前进回到另一个 buffer");
}

/// Edit sites (create / edit text / restyle) are jump points too: Ctrl-o returns to them
#[test]
fn edits_are_jump_points() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Create a box in empty space
    let at = (-10, -10);
    app.view.cursor = at;
    app.refresh_pick();
    app.cmd_new("aa");
    assert_eq!(app.view.cursor, at, "建框不动光标");
    // Walk away (plain movement is not a jump point)
    let away = (30, 30);
    app.view.cursor = away;
    app.refresh_pick();
    // Ctrl-o back to the cell where the box was created; Ctrl-i back to where we walked off
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, at, "Ctrl-o 回到建框的地方");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.view.cursor, away, "Tab(Ctrl-i)前进回去");
    // Restyling counts as a jump point too
    app.view.cursor = at;
    app.refresh_pick();
    app.cmd_apply("box double");
    app.view.cursor = away;
    app.refresh_pick();
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, at, "Ctrl-o 回到改样式的地方");
}

/// A new box lands on the cursor cell; if that overlaps another box it shifts right
#[test]
fn new_box_lands_on_cursor() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // cursor in empty space
    app.view.cursor = (-10, -10);
    app.refresh_pick();
    app.cmd_new("q");
    let last = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[last].at, (-10, -10), "该建在光标格");
    // cursor on a box: shift right, no overlap
    app.view.cursor = app.doc.boxes[0].at;
    app.refresh_pick();
    app.cmd_new("w");
    let last = app.doc.boxes.len() - 1;
    let at = app.doc.boxes[last].at;
    for b in &app.doc.boxes[..last] {
        let overlap = at.0 < b.at.0 + b.size.0 as i32
            && b.at.0 < at.0 + app.doc.boxes[last].size.0 as i32
            && at.1 < b.at.1 + b.size.1 as i32
            && b.at.1 < at.1 + 3;
        assert!(!overlap, "新框 {at:?} 压住了 {}", b.id);
    }
}

/// Ctrl-q starts block selection from any cell
#[test]
fn only_v_starts_block_selection() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let (x0, y0, x1, y1) = app.doc.bbox();
    app.view.cursor = (x0 - 2, y0 - 2);
    app.refresh_pick();
    // Ctrl-q / V have been removed (key de-duplication)
    app.handle(ctrl(KeyCode::Char('q')));
    assert!(app.view.sel.is_none(), "Ctrl-q 不该再开始选区");
    app.handle(key(KeyCode::Char('V')));
    assert!(app.view.sel.is_none(), "V 不该再开始选区");
    // v still takes that path
    app.handle(key(KeyCode::Char('v')));
    let sel = app.view.sel.expect("v 该有选区");
    assert_eq!(sel.anchor, (x0 - 2, y0 - 2), "锚点落在光标格");
    assert!(!sel.confirmed);
    // grow to cover the whole picture
    app.view.cursor = (x1 + 2, y1 + 2);
    let sel = render::selected(
        &app.doc,
        &render::panels(&app.doc, app.area, &app.view),
        &app.view,
    );
    assert!(!sel.is_empty(), "大区块里该有框");
    app.handle(key(KeyCode::Esc));
    assert!(app.view.sel.is_none(), "esc 取消");
}

/// The i / t / c keys go through the prompt line and run on Enter
#[test]
fn prompt_line_creates_edits_and_links() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // On a box i edits in place; only on empty space does it create
    app.handle(key(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "框上按 i 该就地改字");
    app.view.edit = None;
    app.view.cursor = (-10, -10);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "空白处直接建框并进就地编辑");
    for c in "Z".chars() {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(key(KeyCode::Esc));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.boxes.len(), 2);
    assert_eq!(app.doc.text(1).unwrap().text, "Z");
    assert_eq!(app.undo.counts().0, 2, "建框一步、写文字一步");

    // Esc cancels without a step (it drops the box just created)
    app.handle(key(KeyCode::Char('i')));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.doc.boxes.len(), 2, "取消不留空框");

    // :text -> edit the cursor box text (the prompt path)
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    let i = app.cursor_box().unwrap();
    app.cmd_apply("text hello");
    assert_eq!(app.doc.text(i).unwrap().text, "hello");

    // :link -> link to another box (c opens the colour menu now); text changed, look up by id
    let other = app.doc.boxes[1].id.clone();
    app.cmd_apply(&format!("link {other}"));
    assert_eq!(app.doc.edges.len(), 1);
}

/// d works like x: delete the box under the cursor or the selected edge
#[test]
fn d_deletes_element_under_cursor() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.cmd_apply("new b");
    let n = app.doc.boxes.len();
    assert_eq!(n, 2);
    // The cursor lands inside the new box: d deletes it
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n - 1, "d 该删掉光标处的框");
    assert!(app.view.note.clone().unwrap_or_default().contains("剪切"));
    // The cursor stays where it was (empty); to delete the last box move onto it first
    app.jump_to(0);
    app.cmd_apply("del");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("最后一个框"), "{note}");
}

/// Selection: edits are blocked while unconfirmed; once
/// confirmed group delete and group style are one step each
#[test]
fn selection_confirm_group_delete_paste() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Cover the first two boxes (leave one, so the "cannot delete all" rule does not trigger)
    let b0 = app.doc.boxes[0].at;
    let (bx, by, bw, bh) = {
        let b1 = &app.doc.boxes[1];
        (b1.at.0, b1.at.1, b1.size.0 as i32, b1.size.1 as i32)
    };
    let (rx, ry) = (bx + bw + 1, by + bh + 1);
    app.view.cursor = (b0.0 - 1, b0.1 - 1);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = (rx, ry);
    app.refresh_pick();
    let sel = app.view.sel.expect("该有选区");
    assert!(!sel.confirmed, "刚拉出来是未确认");

    // Unconfirmed: the document edit is refused, no undo step
    let undo0 = app.undo.counts().0;
    let n0 = app.doc.boxes.len();
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n0, "未确认不该删");
    assert!(app.view.note.clone().unwrap_or_default().contains("Enter"));
    assert_eq!(app.undo.counts().0, undo0, "被拒绝不该压撤销");

    // Confirmed: batch delete, one undo step, the content goes to the clipboard
    app.handle(key(KeyCode::Enter));
    assert!(app.view.sel.unwrap().confirmed);
    let targets = app.targets();
    assert!(targets.boxes.len() >= 2, "选区该盖住至少两个框");
    let nb = targets.boxes.len();
    app.handle(key(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n0 - nb, "该删掉选区里的框");
    assert_eq!(app.undo.counts().0, undo0 + 1, "组删除只算一步");
    let clip = app.clipboard.clone().expect("删除该写剪贴板");
    assert_eq!(clip.boxes.len(), nb);
    assert!(!clip.texts.is_empty());
    // Undo restores everything, the clipboard survives
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.len(), n0, "撤销恢复全部");
    assert!(app.clipboard.is_some(), "撤销不清剪贴板");
    // Paste one copy
    app.cmd_apply("paste");
    assert_eq!(app.doc.boxes.len(), n0 + nb, "粘贴出一份副本");
    // Paste again, ids do not clash
    let before: std::collections::HashSet<String> =
        app.doc.boxes.iter().map(|b| b.id.clone()).collect();
    app.cmd_apply("paste");
    let after: std::collections::HashSet<String> =
        app.doc.boxes.iter().map(|b| b.id.clone()).collect();
    assert!(after.len() > before.len(), "id 不该撞");
    // Delete the same selection again: would it delete everything? The whole batch is refused
    app.view.sel = None;
    app.refresh_pick();
    app.view.cursor = (b0.0 - 1, b0.1 - 1);
    app.handle(key(KeyCode::Char('v')));
    app.view.cursor = (rx, ry);
    app.handle(key(KeyCode::Enter));
}

/// `:` completion: verb prefixes, value enums, box names / layer names / file names
#[test]
fn command_line_completion() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // verb prefixes
    app.view.cmd = Some("fi".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("fill"));
    assert!(app.view.cmd_menu.is_some(), "该弹补全窗");
    // values
    app.view.cmd = Some("box ".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("box point"), "第一个候选");
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("box none"), "Tab 轮换");
    app.complete_cmd(true);
    assert_eq!(app.view.cmd.as_deref(), Some("box point"), "Shift-Tab 往回");
    // the four dir values
    app.view.cmd = Some("dir b".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("dir back"), "按前缀过滤");
    // box names: link completes id/text
    app.view.cmd = Some("link b".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    let line = app.view.cmd.clone().unwrap();
    assert!(line.starts_with("link b"), "{line}");
    // typing closes the popup
    app.handle(key(KeyCode::Enter));
    assert!(app.view.cmd_menu.is_none());
}

/// The path hint on the right of the command line: where :w writes, where :e reads
#[test]
fn command_line_path_hint() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // No source: say there is no file name yet
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("还没有文件名"), "{hint}");

    // .dot source: by default writes <stem>.gtui next to it and says the source stays untouched
    app.source = Some("/tmp/demo/chart.dot".to_string());
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("/tmp/demo/chart.gtui"), "{hint}");
    assert!(hint.contains("不动"), "{hint}");

    // .gtui source: overwrites the original
    app.source = Some("/tmp/demo/a.gtui".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(
        hint.contains("/tmp/demo/a.gtui") && hint.contains("覆盖"),
        "{hint}"
    );

    // A typed path: hint the resolved absolute path, the input itself is unchanged
    app.view.cmd = Some("w out/x.gtui".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(hint, cwd.join("out/x.gtui").to_string_lossy().to_string());
    assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));

    // :e with no argument: Enter opens the list
    app.view.cmd = Some("e".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("文件列表"), "{hint}");
    // unrelated verb: no hint
    app.view.cmd = Some("notes on".to_string());
    app.refresh_cmd_hint();
    assert!(app.view.cmd_hint.is_none());
    // cleared after Enter
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    app.handle(key(KeyCode::Enter));
    assert!(app.view.cmd_hint.is_none());
}

/// `:w ` with no argument: Tab fills the default path; with spaces the whole segment is replaced
#[test]
fn tab_fills_default_path_and_keeps_spaces() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // No source: nothing to fill, say so
    app.view.cmd = Some("w ".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w "), "空白缓冲不瞎填");
    app.view.cmd = Some("w".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w"), "没空格也不瞎填");
    assert!(app.view.note.is_some());

    // .dot source: fill the absolute path, editing continues
    app.source = Some("/tmp/demo/chart.dot".to_string());
    app.view.cmd = Some("w ".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w /tmp/demo/chart.gtui"));
    assert!(app.view.cmd_hint.as_deref().unwrap().contains("chart.gtui"));

    // Argument with spaces: completion replaces the whole segment, not just after the last space
    let dir = std::env::temp_dir().join(format!("gtui-tab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a b.gtui"), "{}").unwrap();
    let path = format!("{}/a", dir.to_str().unwrap());
    app.view.cmd = Some(format!("w {path}"));
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    let line = app.view.cmd.clone().unwrap();
    assert_eq!(line, format!("w {}/a b.gtui", dir.to_str().unwrap()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The source file on disk changed externally: with no local edits it reloads automatically,
/// with edits it is marked stale; `R` twice or `:e!` forces the reload
#[test]
fn external_change_reloads_clean_document() {
    let dir = std::env::temp_dir().join(format!("gtui-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("w.gtui").to_str().unwrap().to_string();
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply(&format!("w {path}"));
    assert!(!app.check_disk(), "刚存过:没变化");
    let mine = label_of(&app.doc, 0);
    assert!(!mine.is_empty());

    // external change: another App saves a different text to the same path
    let mut other = App::new(sample_document(), zh_opts()).expect("开局");
    other.doc.set_text(0, "外部改的".to_string());
    other.save_document(&path).expect("外部写入");
    // No local edits -> take the disk version
    assert!(app.check_disk(), "该发现改动");
    assert_eq!(label_of(&app.doc, 0), "外部改的", "没本地改动:直接重新载入");
    assert!(!app.view.stale);
    assert!(app.view.note.as_deref().unwrap().contains("重新载入"));

    // Edit one line while the disk changes: only mark stale, do not touch the document
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
    app.cmd_apply("text 本地改过");
    other.doc.set_text(0, "又改了一次".to_string());
    other.save_document(&path).expect("外部再写");
    assert!(app.check_disk());
    assert!(app.view.stale, "有本地改动:只提醒");
    assert_eq!(label_of(&app.doc, 0), "本地改过", "文档不该被换掉");
    assert!(app.view.note.as_deref().unwrap().contains("R"));

    // The first R only warns, the second drops the edits and reloads
    app.reload_step();
    assert_eq!(label_of(&app.doc, 0), "本地改过");
    app.reload_step();
    assert_eq!(label_of(&app.doc, 0), "又改了一次", "第二下 R 真的重读");
    assert!(!app.view.stale);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `:e!` forces a reload regardless of local edits
#[test]
fn force_reload() {
    let dir = std::env::temp_dir().join(format!("gtui-force-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f.gtui").to_str().unwrap().to_string();
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply(&format!("w {path}"));
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
    app.cmd_apply("text 改了");
    let mut other = App::new(sample_document(), zh_opts()).expect("开局");
    other.doc.set_text(0, "磁盘上的".to_string());
    other.save_document(&path).expect("外部写入");
    app.cmd_apply("e!");
    assert_eq!(label_of(&app.doc, 0), "磁盘上的", ":e! 不理会本地改动");
    assert!(!app.view.stale);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Zoom anchors on the pointer / cursor: the canvas
/// coordinate under the same screen cell is unchanged
#[test]
fn zoom_anchors_on_cursor() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.center = None;
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    // The cursor's screen cell is the anchor; record the canvas coord under it, it must stay
    let target = app.doc.center(0);
    app.view.cursor = (target.0.round() as i32, target.1.round() as i32);
    app.view.pick = render::Pick::Box(0);
    let at = app.cursor_screen_cell();
    let point = p.proj.canvas_f(at.0, at.1);

    // Zoom in three steps: the target stays under that cell (1 cell rounding allowed)
    for _ in 0..3 {
        app.zoom_at(app.view.zoom * ZOOM_STEP, at);
        let p = render::panels(&app.doc, app.area, &app.view);
        let now = p.proj.cell(point);
        assert!(
            (now.0 - at.0).abs() <= 1 && (now.1 - at.1).abs() <= 1,
            "锚点漂了: {at:?} -> {now:?} (zoom {})",
            app.view.zoom
        );
    }
    assert!(app.view.zoom > 1.5, "该放大了:{}", app.view.zoom);
    // Zooming back near 1:1 snaps to exactly 1.0
    app.zoom_at(1.02, at);
    assert_eq!(app.view.zoom, 1.0, "1:1 附近要吸附");
}

/// Entering editing from a zoomed view: switch to 1:1
/// without a jump, the cursor stays near the same cell
#[test]
fn edit_returns_to_unity_without_jump() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.center = None;
    let target = app.doc.center(0);
    app.view.cursor = (target.0.round() as i32, target.1.round() as i32);
    app.view.pick = render::Pick::Box(0);
    app.view.zoom = 2.0;
    let before = app.cursor_screen_cell();
    app.require_edit_scale();
    assert_eq!(app.view.zoom, 1.0);
    let after = app.cursor_screen_cell();
    assert!(
        (after.0 - before.0).abs() <= 1 && (after.1 - before.1).abs() <= 1,
        "切回 1:1 时跳了: {before:?} -> {after:?}"
    );
    assert!(app.view.note.as_deref().unwrap().contains("1:1"));
}

/// The wheel zooms at the pointer, not around the view centre
#[test]
fn wheel_zooms_at_pointer() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.center = None;
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    // pointer resting on a box
    let target = app.doc.center(0);
    let at = p.proj.cell(target);
    let me = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: at.0.clamp(0, 79) as u16,
        row: at.1.clamp(0, 23) as u16,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    // Use the clamped cell as the anchor and compare with an equally clamped target
    let anchor = (me.column as i32, me.row as i32);
    let point = p.proj.canvas_f(anchor.0, anchor.1);
    app.handle_mouse(me);
    let p = render::panels(&app.doc, app.area, &app.view);
    let now = p.proj.cell(point);
    assert!(
        (now.0 - anchor.0).abs() <= 1 && (now.1 - anchor.1).abs() <= 1,
        "指针下的点跑了: {anchor:?} -> {now:?}"
    );
    assert!(
        (app.view.zoom - WHEEL_STEP).abs() < 1e-9,
        "{}",
        app.view.zoom
    );
    let _ = target;
}

/// Mouse gestures: drag from a border to another box =
/// link; to empty space = cancel; from an endpoint = relink
fn mev(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}

#[test]
fn drag_links_and_reconnects() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    // Both boxes at 1:1, take their screen rects directly
    let p = render::panels(&app.doc, app.area, &app.view);
    let ra = render::rect_of(&app.doc, 0, &p.proj);
    // Drop on the third box: the first two already have an edge, duplicates are removed
    let rb = render::rect_of(&app.doc, 2, &p.proj);
    let (bx, by) = ((rb.0 + rb.2 / 2) as u16, (rb.1 + rb.3 / 2) as u16);
    // Press the top border of A, drag inside B, release
    let top = (ra.0 + 1) as u16;
    app.handle_mouse(mev(
        MouseEventKind::Down(MouseButton::Left),
        top,
        ra.1 as u16,
    ));
    assert!(
        matches!(app.drag, Some(Drag::Link { .. })),
        "边框起拖该进连边"
    );
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), bx, by));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), bx, by));
    assert!(
        app.doc
            .edges
            .iter()
            .any(|e| e.from == app.doc.boxes[0].id && e.to == app.doc.boxes[2].id),
        "该建出边:{:?}",
        app.doc
            .edges
            .iter()
            .map(|e| (&e.from, &e.to))
            .collect::<Vec<_>>()
    );
    // Drag to empty space: cancel, no edge
    let before = app.doc.edges.len();
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let ra = render::rect_of(&app.doc, 0, &p.proj);
    app.handle_mouse(mev(
        MouseEventKind::Down(MouseButton::Left),
        top,
        ra.1 as u16,
    ));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), 3, 2));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), 3, 2));
    assert_eq!(app.doc.edges.len(), before, "空白处松开不该建边");
    assert!(app.view.link.is_none(), "预览要收掉");

    // Relink endpoint: select an edge and drag from its endpoint cell to another box
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let ei = 0;
    let ends = app.usable_hit().edge_ends.get(&ei).copied().expect("端点");
    let Some(other) = (0..app.doc.boxes.len()).find(|&i| {
        let r = render::rect_of(&app.doc, i, &p.proj);
        let c = (r.0 + r.2 / 2, r.1 + r.3 / 2);
        c != ends.0 && c != ends.1
    }) else {
        return;
    };
    let to_id = app.doc.boxes[other].id.clone();
    app.view.pick = render::Pick::Edge(ei);
    let (ex, ey) = (ends.1 .0 as u16, ends.1 .1 as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Left), ex, ey));
    assert!(matches!(app.drag, Some(Drag::Reconnect { .. })), "端点起拖");
    let r = render::rect_of(&app.doc, other, &p.proj);
    let (cx, cy) = ((r.0 + r.2 / 2) as u16, (r.1 + r.3 / 2) as u16);
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), cx, cy));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), cx, cy));
    assert_eq!(app.doc.edges[ei].to, to_id, "终点该改接到另一个框");
}

/// Right drag past the threshold = pan; below it
/// (including a 1-cell jitter) = menu on the pressed cell
#[test]
fn right_drag_pans_click_opens_menu() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let before = app.view.center;

    // 1) drag 3 cells: pan, no menu
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), 40, 12));
    assert!(app.view.menu.is_none(), "按下时不该弹菜单");
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), 43, 12));
    assert_ne!(app.view.center, before, "拖过阈值该平移");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), 43, 12));
    assert!(app.view.menu.is_none(), "拖过就不弹菜单");
    assert!(app.drag.is_none(), "松手清掉拖动状态");

    // 2) jitter 1 cell: counts as a click, the menu opens on the pressed cell
    app.view.center = before;
    let p = render::panels(&app.doc, app.area, &app.view);
    let (px, py) = (p.map.x + 4, p.map.y + 4);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 1, py));
    assert_eq!(app.view.center, before, "1 格不该平移");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px + 1, py));
    let menu = app.view.menu.as_ref().expect("该弹菜单");
    assert_eq!(menu.at, (px, py), "菜单开在按下格");
    app.menu_cancel();

    // 3) drag out and back to the origin: the threshold was crossed, release still opens no menu
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 5, py));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px, py));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px, py));
    assert!(app.view.menu.is_none(), "拖过再回来也不弹菜单");

    // 4) with the menu open a right click only cancels and does not start panning
    app.open_menu(px, py);
    assert!(app.view.menu.is_some());
    let center = app.view.center;
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
    assert!(app.view.menu.is_none(), "右键该先取消菜单");
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 6, py));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px + 6, py));
    assert_eq!(app.view.center, center, "取消菜单之后这一下不该平移");
}

/// Right drag from a box: pans only, does not move the box, change the selection or push undo
/// While right-dragging a link: the preview follows
/// the cursor off target and snaps to the box on entry
#[test]
fn right_drag_link_preview_follows_pointer() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = app_with_links(row_document(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let r1 = render::rect_of(&app.doc, 1, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    // Empty space: the preview follows the cursor, target empty
    // The rows below the box are empty: moving the pointer there clears the target
    let (ex, ey) = ((r0.0 + 2) as u16, (r0.1 + r0.3 + 3) as u16);
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
    let link = app.view.link.clone().expect("该有连边预览");
    assert_eq!(link.sources, vec![0]);
    assert!(link.targets.is_empty(), "空白处没有目标框");
    assert_eq!(
        app.view.cursor,
        p.proj.canvas(ex as i32, ey as i32),
        "光标跟着指针走"
    );
    // Entering the target box: target attached
    let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
    let link = app.view.link.clone().expect("该还在连边");
    assert_eq!(link.targets, vec![1], "指针下的框成为目标");
    // Back to empty space: target cleared
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
    assert!(app
        .view
        .link
        .as_ref()
        .map(|l| l.targets.is_empty())
        .unwrap_or(false));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), ex, ey));
    assert!(app.view.link.is_none(), "松开后清掉");
}

/// Right drag starts a link on point / none nodes too; on an edge it pans
#[test]
fn right_drag_on_point_nodes_and_edges() {
    use crossterm::event::{MouseButton, MouseEventKind};
    // point node
    let mut doc = row_document(2);
    doc.boxes[1].style = crate::model::BoxStyle::Point;
    doc.rebuild();
    let mut app = app_with_links(doc);
    let p = render::panels(&app.doc, app.area, &app.view);
    let point = 1usize;
    let r = render::rect_of(&app.doc, point, &p.proj);
    app.handle_mouse(mev(
        MouseEventKind::Down(MouseButton::Right),
        r.0 as u16,
        r.1 as u16,
    ));
    assert!(
        matches!(app.drag, Some(Drag::RightLink { .. })),
        "点节点上该起连边"
    );
    app.handle_mouse(mev(
        MouseEventKind::Up(MouseButton::Right),
        r.0 as u16,
        r.1 as u16,
    ));

    // On an edge: treated as empty -> pan
    let mut app = app_with_links(sample_document());
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let (ex, ey) = app
        .hit
        .edge_cells
        .values()
        .flatten()
        .find(|c| app.doc.box_at_cell(**c).is_none())
        .copied()
        .expect("样例该有边笔画");
    let (sx, sy) = (ex as u16, ey as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    assert!(
        matches!(app.drag, Some(Drag::RightPan { .. })),
        "边上该按空白处理"
    );
}

#[test]
fn right_drag_on_box_links() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = app_with_links(row_document(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let r1 = render::rect_of(&app.doc, 1, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
    let center = app.view.center;
    let edges0 = app.doc.edges.len();
    let at0: Vec<(i32, i32)> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
    assert!(app.view.link.is_some(), "拖动中途该有连边预览");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), tx, ty));
    assert_eq!(app.view.center, center, "框上拖不再平移");
    assert_eq!(
        app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(),
        at0,
        "框不该动"
    );
    assert_eq!(app.doc.edges.len(), edges0 + 1, "该建一条边");
    assert!(app.view.link.is_none(), "松开后连边状态清掉");

    // Release in empty space: no edge
    let mut app = app_with_links(row_document(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    let (ex, ey) = ((p.map.x + 2), (p.map.y + 2));
    let edges0 = app.doc.edges.len();
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), ex, ey));
    assert_eq!(app.doc.edges.len(), edges0, "空白处松开不建边");
    assert!(app.view.link.is_none());
    assert!(app.view.note.clone().unwrap_or_default().contains("没连边"));

    // mouse_edges = false: right drag on a box pans only, no link
    let mut app = app_with_links(row_document(2));
    app.mouse_edges = false;
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let r1 = render::rect_of(&app.doc, 1, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
    let edges0 = app.doc.edges.len();
    let center = app.view.center;
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
    assert!(app.view.link.is_none(), "关掉后不该出现连边预览");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), tx, ty));
    assert_eq!(app.doc.edges.len(), edges0, "关掉后拖不出边");
    assert_ne!(app.view.center, center, "改成平移");
    // A click still opens the menu
    let mut app = app_with_links(row_document(2));
    app.mouse_edges = false;
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), sx, sy));
    assert!(app.view.menu.is_some(), "关掉连边不影响右键菜单");

    // Below the threshold: the menu opens
    let mut app = app_with_links(row_document(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), sx + 1, sy));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), sx + 1, sy));
    assert!(app.view.menu.is_some(), "没拖过 2 格就是单击");
    assert_eq!(app.doc.edges.len(), 0);
}

/// Dragging near the viewport edge pans automatically
#[test]
fn drag_to_edge_auto_pans() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    app.view.center = Some((0.0, 0.0));
    let p = render::panels(&app.doc, app.area, &app.view);
    let ra = render::rect_of(&app.doc, 0, &p.proj);
    app.handle_mouse(mev(
        MouseEventKind::Down(MouseButton::Left),
        (ra.0 + 1) as u16,
        ra.1 as u16,
    ));
    let before = app.view.center;
    // Drag to the far right edge of the map
    let x = p.map.x + p.map.width - 1;
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), x, 5));
    assert_ne!(app.view.center, before, "靠边该自动平移");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), x, 5));
}

/// Multiple documents: each tab keeps its document / cursor / zoom / undo, the clipboard is shared
#[test]
fn tabs_are_independent() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // tab 1: edit text, zoom
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.cmd_apply("text tab-one");
    app.view.zoom = 2.0;
    let undo1 = app.undo.counts();
    // new tab: content / cursor / zoom come from the new document
    app.new_tab(None);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tab_at, 1);
    assert_ne!(label_of(&app.doc, 0), "tab-one", "新 tab 是空文档");
    assert_eq!(app.view.zoom, 1.0);
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.cmd_apply("text tab-two");
    // back to tab 1: content, zoom and undo history are all there
    app.switch_tab(0);
    assert_eq!(label_of(&app.doc, 0), "tab-one");
    assert_eq!(app.view.zoom, 2.0);
    assert_eq!(app.undo.counts(), undo1);
    // Undo in tab 1 does not affect tab 2
    app.undo_step();
    assert_ne!(label_of(&app.doc, 0), "tab-one");
    app.switch_tab(1);
    assert_eq!(label_of(&app.doc, 0), "tab-two");
}

/// A dirty tab cannot be closed, only `!` discards it; closing lands on the adjacent tab
#[test]
fn closing_tab_respects_dirty_flag() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.new_tab(None);
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.cmd_apply("text dirty");
    app.cmd_apply("tabclose");
    assert_eq!(app.tabs.len(), 2, "脏 tab 不该被关掉");
    app.cmd_apply("tabclose!");
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tab_at, 0);
    // Closing the last remaining tab = quit
    app.cmd_apply("tabclose!");
    assert!(app.quit);
}

/// The :tabs list and the switching commands with several tabs
#[test]
fn tabs_command_and_list() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.new_tab(None);
    app.cmd_apply("tab 1");
    assert_eq!(app.tab_at, 0);
    app.cmd_apply("tabnext");
    assert_eq!(app.tab_at, 1);
    app.cmd_apply("tabnext");
    assert_eq!(app.tab_at, 0, "循环");
    app.cmd_apply("tabprev");
    assert_eq!(app.tab_at, 1);
    app.cmd_apply("tabs");
    let list = app.view.note.clone().unwrap();
    assert!(list.contains("1") && list.contains("2"), "{list}");
}

/// Drag a box at any zoom: no switch to 1:1, box / cursor
/// follow the canvas delta (Drag state, no border gesture)
#[test]
fn drag_box_while_zoomed() {
    use crossterm::event::{MouseButton, MouseEventKind};
    for (zoom, mouse_steps, want_cells) in [(2.0, 2, 1), (0.5, 1, 2), (1.0, 3, 3)] {
        let mut app = App::new(sample_document(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.view.zoom = zoom;
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let before = app.doc.boxes[0].at;
        app.view.cursor = before;
        // Always start from the same screen cell; the delta
        // depends only on how many cells the mouse moved
        let from = (10u16, 10u16);
        app.drag = Some(Drag::Boxes {
            from,
            at: vec![(0, before)],
            proj: p.proj,
            cursor0: app.view.cursor,
            sel0: None,
            began: false,
        });
        let undo_before = app.undo.counts();
        app.handle_mouse(mev(
            MouseEventKind::Drag(MouseButton::Left),
            from.0 + mouse_steps,
            from.1,
        ));
        assert_eq!(app.view.zoom, zoom, "拖框不该改缩放");
        assert!(app.view.drag_hide_sel, "拖动期间该藏起选区矩形");
        assert_eq!(
            app.doc.boxes[0].at.0,
            before.0 + want_cells,
            "zoom {zoom}: 鼠标走 {mouse_steps} 格该挪 {want_cells} 格"
        );
        assert_eq!(app.view.cursor.0, before.0 + want_cells, "光标跟着走");
        let undo_after = app.undo.counts();
        assert_eq!(undo_after.0, undo_before.0 + 1, "开始拖就压一步");
        app.handle_mouse(mev(
            MouseEventKind::Up(MouseButton::Left),
            from.0 + mouse_steps,
            from.1,
        ));
        assert!(!app.view.drag_hide_sel, "松手该恢复选区矩形");
        assert_eq!(
            app.undo.counts().0,
            undo_before.0 + 1,
            "一次拖动只有一步撤销"
        );
        assert_eq!(app.view.pick.box_at(), Some(0), "松手后光标仍在该框");
    }
}

/// Multi-select drag: relative positions hold, cursor and
/// selection move by the same delta; ten drags do not drift
#[test]
fn multi_selection_drag_keeps_offsets() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.view.zoom = 1.0;
    // select two boxes
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (20, 20),
        confirmed: true,
    });
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let r = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r.0 + r.2 / 2) as u16, (r.1 + r.3 / 2) as u16);
    let at0: Vec<(i32, i32)> = app.doc.boxes.iter().map(|b| b.at).collect();
    let sel0 = app.view.sel.unwrap();
    for _ in 0..10 {
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Left), sx, sy));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), sx + 1, sy));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), sx + 1, sy));
    }
    for (i, &(x, _)) in at0.iter().enumerate() {
        assert_eq!(
            app.doc.boxes[i].at.0,
            x + 10,
            "框 {i} 该整体挪 10 格,不漂移"
        );
    }
    let sel1 = app.view.sel.expect("选区还在");
    assert_eq!(sel1.anchor.0, sel0.anchor.0 + 10, "选区矩形跟着走");
    assert_eq!(sel1.end.0, sel0.end.0 + 10);
}

/// Legend removed: `:legend` is neither completed nor
/// a command (completion and help must not mention it)
#[test]
fn legend_command_removed() {
    assert!(!VERBS.contains(&"legend"), ":legend 不该再是命令:{VERBS:?}");
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply("legend on");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("legend"), "该报未知命令:{note}");
    assert!(
        note.contains("未知命令") || note.contains("unknown command"),
        "{note}"
    );
}

/// A missing .gtui is treated as a new file (like nvim); a missing .dot is still an error
#[test]
fn missing_gtui_is_a_new_file() {
    let dir = std::env::temp_dir().join(format!(
        "graph-tui-newfile-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let gtui = dir.join("fresh.gtui");
    let dot = dir.join("fresh.dot");
    let (doc, fresh) = App::read_doc(gtui.to_str().unwrap()).expect("新文件该能开");
    assert!(fresh, "标记成新文件");
    assert_eq!(doc.boxes.len(), 1, "空白文档带一个起始框");
    assert!(
        App::read_doc(dot.to_str().unwrap()).is_err(),
        "缺 .dot 仍然报错"
    );
    // a real file: not marked as new
    std::fs::write(&gtui, Document::scratch().save()).expect("写盘");
    let (_, fresh) = App::read_doc(gtui.to_str().unwrap()).expect("读得回来");
    assert!(!fresh);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Menu: moving the selection previews (no undo), a click applies; cancel rolls back
#[test]
fn menu_preview_and_rollback() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    let undo0 = app.undo.counts().0;
    let p = render::panels(&app.doc, app.area, &app.view);
    let (cx, cy) = p
        .proj
        .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
    app.open_menu(cx as u16, cy as u16);
    // The first item is box style (a submenu): the document is untouched
    assert_eq!(app.doc.boxes.len(), n0);

    // Enter the stroke colour submenu and preview a colour: the colour changes, no undo step
    let colour = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|i| i.title == "边框色")
        .expect("框菜单该有边框色");
    app.view.menu.as_mut().unwrap().sel = colour;
    app.menu_choose();
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        2,
        "进了子菜单"
    );
    let pick = 3usize;
    app.view.menu.as_mut().unwrap().sel = pick;
    app.menu_preview();
    assert_eq!(
        app.doc.boxes[0].color,
        Some(crate::model::PALETTE[pick - 1]),
        "预览该立刻生效"
    );
    assert_eq!(app.undo.counts().0, undo0, "预览不压撤销");

    // Cancel: back to the original
    app.menu_cancel();
    assert_eq!(app.doc.boxes[0].color, None, "取消该回滚");
    assert!(app.view.menu.is_none());

    // Reopen: a cancelled preview is not remembered (top level again); go through and apply
    let victim = app.view.pick.box_at().expect("光标该在框上");
    app.open_menu(cx as u16, cy as u16);
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        1,
        "取消不算选过"
    );
    app.view.menu.as_mut().unwrap().sel = colour;
    app.menu_choose();
    app.view.menu.as_mut().unwrap().sel = pick;
    app.menu_choose();
    assert_eq!(
        app.doc.boxes[victim].color,
        Some(crate::model::PALETTE[pick - 1]),
        "应用该改掉"
    );
    assert_eq!(app.undo.counts().0, undo0 + 1, "应用才算一步");
    // This time it is remembered: reopening lands on the colour submenu cell
    app.open_menu(cx as u16, cy as u16);
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 2, "该记得上次进过颜色子菜单");
        assert_eq!(m.sel, pick, "该停在上次选的颜色上");
        assert!(m.level()[m.sel].checked, "就是当前颜色");
    }
    app.menu_cancel();
}

/// Right click menu: one level of items for the element under the cursor, submenus go in and out
#[test]
fn right_click_menu_items_by_element() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // put it on the first box and open the menu
    app.jump_to(0);
    let p = render::panels(&app.doc, app.area, &app.view);
    let (cx, cy) = p
        .proj
        .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
    app.open_menu(cx as u16, cy as u16);
    let menu = app.view.menu.clone().expect("该弹菜单");
    let titles: Vec<String> = menu.level().iter().map(|i| i.title.clone()).collect();
    assert!(titles.iter().any(|t| t == "框型"), "{titles:?}");
    assert!(titles.iter().any(|t| t == "边框色"), "{titles:?}");
    assert!(titles.iter().any(|t| t == "删除"), "{titles:?}");

    // enter box style, pick heavy, run it
    let idx = titles.iter().position(|t| t == "框型").unwrap();
    app.view.menu.as_mut().unwrap().sel = idx;
    app.menu_choose();
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        2,
        "该进子菜单"
    );
    let heavy = app
        .view
        .menu
        .as_ref()
        .unwrap()
        .level()
        .iter()
        .position(|i| i.title == "heavy")
        .expect("有 heavy");
    app.view.menu.as_mut().unwrap().sel = heavy;
    app.menu_choose();
    assert!(app.view.menu.is_none(), "执行完该关掉");
    assert_eq!(app.doc.boxes[0].style.name(), "heavy");
    assert_eq!(app.undo.counts().0, 1, "菜单执行也只算一步");

    // right click on empty space offers new box
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    app.open_menu(1, 1);
    let m = app.view.menu.clone().unwrap();
    assert_eq!(m.level()[0].title, "新建框…");
}

/// Undo/redo: setting the same value pushes no step, create / relayout can be undone
#[test]
fn undo_and_redo() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    let before = app.doc.boxes[0].fill;
    app.cmd_apply("fill #123456");
    assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)));
    // the same value again is not a step
    app.cmd_apply("fill #123456");
    assert_eq!(app.undo.counts(), (1, 0), "同值设置不该多一步");

    app.cmd_apply("fill #654321");
    assert_eq!(app.undo.counts(), (2, 0));
    app.handle(key(KeyCode::Char('u')));
    assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)), "撤销一步");
    assert_eq!(app.undo.counts(), (1, 1));
    app.handle(ctrl(KeyCode::Char('r')));
    assert_eq!(app.doc.boxes[0].fill, Some((0x65, 0x43, 0x21)), "重做回来");
    // Ctrl-Z also undoes (same as u)
    app.handle(ctrl(KeyCode::Char('z')));
    assert_eq!(
        app.doc.boxes[0].fill,
        Some((0x12, 0x34, 0x56)),
        "Ctrl-Z 撤销一步"
    );
    app.handle(ctrl(KeyCode::Char('r')));
    assert_eq!(app.doc.boxes[0].fill, Some((0x65, 0x43, 0x21)));

    // create a box -> undo
    let n0 = app.doc.boxes.len();
    app.cmd_apply("new z");
    assert_eq!(app.doc.boxes.len(), n0 + 1);
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.len(), n0, "撤销建框");
    assert!(app.view.note.clone().unwrap_or_default().contains("撤销"));

    // relayout -> undo restores the coordinates
    let at0: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.cmd_apply("layout force");
    assert_ne!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);

    // editing after undo clears the redo stack
    app.cmd_apply("undo");
    assert!(app.undo.counts().1 > 0, "撤销之后该有可重做的");
    app.cmd_apply("fill #abcdef");
    assert_eq!(app.undo.counts().1, 0, "新动作清掉重做栈");
    let _ = before;
}

/// The command line / search caret uses the raw text: `/` in a path is not an italic marker
#[test]
fn command_line_caret_ignores_markup() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.cmd = Some("w /examples/".to_string());
    app.view.cmd_pos = 12;
    let (x, _) = app.cursor_cell().expect("该有光标");
    // `:` plus the raw text width, not the width after parsing /examples/ away
    assert_eq!(x, render::raw_width(":w /examples/") as u16);
    app.view.cmd = Some("text *a*b*".to_string());
    app.view.cmd_pos = 11;
    let (x2, _) = app.cursor_cell().expect("该有光标");
    assert_eq!(x2, render::raw_width(":text *a*b*") as u16);
}

#[test]
fn custom_binding_applies_default_unchanged() {
    use crate::keys::{Action as K, Key, KeyMap};
    let mut opts = zh_opts();
    let mut km = KeyMap::defaults();
    km.bind(Key::parse("h").unwrap(), Some(K::MoveDown));
    opts.keys = km;
    let mut app = App::new(sample_document(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let before = app.view.cursor;
    app.handle(key(KeyCode::Char('h')));
    assert_eq!(app.view.cursor.1, before.1 + 1, "h 该改成往下走");
    // unmodified keys behave as before
    let c = app.view.cursor;
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(app.view.cursor.0, c.0 + 1, "l 还是右移");
}

#[test]
fn mouse_off_ignores_mouse_events() {
    let mut opts = zh_opts();
    opts.mouse = false;
    let mut app = App::new(sample_document(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let before = app.view.cursor;
    app.handle(mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        10,
        5,
    ));
    assert_eq!(app.view.cursor, before, "鼠标关掉后左键不该动光标");
}

/// Only :q quits: q and Esc do not
#[test]
fn only_q_quits() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert_eq!(app.handle(key(KeyCode::Char('q'))), Flow::Continue);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains(":q"), "q 该提示用 :q 退出:{note}");
    assert_eq!(app.handle(key(KeyCode::Esc)), Flow::Continue);
    assert_eq!(
        app.handle(ctrl(KeyCode::Char('c'))),
        Flow::Continue,
        "ctrl-c 也不再退出"
    );
    // :q quits immediately
    app.view.cmd = Some(String::new());
    for c in ":q".chars().skip(1) {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.handle(key(KeyCode::Enter)), Flow::Quit);
    assert!(app.quit);
}

#[test]
/// The cursor moves one cell at a time and a numeric prefix is a cell count; pick follows the cell
fn cursor_moves_by_cell() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let start = app.view.cursor;
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(app.view.cursor, (start.0 + 1, start.1), "向右一格");
    app.handle(key(KeyCode::Char('j')));
    assert_eq!(app.view.cursor, (start.0 + 1, start.1 + 1), "向下一格");
    app.view.cursor = start;
    app.refresh_pick();
    for c in ['5', 'l'] {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (start.0 + 5, start.1), "5l 走五格");
    // 10j: 0 is part of the count prefix and must not trigger "back to 1:1"
    app.view.cursor = start;
    app.view.zoom = 2.0;
    app.refresh_pick();
    for c in ['1', '0', 'j'] {
        app.handle(key(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (start.0, start.1 + 10), "10j 走十格");
    assert_eq!(app.view.zoom, 2.0, "计数里的 0 不能改缩放");
    // With no prefix 0 still resets to 1:1
    app.view.cursor = start;
    app.handle(key(KeyCode::Char('0')));
    assert_eq!(app.view.zoom, 1.0, "单独按 0 回到 1:1");
    assert_eq!(app.view.cursor, start, "单独按 0 不移动光标");
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    assert_eq!(app.view.pick, render::Pick::None);
}

/// Wherever the mouse clicks, the cursor lands on that cell
#[test]
fn mouse_click_lands_cell() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 40,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    let p = render::panels(&app.doc, app.area, &app.view);
    assert_eq!(app.view.cursor, p.proj.canvas(40, 10));
}

#[test]
fn rotate_verb_changes_coords_and_dir() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let before: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.cmd_apply("rotate 90");
    assert_eq!(app.view.angle, 90);
    let after: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    assert_ne!(before, after, "坐标该转过去");
    app.cmd_apply("rotate 0");
    assert_eq!(app.view.angle, 0);
    assert_eq!(
        app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(),
        before,
        "转回来该回到原坐标"
    );
    // auto only changes the direction, the coordinates stay
    let now: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.cmd_apply("rotate auto");
    assert_eq!(app.view.orientation.name(), "auto");
    assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), now);
    // no argument means rotate another 90 degrees
    app.cmd_apply("rotate");
    assert_eq!(app.view.angle, 90);
}

/// With an edge selected `:text` writes its annotation box
/// (empty string drops the box); it joins the undo stack
/// A new node uses the default style: text only, width follows the text
#[test]
fn new_node_defaults_to_text_only() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply("new 你好");
    let i = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[i].style, BoxStyle::None);
    assert_eq!(app.doc.boxes[i].size.1, 1, "一行高");
    assert_eq!(
        app.doc.boxes[i].size.0,
        crate::text::plain_width("你好") as u16,
        "宽随文字"
    );
    assert_eq!(app.doc.texts[i].at, app.doc.boxes[i].at);
}

/// Ctrl+hjkl: the box moves one cell and the cursor
/// follows; past the view margin the view is pulled along
#[test]
fn ctrl_move_box_pushes_view() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 60, 20);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);

    let before_box = app.doc.boxes[0].at;
    let before_cursor = app.view.cursor;
    let press = |app: &mut App, code| {
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char(code),
            KeyModifiers::CONTROL,
        )));
    };
    // Put the cursor on the box first so pick points at it
    let target = app.doc.text(0).map(|t| t.at).unwrap_or(before_box);
    app.view.cursor = target;
    app.refresh_pick();

    press(&mut app, 'l');
    assert_eq!(
        app.doc.boxes[0].at,
        (before_box.0 + 1, before_box.1),
        "框挪一格"
    );
    assert_eq!(app.view.cursor, (target.0 + 1, target.1), "光标跟着走");
    assert_eq!(app.view.pick.box_at(), Some(0), "挪完光标还压在同一个框上");
    assert_ne!(app.view.cursor, before_cursor);

    // Move the view so the box sits on the right margin,
    // then move right again: the view must follow
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or(before_box);
    app.refresh_pick();
    let p = render::panels(&app.doc, app.area, &app.view);
    let x1 = (p.map.x + p.map.width) as i32 - 1 - 2; // the two-cell margin of follow_cursor
    let scale = p.proj.scale;
    let mid = p.map.x as i32 + p.map.width as i32 / 2;
    app.view.center = Some((
        app.view.cursor.0 as f64 - (x1 - mid) as f64 * scale,
        app.view.center.map(|c| c.1).unwrap_or(0.0),
    ));
    app.view.target = None;
    app.view.pan = None;
    let center = app.view.center;
    press(&mut app, 'l');
    assert!(
        app.view.target.is_some() || app.view.pan.is_some() || app.view.center != center,
        "光标挪到边上时视图要跟过去"
    );
}

/// Keyboard movement: at the margin the view scrolls a
/// little, and a few more moves are needed before it does
#[test]
fn cursor_near_edge_scrolls_one_cell() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    let y0 = p.map.y as i32 + 2; // the two-cell margin of follow_cursor
    let anchor = p.map.y as f64 + p.map.height as f64 / 2.0;
    // the cursor starts on the top margin row
    let c = (10, 0);
    app.view.cursor = c;
    app.view.center = Some((c.0 as f64, c.1 as f64 - y0 as f64 + anchor));
    app.view.target = None;
    app.view.pan = None;
    app.refresh_pick();
    // At the margin behave like nvim: each press scrolls the picture
    // one cell at once (no easing), and the cursor stays pinned to
    // the margin row -- otherwise it would move first and bounce back
    let mut prev = app.view.center.expect("有中心");
    for step in 0..4 {
        app.handle(key(KeyCode::Char('k')));
        let now = app.view.center.expect("有中心");
        let dy = now.1 - prev.1;
        assert!((dy + 1.0).abs() < 1e-6, "第 {step} 下该只滚一格,实际 {dy}");
        assert_eq!(app.view.target, None, "贴边时不做缓动");
        let p = render::panels(&app.doc, app.area, &app.view);
        let (_, cy) = p
            .proj
            .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
        assert_eq!(cy, y0, "光标该钉在边距那一行(第 {step} 下)");
        prev = now;
    }
    // the logical position keeps advancing
    assert_eq!(app.view.cursor.1, c.1 - 4);
}

/// The count prefix applies to Ctrl-hjkl too: 10 + Ctrl-l moves 10 cells, one undo step
#[test]
fn ctrl_move_honours_count() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);

    let target = app.doc.text(0).map(|t| t.at).unwrap_or(app.doc.boxes[0].at);
    app.view.cursor = target;
    app.refresh_pick();
    let before = app.doc.boxes[0].at;

    for c in ['1', '0'] {
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('l'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.doc.boxes[0].at,
        (before.0 + 10, before.1),
        "10 应该挪十格"
    );
    assert_eq!(app.view.cursor, (target.0 + 10, target.1), "光标跟着走十格");

    // one Ctrl-l with a count is a single undo step
    app.undo_step();
    assert_eq!(app.doc.boxes[0].at, before, "一步就撤回来");

    // the count is cleared after use: the next Ctrl-l moves one cell
    app.view.cursor = target;
    app.refresh_pick();
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('l'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.doc.boxes[0].at,
        (before.0 + 1, before.1),
        "计数不该留着"
    );
}

/// E cycles only through edges between the cursor node
/// and its neighbours (wrapping in document order)
#[test]
fn e_cycles_edges_of_current_node() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app_with_links(sample_document());
    // In the sample b1 has two edges (b0 -> b1, b2 -- b1)
    let c = app.doc.box_idx("b1").expect("样例有 b1");
    app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
    app.view.pick = render::Pick::None;
    let incident: Vec<usize> = app.doc.adj(c).to_vec();
    assert_eq!(incident.len(), 2, "样例该给 b1 连两条边");

    let press_e = || KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    app.handle(Event::Key(press_e()));
    assert_eq!(app.view.pick.edge(), Some(incident[0]), "第一条");
    app.handle(Event::Key(press_e()));
    assert_eq!(app.view.pick.edge(), Some(incident[1]), "下一条");
    app.handle(Event::Key(press_e()));
    assert_eq!(app.view.pick.edge(), Some(incident[0]), "到头绕回去");
}

/// E mode: with the cursor off the highlighted edges that
/// edge is not selected (only the centre's edges pulse)
#[test]
fn e_mode_only_accepts_highlighted_edges() {
    let mut doc = sample_document();
    // add an edge unrelated to b1
    let base = doc.edges[0].clone();
    let mut far = base.clone();
    far.id = "e-far".into();
    far.from = "b2".into();
    far.to = "b0".into();
    far.group = "g1".into();
    doc.edges.push(far);
    doc.rebuild();
    let mut app = App::new(doc, zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let a = app.doc.box_idx("b1").expect("样例有 b1");
    let lit: Vec<usize> = app.doc.adj(a).to_vec();
    let far_e = app.doc.edges.len() - 1;
    assert!(!lit.contains(&far_e));
    app.sync_hit();
    // find a cell that belongs to far only and not to the highlight
    let lit_cells: std::collections::HashSet<(i32, i32)> = lit
        .iter()
        .filter_map(|e| app.hit.edge_cells.get(e))
        .flat_map(|v| v.iter().copied())
        .collect();
    let Some(only_far) = app
        .hit
        .edge_cells
        .get(&far_e)
        .and_then(|v| v.iter().copied().find(|c| !lit_cells.contains(c)))
    else {
        return; // no such cell: skip (the layout changed)
    };
    let p = render::panels(&app.doc, app.area, &app.view);
    app.view.cursor = p.proj.canvas(only_far.0, only_far.1);
    // E off: this edge can be selected
    app.view.ego = None;
    app.refresh_pick();
    assert_eq!(app.view.pick.edge(), Some(far_e), "没开 E 时照常能选");
    // E on (centre is b1): the same point must not select it
    app.view.ego = Some(a);
    app.refresh_pick();
    assert_eq!(app.view.pick.edge(), None, "E 模式下不认高亮之外的边");
}

/// Isolation: a hidden node does not take over the centre
/// when the cursor lands on it, pick cannot reach it
#[test]
fn isolate_hides_nodes_from_cursor() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // centre = b0: b0 and b1 visible, b2 hidden
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(0));
    // move onto the hidden b2
    app.view.cursor = app.doc.text(2).unwrap().at;
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(0), "隐藏节点不该抢走中心");
    assert_ne!(app.view.pick, render::Pick::Box(2), "隐藏节点选不中");
    // move onto the visible neighbour b1 -> the centre follows
    app.view.cursor = app.doc.text(1).unwrap().at;
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(1), "可见邻居能当新中心");
}

/// Isolation: jumping to a hidden node leaves isolation
/// first, otherwise the landing spot is invisible
#[test]
fn jump_to_hidden_node_leaves_isolate() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('E')));
    app.jump_to(2);
    assert_eq!(app.view.ego, None, "目标被藏着,先退出隔离");
    assert_eq!(app.view.cursor, app.doc.text(2).unwrap().at, "然后才跳过去");
}

/// Turning isolation on clears any selection and link in progress
#[test]
fn isolate_clears_pending_operations() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.view.sel = Some(crate::app::RectSel {
        anchor: (0, 0),
        end: (5, 3),
        confirmed: true,
    });
    app.handle(key(KeyCode::Char('E')));
    assert!(app.view.ego.is_some());
    assert!(app.view.sel.is_none(), "隐藏元素不能被继续操作");
}

/// F: with isolation on, fit the bounds of the visible nodes only, ignoring the hidden ones
#[test]
fn isolate_f_fits_visible_nodes() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 26, 12);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('F')));
    let plain = app.view.zoom;
    app.handle(key(KeyCode::Char('E')));
    app.handle(key(KeyCode::Char('F')));
    let iso = app.view.zoom;
    assert!(iso > plain, "可见范围更小,该放得更大:{plain} -> {iso}");
}

/// n / N skip hits hidden by isolation; when everything is hidden, say so
#[test]
fn isolate_n_skips_hidden_hits() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('E')));
    app.view.hits = vec![2, 0];
    app.view.hit_at = 1;
    app.handle(key(KeyCode::Char('n')));
    assert_eq!(app.view.hit_at, 1, "隐藏的 b2 跳过,只剩可见的 b0");
    assert_eq!(app.view.cursor, app.doc.text(0).unwrap().at);
    // only hidden hits left: a note, no move
    app.view.hits = vec![2];
    app.view.hit_at = 0;
    let before = app.view.cursor;
    app.handle(key(KeyCode::Char('n')));
    assert_eq!(app.view.cursor, before, "藏起来的命中不跳");
    assert!(app.view.note.clone().unwrap_or_default().contains("隔离"));
}

/// The centre is deleted: isolation exits on its own (detected even after indices shifted)
#[test]
fn deleted_centre_leaves_isolate() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(1).unwrap().at;
    app.refresh_pick();
    app.handle(key(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(1));
    // Delete b0 before the centre: the index shifts to 0 but the centre is still b1
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(0), "光标在可见邻居上会换中心");
    app.view.ego = Some(1);
    app.view.ego_id = Some(app.doc.boxes[1].id.clone());
    app.doc.remove_box("b0").unwrap();
    app.draw(&mut buf, app.area);
    assert_eq!(
        app.view
            .ego
            .and_then(|n| app.doc.boxes.get(n).map(|b| b.id.clone())),
        Some("b1".to_string()),
        "下标漂了要按 id 认回来"
    );
    // Deleting the centre itself -> exit
    app.doc.remove_box("b1").unwrap();
    app.draw(&mut buf, app.area);
    assert_eq!(app.view.ego, None, "中心没了就退出隔离");
}

/// E: keep only the cursor node, its neighbours and the
/// edges between them (the rest goes); E or Esc exits
#[test]
fn e_toggles_neighbour_isolate() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);

    let c = app.doc.box_idx("b1").expect("样例有 b1");
    app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
    app.refresh_pick();

    app.handle(key(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(c), "开着的是光标那个节点");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("隔离"), "回显要说清是隔离:{note}");
    assert!(note.contains("2 邻居"), "回显要报邻居数:{note}");

    // Move the cursor to another node: the highlight follows without pressing E again
    let other = app.doc.box_idx("b0").expect("样例有 b0");
    app.view.cursor = app.doc.text(other).map(|t| t.at).unwrap_or((0, 0));
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(other), "点到/挪到别的节点就换过去亮它");

    // E again: isolation off
    app.handle(key(KeyCode::Char('E')));
    assert_eq!(app.view.ego, None, "E 再按一次该关掉");

    // Once on, Esc exits too
    app.handle(key(KeyCode::Char('E')));
    assert!(app.view.ego.is_some());
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.view.ego, None, "Esc 该退掉");

    // Cursor not on a node: only a note, no isolation
    app.view.cursor = (-99, -99);
    app.refresh_pick();
    app.handle(key(KeyCode::Char('E')));
    assert_eq!(app.view.ego, None);
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有节点"));
}

#[test]
fn prompt_line_opens_external_editor() {
    use crate::external::Outcome;
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.open_prompt(Prompt::NewBox, "旧".to_string());
    app.handle(ctrl(KeyCode::Char('g')));
    assert!(app.edit_pending(), "Ctrl+G 该请求外部编辑器");
    assert_eq!(app.edit_initial().as_deref(), Some("旧"));
    let boxes = app.doc.boxes.len();
    app.apply_edit_result(Outcome::Changed("第一行\n第二行".to_string()));
    assert!(!app.edit_pending(), "应用之后没有待办");
    assert!(app.view.prompt.is_none(), "直接提交就不要再留输入框");
    assert_eq!(app.doc.boxes.len(), boxes + 1, "编辑器内容直接建了框");

    // The link target field is single line: multi-line is refused, the prompt stays
    app.open_prompt(Prompt::Link(0), "网关".to_string());
    app.handle(ctrl(KeyCode::Char('g')));
    assert_eq!(app.edit_initial().as_deref(), Some("网关"));
    app.apply_edit_result(Outcome::Changed("甲\n乙".to_string()));
    assert_eq!(
        app.view.prompt.as_ref().map(|(_, t)| t.as_str()),
        Some("网关"),
        "单行字段不收换行"
    );
    assert!(app.view.note.clone().unwrap_or_default().contains("一行"));

    // failure: unchanged plus a note
    app.handle(ctrl(KeyCode::Char('g')));
    assert_eq!(app.edit_initial().as_deref(), Some("网关"));
    app.apply_edit_result(Outcome::Failed(3));
    assert_eq!(
        app.view.prompt.as_ref().map(|(_, t)| t.as_str()),
        Some("网关")
    );
}

/// Ctrl+G (normal mode) edits the box text in the
/// editor directly; changes apply on exit, one undo step
#[test]
fn external_editor_edits_box_text() {
    use crate::external::Outcome;
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.handle(ctrl(KeyCode::Char('g')));
    assert!(app.edit_pending(), "该请求外部编辑器");
    assert_eq!(
        app.edit_initial().as_deref(),
        Some("甲"),
        "初始内容是光标框的原文"
    );
    let undo = app.undo.counts().0;
    app.apply_edit_result(Outcome::Changed("新文字".to_string()));
    assert!(app.view.prompt.is_none(), "不经过底行输入框");
    let id = app.doc.boxes[0].id.clone();
    assert_eq!(
        app.doc.text(0).map(|t| t.text.as_str()),
        Some("新文字"),
        "文字直接写进框"
    );
    assert_eq!(app.undo.counts().0, undo + 1, "整次编辑一步撤销");
    app.undo_step();
    assert_eq!(app.doc.text(0).map(|t| t.text.as_str()), Some("甲"));
    assert!(app.doc.boxes.iter().any(|b| b.id == id), "框还在");

    // no change: untouched
    app.handle(ctrl(KeyCode::Char('g')));
    app.apply_edit_result(Outcome::Unchanged);
    assert_eq!(app.doc.text(0).map(|t| t.text.as_str()), Some("甲"));
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有改动"));

    // no box: a note, the editor is not requested
    app.view.cursor = (200, 200);
    app.refresh_pick();
    app.handle(ctrl(KeyCode::Char('g')));
    assert!(!app.edit_pending());
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有可编辑的框"));
}

/// The annotation text of an edge (None when there is no annotation box)
fn edge_annotation(app: &App, e: usize) -> Option<String> {
    let id = app.doc.edges.get(e)?.id.clone();
    let b = app.doc.edge_label_of(&id)?;
    app.doc.text(b).map(|t| t.text.clone())
}

#[test]
fn selected_edge_text_writes_annotation() {
    let mut app = app_with_links(sample_document());
    assert!(!app.doc.edges.is_empty(), "样例该有边");
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    assert_eq!(edge_annotation(&app, 0).as_deref(), Some("收到 FIN"));
    let b = app.doc.edge_label_of(&app.doc.edges[0].id).unwrap();
    assert_eq!(app.doc.boxes[b].style, crate::model::BoxStyle::None);
    app.undo_step();
    assert!(edge_annotation(&app, 0).is_none(), "撤销该把注释框撤掉");

    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text");
    assert!(edge_annotation(&app, 0).is_none(), "空串 = 连注释框一起删");
}

#[test]
fn i_on_selected_edge_makes_annotation_box() {
    let mut app = app_with_links(sample_document());
    // Move the cursor onto a real edge stroke (not on a box) so i means annotate this edge
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let e = 0usize;
    let cell = app
        .hit
        .edge_cells
        .get(&e)
        .and_then(|c| {
            c.iter()
                .copied()
                .find(|c| app.doc.box_at_cell(*c).is_none())
        })
        .expect("样例该有边笔画");
    app.view.cursor = cell;
    app.refresh_pick();
    app.view.pick = render::Pick::Edge(0);
    app.handle(key(KeyCode::Char('i')));
    assert!(
        matches!(app.view.prompt, Some((Prompt::Text(_), _))),
        "该进输入"
    );
    app.handle(key(KeyCode::Esc));
    assert!(edge_annotation(&app, 0).is_none(), "Esc 该把空注释框撤掉");

    // Once more; typed text is kept
    app.view.pick = render::Pick::Edge(0);
    app.handle(key(KeyCode::Char('i')));
    for c in ['收', '到', 'A', 'C', 'K'] {
        app.handle(key(KeyCode::Char(c)));
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(edge_annotation(&app, 0).as_deref(), Some("收到ACK"));
}

#[test]
fn deleting_edge_removes_annotation_box() {
    let mut app = app_with_links(sample_document());
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    let n0 = app.doc.boxes.len();
    let id = app.doc.edges[0].id.clone();
    app.doc.remove_edge(&id).unwrap();
    assert_eq!(app.doc.boxes.len(), n0 - 1, "注释框该一起删");
    assert!(app.doc.edge_label_of(&id).is_none());
}

#[test]
fn real_cursor_position_matches_in_other_panels() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // real cursor: at the text start of the cursor box
    let p = render::panels(&app.doc, app.area, &app.view);
    let want = p
        .proj
        .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
    assert_eq!(app.cursor_cell(), Some((want.0 as u16, want.1 as u16)));
    // on the prompt line while it is open
    app.view.cmd = Some("notes on".to_string());
    app.view.cmd_pos = 8;
    let (x, y) = app.cursor_cell().expect("输入行该有光标");
    assert_eq!(y, 23);
    assert_eq!(x, render::width(":notes on") as u16);
    app.view.cmd = None;
    // hidden while the help overlay is open
    app.view.help = true;
    assert_eq!(app.cursor_cell(), None);
}

#[test]
fn empty_document_create_link_delete() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert_eq!(app.doc.boxes.len(), 1, "空白文档先给一个框");
    assert_eq!(
        app.doc.text(0).map(|t| t.text.clone()).as_deref(),
        Some("a")
    );

    app.cmd_apply("new b");
    assert_eq!(app.doc.boxes.len(), 2);
    assert_eq!(app.view.cursor, app.doc.text(1).unwrap().at, "光标跟到新框");
    assert_eq!(
        app.doc.text(1).map(|t| t.text.clone()).as_deref(),
        Some("b")
    );

    app.cmd_apply("link a");
    assert_eq!(app.doc.edges.len(), 1);
    assert_eq!(app.doc.edges[0].from, "b2");
    assert_eq!(app.doc.edges[0].to, "b1");
    assert_eq!(app.doc.edges[0].dir, Dir::Forward);
    // the same edge cannot be linked twice
    app.cmd_apply("link a");
    assert_eq!(app.doc.edges.len(), 1);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("连不上"), "{note}");

    // delete a box: its text and edges go with it
    app.cmd_apply("del");
    assert_eq!(app.doc.boxes.len(), 1);
    assert!(app.doc.edges.is_empty());
    assert_eq!(app.doc.texts.len(), 1);
    // The cursor stays where the box was; move back onto the last box first
    app.jump_to(0);
    app.cmd_apply("del");
    assert_eq!(app.doc.boxes.len(), 1);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("最后一个框"), "{note}");
}

#[test]
fn new_box_default_text_uses_id() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.cmd_apply("new");
    let last = app.doc.boxes.len() - 1;
    let id = app.doc.boxes[last].id.clone();
    assert_eq!(app.doc.text(last).map(|t| t.text.clone()), Some(id));
}

#[test]
fn layer_cycle_and_tolayer_creates_layer() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert_eq!(app.view.layer, None);
    app.handle(key(KeyCode::Char('L')));
    assert_eq!(app.view.layer.as_deref(), Some(""));
    app.handle(key(KeyCode::Char('L')));
    assert_eq!(app.view.layer, None, "循环回全部");
    app.cmd_apply("layer all");
    app.jump_to(0);
    app.view.sel = None;
    app.cmd_apply("tolayer 新层");
    assert_eq!(app.doc.boxes[0].layer, "新层");
    assert_eq!(app.doc.text(0).unwrap().layer, "新层", "文字跟着框走层");
    assert!(app.doc.layer_names().iter().any(|l| l == "新层"));
}

#[test]
fn edit_verbs_set_attributes() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.jump_to(0);
    app.cmd_apply("box double");
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Double);
    app.cmd_apply("nodecolor #ff0000");
    assert_eq!(app.doc.boxes[0].color, Some((0xff, 0, 0)));
    app.cmd_apply("nodecolor auto");
    assert_eq!(app.doc.boxes[0].color, None);
    app.cmd_apply("text **粗** 体 /斜/  空格");
    assert_eq!(app.doc.text(0).unwrap().text, "**粗** 体 /斜/  空格");
    // the box width is recomputed after the text change
    assert!(app.doc.boxes[0].size.0 >= 4);
    app.view.pick = render::Pick::Edge(1);
    app.cmd_apply("style dashed");
    assert_eq!(app.doc.edges[1].style, EdgeStyle::Dashed);
    app.cmd_apply("edgecolor #00ff00");
    assert_eq!(app.doc.edges[1].color, Some((0, 0xff, 0)));
    app.cmd_apply("via 1,2 3,4");
    assert_eq!(app.doc.edges[1].via, vec![(1, 2), (3, 4)]);
    app.cmd_apply("via off");
    assert!(app.doc.edges[1].via.is_empty());
}

/// fill sets the background of the selected box; none clears
/// it to null; point boxes may set it (rendering ignores it)
#[test]
fn fill_verb_sets_background_and_clears() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.jump_to(0);
    app.cmd_apply("fill #102030");
    assert_eq!(app.doc.boxes[0].fill, Some((0x10, 0x20, 0x30)));
    assert!(app.view.note.as_deref().unwrap_or("").contains("填充"));
    app.cmd_apply("fill none");
    assert_eq!(app.doc.boxes[0].fill, None, "none 要清成 null");
    // point boxes may set it too
    app.doc.boxes[0].style = BoxStyle::Point;
    app.cmd_apply("fill #abcdef");
    assert_eq!(app.doc.boxes[0].fill, Some((0xab, 0xcd, 0xef)));
}

#[test]
fn bad_fill_arg_echoes_and_keeps_document() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.cmd_apply("fill blue");
    assert_eq!(app.doc.boxes[0].fill, None, "坏参数不该改图");
    assert!(app.view.note.as_deref().unwrap_or("").contains("fill"));
    app.cmd_apply("fill");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("fill"), "缺参数要有回显:{note}");
}

/// textcolor changes the text colour inside the box; auto sets it back to null (follow the box)
#[test]
fn textcolor_verb_sets_colour_and_auto() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.jump_to(0);
    app.cmd_apply("textcolor #112233");
    assert_eq!(app.doc.texts[0].color, Some((0x11, 0x22, 0x33)));
    assert!(app.view.note.as_deref().unwrap_or("").contains("文字颜色"));
    app.cmd_apply("textcolor auto");
    assert_eq!(app.doc.texts[0].color, None);
}

#[test]
fn bad_textcolor_arg_echoes() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.cmd_apply("textcolor nope");
    assert_eq!(app.doc.texts[0].color, None);
    assert!(app.view.note.as_deref().unwrap_or("").contains("textcolor"));
    app.cmd_apply("textcolor");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("textcolor"), "缺参数要有回显:{note}");
}

/// textalign sets the text alignment inside the box
#[test]
fn textalign_verb_sets_align_and_rejects_bad_arg() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.jump_to(0);
    app.cmd_apply("textalign center");
    assert_eq!(app.doc.texts[0].align, Align::Center);
    assert!(app.view.note.as_deref().unwrap_or("").contains("文字对齐"));
    app.cmd_apply("textalign right");
    assert_eq!(app.doc.texts[0].align, Align::Right);
    app.cmd_apply("textalign middle");
    assert_eq!(app.doc.texts[0].align, Align::Right, "坏参数不该改");
    assert!(app.view.note.as_deref().unwrap_or("").contains("textalign"));
    app.cmd_apply("textalign");
    assert!(app.view.note.as_deref().unwrap_or("").contains("textalign"));
}

/// dir sets the four-state direction of the selected edge
#[test]
fn dir_verb_sets_direction_and_rejects_bad_arg() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.view.pick = render::Pick::Edge(1);
    app.cmd_apply("dir both");
    assert_eq!(app.doc.edges[1].dir, Dir::Both);
    assert!(app.view.note.as_deref().unwrap_or("").contains("边方向"));
    app.cmd_apply("dir backward");
    assert_eq!(app.doc.edges[1].dir, Dir::Both, "坏参数不该改");
    assert!(app.view.note.as_deref().unwrap_or("").contains("dir"));
    app.cmd_apply("dir");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(
        note.contains("dir") && note.contains("缺参数"),
        "缺参数要有回显:{note}"
    );
}

/// head sets the arrowhead glyph of the selected edge; undo reverts it
#[test]
fn head_verb_sets_glyph_and_rejects_bad_arg() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.view.pick = render::Pick::Edge(1);
    app.cmd_apply("head triangles");
    assert_eq!(
        app.doc.edges[1].head,
        Some(crate::model::ArrowStyle::Triangles)
    );
    assert!(app.view.note.as_deref().unwrap_or("").contains("箭头"));
    app.cmd_apply("head auto");
    assert_eq!(app.doc.edges[1].head, None, "auto 就是默认形状");
    app.cmd_apply("head square");
    assert_eq!(app.doc.edges[1].head, None, "坏参数不该改");
    assert!(app.view.note.as_deref().unwrap_or("").contains("head"));
    app.cmd_apply("head");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(
        note.contains("head") && note.contains("缺参数"),
        "缺参数要有回显:{note}"
    );
    app.cmd_apply("head triangles");
    let text = app.doc.save();
    assert!(
        text.contains("\"head\": \"triangles\""),
        "存出去该带着:{text}"
    );
    // one change per undo step
    app.cmd_apply("head arrows");
    assert_eq!(
        app.doc.edges[1].head,
        Some(crate::model::ArrowStyle::Arrows)
    );
    app.undo_step();
    assert_eq!(
        app.doc.edges[1].head,
        Some(crate::model::ArrowStyle::Triangles),
        "undo 只退一步"
    );
    app.undo_step();
    assert_eq!(app.doc.edges[1].head, None, "再退一步回到 auto");
}

/// The edge right click menu offers arrowhead glyphs and ticks the current value
#[test]
fn edge_menu_has_head_glyph() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.doc.edges[1].head = Some(crate::model::ArrowStyle::Triangles);
    app.view.pick = render::Pick::Edge(1);
    let items = app.menu_for();
    let i = items
        .iter()
        .position(|it| it.title == "箭头")
        .expect("该有箭头一项");
    let MenuAction::Sub(sub) = &items[i].action else {
        panic!("箭头该是子菜单");
    };
    let titles: Vec<&str> = sub.iter().map(|it| it.title.as_str()).collect();
    assert_eq!(titles, ["auto", "ascii", "arrows", "triangles"]);
    assert!(
        sub.iter()
            .find(|it| it.title == "triangles")
            .map(|it| it.checked)
            .unwrap_or(false),
        "当前值该打勾"
    );
    assert!(!sub
        .iter()
        .find(|it| it.title == "auto")
        .map(|it| it.checked)
        .unwrap_or(true));
}

/// After the view is panned E still selects the edge under
/// the cursor (hit table in screen, cursor in canvas)
#[test]
fn panned_view_still_selects_edge() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let (cx, cy) = app.view.center.unwrap_or((0.0, 0.0));
    app.view.center = Some((cx + 13.0, cy + 7.0));
    let p = render::panels(&app.doc, app.area, &app.view);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // Find a cell an edge occupies (the hit table stores
    // screen cells) and put the cursor on its canvas cell
    let Some(screen) = app.hit.edge_cells.values().find_map(|c| c.first().copied()) else {
        panic!("样例文档该有边");
    };
    let canvas = p.proj.canvas(screen.0, screen.1);
    assert_ne!(screen, canvas, "视图挪开后屏幕格和画布格不该重合");
    app.view.cursor = canvas;
    app.cycle_edge();
    assert!(
        app.view.pick.edge().is_some(),
        "该选中一条边:{:?}",
        app.view.note
    );
}

/// corner sets the corner of the selected edge
#[test]
fn corner_verb_sets_corner_and_rejects_bad_arg() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.view.pick = render::Pick::Edge(1);
    app.cmd_apply("corner sharp");
    assert_eq!(app.doc.edges[1].corner, Corner::Sharp);
    assert!(app.view.note.as_deref().unwrap_or("").contains("边拐角"));
    app.cmd_apply("corner round");
    assert_eq!(app.doc.edges[1].corner, Corner::Sharp, "坏参数不该改");
    assert!(app.view.note.as_deref().unwrap_or("").contains("corner"));
    app.cmd_apply("corner");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(
        note.contains("corner") && note.contains("缺参数"),
        "缺参数要有回显:{note}"
    );
}

/// dir / corner with no edge selected must say so
#[test]
fn edge_verbs_require_selected_edge() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.view.pick = render::Pick::None;
    for line in ["dir both", "dir", "corner sharp", "corner"] {
        app.view.note = None;
        app.cmd_apply(line);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            note.contains("没有选中边") || note.contains("no edge selected"),
            "{line} 该提示没选边:{note}"
        );
    }
}

/// New verbs follow the same 1:1 gate: at another zoom switch to 1:1 first and then run
#[test]
fn new_edit_verbs_return_to_unity_zoom() {
    let mut app = App::new(sample_document(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 2.0;
    app.jump_to(0);
    app.cmd_apply("fill #102030");
    assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
    assert_eq!(app.doc.boxes[0].fill, Some((0x10, 0x20, 0x30)));
    app.view.zoom = 2.0;
    app.view.pick = render::Pick::Edge(1);
    app.cmd_apply("corner sharp");
    assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
    assert_eq!(app.doc.edges[1].corner, Corner::Sharp);
}

/// Every command name and alias in the completion table must land in a verb family, never unknown
#[test]
fn every_command_name_is_dispatched() {
    let mut names: Vec<&str> = VERBS.to_vec();
    names.extend([
        "quit",
        "delete",
        "put",
        "tabnew",
        "tabnext",
        "tabprev",
        "tab 1",
        "tabclose",
        "tabclose!",
        "tabs",
        "fromside",
        "toside",
        "port",
        "region",
        "e!",
    ]);
    for name in names {
        let mut app = App::new(sample_document(), zh_opts()).expect("开局");
        app.cmd_apply(name);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            !note.starts_with("未知命令") && !note.starts_with("unknown command"),
            "{name} 没被认识: {note}"
        );
    }
}
