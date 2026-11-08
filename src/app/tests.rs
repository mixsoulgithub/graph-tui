use super::*;
use crate::document::{Box, Dir, DocEdge, Group, Text};

fn 样例文档() -> Document {
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

fn 临时路径(tag: &str, ext: &str) -> String {
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

/// 测试里统一用中文界面(默认已经改成英文;中文文案另有用例守着)
fn zh_opts() -> Options {
    Options {
        lang: Lang::Zh,
        ..Options::default()
    }
}

fn 键(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn 鼠标(kind: MouseEventKind, col: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn 一比一下挪一格同步文字且存读一致() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.view.zoom = 1.0;
    app.jump_to(app.doc.box_idx("b0").unwrap());
    app.handle(ctrl(KeyCode::Char('l')));
    assert_eq!(app.doc.boxes[0].at, (1, 0), "框该右移一格");
    assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字跟着框走");
    let path = 临时路径("roundtrip", "gtui");
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

/// 单击就把光标放到点到的这一格(不吸到节点的角上),也不会挪动框
#[test]
fn 单击落下光标() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // 点在框内部(中间那行)、不是边框,也不是文字起点
    let (x, y) = p.proj.cell((4.0, 1.0));
    let before = app.doc.boxes[0].at;
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    assert_eq!(app.view.cursor, (4, 1), "光标就是点到的那一格");
    assert_eq!(app.view.pick.box_at(), Some(0), "点到的还是这个框");
    assert_eq!(app.doc.boxes[0].at, before, "单击不改几何");
    assert!(app.view.sel.is_none(), "单击不起选区");
}

/// 选区已经确认之后:再拖一次换成新选区;点它外面取消,点里面保留
#[test]
fn 选区确认后再拖或点外面() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));

    // 第一块:从空白处起手,盖住 b0(按在框上是挪框)
    let (ax, ay) = at((-2, 8));
    let (bx, by) = at((5, 2));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    let first = app.view.sel.expect("第一块选区");
    assert!(first.confirmed);

    // 在里面点一下:选区留着(挑一格在选区矩形里、又不在框上的)
    let (cx, cy) = at((2, 4));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        cx as u16,
        cy as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        cx as u16,
        cy as u16,
    ));
    assert!(app.view.sel.is_some(), "点选区里面不该取消");

    // 点它外面:取消
    // 地图里面、但离选区很远的一格(注意别点到状态行)
    let (ox, oy) = at((30, 9));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        ox as u16,
        oy as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        ox as u16,
        oy as u16,
    ));
    assert!(app.view.sel.is_none(), "点选区外面该取消");

    // 再拖一块别的:直接换成新选区(旧的不能挡着)
    let (dx, dy) = at((10, 8));
    let (ex, ey) = at((16, 2));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        dx as u16,
        dy as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Drag(MouseButton::Left),
        ex as u16,
        ey as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        ex as u16,
        ey as u16,
    ));
    let second = app.view.sel.expect("第二块选区");
    assert!(second.confirmed, "松手还是即确认");
    assert_eq!(second.anchor, (10, 8), "锚点换成了新按下的地方");
    assert_ne!(second.anchor, first.anchor, "不是旧的那一块");
}

/// 空处拖出来的选区:松开就确认,可以直接接着改(不用再按 Enter)
#[test]
fn 空处拖完松手就能直接改() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // 从空白处起手(按在框上是挪框)
    let (ax, ay) = p.proj.cell((-2.0, 8.0));
    let (bx, by) = p.proj.cell((5.0, 2.0));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    let sel = app.view.sel.expect("该有选区");
    assert!(sel.confirmed, "松手即确认");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("已确认"), "回显要说一声:{note}");
    // 接着就能直接改,不会被"先按 Enter 确认"拦住
    app.cmd_apply("box double");
    assert!(
        app.doc.boxes[0].style == BoxStyle::Double,
        "拖完的选区该能直接改:{}",
        app.view.note.clone().unwrap_or_default()
    );
}

/// 两条边压在同一格上时,连点这一格就在它们之间轮(否则永远只能选到一条)
#[test]
fn 叠在同一格的两条边点得到() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    // 给两条边同一组手工折点:它们会走同一条路,互相压着
    app.doc.edges[0].via = vec![(8, 8)];
    app.doc.edges[1].via = vec![(8, 8)];
    app.doc.rebuild();
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // 找一格压着两条边、而且不在框上的
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

/// 空白处双击 = 按 i:弹新建节点的输入框;节点上双击 = 改文字
#[test]
fn 双击空处新建节点() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // 空白处:样例的框都在最上面两行,底下是空的(别落到状态行上)
    let (x, y) = p.proj.cell((8.0, 8.0));
    assert!(y < (p.map.y + p.map.height) as i32, "测试点该在地图里");
    let click = |app: &mut App| {
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            x as u16,
            y as u16,
        ));
        app.handle(鼠标(
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

/// 选区里的东西被删光之后,空选区自动取消(和点选区外面一个效果)
#[test]
fn 选区内容没了就自动取消() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
    // 从空处拉一块盖住 b0
    let (ax, ay) = at((-2, 8));
    let (bx, by) = at((5, 2));
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        ax as u16,
        ay as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Drag(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    app.handle(鼠标(
        MouseEventKind::Up(MouseButton::Left),
        bx as u16,
        by as u16,
    ));
    assert!(app.view.sel.is_some(), "先有一块选区");

    // 把选区里的框删掉:选区自动消失
    app.handle(键(KeyCode::Char('d')));
    app.draw(&mut buf, app.area);
    assert!(app.view.sel.is_none(), "选区空了就该自己取消");
    assert_eq!(app.doc.boxes.len(), 2, "只删掉被选中的那个框");
}

/// 在节点上拖:挪框(整段拖动算一步撤销);空处拖才是拉选区
#[test]
fn 在节点上拖是挪框() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let p = render::panels(&app.doc, app.area, &app.view);
    // 框内部起拖:仍然是挪框(边框起拖才是连边)
    let (x, y) = p.proj.cell((4.0, 1.0));
    let before = app.doc.boxes[0].at;
    let undo_before = app.undo.counts();
    app.handle(鼠标(
        MouseEventKind::Down(MouseButton::Left),
        x as u16,
        y as u16,
    ));
    assert!(app.view.sel.is_none(), "按下不该起选区");
    app.handle(鼠标(
        MouseEventKind::Drag(MouseButton::Left),
        (x + 2) as u16,
        (y + 1) as u16,
    ));
    app.handle(鼠标(
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

/// `:e` 不带参数:弹出选择浮层,过滤后回车打开
#[test]
fn 冒号e不带参数弹出选择器并过滤打开() {
    let one = 临时路径("pick-one", "gtui");
    let two = 临时路径("pick-two", "gtui");
    let body = 样例文档().save();
    std::fs::write(&one, &body).expect("写 gtui");
    std::fs::write(&two, &body).expect("写 gtui");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.source = Some(one.clone());
    // :e 不带参数
    for c in ":e".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(键(KeyCode::Enter));
    let pick = app.view.file_pick.as_ref().expect("该弹出选择浮层");
    assert!(pick.items.len() >= 2, "该列出候选:{:?}", pick.items);
    // 输入过滤词(文件名本身)缩小到一个候选
    let stem = std::path::Path::new(&two)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    for c in stem.chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.file_pick.as_ref().unwrap().shown().len(), 1);
    // 回车打开
    app.handle(键(KeyCode::Enter));
    assert!(app.view.file_pick.is_none(), "打开后该关掉浮层");
    assert_eq!(app.source.as_deref(), Some(two.as_str()));
    assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
    let _ = std::fs::remove_file(&one);
    let _ = std::fs::remove_file(&two);
}

/// 规格:不在 1:1 时"先切到 1:1,再执行",不是拒绝执行
#[test]
fn 缩放不是一比一时先切回一比一再执行() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 2.0;
    app.handle(ctrl(KeyCode::Char('l')));
    assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
    assert_eq!(app.doc.boxes[0].at, (1, 0), "切回 1:1 之后这一下就生效");
    assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字同量平移");
    assert!(app.view.note.is_some(), "该给一句回显");
}

/// i:有选中就改文字(整批),空处才是新建
/// w / b:射线跳。横排图:w 向右、b 向左;撞不到就只提示,光标不动
#[test]
fn w_b_射线跳到下一个框或边() {
    let mut app = 连边应用(横排文档(3));
    app.jump_to(0);
    let start = app.view.cursor;
    // 框 0 在 (0,0)-(5,2),下一个东西在右边
    app.handle(键(KeyCode::Char('w')));
    assert!(
        app.view.cursor.0 > start.0,
        "w 该往右走:{start:?} -> {:?}",
        app.view.cursor
    );
    let first = app.view.cursor;
    // 再按一次继续往右,不会停在原地
    app.handle(键(KeyCode::Char('w')));
    assert!(app.view.cursor.0 > first.0, "第二次 w 还要往前走");
    // 到头了:提示,光标不动
    let at_end = app.view.cursor;
    app.handle(键(KeyCode::Char('w')));
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
    // b 往回走
    app.handle(键(KeyCode::Char('b')));
    assert!(app.view.cursor.0 < at_end.0, "b 该往左走");
    // 一直 b 到最左边,再按就提示
    for _ in 0..20 {
        app.handle(键(KeyCode::Char('b')));
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

/// S:在光标行与下一行之间拆框;最后一行 / 点节点 / 注释框拒绝
#[test]
fn 拆分_在光标行下面切一刀() {
    let mut app = 连边应用(横排文档(1));
    let i = 0usize;
    app.doc.set_text(i, "a\nb\nc");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let at = app.doc.text(i).unwrap().at;
    app.view.cursor = at;
    app.refresh_pick();
    let n0 = app.doc.boxes.len();
    let undo = app.undo.counts().0;
    app.handle(键(KeyCode::Char('S')));
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

    // 光标在最后一行:提示,不压撤销
    let undo = app.undo.counts().0;
    let n0 = app.doc.boxes.len();
    app.view.cursor = (at.0, at.1);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('S')));
    assert_eq!(app.undo.counts().0, undo, "没东西可拆就不压撤销");
    assert_eq!(app.doc.boxes.len(), n0);
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("最后一行"));
}

/// M:把选中的框并成一个:文字按阅读顺序拼,内部边删掉,边界边改接到目标框
#[test]
fn 合并_选中的多个框() {
    let mut app = 连边应用(横排文档(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("内部边");
    app.doc
        .add_edge("b1", "b2", "g0", Dir::Forward)
        .expect("边界边");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 选中前两个框
    app.view.cursor = (0, 0);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(键(KeyCode::Enter));
    let undo = app.undo.counts().0;
    app.handle(键(KeyCode::Char('M')));
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

    // 少于两个框:只提示
    app.view.sel = None;
    app.jump_to(0);
    let undo = app.undo.counts().0;
    app.handle(键(KeyCode::Char('M')));
    assert_eq!(app.undo.counts().0, undo);
    assert!(app.view.note.clone().unwrap_or_default().contains("至少"));
}

/// y:只复制不改文档;边界边不进剪贴板
#[test]
fn 复制_不改文档且跳过边界边() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    let (boxes, edges) = (app.doc.boxes.len(), app.doc.edges.len());
    let undo = app.undo.counts().0;
    app.handle(键(KeyCode::Char('y')));
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

    // 光标在空白处:提示
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('y')));
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有可复制"));
}

/// i 只动光标下的一个框:区块选择时只提示,不改任何文字
#[test]
fn i_只改光标下的一个框() {
    let mut app = 连边应用(横排文档(2));
    app.view.sel = Some(RectSel {
        anchor: (-2, -2),
        end: (14, 5),
        confirmed: true,
    });
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    app.handle(键(KeyCode::Char('i')));
    assert!(app.view.edit.is_none(), "区块选择时不进编辑");
    assert!(app.view.note.clone().unwrap_or_default().contains("一个框"));
    let after: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    assert_eq!(before, after, "一个字都不改");

    // 取消选区后:i 只编辑光标下的那个框,别的框不动
    app.view.sel = None;
    app.jump_to(0);
    app.handle(键(KeyCode::Char('i')));
    let edit = app.view.edit.clone().expect("该进就地编辑");
    assert_eq!(edit.box_idx, 0);
    let _ = app;
}

#[test]
fn i_在框里直接改字() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let n0 = app.doc.boxes.len();
    // 光标压在文字的第一个字上:i 就地编辑,光标就在那个字上(不是末尾)
    let at = app.doc.text(0).unwrap().at;
    app.view.cursor = at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('i')));
    let edit = app.view.edit.clone().expect("该进就地编辑");
    assert_eq!(edit.box_idx, 0);
    assert_eq!(edit.text, "甲", "草稿从原文开始");
    assert_eq!(edit.caret, 0, "光标落在画布光标指到的那个字上");
    assert!(app.view.prompt.is_none(), "不弹底行输入框");
    // End 到行尾,再打两个字
    app.handle(键(KeyCode::End));
    assert_eq!(app.view.edit.as_ref().map(|e| e.caret), Some(1));
    for c in "XY".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲XY")
    );
    assert_eq!(app.doc.text(0).unwrap().text, "甲", "提交前文档不变");
    app.handle(键(KeyCode::Esc));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.text(0).unwrap().text, "甲XY");
    assert_eq!(app.doc.boxes.len(), n0, "不该新建框");
    assert_eq!(app.undo.counts().0, 1, "改文字一步撤销");
    app.undo_step();
    assert_eq!(app.doc.text(0).unwrap().text, "甲");

    // Alt+退格:删掉光标前的一个词,和命令行 / 搜索栏一致
    app.handle(键(KeyCode::Char('i')));
    app.handle(键(KeyCode::End));
    for c in " alpha beta".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲 alpha ")
    );
    assert_eq!(app.view.edit.as_ref().map(|e| e.caret), Some(8));
    // 光标在行首时什么都不删
    app.handle(键(KeyCode::Home));
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(
        app.view.edit.as_ref().map(|e| e.text.as_str()),
        Some("甲 alpha ")
    );

    // Ctrl-C 取消:文档不动
    app.handle(键(KeyCode::Char('i')));
    app.handle(键(KeyCode::Char('X')));
    app.handle(ctrl(KeyCode::Char('c')));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.text(0).unwrap().text, "甲", "取消不写回");
    assert!(app.view.note.clone().unwrap_or_default().contains("取消"));

    // 换行:Enter 插一个换行,提交后框变高
    app.handle(键(KeyCode::Char('i')));
    app.handle(键(KeyCode::End));
    app.handle(键(KeyCode::Enter));
    app.handle(键(KeyCode::Char('乙')));
    app.handle(键(KeyCode::Esc));
    assert_eq!(app.doc.text(0).unwrap().text, "甲\n乙");
    assert!(app.doc.boxes[0].size.1 >= 2, "框跟着长高");

    // 空白处按 i:直接建框并进编辑;Ctrl-C 把框整步撤掉
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    let n1 = app.doc.boxes.len();
    app.handle(键(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "空白处也进就地编辑");
    assert_eq!(app.doc.boxes.len(), n1 + 1, "先建了一个空框");
    app.handle(ctrl(KeyCode::Char('c')));
    assert_eq!(app.doc.boxes.len(), n1, "取消时新建的框一起撤掉");

    // 空白处按 i 打几个字再 Esc:框里就是这些字
    app.view.cursor = (-20, -20);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('i')));
    for c in "hello".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(键(KeyCode::Esc));
    assert_eq!(
        app.doc.texts.last().map(|t| t.text.as_str()),
        Some("hello"),
        "新框里是刚打的字"
    );
}

/// 光标移到边的笔画上就该选中那条边(以前被 sync_edge_sel 清掉了)
#[test]
fn 光标落在边上选中边() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 找一条边的可见格里、换算回画布后不属于任何框的那一格
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
    // 走一次不改光标的按键,选中项不该被清掉
    app.handle(键(KeyCode::Char('B')));
    assert_eq!(app.view.pick, picked, "按键后仍选中同一条边");
    // 移开一格就不再是它
    app.handle(键(KeyCode::Left));
    app.handle(键(KeyCode::Left));
    app.handle(键(KeyCode::Up));
    app.handle(键(KeyCode::Up));
    assert_ne!(app.view.pick, picked, "移开就该换选中项");
}

/// e 把选中的框按顺序连成边,只算一步撤销
/// n 个框横排、一个分组、没有任何边
fn 横排文档(n: usize) -> Document {
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

fn 连边应用(doc: Document) -> App {
    let mut app = App::new(doc, zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 30);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app
}

/// 直接摆好源/目标,走 link_commit(批量规则的单测入口)
fn 直连(app: &mut App, sources: Vec<usize>, targets: Vec<usize>) {
    app.view.link = Some(LinkState {
        sources,
        targets,
        confirming: false,
    });
    app.link_commit();
}

fn 边名(app: &App) -> Vec<(String, String)> {
    app.doc
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect()
}

#[test]
fn 连边_一比一() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    assert!(app.view.link.is_some(), "该进入连边模式");
    app.jump_to(1);
    app.handle(键(KeyCode::Enter));
    assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
    assert!(app.view.link.is_none(), "建完退出连边模式");
    assert_eq!(app.doc.edges[0].dir, Dir::Forward);
}

#[test]
fn 连边_一对多() {
    let mut app = 连边应用(横排文档(4));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    // 目标用 v 框住 1、2、3
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(3);
    app.handle(键(KeyCode::Enter));
    let made = 边名(&app);
    // 从 b0 起框,选区含源本身:自环 b0->b0 也建
    assert_eq!(made.len(), 4);
    for (from, to) in made {
        assert_eq!(from, "b0");
        assert!(to == "b0" || to == "b1" || to == "b2" || to == "b3", "{to}");
    }
}

#[test]
fn 连边_多对一() {
    let mut app = 连边应用(横排文档(4));
    // 源:0、1、2 选区
    app.view.cursor = app.cell_of(0);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(2);
    app.handle(键(KeyCode::Enter));
    app.handle(键(KeyCode::Char('e')));
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1, 2]);
    app.jump_to(3);
    app.handle(键(KeyCode::Enter));
    assert_eq!(边名(&app).len(), 3);
    assert!(app.doc.edges.iter().all(|e| e.to == "b3"));
}

#[test]
fn 连边_多对多() {
    let mut app = 连边应用(横排文档(5));
    直连(&mut app, vec![0, 1], vec![2, 3, 4]);
    assert_eq!(边名(&app).len(), 6, "2x3 该建 6 条");
}

#[test]
fn 连边_自环与重复() {
    let mut app = 连边应用(横排文档(2));
    直连(&mut app, vec![0, 1], vec![0, 1]);
    // 四对:两条自环(b1->b1、b2->b2)与 0->1、1->0
    assert_eq!(边名(&app).len(), 4, "自环也该建出来");
    assert_eq!(
        app.doc.edges.iter().filter(|e| e.from == e.to).count(),
        2,
        "两条自环"
    );
    // 再来一次:全是重复
    let undo0 = app.undo.counts().0;
    直连(&mut app, vec![0], vec![1]);
    assert_eq!(边名(&app).len(), 4, "重复的不再加");
    assert_eq!(app.undo.counts().0, undo0, "一条没建不压撤销");
}

#[test]
fn 连边_整批一步撤销() {
    let mut app = 连边应用(横排文档(5));
    let undo0 = app.undo.counts().0;
    直连(&mut app, vec![0, 1], vec![2, 3, 4]);
    assert_eq!(app.doc.edges.len(), 6);
    assert_eq!(app.undo.counts().0, undo0 + 1, "整批只算一步");
    app.undo_step();
    assert!(app.doc.edges.is_empty(), "撤一次全部消失");
}

#[test]
fn 连边_esc取消不动文档() {
    let mut app = 连边应用(横排文档(3));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    app.jump_to(2);
    app.handle(键(KeyCode::Esc));
    assert!(app.view.link.is_none());
    assert!(app.doc.edges.is_empty(), "取消不该建边");
}

#[test]
fn 连边_源选区未确认先确认() {
    let mut app = 连边应用(横排文档(3));
    app.view.cursor = app.cell_of(0);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(1);
    // 没按 Enter 直接按 e
    app.handle(键(KeyCode::Char('e')));
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1]);
    assert!(app.view.sel.is_none(), "源选区交给目标用");
}

#[test]
fn 连边_目标选区未确认也能建() {
    let mut app = 连边应用(横排文档(4));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(2);
    // 没确认目标选区,Enter 一气确认 + 建边(选区含源,多一条自环)
    app.handle(键(KeyCode::Enter));
    assert_eq!(边名(&app).len(), 3);
}

#[test]
fn 连边_目标阶段的v不覆盖源() {
    let mut app = 连边应用(横排文档(4));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = app.cell_of(3);
    assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0]);
}

#[test]
fn 连边_数量大要二次确认() {
    let mut app = 连边应用(横排文档(20));
    // 8 源 x 9 目标 = 72 条:第一次 Enter 只进确认态
    直连(&mut app, (0..8).collect(), (9..18).collect());
    assert!(app.doc.edges.is_empty(), "第一次 Enter 不建");
    let link = app.view.link.clone().expect("还在连边模式");
    assert!(link.confirming);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("再按 Enter"), "{note}");
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.edges.len(), 72);
}

#[test]
fn 连边_超过上限直接拒绝() {
    let mut app = 连边应用(横排文档(60));
    直连(&mut app, (0..30).collect(), (30..50).collect());
    assert!(app.doc.edges.is_empty(), "600 条超上限,一条都不建");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("512"), "{note}");
    assert!(app.view.link.is_some(), "还在连边模式等缩小选区");
}

#[test]
fn 连边_分组按聚焦或源自己的分组() {
    // 样例文档里 b0 属于 g0、b2 属于 g0;只聚焦 g1 时全体用 g1
    let mut app = 连边应用(样例文档());
    app.view.focus = vec![1];
    直连(&mut app, vec![0], vec![2]);
    assert_eq!(app.doc.edges.last().unwrap().group, "g1");
    // 不聚焦:用源框自己的第一个分组
    let mut app = 连边应用(样例文档());
    直连(&mut app, vec![0], vec![2]);
    assert_eq!(app.doc.edges.last().unwrap().group, "g0");
}

#[test]
fn 连边_shift点击加目标() {
    let mut app = 连边应用(横排文档(4));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    let p = render::panels(&app.doc, app.area, &app.view);
    let at: Vec<(u16, u16)> = (0..app.doc.boxes.len())
        .map(|i| {
            let (x, y) = p.proj.cell(app.doc.center(i));
            (x as u16, y as u16)
        })
        .collect();
    let 点 = |app: &mut App, i: usize, shift: bool| {
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
    // 普通点击:目标就设成这一个
    点(&mut app, 2, false);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
    // Shift 点击:加进来
    点(&mut app, 3, true);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2, 3]);
    // 再 Shift 点一次:移除
    点(&mut app, 3, true);
    assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
}

/// 剪贴板里带边时,粘贴的落点要和没边时一样(不能被边的另一头带跑)
#[test]
fn 粘贴位置不受边影响() {
    // b 在左上、a 在右下,两者之间有边;删掉 a 会把这条边也带进剪贴板
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加一条边");
    app.doc.move_box(1, -20, 10);
    app.doc.move_box(0, 20, 0);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let a = app.doc.box_idx("b0").unwrap();
    app.jump_to(a);
    // d = 带边剪切:边从文档里删掉,但它是边界边(另一头还在),不进剪贴板
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(1),
        0,
        "边界边不进剪贴板"
    );
    app.view.cursor = (20, 0);
    app.handle(键(KeyCode::Char('p')));
    let pasted = app
        .doc
        .boxes
        .iter()
        .find(|b| b.id != "b0" && b.id != "b1")
        .expect("该粘回来一个框");
    assert_eq!(pasted.at, (20, 0), "粘贴落点该是光标处,和边无关");
}

#[test]
fn 连边_删掉源之后按id重新定位() {
    let mut app = 连边应用(横排文档(3));
    app.jump_to(2);
    app.handle(键(KeyCode::Char('e'))); // 源 = b2
    let src = app.doc.boxes[2].id.clone();
    app.view.cursor = app.cell_of(0);
    app.refresh_pick();
    // 源框被删:连边状态该自己退出
    app.doc.remove_box(&src).expect("删源");
    app.doc.rebuild();
    app.handle(键(KeyCode::Enter));
    assert!(app.view.link.is_none(), "源都没了,连边该退出");
}

/// d 剪贴板含关联边,x 只含框和文字;两者都把边从文档里删干净
#[test]
fn d_带边剪切带走关联边() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.edges.len(), 0, "关联边要一起删,不能留悬空");
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 1);
    assert_eq!(clip.texts.len(), 1);
    assert!(clip.edges.is_empty(), "边界边(另一头还在文档里)不进剪贴板");
    // x 不再是绑定键
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('x')));
    assert!(app.clipboard.is_none(), "x 不该再剪切任何东西");
}

/// x 粘出来的内容没有边;d 粘出来的内容带边
#[test]
fn 粘贴不带边界边() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    app.handle(键(KeyCode::Char('p')));
    assert_eq!(app.doc.edges.len(), 0, "边界边粘不出来(另一端不在片段里)");
}

/// 连按 p:三份不会完全重叠,id 全不同
#[test]
fn 连按粘贴错开且id不重复() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    let mut seen: Vec<(i32, i32)> = Vec::new();
    for _ in 0..3 {
        app.handle(键(KeyCode::Char('p')));
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

/// 第一份粘在光标的精确位置,并且选择指向它
#[test]
fn 第一份粘贴落在光标处并被选中() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    app.view.cursor = (40, 12);
    app.handle(键(KeyCode::Char('p')));
    let i = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[i].at, (40, 12), "第一份该精确落在光标处");
    assert_eq!(app.view.pick, render::Pick::Box(i), "该选中刚粘的那份");
    assert_eq!(
        app.view.cursor,
        app.cell_of(i),
        "光标该落到刚粘的那份上(下一次命令默认动它)"
    );
}

/// 深拷贝:改一份不影响别的;via 也各是各的
#[test]
fn 粘贴内容是深拷贝() {
    // 三个框,剪前两个:b0-b1 是内部边(进剪贴板),b1-b2 是边界边(只从文档删)
    let mut app = 连边应用(横排文档(3));
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
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), 1, "只剩第三个框");
    assert_eq!(app.doc.edges.len(), 0, "边界边一起删掉");
    app.handle(键(KeyCode::Char('p')));
    app.handle(键(KeyCode::Char('p')));
    assert_eq!(app.doc.edges.len(), 2, "两次粘贴各带一条边");
    // 改第一条边的折点,第二条不受影响
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
    // 剪贴板不被后续修改影响
    let clip = app.clipboard.clone().unwrap();
    assert!(clip.edges[0].via.is_empty(), "剪贴板保持删除时的样子");
}

/// 主回归:三份里删掉第三份的边,前两份的边还在(不再删错下标)
#[test]
fn 删一份的边不影响别的份() {
    // 选区里两个框 + 它们之间的边:这条边是内部的,进剪贴板
    let mut app = 连边应用(横排文档(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.view.sel = Some(RectSel {
        anchor: (-2, -2),
        end: (14, 5),
        confirmed: true,
    });
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.view.sel, None);
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(0),
        1,
        "内部边进剪贴板"
    );
    for _ in 0..3 {
        app.handle(键(KeyCode::Char('p')));
    }
    assert_eq!(app.doc.edges.len(), 3, "三份各带一条边(边界边已经剪掉)");
    // 删掉最后粘的那份:
    let last = app.doc.boxes.len() - 1;
    app.view.cursor = app.cell_of(last);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.edges.len(), 2, "只删掉那一份的边");
}

/// 文档改过之后,旧命中表里的边下标不能再当选择用
#[test]
fn 旧命中表不能跨修改选边() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area); // 生成一张命中表
    assert_eq!(app.hit.rev, app.doc.rev(), "命中表和文档同版");
    // 改文档(粘贴一个框)
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    app.handle(键(KeyCode::Char('p')));
    assert_ne!(app.hit.rev, app.doc.rev(), "旧命中表该作废");
    assert!(app.usable_hit().candidates.is_empty(), "作废后不返回候选");
}

/// 重叠的框:取最上面那个,E 能在它们之间轮
#[test]
fn 重叠的框取最上面且能轮选() {
    let mut app = 连边应用(横排文档(2));
    // 把第二个框挪到和第一个完全重合
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

/// 连着删两条边,中间不重绘:第二条不能因为下标前移而删错
#[test]
fn 连续删边不错位() {
    let mut app = 连边应用(横排文档(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("e1");
    app.doc
        .add_edge("b1", "b2", "g0", Dir::Forward)
        .expect("e2");
    app.jump_to(1); // 停在中间那个框上,它两边各连着一条
    let mut gone = Vec::new();
    for _ in 0..2 {
        app.handle(ctrl(KeyCode::Char('e')));
        let e = app.view.pick.edge().expect("该选到一条边");
        gone.push(app.doc.edges[e].id.clone());
        app.handle(键(KeyCode::Char('d')));
    }
    assert!(app.doc.edges.is_empty(), "两条边该都删掉");
    assert_ne!(gone[0], gone[1], "两次删的该是两条不同的边");
}

/// 粘贴/删除各一步撤销;撤销重做不动剪贴板
#[test]
fn 粘贴单步撤销且不动剪贴板() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    let clip_after_cut = app.clipboard.clone().unwrap();
    let undo_after_cut = app.undo.counts().0;
    app.handle(键(KeyCode::Char('p')));
    assert_eq!(app.undo.counts().0, undo_after_cut + 1, "粘贴一步撤销");
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.boxes.len()),
        Some(clip_after_cut.boxes.len()),
        "粘贴不改剪贴板"
    );
    let boxes_before = app.doc.boxes.len();
    app.handle(键(KeyCode::Char('u')));
    assert_eq!(app.doc.boxes.len(), boxes_before - 1, "撤销掉这次粘贴");
    assert_eq!(
        app.clipboard.as_ref().map(|c| c.boxes.len()),
        Some(clip_after_cut.boxes.len()),
        "撤销不改剪贴板"
    );
    app.handle(ctrl(KeyCode::Char('r')));
    assert_eq!(app.doc.boxes.len(), boxes_before, "重做回来");
}

/// 选区里的边:d 复制进剪贴板,x 只删不复制
#[test]
fn 选区里的边_剪贴板只收内部边() {
    let mut app = 连边应用(横排文档(3));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    // 框住前两个框(它们的边是内部边)
    app.view.cursor = (0, 0);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(键(KeyCode::Enter));
    app.handle(键(KeyCode::Char('d')));
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 2, "两个框");
    assert_eq!(clip.edges.len(), 1, "内部边进剪贴板");

    // 边界边:只选一个框,边从文档删掉但不进剪贴板
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(键(KeyCode::Char('d')));
    let clip = app.clipboard.clone().expect("剪贴板");
    assert_eq!(clip.boxes.len(), 1);
    assert!(clip.edges.is_empty(), "边界边不进剪贴板");
    assert_eq!(app.doc.edges.len(), 0, "但文档里的边已经删掉");
}

/// 删除之后光标留在原地(不跳到最近的框上)
#[test]
fn 删除后光标不动() {
    let mut app = 连边应用(横排文档(3));
    app.jump_to(1);
    let before = app.view.cursor;
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.view.cursor, before, "光标该留在原地");
    assert!(
        app.doc.box_at_cell(before).is_none(),
        "那个框已经删掉了,光标格上应该空了"
    );
}

#[test]
fn 连边_右键菜单能确认和取消() {
    // 确认连边
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    app.jump_to(1);
    app.open_menu(40, 10);
    let menu = app.view.menu.clone().expect("该弹出菜单");
    let titles: Vec<&str> = menu.level().iter().map(|i| i.title.as_str()).collect();
    assert!(titles.contains(&"确认连边"), "{titles:?}");
    app.menu_choose();
    assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
    // 取消连边
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    app.open_menu(40, 10);
    app.view.menu.as_mut().unwrap().sel = 1;
    app.menu_choose();
    assert!(app.view.link.is_none());
    assert!(app.doc.edges.is_empty());
}

#[test]
fn 连边_要求一比一() {
    let mut app = 连边应用(横排文档(2));
    app.view.zoom = 2.0;
    app.jump_to(0);
    app.handle(键(KeyCode::Char('e')));
    assert!((app.view.zoom - 1.0).abs() < 1e-9, "连边前先切回 1:1");
}

/// s:框上开框菜单,边上开边菜单,空处只提示
#[test]
fn s_按类型打开样式菜单() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(键(KeyCode::Char('s')));
    let menu = app.view.menu.clone().expect("框上该开菜单");
    assert_eq!(menu.level()[0].title, "框型");
    assert!(matches!(menu.level()[0].action, MenuAction::Sub(_)));
    app.handle(键(KeyCode::Esc));

    // 边上:先选到边
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
    app.handle(键(KeyCode::Char('s')));
    let menu = app.view.menu.clone().expect("边上该开菜单");
    assert_eq!(menu.level()[0].title, "线型");
    app.handle(键(KeyCode::Esc));

    // 空处:不弹菜单,只提示
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('s')));
    assert!(app.view.menu.is_none());
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("没有可改样式"), "{note}");
}

/// 新建边照抄"最近选中的那条边"的分组/线型/拐角/颜色
#[test]
fn 新建边照抄最近选中的边() {
    let mut app = 连边应用(横排文档(3));
    let e0 = app
        .doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.doc.edges[e0].style = crate::model::EdgeStyle::Dashed;
    app.doc.edges[e0].corner = crate::model::Corner::Sharp;
    app.doc.edges[e0].color = Some((9, 8, 7));
    app.doc.edges[e0].bg = Some((1, 1, 1));
    app.doc.rebuild();
    // 先记下这条边的样式,再建新边
    app.view.pick = render::Pick::Edge(e0);
    app.remember_look();
    assert!(app.edge_look.is_some(), "选中边该记下样式");

    直连(&mut app, vec![1], vec![2]);
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

    // 区块选区时回到默认样式
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (3, 3),
        confirmed: true,
    });
    直连(&mut app, vec![2], vec![0]);
    let e2 = app
        .doc
        .edges
        .iter()
        .position(|e| e.from == "b2" && e.to == "b0")
        .expect("又建了一条");
    assert_eq!(app.doc.edges[e2].style, crate::model::EdgeStyle::Solid);
    assert_eq!(app.doc.edges[e2].color, None);
}

/// 新建框照抄"最近选中的那个框"的样式/颜色/对齐;区块选区时不照抄
#[test]
fn 新建框照抄最近选中的框() {
    let mut app = 连边应用(横排文档(2));
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

    // 区块选区:不照抄,回到默认(只有文字)
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

/// 底行输入框:i 预填原文,光标在末尾;Alt-B/F 与 Alt-左右按词跳;emacs 基本键照旧
#[test]
fn 输入框预填原文且能按词跳() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    // 底行输入框:直接打开(默认键位里已经没有开它的键了,用 :text 或菜单)
    app.open_prompt(Prompt::Text(vec![0]), app.common_text(&[0]));
    match app.view.prompt.clone() {
        Some((Prompt::Text(_), text)) => assert_eq!(text, "甲", "预填原文"),
        other => panic!("该是改文字:{other:?}"),
    }
    assert_eq!(app.view.prompt_pos, 1, "光标在末尾");

    // Ctrl-U 清空,打三个词
    app.handle(ctrl(KeyCode::Char('u')));
    for c in "one two three".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    let text = |app: &App| app.view.prompt.as_ref().map(|(_, t)| t.clone()).unwrap();
    assert_eq!(text(&app), "one two three");
    assert_eq!(app.view.prompt_pos, 13);

    // Alt-左:跳到 three 前面;再跳两次到 one 前面
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.prompt_pos, 8);
    app.handle(alt(KeyCode::Char('b')));
    assert_eq!(app.view.prompt_pos, 4);
    app.handle(alt(KeyCode::Char('b')));
    assert_eq!(app.view.prompt_pos, 0);
    // Alt-右:按词尾往后
    app.handle(alt(KeyCode::Right));
    assert_eq!(app.view.prompt_pos, 3);
    app.handle(alt(KeyCode::Char('f')));
    assert_eq!(app.view.prompt_pos, 7);

    // Ctrl-K 删到行尾,Ctrl-A/E 走首尾,插入落在光标处
    app.handle(ctrl(KeyCode::Char('k')));
    assert_eq!(text(&app), "one two");
    app.handle(ctrl(KeyCode::Char('a')));
    app.handle(键(KeyCode::Char('>')));
    assert_eq!(text(&app), ">one two");
    // Ctrl-W 删掉前一个词(光标前的 "one")
    app.handle(ctrl(KeyCode::Char('e')));
    app.handle(ctrl(KeyCode::Char('w')));
    assert_eq!(text(&app), ">one ");
    // Alt+退格:删掉前一个词(和 Ctrl-W 一样)
    app.handle(ctrl(KeyCode::Char('u')));
    for c in "alpha beta gamma".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(键(KeyCode::End));
    app.handle(alt(KeyCode::Backspace));
    assert_eq!(text(&app), "alpha beta ");

    // Delete 删光标处的字符;Backspace 删光标前
    app.handle(ctrl(KeyCode::Char('a')));
    app.handle(键(KeyCode::Delete));
    assert_eq!(text(&app), "lpha beta ");
    app.handle(键(KeyCode::End));
    app.handle(键(KeyCode::Backspace));
    assert_eq!(text(&app), "lpha beta");
    app.handle(键(KeyCode::Esc));
    assert!(app.view.prompt.is_none());
}

/// 命令行(:)用同一套行编辑:光标能进能出,按词跳,Home/End
#[test]
fn 命令行也能按词跳() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.handle(键(KeyCode::Char(':')));
    for c in "w out/x.gtui".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));
    assert_eq!(app.view.cmd_pos, 12);
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.cmd_pos, 2, "跳到 out/x.gtui 前面");
    app.handle(alt(KeyCode::Left));
    assert_eq!(app.view.cmd_pos, 0);
    app.handle(键(KeyCode::Home));
    assert_eq!(app.view.cmd_pos, 0);
    app.handle(键(KeyCode::Char('#')));
    assert_eq!(app.view.cmd.as_deref(), Some("#w out/x.gtui"));
    // 光标在 '#' 后面:Ctrl-K 只删到行尾,留下已经打的 '#';再 Ctrl-U 清掉行首
    app.handle(ctrl(KeyCode::Char('k')));
    assert_eq!(app.view.cmd.as_deref(), Some("#"));
    app.handle(ctrl(KeyCode::Char('u')));
    assert_eq!(app.view.cmd.as_deref(), Some(""));
    app.handle(键(KeyCode::Esc));
}

/// 拆分标记:一个框拆成多个,右边的框排在后面,一步撤销
#[test]
fn 用标记把一个框拆成多个() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let n0 = app.doc.boxes.len();
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    改文字(&mut app, "hello,<|new box|>world!");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"hello,"), "{texts:?}");
    assert!(texts.contains(&"world!"), "{texts:?}");
    assert_eq!(app.doc.boxes.len(), n0 + 1, "多出一个框");
    // 光标还在原框
    assert_eq!(app.view.pick, render::Pick::Box(i));
    assert!(app
        .view
        .note
        .as_deref()
        .unwrap_or("")
        .contains("拆成 2 个框"));
    // 新框在原框右边、顶对齐
    let new = app.doc.boxes.len() - 1;
    assert!(app.doc.boxes[new].at.0 > app.doc.boxes[i].at.0, "放在右边");
    assert_eq!(app.doc.boxes[new].at.1, app.doc.boxes[i].at.1, "顶对齐");
    // 一步撤销:文字回到拆分前,新框一起消失
    app.undo_step();
    let back: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    assert_eq!(back, before, "撤销该恢复拆分前的文字");
    assert_eq!(app.doc.boxes.len(), n0, "新框一起撤销");
}

/// 在光标框上把 `t` 提示行改成 `text` 再回车
fn 改文字(app: &mut App, text: &str) {
    // t 已经不是默认键了:直接走 :text 那条命令路径(和输入框提交同一个入口)
    let mut ids = app.targets().boxes;
    if ids.is_empty() {
        if let Some(i) = app.cursor_box() {
            ids.push(i);
        }
    }
    app.cmd_set_text(&ids, text);
}

/// 多个标记:从左到右依次排开
#[test]
fn 标记可以拆成三个框() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "a<|new box|>b<|new box|>c");
    assert_eq!(app.doc.boxes.len(), n0 + 2);
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    for t in ["a", "b", "c"] {
        assert!(texts.contains(&t), "{t} 没切出来:{texts:?}");
    }
    // b、c 依次在原框右侧,互不重叠
    let mut xs: Vec<(i32, u16)> = (0..app.doc.boxes.len())
        .filter(|&k| k == i || k >= n0)
        .map(|k| (app.doc.boxes[k].at.0, app.doc.boxes[k].size.0))
        .collect();
    xs.sort();
    for w in xs.windows(2) {
        assert!(w[0].0 + w[0].1 as i32 <= w[1].0, "框互相压住:{xs:?}");
    }
}

/// 空段丢弃:开头 / 结尾 / 连续标记都不建空框;只剩一段就不拆
#[test]
fn 拆分的空段丢掉() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "<|new box|>a<|new box|><|new box|>b<|new box|>");
    assert_eq!(app.doc.boxes.len(), n0 + 1, "只有 a / b 两个非空段");
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "<|new box|><|new box|>");
    assert_eq!(app.doc.boxes.len(), n0, "没有内容就不拆");
    assert_eq!(
        app.doc
            .texts
            .iter()
            .map(|t| t.text.clone())
            .collect::<Vec<_>>(),
        before,
        "失败时不写文字"
    );
    assert!(app
        .view
        .note
        .as_deref()
        .unwrap_or("")
        .contains("nothing to split"));
}

/// 转义:反斜杠保住字面标记,不拆
#[test]
fn 转义的标记当文字() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "x\\<|new box|>y");
    assert_eq!(app.doc.boxes.len(), n0, "转义不拆");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"x\\<|new box|>y"), "{texts:?}");
}

/// 右边有框就让开,绝不重叠
#[test]
fn 拆分的新框避开右边的框() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let i = app.cursor_box().unwrap();
    let before: Vec<(i32, i32, u16, u16)> = app
        .doc
        .boxes
        .iter()
        .map(|b| (b.at.0, b.at.1, b.size.0, b.size.1))
        .collect();
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "long text here<|new box|>z");
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

/// 不能拆的:多选 / 边注释框 / 点节点,都按普通文字写
#[test]
fn 不能拆的框标记当文字() {
    // 多选
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.view.sel = Some(RectSel {
        anchor: (-5, -5),
        end: (40, 20),
        confirmed: true,
    });
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "多选不拆");
    let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(
        texts.contains(&"a<|new box|>b"),
        "标记按普通文字写:{texts:?}"
    );
    // 点节点
    let mut doc = 横排文档(2);
    doc.boxes[0].style = crate::model::BoxStyle::Point;
    let mut app = 连边应用(doc);
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    改文字(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "点节点不拆");
    assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
    // 边注释框
    let mut app = 连边应用(横排文档(2));
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
    改文字(&mut app, "a<|new box|>b");
    assert_eq!(app.doc.boxes.len(), n0, "注释框不拆");
    assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
}

/// 新框复制原框的框型、颜色、填充、对齐、图层
#[test]
fn 拆分的新框照抄样式() {
    let mut app = 连边应用(横排文档(2));
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
    改文字(&mut app, "p<|new box|>q");
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

/// 边留在原框;新框不连边;缩放不影响拆分
#[test]
fn 拆分后边留在原框() {
    let mut app = 连边应用(横排文档(3));
    let _ = app.doc.add_edge("b0", "b1", "g0", Dir::Forward);
    app.refresh_pick();
    app.jump_to(0);
    app.view.zoom = 2.0;
    app.view.center = Some((app.view.center.unwrap_or((0.0, 0.0)).0 + 1.0, 0.0));
    let i = app.cursor_box().unwrap();
    改文字(&mut app, "p<|new box|>q");
    assert_eq!(app.doc.edges.len(), 1, "边数不变");
    assert_eq!(app.doc.edges[0].from, app.doc.boxes[i].id, "边还挂在原框");
    assert!(
        app.doc.boxes.last().unwrap().edge.is_none(),
        "新框没有注释关系"
    );
}

/// `:text` 与 headless 都不拆:标记是普通文字
#[test]
fn 命令行走文本不拆() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let n0 = app.doc.boxes.len();
    app.cmd_apply("text q<|new box|>r");
    assert_eq!(app.doc.boxes.len(), n0, ":text 不拆");
}

/// 英文界面:样式菜单(框/边)的标题不能出现 CJK;中文界面照旧
#[test]
fn 英文菜单不出现中文() {
    fn has_cjk(s: &str) -> bool {
        s.chars()
            .any(|c| matches!(c as u32, 0x3000..=0x303f | 0x4e00..=0x9fff | 0xff00..=0xffef))
    }
    let en_opts = Options {
        lang: Lang::En,
        ..Options::default()
    };
    for on_edge in [false, true] {
        let mut doc = 横排文档(2);
        doc.add_edge("b0", "b1", "g0", Dir::Forward).expect("加边");
        let mut app = 连边应用(doc);
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
        let _ = &en_opts;
    }
}

/// 默认界面英文;`--lang zh`(Options.lang)与文档里的 view.lang 都能切回中文
#[test]
fn 默认英文两种办法切中文() {
    assert_eq!(Lang::default(), Lang::En);
    assert_eq!(crate::document::ViewState::default().lang, "en");

    // 默认:英文
    let app = App::new(Document::scratch(), Options::default()).expect("开局");
    assert_eq!(app.view.lang, Lang::En);

    // 命令行 --lang zh
    let app = App::new(Document::scratch(), zh_opts()).expect("开局");
    assert_eq!(app.view.lang, Lang::Zh);

    // 文档里的 view.lang = zh:新开还是英文,打开这份文档就变中文
    let mut doc = Document::scratch();
    doc.view.lang = "zh".to_string();
    let app = App::new(doc, Options::default()).expect("开局");
    assert_eq!(app.view.lang, Lang::Zh, "文档里的语言要生效");
}

/// `s` 菜单改过样式的框也算"选中":光标挪到空白处再 :new,照样照抄它
#[test]
fn 改过样式的框也算选中() {
    let mut app = 连边应用(横排文档(2));
    app.view.cursor = app.cell_of(0);
    app.refresh_pick();
    // 走菜单改样式:框型 => double
    app.handle(键(KeyCode::Char('s')));
    app.handle(键(KeyCode::Char('l')));
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
        app.handle(键(KeyCode::Char('j')));
    }
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Double);

    // 光标挪到空白处(那里没有框),再新建
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

/// 边菜单的"端口":起点/终点各一档 auto + 四侧,命令就是 :fromside / :toside
#[test]
fn 端口菜单能改边两端的侧() {
    let mut app = 连边应用(横排文档(2));
    let e = app
        .doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
    app.handle(键(KeyCode::Char('s')));
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
    app.handle(键(KeyCode::Esc));
    // 关菜单会把 pick 交回光标下的元素,重新选一次边
    app.handle(ctrl(KeyCode::Char('e')));
    assert!(app.view.pick.edge().is_some(), "重新选到边");

    // 走菜单背后那条命令:起点 left、终点 up 各一步撤销
    app.cmd_apply("fromside left");
    app.cmd_apply("toside up");
    assert_eq!(app.doc.edges[e].from_side, Some(Side::Left));
    assert_eq!(app.doc.edges[e].to_side, Some(Side::Up));

    app.handle(键(KeyCode::Char('u')));
    assert_eq!(app.doc.edges[e].from_side, Some(Side::Left), "只回退一步");
    assert_eq!(app.doc.edges[e].to_side, None);
}

/// c:颜色菜单 = auto + 12 色,当前值打 *;只改边框色
#[test]
fn c_颜色菜单只改主颜色() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('c')));
    let menu = app.view.menu.clone().expect("该开颜色菜单");
    let items = menu.level().to_vec();
    assert_eq!(items.len(), 13, "auto + 12 色");
    assert_eq!(items[0].title, "auto");
    assert!(items[0].checked, "还没设过颜色,auto 该打 *");
    app.handle(键(KeyCode::Esc));
    // 设一个颜色和填充,再看标记
    app.cmd_apply("nodecolor #ef4b4b");
    app.cmd_apply("fill #22b46a");
    app.handle(键(KeyCode::Char('c')));
    // 再开一次:这次当前值在调色板里
    app.handle(键(KeyCode::Char('c')));
    let menu = app.view.menu.clone().expect("再开");
    let hit = menu
        .level()
        .iter()
        .position(|i| i.checked)
        .expect("该有打 * 的项");
    assert_eq!(hit, 2, "12 色里第二个 #ef4b4b");
    // 选第三个颜色(索引 3)回车:只改边框色
    app.handle(键(KeyCode::Char('j')));
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].color, Some((0x22, 0xb4, 0x6a)), "边框色");
    assert_eq!(app.doc.boxes[0].fill, Some((0x22, 0xb4, 0x6a)), "填充不变");
}

/// 预览不动撤销,Esc 还原
#[test]
fn 菜单预览不压撤销_esc还原() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let undo0 = app.undo.counts().0;
    let before = app.doc.boxes[0].style;
    app.handle(键(KeyCode::Char('s'))); // 框型
    app.handle(键(KeyCode::Char('l'))); // 进子菜单(预览第一项 point)
    app.handle(键(KeyCode::Char('j'))); // 预览 single
    assert_ne!(app.doc.boxes[0].style, before, "预览该实时改文档");
    assert_eq!(app.undo.counts().0, undo0, "预览不压撤销");
    app.handle(键(KeyCode::Esc)); // 退回上一级
    app.handle(键(KeyCode::Esc)); // 关掉
    assert_eq!(app.doc.boxes[0].style, before, "Esc 该还原");
    assert_eq!(app.undo.counts().0, undo0);
}

/// 多选一次改多个,只算一步
#[test]
fn 菜单多选一次改多个() {
    let mut app = 连边应用(横排文档(3));
    app.view.cursor = (0, 0);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = (13, 2);
    app.handle(键(KeyCode::Enter));
    // 光标回到框上,菜单按框类型走
    app.jump_to(0);
    let undo0 = app.undo.counts().0;
    app.handle(键(KeyCode::Char('s')));
    app.handle(键(KeyCode::Char('l')));
    // 子菜单里按标题找 rounded(插了 none 之后下标会变)
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
        app.handle(键(KeyCode::Char('j')));
    }
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Rounded);
    assert_eq!(app.doc.boxes[1].style, BoxStyle::Rounded, "两个都改");
    assert_eq!(app.undo.counts().0, undo0 + 1, "整批一步");
    assert!(app.view.sel.is_some(), "菜单不该动选区");
}

/// 边的线型菜单:应用后一步撤销
#[test]
fn 边菜单改线型() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.handle(ctrl(KeyCode::Char('e')));
    let undo0 = app.undo.counts().0;
    app.handle(键(KeyCode::Char('s')));
    app.handle(键(KeyCode::Char('l')));
    app.handle(键(KeyCode::Char('j')));
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.edges[0].style, EdgeStyle::Dashed);
    assert_eq!(app.undo.counts().0, undo0 + 1);
}

/// 同值选择不压撤销
#[test]
fn 菜单选同一个值不压撤销() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.cmd_apply("box single");
    let undo0 = app.undo.counts().0;
    app.handle(键(KeyCode::Char('s')));
    app.handle(键(KeyCode::Char('l')));
    // 子菜单里按标题找 single
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
        app.handle(键(KeyCode::Char('j')));
    }
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.doc.boxes[0].style, BoxStyle::Single);
    assert_eq!(app.undo.counts().0, undo0, "同值不该压撤销");
}

/// 边菜单的"分组"项走 group 动词
#[test]
fn 边菜单改分组() {
    let mut app = 连边应用(样例文档());
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(1); // b1 上有两条边
    app.handle(ctrl(KeyCode::Char('e')));
    let e = app.view.pick.edge().expect("选到边");
    let before = app.doc.edges[e].group.clone();
    app.handle(键(KeyCode::Char('s')));
    // 找到"分组"那一项并进去,选另一组
    let menu = app.view.menu.clone().expect("菜单");
    let pos = menu
        .level()
        .iter()
        .position(|i| i.title == "分组")
        .expect("边菜单该有分组项");
    for _ in 0..pos {
        app.handle(键(KeyCode::Char('j')));
    }
    app.handle(键(KeyCode::Char('l')));
    let sub = app.view.menu.clone().expect("子菜单");
    let other = (0..sub.level().len())
        .find(|i| !sub.level()[*i].checked)
        .expect("该有别的分组");
    for _ in 0..other {
        app.handle(键(KeyCode::Char('j')));
    }
    app.handle(键(KeyCode::Enter));
    assert_ne!(app.doc.edges[e].group, before, "分组该换掉");
}

/// textbg / edgebg:文字底色与边底色各改各的,而且能清掉
#[test]
fn 文字底色与边底色动词() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("加边");
    app.jump_to(0);
    app.cmd_apply("textbg #334455");
    assert_eq!(app.doc.text(0).unwrap().bg, Some((0x33, 0x44, 0x55)));
    assert_eq!(app.doc.boxes[0].fill, None, "文字底色不碰填充");
    // 边:先选中再改
    app.handle(ctrl(KeyCode::Char('e')));
    app.cmd_apply("edgebg #552211");
    let e = app.view.pick.edge().expect("选到边");
    assert_eq!(app.doc.edges[e].bg, Some((0x55, 0x22, 0x11)));
    assert_eq!(app.doc.edges[e].color, None, "边底色不该改边的前景色");
    // 清掉
    app.cmd_apply("edgebg none");
    assert_eq!(app.doc.edges[e].bg, None);
    app.jump_to(0);
    app.cmd_apply("textbg none");
    assert_eq!(app.doc.text(0).unwrap().bg, None);
}

/// 菜单里 h / l 只管层级:l 往下钻,h 往回收,叶子项不会被 l 执行
#[test]
fn 菜单hl切换层级() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.handle(键(KeyCode::Char('s')));
    assert_eq!(app.view.menu.as_ref().unwrap().levels.len(), 1);
    // l 进"框型"
    app.handle(键(KeyCode::Char('l')));
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        2,
        "l 该进子菜单"
    );
    assert_eq!(app.view.menu.as_ref().unwrap().parents.len(), 1);
    // h 回上一层
    app.handle(键(KeyCode::Char('h')));
    assert_eq!(
        app.view.menu.as_ref().unwrap().levels.len(),
        1,
        "h 该回上一层"
    );
    assert!(app.view.menu.as_ref().unwrap().parents.is_empty());
    // 再 h:顶层就关掉
    app.handle(键(KeyCode::Char('h')));
    assert!(app.view.menu.is_none(), "顶层 h 该关掉菜单");
    // 叶子项上按 l 不执行
    let undo0 = app.undo.counts().0;
    let before = app.doc.boxes[0].style;
    app.handle(键(KeyCode::Char('s')));
    app.handle(键(KeyCode::Char('l'))); // 进框型
    app.handle(键(KeyCode::Char('j'))); // 预览 single
    app.handle(键(KeyCode::Char('l'))); // 叶子:不该执行
    assert!(app.view.menu.is_some(), "l 在叶子上不该关菜单");
    assert_eq!(app.undo.counts().0, undo0, "l 在叶子上不该压撤销");
    // Enter 才执行
    app.handle(键(KeyCode::Enter));
    assert!(app.view.menu.is_none());
    assert_ne!(app.doc.boxes[0].style, before, "Enter 该应用");
    assert_eq!(app.undo.counts().0, undo0 + 1);
}

/// 菜单画出来:色块用底色、当前值打 *、状态栏报数量和值
#[test]
fn 菜单的色块与状态栏() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.cmd_apply("nodecolor #ef4b4b");
    app.handle(键(KeyCode::Char('c')));
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
    // 底部两行合起来看(菜单信息在下边那行,状态在上行)
    let rows: String = (app.area.height.saturating_sub(2)..app.area.height)
        .flat_map(|y| (0..app.area.width).map(move |x| (x, y)))
        .flat_map(|(x, y)| buf[(x, y)].symbol().chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(rows.contains("改1个框"), "状态栏该报作用数量:{rows}");
    assert!(rows.contains("边框色"), "状态栏该报属性:{rows}");
}

/// 右键菜单和键盘菜单内容一致
#[test]
fn 右键菜单和键盘菜单一致() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    let keyboard = app.menu_for();
    let p = render::panels(&app.doc, app.area, &app.view);
    let (cx, cy) = p.proj.cell(app.doc.center(0));
    app.open_menu(cx as u16, cy as u16);
    let mouse = app.view.menu.clone().expect("右键菜单");
    let a: Vec<&str> = keyboard.iter().map(|i| i.title.as_str()).collect();
    let b: Vec<&str> = mouse.level().iter().map(|i| i.title.as_str()).collect();
    assert_eq!(a, b);
}

#[test]
fn link_动词仍然单目标快速连边() {
    let mut app = 连边应用(横排文档(2));
    app.jump_to(0);
    app.cmd_apply("link b1");
    assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
    // c 不再是连边:它开颜色菜单
    app.handle(键(KeyCode::Char('c')));
    assert!(app.view.menu.is_some(), "c 该打开颜色菜单");
    assert!(app.view.prompt.is_none(), "c 不该再进连边提示");
}

#[test]
fn w_从dot来源写同目录gtui且不改原文件() {
    let dot = 临时路径("dot-src", "dot");
    std::fs::write(&dot, "// sentinel\n").expect("写 .dot");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// :wq 存成功才退;没有文件名时不退
#[test]
fn wq_存盘并退出() {
    let path = 临时路径("wq", "gtui");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.source = Some(path.clone());
    app.cmd_apply("wq");
    assert!(app.quit, "存成功该退出");
    assert!(Path::new(&path).exists(), "该写出来");
    let body = std::fs::read_to_string(&path).expect("读回来");
    assert!(body.contains("\"graph-tui\""));
    let _ = std::fs::remove_file(&path);

    // 没有文件名:提示要路径,且不退出
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.source = None;
    app.cmd_apply("wq");
    assert!(!app.quit, "没存成不该退出");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("文件名"), "{note}");

    // :wq <路径> 也能用
    let p2 = 临时路径("wq-path", "gtui");
    app.cmd_apply(&format!("wq {p2}"));
    assert!(app.quit, "带路径存成功该退出");
    assert!(Path::new(&p2).exists());
    let _ = std::fs::remove_file(&p2);
}

/// :wq 存失败也不退出(比如给了 .dot 路径)
#[test]
fn wq_存失败不退出() {
    let dot = 临时路径("wq-refuse", "dot");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("wq {dot}"));
    assert!(!app.quit, "存失败要留在原地");
    assert!(!Path::new(&dot).exists());
}

#[test]
fn w_从gtui来源写回原路径() {
    let path = 临时路径("gtui-src", "gtui");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.source = Some(path.clone());
    app.cmd_apply("w");
    let body = std::fs::read_to_string(&path).expect("该写出来");
    assert!(body.contains("\"graph-tui\""), "该是 native 文档:{body}");
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn stdin来源存盘要求显式路径() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.source = None;
    app.cmd_apply("w");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("文件名"), "该提示要显式路径:{note}");
    // :save 带路径就能写
    let path = 临时路径("stdin-save", "gtui");
    app.cmd_apply(&format!("save {path}"));
    assert!(Path::new(&path).exists());
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_只写gtui不写dot() {
    let dot = 临时路径("refuse", "dot");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("save {dot}"));
    assert!(!Path::new(&dot).exists(), "绝不写回 .dot");
    assert!(app.view.note.as_deref().unwrap_or("").contains(".gtui"));
}

#[test]
fn 导出dot不动图且把警告放回显位() {
    let dot = 临时路径("export", "dot");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
fn r_换布局后坐标仍是整数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 2.5;
    let want = app.view.layout.next();
    app.handle(键(KeyCode::Char('r')));
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
fn 视图状态往返一致并且开局映射进view() {
    let mut doc = 样例文档();
    doc.view = ViewState {
        center: (12.5, -3.0),
        zoom: 2.0,
        layout: "force".into(),
        cursor: Some(crate::document::Cursor::Box("b1".into())),
        focus: vec!["g1".into()],
        labels: true,
        lang: "en".into(),
        layer: Some(String::new()),
        side: 3.5,
        cycle: Some(1.25),
        orientation: "90".to_string(),
        notes: false,
    };
    let body = doc.save();
    let back = Document::load(&body).expect("读回");
    // 光标写出去会换成格子,比较解析后的格子
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
    // 默认选项不压文档里的语言:文档写 en 就用 en
    let app = App::new(back, Options::default()).expect("开局");
    assert_eq!(
        app.view.cursor,
        app.doc.text(1).unwrap().at,
        "cursor 按 id 映射成文字起点格"
    );
    assert_eq!(app.view.focus, vec![1], "focus 按 id 映射成下标");
    assert!(app.view.labels);
    assert_eq!(app.view.lang, Lang::En);
    // 打开即"按一次 0":文件里存的 zoom/center 不再恢复
    assert_eq!(app.view.zoom, 1.0, "打开该回到 1:1");
    assert!(app.view.center.is_none(), "打开该居中到整幅");
    assert_eq!(app.view.side, 3.5);
    assert_eq!(app.view.cycle, Some(1.25));
    assert_eq!(app.view.layout, Layout::Force);
    assert_eq!(app.view.layer.as_deref(), Some(""));
}

/// 打开即按 `0`:1:1、居中到整幅(`:e` 换文件也一样)
#[test]
fn 打开即按0() {
    let mut doc = 样例文档();
    doc.view.zoom = 3.0;
    doc.view.center = (40.0, 12.0);
    let mut app = App::new(doc.clone(), zh_opts()).expect("开局");
    assert_eq!(app.view.zoom, 1.0);
    assert!(app.view.center.is_none());
    assert!(app.view.pan.is_none() && app.view.target.is_none());

    // :e 打开另一个文件:同样归位
    let path = 临时路径("open-zero", "gtui");
    let mut other = 样例文档();
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
fn 冒号e打开gtui并重置视图() {
    let src = 临时路径("open-src", "gtui");
    let mut doc = 样例文档();
    doc.view.cursor = Some(crate::document::Cursor::Box("b2".into()));
    doc.view.zoom = 1.0;
    std::fs::write(&src, doc.save()).expect("写源文件");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 打开的是 en 文档:界面跟着变成英文
    assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
    let _ = std::fs::remove_file(&src);
}

#[test]
fn 冒号w写到指定路径并切换来源() {
    let path = 临时路径("colon-w", "gtui");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.cmd_apply(&format!("w {path}"));
    assert!(Path::new(&path).exists());
    assert_eq!(app.source.as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 数字前缀与命令解析保持原样() {
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
fn 搜索命中刷新() {
    let doc = 样例文档();
    // 数字:聚焦那一组
    let mut view = View::default();
    view.query = Some("1".into());
    refresh(&doc, &mut view);
    assert_eq!(view.focus, vec![0]);
    assert_eq!(view.hits, vec![0, 1]);
    // 文字:只挑命中的框,不碰聚焦(同组的其它框不该被一起点亮)
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

#[test]
fn 选边键循环并按光标清掉() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 鼠标点空白处的边 = 选中那条边,不起拖动
#[test]
fn 鼠标点边选中() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let area = app.area;
    let mut buf = ratatui::buffer::Buffer::empty(area);
    app.draw(&mut buf, area);
    // 找一格属于边、且换算回画布后不在任何框里的屏幕格
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

/// 鼠标在空白处拖:拉出区块选择(单击不算);Ctrl+左键/中键拖还是平移
#[test]
fn 鼠标拖出区块选择() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 找一个空白行:地图右下角附近
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
    // 这一块圈出来是空的:松手就不留选区(免得挡着后面的编辑)
    assert!(app.view.sel.is_none(), "空选区该被丢掉");

    // 拉一块盖住整幅图的选区:相交的框都算选中
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
    // 松开左键就是"选完了":直接确认,不用再按 Enter
    assert!(
        app.view.sel.expect("选区还在").confirmed,
        "拖完松手该已确认"
    );
    // Ctrl + 左键拖是平移,不起选区
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

/// 跳转历史:搜索也记一笔,Ctrl-o 退、Tab 进
#[test]
fn 跳转历史前进后退() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    let start = app.view.cursor;
    app.jump_to(2);
    let a = app.view.cursor;
    app.jump_to(1);
    let b = app.view.cursor;
    app.handle(键(KeyCode::Char('u'))); // 撤销不影响历史,这里只是热一下按键
    app.cmd_apply("undo");
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, a, "Ctrl-o 退回上一个跳点");
    app.handle(键(KeyCode::Tab));
    assert_eq!(app.view.cursor, b, "Tab(即 Ctrl-i)前进");
    app.handle(ctrl(KeyCode::Char('o')));
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, start, "连续退回到起点");
}

/// `a,bG` 跳到那一格,`gg` 回原点;序列没走完时挂在 showcmd 上
#[test]
fn 坐标跳转与gg() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    for c in "100,100".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.showcmd, "100,100", "没走完的序列该显示出来");
    app.handle(键(KeyCode::Char('G')));
    assert_eq!(app.view.cursor, (100, 100), "跳到那一格");
    assert!(app.view.showcmd.is_empty(), "按完就清掉");
    // gg 回原点
    app.handle(键(KeyCode::Char('g')));
    assert_eq!(app.view.showcmd, "g");
    app.handle(键(KeyCode::Char('g')));
    assert_eq!(app.view.cursor, (0, 0));
    assert!(app.view.showcmd.is_empty());
    // 单个数按 vim 的"第 n 行"算,列不动
    app.view.cursor = (7, 0);
    app.refresh_pick();
    for c in "40G".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (7, 40));
    // 纯数字前缀照旧管方向键
    app.view.cursor = (0, 0);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('5')));
    assert_eq!(app.view.showcmd, "5");
    app.handle(键(KeyCode::Char('l')));
    assert_eq!(app.view.cursor, (5, 0));
    assert!(app.view.showcmd.is_empty(), "动作做完序列就没了");
    // Esc 把没走完的序列丢掉
    app.handle(键(KeyCode::Char('1')));
    app.handle(键(KeyCode::Esc));
    assert!(app.view.showcmd.is_empty());
}

/// `0` 只把缩放拨回 1:1,不挪画面(回原点用 gg)
#[test]
fn 零只回缩放不动画面() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.zoom = 2.5;
    app.view.center = Some((40.0, 12.0));
    app.view.target = None;
    app.view.pan = None;
    app.handle(键(KeyCode::Char('0')));
    assert_eq!(app.view.zoom, 1.0, "回到 1:1");
    assert_eq!(app.view.center, Some((40.0, 12.0)), "画面不该挪");
    assert_eq!(app.view.target, None);
}

/// 跳到很远的地方:视图必须真的跟过去,而且落在正中    /// 跳到很远的地方:视图必须真的跟过去,而且落在正中
#[test]
fn 远距离跳转把光标放正中() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    for c in "1000,1000G".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (1000, 1000));
    assert_eq!(app.view.target, Some((1000.0, 1000.0)), "第一次跳转就居中");
    // 模拟缓动走完
    app.view.center = Some((1000.0, 1000.0));
    app.view.target = None;
    // 再跳回来:目标必须是 (10,10) 正中,不是只走 85%
    for c in "10,10G".chars() {
        app.handle(键(KeyCode::Char(c)));
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

/// 带链接文字的样例:框 a 的文字是 `[site](https://example.com) x`
fn 链接文档() -> Document {
    链接文档目标("https://example.com")
}

fn 链接文档目标(target: &str) -> Document {
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

fn 目标图(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("graph-tui-target-app");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{tag}.gtui"));
    std::fs::write(&p, 链接文档().save()).unwrap();
    p
}

/// 把光标放到链接上(测试里每次都重画一次命中表)
fn 点在链接上(app: &mut App) {
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    assert!(app.link_at_cursor().is_some(), "光标该在链接上");
}

/// `self:x,y`:当前 tab 跳过去,不新开,跳转表记一条
#[test]
fn self链接在图内跳不新开tab() {
    let mut app = 连边应用(链接文档目标("self:10,10"));
    点在链接上(&mut app);
    let tabs = app.tabs.len();
    let jumps = app.jumps.len();
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.tabs.len(), tabs, "self 不该新开 tab");
    assert_eq!(app.view.cursor, (10, 10));
    assert!(
        app.take_open_request().is_none(),
        "跳转由 App 做完,不甩给 main"
    );
    assert_eq!(app.jumps.len(), jumps + 1, "该记一个跳点");
    // 画得出来(越界坐标也不 panic)
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
}

/// 当前文档自己的路径 + `:x,y`:同样在本地跳
#[test]
fn 当前文档路径链接也在本地跳() {
    let path = 目标图("selfdoc");
    let mut app = 连边应用(链接文档目标(&format!("{}:4,5", path.display())));
    app.source = Some(path.display().to_string());
    点在链接上(&mut app);
    let tabs = app.tabs.len();
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.tabs.len(), tabs, "同一份文档不该新开 tab");
    assert_eq!(app.view.cursor, (4, 5));
}

/// 目标已开在另一个 tab:切过去再跳
#[test]
fn 别的已开图档切tab后跳() {
    let other = 目标图("open-other");
    let mut app = 连边应用(链接文档目标(&format!("{}:7,8", other.display())));
    app.new_tab(Some(&other.display().to_string()));
    assert_eq!(app.tab_at, 1, "先开成第二个 tab");
    app.switch_tab(0);
    点在链接上(&mut app);
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.tab_at, 1, "该切回那个 tab");
    assert_eq!(app.view.cursor, (7, 8));
    assert_eq!(app.tabs.len(), 2, "不该再多开一个");
}

/// 目标没开:新开 tab 再跳
#[test]
fn 没开过的图档新tab后跳() {
    let other = 目标图("fresh-other");
    let mut app = 连边应用(链接文档目标(&format!("{}:3,4", other.display())));
    点在链接上(&mut app);
    app.handle(键(KeyCode::Enter));
    assert_eq!(app.tabs.len(), 2, "该新开一个 tab");
    assert_eq!(app.tab_at, 1);
    assert_eq!(app.view.cursor, (3, 4));
    // 跳过去以后 Ctrl-o 要回到原来那个 tab 和格子
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.tab_at, 0, "Ctrl-o 该回到原来的 tab");
    assert_eq!(app.view.cursor, (1, 1), "回到点链接之前的位置");
}

/// 带位置的本地文本交给 main 的编辑器计划,不是就地跳
#[test]
fn 文本链接带位置给编辑器计划() {
    let dir = std::env::temp_dir().join("graph-tui-target-app");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("note.md");
    std::fs::write(&f, "hello\n").unwrap();
    let mut app = 连边应用(链接文档目标(&format!("{}:12,4", f.display())));
    点在链接上(&mut app);
    app.handle(键(KeyCode::Enter));
    let plan = app.take_open_request().expect("该有打开计划");
    assert_eq!(
        plan,
        crate::target::OpenPlan::Editor {
            path: f,
            position: Some(crate::target::Position { a: 12, b: 4 })
        }
    );
}

/// Enter / gf:打开光标下的链接;有未确认选区时 Enter 仍然只确认选区
#[test]
fn 打开光标下的链接() {
    let mut app = 连边应用(链接文档());
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 光标放到链接那几个字上
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    assert_eq!(app.link_at_cursor().as_deref(), Some("https://example.com"));
    app.handle(键(KeyCode::Enter));
    let req = app.take_open_request().expect("Enter 该请求打开链接");
    assert_eq!(
        req,
        crate::target::OpenPlan::Spawn {
            program: app.opener_command(),
            args: vec!["https://example.com".to_string()]
        }
    );
    // gf:同一件事
    app.handle(键(KeyCode::Char('g')));
    app.handle(键(KeyCode::Char('f')));
    assert!(app.take_open_request().is_some(), "gf 也该请求打开");
    // 没有链接:给提示,不退回"铺满画面"
    app.view.cursor = (20, 12);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    let zoom = app.view.zoom;
    app.handle(键(KeyCode::Char('g')));
    app.handle(键(KeyCode::Char('f')));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.view.cursor, (20, 12), "不能跳走");
    assert_eq!(app.view.zoom, zoom, "不能变成铺满画面");
    assert!(app.view.note.as_deref().unwrap_or("").contains("没有链接"));
    // 未确认的选区优先:Enter 只确认选区
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    app.view.sel = Some(RectSel {
        anchor: (0, 0),
        end: (5, 2),
        confirmed: false,
    });
    app.handle(键(KeyCode::Enter));
    assert!(app.take_open_request().is_none(), "选区优先,不打开链接");
    assert!(app.view.sel.expect("选区还在").confirmed);
}

/// 把 Enter 改回 confirm_selection 之后,Enter 不再打开链接
#[test]
fn enter改绑后不打开链接() {
    let mut opts = zh_opts();
    opts.keys.bind(
        crate::keys::Key::parse("enter").expect("解析"),
        Some(crate::keys::Action::ConfirmSelection),
    );
    let mut app = App::new(链接文档(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = (1, 1);
    app.refresh_pick();
    app.draw(&mut buf, app.area);
    app.handle(键(KeyCode::Enter));
    assert!(app.take_open_request().is_none(), "改绑后 Enter 只确认选区");
}

/// 菜单记得上次选了什么:再开一次直接停在那一项(换过的颜色也跟着亮着)    /// 菜单记得上次选了什么:再开一次直接停在那一项(换过的颜色也跟着亮着)
#[test]
fn 菜单记住上次选择() {
    let mut app = 连边应用(横排文档(2));
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let i = app.cursor_box().expect("在框上");
    // 第一次:进"框型"子菜单,选 double
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    let top = menu.levels.len();
    assert_eq!(top, 1, "第一次开是顶层");
    // 找到"框型"那一项并进去
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
    // 第二次:直接停在"框型"子菜单里的 double 上
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    assert_eq!(menu.levels.len(), 2, "记得上次进过子菜单");
    assert_eq!(menu.sel, dbl, "停在同一个选项上");
    assert!(menu.level()[menu.sel].checked, "就是当前值");
    app.menu_cancel();
}

/// 菜单记忆是个"栈":多层子菜单整条路径都记,退回上一层也回到原来那一项
#[test]
fn 菜单记忆整条路径() {
    let mut app = 连边应用(横排文档(2));
    app.doc
        .add_edge("b0", "b1", "g0", Dir::Forward)
        .expect("连边");
    app.refresh_pick();
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    app.view.pick = render::Pick::Edge(0);
    // 边菜单:端口 -> 起点 -> right
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
    // 再开:三层都还原
    app.open_style_menu();
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 3, "端口 -> 起点 -> right 三层都记着");
        assert_eq!(m.sel, right, "停在 right");
        assert_eq!(m.parents, vec!["端口".to_string(), "起点".to_string()]);
    }
    // 退回上一层:停在"起点",不是第一项
    app.handle(键(KeyCode::Char('h')));
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 2);
        assert_eq!(m.sel, from, "回到进来时那一项");
    }
    app.handle(键(KeyCode::Char('h')));
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 1);
        assert_eq!(m.sel, ports, "再退回到端口那一项");
    }
    app.menu_cancel();
}

/// 改了框的颜色之后,下次 s 直接停在颜色子菜单的那一格上    /// 改了框的颜色之后,下次 s 直接停在颜色子菜单的那一格上
#[test]
fn 菜单记住上次选的颜色() {
    let mut app = 连边应用(横排文档(2));
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let i = app.cursor_box().expect("在框上");
    // 第一次:边框色 -> 第 4 个颜色
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
    // 第二次:s 直接进颜色子菜单,停在同一格,而且它是当前值
    app.open_style_menu();
    let menu = app.view.menu.as_ref().expect("菜单开了");
    assert_eq!(menu.levels.len(), 2, "记得上次进过颜色子菜单");
    assert_eq!(menu.sel, want, "停在同一个颜色上");
    assert!(menu.level()[menu.sel].checked, "就是当前颜色");
    app.menu_cancel();
}

/// 框的样式菜单里不再有"改文字…",但"删除"还在
#[test]
fn 框样式菜单没有改文字但有删除() {
    let mut app = 连边应用(横排文档(2));
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

/// 跳转历史跨标签页:在另一个 buffer 里也能 Ctrl-o 退回来
#[test]
fn 跳转跨标签页() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(1); // 记下第一个 tab 里的位置
    let a = app.view.cursor;
    assert_eq!(app.tab_at, 0);
    // 开第二个 tab(和 gf 打开另一个文件等价)
    app.new_tab(None);
    assert_eq!(app.tab_at, 1);
    app.draw(&mut buf, app.area);
    // Ctrl-o:回到第一个 tab 的那个格子
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.tab_at, 0, "该切回原来的 tab");
    assert_eq!(app.view.cursor, a, "格子也要回去");
    // Ctrl-i:再回到第二个 tab
    app.handle(键(KeyCode::Tab));
    assert_eq!(app.tab_at, 1, "前进回到另一个 buffer");
}

/// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿
#[test]
fn 编辑位置也算跳点() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 在空处新建一个框
    let at = (-10, -10);
    app.view.cursor = at;
    app.refresh_pick();
    app.cmd_new("aa");
    assert_eq!(app.view.cursor, at, "建框不动光标");
    // 走开(普通移动不记跳点)
    let away = (30, 30);
    app.view.cursor = away;
    app.refresh_pick();
    // Ctrl-o 回到刚建框的那一格;Ctrl-i 再回到走开的位置
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, at, "Ctrl-o 回到建框的地方");
    app.handle(键(KeyCode::Tab));
    assert_eq!(app.view.cursor, away, "Tab(Ctrl-i)前进回去");
    // 改样式同样算一次跳点
    app.view.cursor = at;
    app.refresh_pick();
    app.cmd_apply("box double");
    app.view.cursor = away;
    app.refresh_pick();
    app.handle(ctrl(KeyCode::Char('o')));
    assert_eq!(app.view.cursor, at, "Ctrl-o 回到改样式的地方");
}

/// 新建框落在光标那一格;压着别的框就往右让    /// 新建框落在光标那一格;压着别的框就往右让
#[test]
fn 新建框落在光标处() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 光标放在空处
    app.view.cursor = (-10, -10);
    app.refresh_pick();
    app.cmd_new("q");
    let last = app.doc.boxes.len() - 1;
    assert_eq!(app.doc.boxes[last].at, (-10, -10), "该建在光标格");
    // 光标压在一个框上:往右让,不重叠
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

/// Ctrl-q 从任意格开始区块选择
#[test]
fn 区块选择只留v() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let (x0, y0, x1, y1) = app.doc.bbox();
    app.view.cursor = (x0 - 2, y0 - 2);
    app.refresh_pick();
    // Ctrl-q / V 已经删掉(键位去重)
    app.handle(ctrl(KeyCode::Char('q')));
    assert!(app.view.sel.is_none(), "Ctrl-q 不该再开始选区");
    app.handle(键(KeyCode::Char('V')));
    assert!(app.view.sel.is_none(), "V 不该再开始选区");
    // v 还是那条路
    app.handle(键(KeyCode::Char('v')));
    let sel = app.view.sel.expect("v 该有选区");
    assert_eq!(sel.anchor, (x0 - 2, y0 - 2), "锚点落在光标格");
    assert!(!sel.confirmed);
    // 扩大到盖住整幅图
    app.view.cursor = (x1 + 2, y1 + 2);
    let sel = render::selected(
        &app.doc,
        &render::panels(&app.doc, app.area, &app.view),
        &app.view,
    );
    assert!(!sel.is_empty(), "大区块里该有框");
    app.handle(键(KeyCode::Esc));
    assert!(app.view.sel.is_none(), "esc 取消");
}

/// i / t / c 三个键走底行提示,回车执行
#[test]
fn 提示行建框改文字连边() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 光标在框上时 i 是就地改字,挪到空白处才是新建
    app.handle(键(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "框上按 i 该就地改字");
    app.view.edit = None;
    app.view.cursor = (-10, -10);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('i')));
    assert!(app.view.edit.is_some(), "空白处直接建框并进就地编辑");
    for c in "Z".chars() {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(键(KeyCode::Esc));
    assert!(app.view.edit.is_none());
    assert_eq!(app.doc.boxes.len(), 2);
    assert_eq!(app.doc.text(1).unwrap().text, "Z");
    assert_eq!(app.undo.counts().0, 2, "建框一步、写文字一步");

    // esc 取消不产生步(取消掉这次新建的框)
    app.handle(键(KeyCode::Char('i')));
    app.handle(键(KeyCode::Esc));
    assert_eq!(app.doc.boxes.len(), 2, "取消不留空框");

    // :text -> 改光标框文字(输入框那条路)
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    let i = app.cursor_box().unwrap();
    app.cmd_apply("text hello");
    assert_eq!(app.doc.text(i).unwrap().text, "hello");

    // :link -> 连到另一个框(c 现在开颜色菜单);文字已经改过,按 id 找
    let other = app.doc.boxes[1].id.clone();
    app.cmd_apply(&format!("link {other}"));
    assert_eq!(app.doc.edges.len(), 1);
}

/// d 和 x 一样:删掉光标处的框或选中的边
#[test]
fn d_删除光标处的元素() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.cmd_apply("new b");
    let n = app.doc.boxes.len();
    assert_eq!(n, 2);
    // 光标落在新框里:按 d 删掉它
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n - 1, "d 该删掉光标处的框");
    assert!(app.view.note.clone().unwrap_or_default().contains("剪切"));
    // 光标留在原地(空的),要删最后一个框得先挪过去
    app.jump_to(0);
    app.cmd_apply("del");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("最后一个框"), "{note}");
}

/// 选区:未确认时改文档被拦,确认后组删除、组样式各一步撤销
#[test]
fn 选区确认与组删除粘贴() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 框住前两个框(留一个,免得撞上"不能删光"的规则)
    let b0 = app.doc.boxes[0].at;
    let (bx, by, bw, bh) = {
        let b1 = &app.doc.boxes[1];
        (b1.at.0, b1.at.1, b1.size.0 as i32, b1.size.1 as i32)
    };
    let (rx, ry) = (bx + bw + 1, by + bh + 1);
    app.view.cursor = (b0.0 - 1, b0.1 - 1);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = (rx, ry);
    app.refresh_pick();
    let sel = app.view.sel.expect("该有选区");
    assert!(!sel.confirmed, "刚拉出来是未确认");

    // 未确认:改文档被拒绝,不压撤销
    let undo0 = app.undo.counts().0;
    let n0 = app.doc.boxes.len();
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n0, "未确认不该删");
    assert!(app.view.note.clone().unwrap_or_default().contains("Enter"));
    assert_eq!(app.undo.counts().0, undo0, "被拒绝不该压撤销");

    // 确认之后:整批删除,一步撤销,内容进剪贴板
    app.handle(键(KeyCode::Enter));
    assert!(app.view.sel.unwrap().confirmed);
    let targets = app.targets();
    assert!(targets.boxes.len() >= 2, "选区该盖住至少两个框");
    let nb = targets.boxes.len();
    app.handle(键(KeyCode::Char('d')));
    assert_eq!(app.doc.boxes.len(), n0 - nb, "该删掉选区里的框");
    assert_eq!(app.undo.counts().0, undo0 + 1, "组删除只算一步");
    let clip = app.clipboard.clone().expect("删除该写剪贴板");
    assert_eq!(clip.boxes.len(), nb);
    assert!(!clip.texts.is_empty());
    // 撤销恢复全部,剪贴板还在
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.len(), n0, "撤销恢复全部");
    assert!(app.clipboard.is_some(), "撤销不清剪贴板");
    // 粘贴一份副本
    app.cmd_apply("paste");
    assert_eq!(app.doc.boxes.len(), n0 + nb, "粘贴出一份副本");
    // 再粘一次 id 不冲突
    let before: std::collections::HashSet<String> =
        app.doc.boxes.iter().map(|b| b.id.clone()).collect();
    app.cmd_apply("paste");
    let after: std::collections::HashSet<String> =
        app.doc.boxes.iter().map(|b| b.id.clone()).collect();
    assert!(after.len() > before.len(), "id 不该撞");
    // 再删一次同样的选区会删光?整体拒绝的分支
    app.view.sel = None;
    app.refresh_pick();
    app.view.cursor = (b0.0 - 1, b0.1 - 1);
    app.handle(键(KeyCode::Char('v')));
    app.view.cursor = (rx, ry);
    app.handle(键(KeyCode::Enter));
}

/// `:` 补全:动词前缀、取值枚举、框名/图层名/文件名
#[test]
fn 命令行补全() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 动词前缀
    app.view.cmd = Some("fi".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("fill"));
    assert!(app.view.cmd_menu.is_some(), "该弹补全窗");
    // 取值
    app.view.cmd = Some("box ".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("box point"), "第一个候选");
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("box none"), "Tab 轮换");
    app.complete_cmd(true);
    assert_eq!(app.view.cmd.as_deref(), Some("box point"), "Shift-Tab 往回");
    // dir 的四个值
    app.view.cmd = Some("dir b".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("dir back"), "按前缀过滤");
    // 框名:link 补 id/文字
    app.view.cmd = Some("link b".to_string());
    app.view.cmd_menu = None;
    app.complete_cmd(false);
    let line = app.view.cmd.clone().unwrap();
    assert!(line.starts_with("link b"), "{line}");
    // 输入会把弹窗收掉
    app.handle(键(KeyCode::Enter));
    assert!(app.view.cmd_menu.is_none());
}

/// 命令行右侧的路径提示:存盘会写到哪、`:e` 会从哪读
#[test]
fn 命令行路径提示() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 没有来源:明说"还没文件名"
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("还没有文件名"), "{hint}");

    // .dot 来源:默认写到同目录的 <stem>.gtui,并说明源文件不动
    app.source = Some("/tmp/demo/chart.dot".to_string());
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("/tmp/demo/chart.gtui"), "{hint}");
    assert!(hint.contains("不动"), "{hint}");

    // .gtui 来源:覆盖原文件
    app.source = Some("/tmp/demo/a.gtui".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(
        hint.contains("/tmp/demo/a.gtui") && hint.contains("覆盖"),
        "{hint}"
    );

    // 手写路径:提示解析后的绝对路径,输入本身不变
    app.view.cmd = Some("w out/x.gtui".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(hint, cwd.join("out/x.gtui").to_string_lossy().to_string());
    assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));

    // :e 空参数说明回车弹列表
    app.view.cmd = Some("e".to_string());
    app.refresh_cmd_hint();
    let hint = app.view.cmd_hint.clone().unwrap();
    assert!(hint.contains("文件列表"), "{hint}");
    // 无关动词:不提示
    app.view.cmd = Some("notes on".to_string());
    app.refresh_cmd_hint();
    assert!(app.view.cmd_hint.is_none());
    // 回车后清掉
    app.view.cmd = Some("w".to_string());
    app.refresh_cmd_hint();
    app.handle(键(KeyCode::Enter));
    assert!(app.view.cmd_hint.is_none());
}

/// `:w ` 空参数按 Tab 填默认路径;参数带空格时整段替换
#[test]
fn 制表键填默认路径与空格参数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 没有来源:不填,说一声
    app.view.cmd = Some("w ".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w "), "空白缓冲不瞎填");
    app.view.cmd = Some("w".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w"), "没空格也不瞎填");
    assert!(app.view.note.is_some());

    // .dot 来源:填绝对路径,还能接着编辑
    app.source = Some("/tmp/demo/chart.dot".to_string());
    app.view.cmd = Some("w ".to_string());
    app.complete_cmd(false);
    assert_eq!(app.view.cmd.as_deref(), Some("w /tmp/demo/chart.gtui"));
    assert!(app.view.cmd_hint.as_deref().unwrap().contains("chart.gtui"));

    // 参数带空格:补全整段替换,不能按最后一个空格切
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

/// 磁盘上的源文件被外部改过:没本地改动就自动重新载入,
/// 有改动就标 stale,`R` 两下 / `:e!` 才是强制重读
#[test]
fn 发现磁盘改动并重新载入() {
    let dir = std::env::temp_dir().join(format!("gtui-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("w.gtui").to_str().unwrap().to_string();
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply(&format!("w {path}"));
    assert!(!app.check_disk(), "刚存过:没变化");
    let mine = label_of(&app.doc, 0);
    assert!(!mine.is_empty());

    // 外部改:另一个 App 往同一个路径存一版文字不同的
    let mut other = App::new(样例文档(), zh_opts()).expect("开局");
    other.doc.set_text(0, "外部改的".to_string());
    other.save_document(&path).expect("外部写入");
    // 本地没动过 -> 自动换成磁盘上的版本
    assert!(app.check_disk(), "该发现改动");
    assert_eq!(label_of(&app.doc, 0), "外部改的", "没本地改动:直接重新载入");
    assert!(!app.view.stale);
    assert!(app.view.note.as_deref().unwrap().contains("重新载入"));

    // 本地改一行,再让磁盘变:只标 stale,不动文档
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
    app.cmd_apply("text 本地改过");
    other.doc.set_text(0, "又改了一次".to_string());
    other.save_document(&path).expect("外部再写");
    assert!(app.check_disk());
    assert!(app.view.stale, "有本地改动:只提醒");
    assert_eq!(label_of(&app.doc, 0), "本地改过", "文档不该被换掉");
    assert!(app.view.note.as_deref().unwrap().contains("R"));

    // 第一次 R 只提醒,第二次才丢改动重读
    app.reload_step();
    assert_eq!(label_of(&app.doc, 0), "本地改过");
    app.reload_step();
    assert_eq!(label_of(&app.doc, 0), "又改了一次", "第二下 R 真的重读");
    assert!(!app.view.stale);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `:e!` 不论有没有本地改动都强制重读
#[test]
fn 强制重新载入() {
    let dir = std::env::temp_dir().join(format!("gtui-force-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f.gtui").to_str().unwrap().to_string();
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply(&format!("w {path}"));
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
    app.cmd_apply("text 改了");
    let mut other = App::new(样例文档(), zh_opts()).expect("开局");
    other.doc.set_text(0, "磁盘上的".to_string());
    other.save_document(&path).expect("外部写入");
    app.cmd_apply("e!");
    assert_eq!(label_of(&app.doc, 0), "磁盘上的", ":e! 不理会本地改动");
    assert!(!app.view.stale);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 缩放锚在指针/光标:缩放前后同一屏幕格下面的画布坐标不变
#[test]
fn 缩放锚在光标() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.center = None;
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    // 光标所在的屏幕格就是锚;记下这一格下面的画布坐标,缩放后它还得在这一格
    let target = app.doc.center(0);
    app.view.cursor = (target.0.round() as i32, target.1.round() as i32);
    app.view.pick = render::Pick::Box(0);
    let at = app.cursor_screen_cell();
    let point = p.proj.canvas_f(at.0, at.1);

    // 放大三档:目标点始终还在那一格(允许 1 格取整误差)
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
    // 缩小回 1:1 附近会吸附到正好 1.0
    app.zoom_at(1.02, at);
    assert_eq!(app.view.zoom, 1.0, "1:1 附近要吸附");
}

/// 从缩放状态进编辑:切回 1:1 但不跳,光标还在原来那一格附近
#[test]
fn 编辑切回一比一不跳() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 滚轮在指针处缩放,不是绕视图中心
#[test]
fn 滚轮锚在指针() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.center = None;
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    // 指针停在某个框上
    let target = app.doc.center(0);
    let at = p.proj.cell(target);
    let me = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: at.0.clamp(0, 79) as u16,
        row: at.1.clamp(0, 23) as u16,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    // 用钳过之后的格子当锚,比对同样钳过的目标
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

/// 鼠标手势:从边框拖到另一个框 = 连边;拖到空白取消;拖端点 = 改接
fn mev(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}

#[test]
fn 拖拽连边与重连() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    // 两个框都在 1:1,屏幕上直接取它们的矩形
    let p = render::panels(&app.doc, app.area, &app.view);
    let ra = render::rect_of(&app.doc, 0, &p.proj);
    // 丢到第三个框:前两个本来就有边,重复边会被去重
    let rb = render::rect_of(&app.doc, 2, &p.proj);
    let (bx, by) = ((rb.0 + rb.2 / 2) as u16, (rb.1 + rb.3 / 2) as u16);
    // 按在 A 的上边框,拖到 B 内部,松开
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
    // 拖到空白:取消,不加边
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

    // 端点重连:选中一条边,按它的端点格拖到另一个框
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

/// 右键拖过阈值 = 平移;没拖过(含 1 格微动)= 在按下格弹菜单
#[test]
fn 右键拖动平移单击弹菜单() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
    let before = app.view.center;

    // 1) 拖 3 格:平移,不弹菜单
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), 40, 12));
    assert!(app.view.menu.is_none(), "按下时不该弹菜单");
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), 43, 12));
    assert_ne!(app.view.center, before, "拖过阈值该平移");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), 43, 12));
    assert!(app.view.menu.is_none(), "拖过就不弹菜单");
    assert!(app.drag.is_none(), "松手清掉拖动状态");

    // 2) 微动 1 格:算单击,菜单在按下格打开
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

    // 3) 拖出去再拖回原点:已经越过阈值,松手仍不开菜单
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 5, py));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px, py));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px, py));
    assert!(app.view.menu.is_none(), "拖过再回来也不弹菜单");

    // 4) 菜单开着时右键只取消,不开始平移
    app.open_menu(px, py);
    assert!(app.view.menu.is_some());
    let center = app.view.center;
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
    assert!(app.view.menu.is_none(), "右键该先取消菜单");
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 6, py));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px + 6, py));
    assert_eq!(app.view.center, center, "取消菜单之后这一下不该平移");
}

/// 从框上按右键拖:只平移,不动框、不改选区、不压撤销
/// 右键连边途中:指针不在目标框上时预览跟光标,进入目标框时接到框
#[test]
fn 右键连边途中跟随指针() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = 连边应用(横排文档(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let r1 = render::rect_of(&app.doc, 1, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    // 空白处:预览跟光标,目标为空
    // 框下面几行是空的:指针挪到那儿就没有目标
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
    // 进入目标框:目标接上
    let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
    let link = app.view.link.clone().expect("该还在连边");
    assert_eq!(link.targets, vec![1], "指针下的框成为目标");
    // 回到空白处:目标清掉
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

/// 右键在 point / none 节点上也能起拖连边;在边上则是平移
#[test]
fn 右键在点节点和边上() {
    use crossterm::event::{MouseButton, MouseEventKind};
    // point 节点
    let mut doc = 横排文档(2);
    doc.boxes[1].style = crate::model::BoxStyle::Point;
    doc.rebuild();
    let mut app = 连边应用(doc);
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

    // 边上:按空白处理 -> 平移
    let mut app = 连边应用(样例文档());
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
fn 右键在框上拖动是连边() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = 连边应用(横排文档(2));
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

    // 松开在空白处:不建边
    let mut app = 连边应用(横排文档(2));
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

    // mouse_edges = false:框上按右键拖动只平移,不连边
    let mut app = 连边应用(横排文档(2));
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
    // 单击仍然弹菜单
    let mut app = 连边应用(横排文档(2));
    app.mouse_edges = false;
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), sx, sy));
    assert!(app.view.menu.is_some(), "关掉连边不影响右键菜单");

    // 没拖过阈值:弹菜单
    let mut app = 连边应用(横排文档(2));
    let p = render::panels(&app.doc, app.area, &app.view);
    let r0 = render::rect_of(&app.doc, 0, &p.proj);
    let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
    app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), sx + 1, sy));
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), sx + 1, sy));
    assert!(app.view.menu.is_some(), "没拖过 2 格就是单击");
    assert_eq!(app.doc.edges.len(), 0);
}

/// 拖动靠近视口边缘时自动平移    /// 拖动靠近视口边缘时自动平移
#[test]
fn 拖拽到边缘自动平移() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 拖到地图右边最边上
    let x = p.map.x + p.map.width - 1;
    app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), x, 5));
    assert_ne!(app.view.center, before, "靠边该自动平移");
    app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), x, 5));
}

/// 多文档:两个 tab 各自保留文档/光标/缩放/撤销,剪贴板跨 tab
#[test]
fn 多tab互不干扰() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // tab 1:改文字、缩放
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.cmd_apply("text tab-one");
    app.view.zoom = 2.0;
    let undo1 = app.undo.counts();
    // 新开 tab:内容/光标/缩放归新文档
    app.new_tab(None);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tab_at, 1);
    assert_ne!(label_of(&app.doc, 0), "tab-one", "新 tab 是空文档");
    assert_eq!(app.view.zoom, 1.0);
    app.view.pick = render::Pick::Box(0);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.cmd_apply("text tab-two");
    // 切回 tab 1:内容、缩放、撤销历史都还在
    app.switch_tab(0);
    assert_eq!(label_of(&app.doc, 0), "tab-one");
    assert_eq!(app.view.zoom, 2.0);
    assert_eq!(app.undo.counts(), undo1);
    // 在 tab 1 撤销不影响 tab 2
    app.undo_step();
    assert_ne!(label_of(&app.doc, 0), "tab-one");
    app.switch_tab(1);
    assert_eq!(label_of(&app.doc, 0), "tab-two");
}

/// 脏 tab 关不掉,`!` 才能丢;关掉后落到相邻 tab
#[test]
fn 关闭tab要看脏标记() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 只剩一个 tab 时关闭 = 退出
    app.cmd_apply("tabclose!");
    assert!(app.quit);
}

/// 多 tab 的 :tabs 列表与切换命令
#[test]
fn tab命令与列表() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 任意缩放下拖框:不切 1:1,框/光标按画布位移走(直接用 Drag 状态,绕开边框手势)
#[test]
fn 缩放下拖框() {
    use crossterm::event::{MouseButton, MouseEventKind};
    for (zoom, mouse_steps, want_cells) in [(2.0, 2, 1), (0.5, 1, 2), (1.0, 3, 3)] {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.view.zoom = zoom;
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let before = app.doc.boxes[0].at;
        app.view.cursor = before;
        // 每次都从同一屏幕格起拖,位移只取决于鼠标走了几格
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

/// 多选拖动:相对位置不变,光标与选区按同一位移走;拖十次不漂移
#[test]
fn 多选拖框与漂移() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 120, 40);
    app.view.zoom = 1.0;
    // 选两个框
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

/// 图例已移除:`:legend` 既不在补全里,也不再是命令(补全表和帮助都不该提到它)
#[test]
fn 图例命令已经移除() {
    assert!(!VERBS.contains(&"legend"), ":legend 不该再是命令:{VERBS:?}");
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.cmd_apply("legend on");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("legend"), "该报未知命令:{note}");
    assert!(
        note.contains("未知命令") || note.contains("unknown command"),
        "{note}"
    );
}

/// 不存在的 .gtui 当成新文件(和 nvim 一样);缺 .dot 仍然是错
#[test]
fn 不存在的gtui是新文件() {
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
    // 真的文件:不标记成新文件
    std::fs::write(&gtui, Document::scratch().save()).expect("写盘");
    let (_, fresh) = App::read_doc(gtui.to_str().unwrap()).expect("读得回来");
    assert!(!fresh);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 菜单:选中即预览(不改撤销),点一下才真正应用;取消回滚
#[test]
fn 菜单预览与回滚() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 第一项是"框型"(子菜单):不动文档
    assert_eq!(app.doc.boxes.len(), n0);

    // 进"边框色"子菜单,挑一个颜色预览:颜色变了,但不留撤销步
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

    // 取消:回到原样
    app.menu_cancel();
    assert_eq!(app.doc.boxes[0].color, None, "取消该回滚");
    assert!(app.view.menu.is_none());

    // 再开一次:取消过就不该记(菜单回到顶层),走完一遍并应用
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
    // 这一次真的记下了:再开直接停在颜色子菜单那一格
    app.open_menu(cx as u16, cy as u16);
    {
        let m = app.view.menu.as_ref().unwrap();
        assert_eq!(m.levels.len(), 2, "该记得上次进过颜色子菜单");
        assert_eq!(m.sel, pick, "该停在上次选的颜色上");
        assert!(m.level()[m.sel].checked, "就是当前颜色");
    }
    app.menu_cancel();
}

/// 右键菜单:按光标处的东西给一层,子菜单能进能退
#[test]
fn 右键菜单按元素给项并能执行() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 放到第一个框上再开菜单
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

    // 进"框型"子菜单,选 heavy,执行
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

    // 空处右键给"新建框"
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    app.open_menu(1, 1);
    let m = app.view.menu.clone().unwrap();
    assert_eq!(m.level()[0].title, "新建框…");
}

/// 撤销/重做:同值设置不占步,新建/换布局都能撤
#[test]
fn 撤销与重做() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    let before = app.doc.boxes[0].fill;
    app.cmd_apply("fill #123456");
    assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)));
    // 同一个值再来一次不算一步
    app.cmd_apply("fill #123456");
    assert_eq!(app.undo.counts(), (1, 0), "同值设置不该多一步");

    app.cmd_apply("fill #654321");
    assert_eq!(app.undo.counts(), (2, 0));
    app.handle(键(KeyCode::Char('u')));
    assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)), "撤销一步");
    assert_eq!(app.undo.counts(), (1, 1));
    app.handle(ctrl(KeyCode::Char('r')));
    assert_eq!(app.doc.boxes[0].fill, Some((0x65, 0x43, 0x21)), "重做回来");

    // 新建框 -> 撤销
    let n0 = app.doc.boxes.len();
    app.cmd_apply("new z");
    assert_eq!(app.doc.boxes.len(), n0 + 1);
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.len(), n0, "撤销建框");
    assert!(app.view.note.clone().unwrap_or_default().contains("撤销"));

    // 换布局 -> 撤销后坐标回原样
    let at0: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.cmd_apply("layout force");
    assert_ne!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);
    app.cmd_apply("undo");
    assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);

    // 撤销后再改东西,重做栈清空
    app.cmd_apply("undo");
    assert!(app.undo.counts().1 > 0, "撤销之后该有可重做的");
    app.cmd_apply("fill #abcdef");
    assert_eq!(app.undo.counts().1, 0, "新动作清掉重做栈");
    let _ = before;
}

/// 命令行/搜索的光标按原样文本算:路径里的 `/` 不是斜体标记
#[test]
fn 命令行的光标不被标记语法影响() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.cmd = Some("w /examples/".to_string());
    app.view.cmd_pos = 12;
    let (x, _) = app.cursor_cell().expect("该有光标");
    // `:` + 原文的宽度,而不是解析掉 /examples/ 之后的宽度
    assert_eq!(x, render::raw_width(":w /examples/") as u16);
    app.view.cmd = Some("text *a*b*".to_string());
    app.view.cmd_pos = 11;
    let (x2, _) = app.cursor_cell().expect("该有光标");
    assert_eq!(x2, render::raw_width(":text *a*b*") as u16);
}

#[test]
fn 自定义键位生效而默认键不变() {
    use crate::keys::{Action as K, Key, KeyMap};
    let mut opts = zh_opts();
    let mut km = KeyMap::defaults();
    km.bind(Key::parse("h").unwrap(), Some(K::MoveDown));
    opts.keys = km;
    let mut app = App::new(样例文档(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let before = app.view.cursor;
    app.handle(键(KeyCode::Char('h')));
    assert_eq!(app.view.cursor.1, before.1 + 1, "h 该改成往下走");
    // 没改的键照旧
    let c = app.view.cursor;
    app.handle(键(KeyCode::Char('l')));
    assert_eq!(app.view.cursor.0, c.0 + 1, "l 还是右移");
}

#[test]
fn 关掉鼠标就不理鼠标事件() {
    let mut opts = zh_opts();
    opts.mouse = false;
    let mut app = App::new(样例文档(), opts).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let before = app.view.cursor;
    app.handle(鼠标(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        10,
        5,
    ));
    assert_eq!(app.view.cursor, before, "鼠标关掉后左键不该动光标");
}

/// 退出只认 :q:q 与 esc 都不退
#[test]
fn 只有_q_能退出() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert_eq!(app.handle(键(KeyCode::Char('q'))), Flow::Continue);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains(":q"), "q 该提示用 :q 退出:{note}");
    assert_eq!(app.handle(键(KeyCode::Esc)), Flow::Continue);
    assert_eq!(
        app.handle(ctrl(KeyCode::Char('c'))),
        Flow::Continue,
        "ctrl-c 也不再退出"
    );
    // :q 立刻退
    app.view.cmd = Some(String::new());
    for c in ":q".chars().skip(1) {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.handle(键(KeyCode::Enter)), Flow::Quit);
    assert!(app.quit);
}

#[test]
/// 光标一格一步,数字前缀是格数;pick 跟着格子走
fn 光标按格子移动() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.jump_to(0);
    let start = app.view.cursor;
    app.handle(键(KeyCode::Char('l')));
    assert_eq!(app.view.cursor, (start.0 + 1, start.1), "向右一格");
    app.handle(键(KeyCode::Char('j')));
    assert_eq!(app.view.cursor, (start.0 + 1, start.1 + 1), "向下一格");
    app.view.cursor = start;
    app.refresh_pick();
    for c in ['5', 'l'] {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (start.0 + 5, start.1), "5l 走五格");
    // 10j:0 属于计数前缀, 不能触发"回到 1:1"
    app.view.cursor = start;
    app.view.zoom = 2.0;
    app.refresh_pick();
    for c in ['1', '0', 'j'] {
        app.handle(键(KeyCode::Char(c)));
    }
    assert_eq!(app.view.cursor, (start.0, start.1 + 10), "10j 走十格");
    assert_eq!(app.view.zoom, 2.0, "计数里的 0 不能改缩放");
    // 没有前缀时 0 仍然是回到 1:1
    app.view.cursor = start;
    app.handle(键(KeyCode::Char('0')));
    assert_eq!(app.view.zoom, 1.0, "单独按 0 回到 1:1");
    assert_eq!(app.view.cursor, start, "单独按 0 不移动光标");
    app.view.cursor = (-50, -50);
    app.refresh_pick();
    assert_eq!(app.view.pick, render::Pick::None);
}

/// 鼠标点哪儿,光标就落到哪一格
#[test]
fn 鼠标点击落格() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
fn 旋转动词改坐标并记住方向() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // auto 只改方向,不动坐标
    let now: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
    app.cmd_apply("rotate auto");
    assert_eq!(app.view.orientation.name(), "auto");
    assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), now);
    // 不带参数就是再转 90 度
    app.cmd_apply("rotate");
    assert_eq!(app.view.angle, 90);
}

/// 选中一条边时 `:text` 写的是这条边的注释框(空串连框删掉);进撤销栈
/// 新建的节点用默认样式:只有文字,宽随文字
#[test]
fn 新建节点默认是只有文字() {
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

/// Ctrl+hjkl:框挪一格,光标跟着走;光标挪出画面边距时视图追过去
#[test]
fn ctrl_方向键光标跟框走并推动视图() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 光标先落到框上,这样 pick 指向它
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

    // 把视图挪到"框就在右边距上",再往右挪:视图该开始跟过去
    app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or(before_box);
    app.refresh_pick();
    let p = render::panels(&app.doc, app.area, &app.view);
    let x1 = (p.map.x + p.map.width) as i32 - 1 - 2; // follow_cursor 的两格边距
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

/// 键盘挪光标:贴边时视图只挪一小段,挪完还能再走几格才需要再挪
#[test]
fn 光标贴边只挪一小段视图() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.zoom = 1.0;
    let p = render::panels(&app.doc, app.area, &app.view);
    let y0 = p.map.y as i32 + 2; // follow_cursor 的两格边距
    let anchor = p.map.y as f64 + p.map.height as f64 / 2.0;
    // 光标先停在上边距那一行
    let c = (10, 0);
    app.view.cursor = c;
    app.view.center = Some((c.0 as f64, c.1 as f64 - y0 as f64 + anchor));
    app.view.target = None;
    app.view.pan = None;
    app.refresh_pick();
    // 贴到边距之后像 nvim:每按一下画面当场滚一格(不缓动),
    // 光标在屏幕上钉在边距那一行 —— 不然会先动再弹回来
    let mut prev = app.view.center.expect("有中心");
    for step in 0..4 {
        app.handle(键(KeyCode::Char('k')));
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
    // 逻辑位置一直在走
    assert_eq!(app.view.cursor.1, c.1 - 4);
}

/// 计数前缀也管 Ctrl-hjkl:10 + Ctrl-l = 挪 10 格,一次撤销
#[test]
fn ctrl_方向键吃计数前缀() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

    // 一次 Ctrl-l 带计数只算一步撤销
    app.undo_step();
    assert_eq!(app.doc.boxes[0].at, before, "一步就撤回来");

    // 计数用过就清掉:再按一次 Ctrl-l 只挪一格
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

/// E 只在光标那个节点与邻居之间的边里轮(按文档顺序绕回)
#[test]
fn e_只在当前节点的相连边里轮() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = 连边应用(样例文档());
    // 样例里 b1 连着两条边(b0 -> b1、b2 -- b1)
    let c = app.doc.box_idx("b1").expect("样例有 b1");
    app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
    app.view.pick = render::Pick::None;
    let incident: Vec<usize> = app.doc.adj(c).to_vec();
    assert_eq!(incident.len(), 2, "样例该给 b1 连两条边");

    let key = || KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    app.handle(Event::Key(key()));
    assert_eq!(app.view.pick.edge(), Some(incident[0]), "第一条");
    app.handle(Event::Key(key()));
    assert_eq!(app.view.pick.edge(), Some(incident[1]), "下一条");
    app.handle(Event::Key(key()));
    assert_eq!(app.view.pick.edge(), Some(incident[0]), "到头绕回去");
}

/// E 模式:光标压在高亮边之外的线上时,不选中那条边(只有中心连着的才闪)
#[test]
fn e_模式光标只认高亮边() {
    let mut doc = 样例文档();
    // 加一条与 b1 无关的边
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
    // 找一条"只属于 far、不属于高亮"的格
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
        return; // 没有这种格就跳过(布局变了的情况)
    };
    let p = render::panels(&app.doc, app.area, &app.view);
    app.view.cursor = p.proj.canvas(only_far.0, only_far.1);
    // 没开 E:这条边能被选中
    app.view.ego = None;
    app.refresh_pick();
    assert_eq!(app.view.pick.edge(), Some(far_e), "没开 E 时照常能选");
    // 开 E(中心是 b1):同一点不该选中它
    app.view.ego = Some(a);
    app.refresh_pick();
    assert_eq!(app.view.pick.edge(), None, "E 模式下不认高亮之外的边");
}

/// 隔离:光标落到隐藏节点上不换中心,pick 也拿不到它
#[test]
fn 隔离时隐藏节点抓不到光标() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 中心 = b0:可见 b0、b1,b2 藏起来
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(0));
    // 挪到藏起来的 b2 上
    app.view.cursor = app.doc.text(2).unwrap().at;
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(0), "隐藏节点不该抢走中心");
    assert_ne!(app.view.pick, render::Pick::Box(2), "隐藏节点选不中");
    // 挪到可见邻居 b1 上 -> 中心跟着换
    app.view.cursor = app.doc.text(1).unwrap().at;
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(1), "可见邻居能当新中心");
}

/// 隔离:跳到藏起来的节点先把隔离退掉,不然落点看不见
#[test]
fn 跳到隐藏节点先退出隔离() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('E')));
    app.jump_to(2);
    assert_eq!(app.view.ego, None, "目标被藏着,先退出隔离");
    assert_eq!(app.view.cursor, app.doc.text(2).unwrap().at, "然后才跳过去");
}

/// 开启隔离:清掉正在进行的选区与连边
#[test]
fn 开隔离清掉进行中的操作() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    app.handle(键(KeyCode::Char('E')));
    assert!(app.view.ego.is_some());
    assert!(app.view.sel.is_none(), "隐藏元素不能被继续操作");
}

/// F:隔离开着时按可见节点的范围铺满,不管藏起来的那些
#[test]
fn 隔离时_f_只看可见范围() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 26, 12);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('F')));
    let plain = app.view.zoom;
    app.handle(键(KeyCode::Char('E')));
    app.handle(键(KeyCode::Char('F')));
    let iso = app.view.zoom;
    assert!(iso > plain, "可见范围更小,该放得更大:{plain} -> {iso}");
}

/// n / N 跳过被隔离藏起来的命中;全都藏着时给一句提示
#[test]
fn 隔离时_n_跳过隐藏命中() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(0).unwrap().at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('E')));
    app.view.hits = vec![2, 0];
    app.view.hit_at = 1;
    app.handle(键(KeyCode::Char('n')));
    assert_eq!(app.view.hit_at, 1, "隐藏的 b2 跳过,只剩可见的 b0");
    assert_eq!(app.view.cursor, app.doc.text(0).unwrap().at);
    // 只剩隐藏命中:提示,不动
    app.view.hits = vec![2];
    app.view.hit_at = 0;
    let before = app.view.cursor;
    app.handle(键(KeyCode::Char('n')));
    assert_eq!(app.view.cursor, before, "藏起来的命中不跳");
    assert!(app.view.note.clone().unwrap_or_default().contains("隔离"));
}

/// 中心被删掉:隔离自动退出(下标漂了也认得出)
#[test]
fn 中心被删掉就退出隔离() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    app.view.cursor = app.doc.text(1).unwrap().at;
    app.refresh_pick();
    app.handle(键(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(1));
    // 删掉中心前面的 b0:下标漂到 0,但中心还是 b1
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
    // 删掉中心自己 -> 退出
    app.doc.remove_box("b1").unwrap();
    app.draw(&mut buf, app.area);
    assert_eq!(app.view.ego, None, "中心没了就退出隔离");
}

/// E:只留光标节点、邻居与相连的边(其余消失);再按 E 或 Esc 退出
#[test]
fn e_开关邻居隔离() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);

    let c = app.doc.box_idx("b1").expect("样例有 b1");
    app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
    app.refresh_pick();

    app.handle(键(KeyCode::Char('E')));
    assert_eq!(app.view.ego, Some(c), "开着的是光标那个节点");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("隔离"), "回显要说清是隔离:{note}");
    assert!(note.contains("2 邻居"), "回显要报邻居数:{note}");

    // 光标记到另一个节点上:高亮跟着换过去(不用再按 E)
    let other = app.doc.box_idx("b0").expect("样例有 b0");
    app.view.cursor = app.doc.text(other).map(|t| t.at).unwrap_or((0, 0));
    app.refresh_pick();
    assert_eq!(app.view.ego, Some(other), "点到/挪到别的节点就换过去亮它");

    // 再按一次 E:关掉隔离
    app.handle(键(KeyCode::Char('E')));
    assert_eq!(app.view.ego, None, "E 再按一次该关掉");

    // 开起来之后 Esc 也能退
    app.handle(键(KeyCode::Char('E')));
    assert!(app.view.ego.is_some());
    app.handle(键(KeyCode::Esc));
    assert_eq!(app.view.ego, None, "Esc 该退掉");

    // 光标不在节点上:只提示,不开
    app.view.cursor = (-99, -99);
    app.refresh_pick();
    app.handle(键(KeyCode::Char('E')));
    assert_eq!(app.view.ego, None);
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有节点"));
}

#[test]
fn 输入行交给外部编辑器() {
    use crate::external::Outcome;
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

    // 连边目标字段是单行:多行拒绝,输入框留着
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

    // 失败:原样 + 提示
    app.handle(ctrl(KeyCode::Char('g')));
    assert_eq!(app.edit_initial().as_deref(), Some("网关"));
    app.apply_edit_result(Outcome::Failed(3));
    assert_eq!(
        app.view.prompt.as_ref().map(|(_, t)| t.as_str()),
        Some("网关")
    );
}

/// Ctrl+G(普通模式)直接用编辑器改框里的字,退出即生效,一步撤销
#[test]
fn 外部编辑器直接改框文字() {
    use crate::external::Outcome;
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

    // 没改动:不动
    app.handle(ctrl(KeyCode::Char('g')));
    app.apply_edit_result(Outcome::Unchanged);
    assert_eq!(app.doc.text(0).map(|t| t.text.as_str()), Some("甲"));
    assert!(app
        .view
        .note
        .clone()
        .unwrap_or_default()
        .contains("没有改动"));

    // 没有框:提示,不请求编辑器
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

/// 一条边的注释文字(没有注释框就是 None)
fn 边的注释文字(app: &App, e: usize) -> Option<String> {
    let id = app.doc.edges.get(e)?.id.clone();
    let b = app.doc.note_of_edge(&id)?;
    app.doc.text(b).map(|t| t.text.clone())
}

#[test]
fn 选中边时_text写注释() {
    let mut app = 连边应用(样例文档());
    assert!(!app.doc.edges.is_empty(), "样例该有边");
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    assert_eq!(边的注释文字(&app, 0).as_deref(), Some("收到 FIN"));
    let b = app.doc.note_of_edge(&app.doc.edges[0].id).unwrap();
    assert_eq!(app.doc.boxes[b].style, crate::model::BoxStyle::None);
    app.undo_step();
    assert!(边的注释文字(&app, 0).is_none(), "撤销该把注释框撤掉");

    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text");
    assert!(边的注释文字(&app, 0).is_none(), "空串 = 连注释框一起删");
}

#[test]
fn 选中边按i建注释框() {
    let mut app = 连边应用(样例文档());
    // 光标先挪到真正的边笔画上(不在框上),i 才是"给这条边建注释"
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
    app.handle(键(KeyCode::Char('i')));
    assert!(
        matches!(app.view.prompt, Some((Prompt::Text(_), _))),
        "该进输入"
    );
    app.handle(键(KeyCode::Esc));
    assert!(边的注释文字(&app, 0).is_none(), "Esc 该把空注释框撤掉");

    // 再来一次,写了字就留下
    app.view.pick = render::Pick::Edge(0);
    app.handle(键(KeyCode::Char('i')));
    for c in ['收', '到', 'A', 'C', 'K'] {
        app.handle(键(KeyCode::Char(c)));
    }
    app.handle(键(KeyCode::Enter));
    assert_eq!(边的注释文字(&app, 0).as_deref(), Some("收到ACK"));
}

#[test]
fn 删边连注释框一起走() {
    let mut app = 连边应用(样例文档());
    app.view.pick = render::Pick::Edge(0);
    app.cmd_apply("text 收到 FIN");
    let n0 = app.doc.boxes.len();
    let id = app.doc.edges[0].id.clone();
    app.doc.remove_edge(&id).unwrap();
    assert_eq!(app.doc.boxes.len(), n0 - 1, "注释框该一起删");
    assert!(app.doc.note_of_edge(&id).is_none());
}

#[test]
fn 真实光标位置在其他面板出现时也对() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    // 真实光标:在光标框的文字起点
    let p = render::panels(&app.doc, app.area, &app.view);
    let want = p
        .proj
        .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
    assert_eq!(app.cursor_cell(), Some((want.0 as u16, want.1 as u16)));
    // 命令行打开时落到输入行
    app.view.cmd = Some("notes on".to_string());
    app.view.cmd_pos = 8;
    let (x, y) = app.cursor_cell().expect("输入行该有光标");
    assert_eq!(y, 23);
    assert_eq!(x, render::width(":notes on") as u16);
    app.view.cmd = None;
    // 帮助浮层打开时藏起来
    app.view.help = true;
    assert_eq!(app.cursor_cell(), None);
}

#[test]
fn 空白文档里建框连边再删掉() {
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
    // 同一条边不能连两次
    app.cmd_apply("link a");
    assert_eq!(app.doc.edges.len(), 1);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("连不上"), "{note}");

    // 删框:它的文字与边一起走
    app.cmd_apply("del");
    assert_eq!(app.doc.boxes.len(), 1);
    assert!(app.doc.edges.is_empty());
    assert_eq!(app.doc.texts.len(), 1);
    // 光标留在被删掉的位置,先挪回最后一个框
    app.jump_to(0);
    app.cmd_apply("del");
    assert_eq!(app.doc.boxes.len(), 1);
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("最后一个框"), "{note}");
}

#[test]
fn 新建框默认文字用框id() {
    let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
    app.cmd_apply("new");
    let last = app.doc.boxes.len() - 1;
    let id = app.doc.boxes[last].id.clone();
    assert_eq!(app.doc.text(last).map(|t| t.text.clone()), Some(id));
}

#[test]
fn 图层循环与tolayer建层() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    assert_eq!(app.view.layer, None);
    app.handle(键(KeyCode::Char('L')));
    assert_eq!(app.view.layer.as_deref(), Some(""));
    app.handle(键(KeyCode::Char('L')));
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
fn 编辑动词改属性() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    // 改文字后框宽重算
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

/// fill 改选中框的背景;none 清成 null;point 框也允许设(渲染自己忽略)
#[test]
fn 填充动词改背景并支持清空() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.jump_to(0);
    app.cmd_apply("fill #102030");
    assert_eq!(app.doc.boxes[0].fill, Some((0x10, 0x20, 0x30)));
    assert!(app.view.note.as_deref().unwrap_or("").contains("填充"));
    app.cmd_apply("fill none");
    assert_eq!(app.doc.boxes[0].fill, None, "none 要清成 null");
    // 点框也允许设
    app.doc.boxes[0].style = BoxStyle::Point;
    app.cmd_apply("fill #abcdef");
    assert_eq!(app.doc.boxes[0].fill, Some((0xab, 0xcd, 0xef)));
}

#[test]
fn 填充参数坏了有回显且不改图() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.cmd_apply("fill blue");
    assert_eq!(app.doc.boxes[0].fill, None, "坏参数不该改图");
    assert!(app.view.note.as_deref().unwrap_or("").contains("fill"));
    app.cmd_apply("fill");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("fill"), "缺参数要有回显:{note}");
}

/// textcolor 改的是框内文字颜色;auto 清成 null(跟着框)
#[test]
fn 文字颜色动词改颜色并支持auto() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
fn 文字颜色参数坏了有回显() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    app.cmd_apply("textcolor nope");
    assert_eq!(app.doc.texts[0].color, None);
    assert!(app.view.note.as_deref().unwrap_or("").contains("textcolor"));
    app.cmd_apply("textcolor");
    let note = app.view.note.clone().unwrap_or_default();
    assert!(note.contains("textcolor"), "缺参数要有回显:{note}");
}

/// textalign 改框内文字对齐
#[test]
fn 文字对齐动词改对齐并拒绝坏参数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// dir 改选中边的四态方向
#[test]
fn 边方向动词改方向并拒绝坏参数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// head 改选中边的箭头字形;undo 能撤回
#[test]
fn 边箭头动词改字形并拒绝坏参数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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
    assert_eq!(app.doc.edges[1].head, None, "auto 就是跟全局走");
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
    // 一次改动一步 undo
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

/// 边的右键菜单里有箭头字形,比当前值打勾
#[test]
fn 边菜单有箭头字形() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 视图挪开之后 E 仍然能选中光标下的边(命中表按屏幕格,光标是画布格)
#[test]
fn 视图挪过也能选中边() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
    app.area = Rect::new(0, 0, 80, 24);
    app.view.zoom = 1.0;
    let (cx, cy) = app.view.center.unwrap_or((0.0, 0.0));
    app.view.center = Some((cx + 13.0, cy + 7.0));
    let p = render::panels(&app.doc, app.area, &app.view);
    let mut buf = ratatui::buffer::Buffer::empty(app.area);
    app.draw(&mut buf, app.area);
    // 找一条边占着的一格(命中表里存的是屏幕格),把光标放到它对应的画布格
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

/// corner 改选中边的拐角
#[test]
fn 边拐角动词改拐角并拒绝坏参数() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// dir / corner 没选边时要说一句
#[test]
fn 需要选边的动词没选边时报一句() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 新动词同样受 1:1 门槛约束:缩放不在一比一时先切回再执行
#[test]
fn 新编辑动词缩放不是一比一时先切回再执行() {
    let mut app = App::new(样例文档(), zh_opts()).expect("开局");
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

/// 补全表里的命令名与别名都必须落到某个动词族,不能掉进 unknown
#[test]
fn 所有命令名都被分发认识() {
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
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.cmd_apply(name);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            !note.starts_with("未知命令") && !note.starts_with("unknown command"),
            "{name} 没被认识: {note}"
        );
    }
}
