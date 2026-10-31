//! headless 协议:请求/响应、事务、dry_run、渲染、错误码与退出码.

use std::io::Cursor;

use graph_tui::document::{Box, DocEdge, Document, Group, Layer, Legend, Text, ViewState};
use graph_tui::headless::{self, Config, Status};
use graph_tui::json::Value;
use graph_tui::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};

fn temp(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!(
            "graph-tui-headless-{tag}-{}-{nanos}.gtui",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

/// 三框一线的小文档: a(0,0) -> B(12,0) -> C(12,8),id 固定方便断言
fn fixture(tag: &str) -> String {
    let group = Group {
        id: "g0".into(),
        label: "线".into(),
        color: (0x4c, 0x8e, 0xf8),
    };
    let layer = Layer {
        name: String::new(),
        order: 0,
    };
    let b = |id: &str, at: (i32, i32), style: BoxStyle| Box {
        id: id.into(),
        at,
        size: (4, 3),
        style,
        color: None,
        fill: None,
        layer: String::new(),
        terms: Vec::new(),
        edge: None,
    };
    let t = |id: &str, box_id: &str, at: (i32, i32), text: &str| Text {
        id: id.into(),
        text: text.into(),
        at,
        box_id: box_id.into(),
        color: None,
        bg: None,
        align: Align::Left,
        layer: String::new(),
    };
    let edge = |id: &str, from: &str, to: &str| DocEdge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        dir: Dir::Forward,
        group: "g0".into(),
        style: EdgeStyle::Solid,
        color: None,
        bg: None,
        via: Vec::new(),
        corner: Corner::Rounded,
        layer: String::new(),
    };
    let doc = Document::from_parts(
        "graph-tui test",
        vec![group],
        vec![layer],
        vec![
            b("a", (0, 0), BoxStyle::Rounded),
            b("B", (12, 0), BoxStyle::Rounded),
            b("C", (12, 8), BoxStyle::Single),
        ],
        vec![
            t("t1", "a", (1, 1), "a"),
            t("t2", "B", (13, 1), "B"),
            t("t3", "C", (13, 9), "C"),
        ],
        vec![edge("e1", "a", "B"), edge("e2", "B", "C")],
        // 视图固定住:光标在视口里,App 首帧不会自己居中,两边画出来才能逐字比
        ViewState {
            center: (8.0, 5.0),
            cursor: Some(graph_tui::document::Cursor::Cell((1, 1))),
            ..ViewState::default()
        },
        Legend::default(),
    )
    .expect("样例文档");
    let path = temp(tag);
    std::fs::write(&path, doc.save()).expect("write fixture");
    path
}

/// 跑一次会话:返回(退出码, stdout 里的响应, stderr)
fn session(file: &str, input: &str) -> (i32, Vec<Value>, String) {
    let cfg = Config {
        file: Some(file.to_string()),
        limit: 100_000,
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = headless::run_with(&cfg, &mut Cursor::new(input), &mut out, &mut err);
    let text = String::from_utf8_lossy(&out).into_owned();
    let resp: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| graph_tui::json::parse(l).expect("每行都是 JSON"))
        .collect();
    (code, resp, String::from_utf8_lossy(&err).into_owned())
}

fn one(file: &str, req: &str) -> Value {
    let (code, resp, err) = session(file, req);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(resp.len(), 1, "只该回一行");
    resp[0].clone()
}

fn ok(resp: &Value) -> &Value {
    assert_eq!(
        resp.get("ok").and_then(Value::as_bool),
        Some(true),
        "{resp:?}"
    );
    resp.get("result").expect("result")
}

fn err(resp: &Value) -> &Value {
    assert_eq!(
        resp.get("ok").and_then(Value::as_bool),
        Some(false),
        "{resp:?}"
    );
    resp.get("error").expect("error")
}

fn int(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(-1.0) as i64
}

/// 1. --op 只管一行;id 原样回传
#[test]
fn op_模式一行一个响应() {
    let path = fixture("info");
    let resp = one(&path, r##"{"id":"x1","op":"info"}"##);
    assert_eq!(
        resp.get("id").and_then(Value::as_str),
        Some("x1"),
        "id 该原样回传"
    );
    let r = ok(&resp);
    assert_eq!(int(r, "boxes"), 3);
    assert_eq!(int(r, "edges"), 2);
    assert_eq!(
        resp.get("protocol").and_then(Value::as_i32),
        Some(1),
        "响应带协议版本"
    );
    let _ = std::fs::remove_file(&path);
}

/// 2. NDJSON 会话:状态保留
#[test]
fn 会话里状态保留() {
    let path = fixture("session");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"info"}
{"id":2,"op":"add_box","label":"D","at":[30,0]}
{"id":3,"op":"info"}"##,
    );
    assert_eq!(code, 0);
    assert_eq!(resp.len(), 3);
    assert_eq!(
        int(ok(&resp[2]), "boxes"),
        4,
        "第三条请求看得到前一条的结果"
    );
    let _ = std::fs::remove_file(&path);
}

/// 3. add_box / add_edges 返回新 id;事务里能引用前面刚建的 id
#[test]
fn 事务里能引用刚建的id() {
    let path = fixture("tx");
    // a/B/C 已占,a、B、C 不是 bN 编号,所以新框从 b1 起
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"transaction","ops":[{"op":"add_box","label":"P","at":[40,0]},{"op":"add_box","label":"Q","at":[40,8]},{"op":"add_edges","sources":["b1"],"targets":["b2"]},{"op":"set","ids":["b1"],"attr":"style","value":"double"}]}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let r = ok(&resp[0]);
    let results = r.get("results").and_then(Value::as_arr).expect("results");
    assert_eq!(results.len(), 4);
    let p = results[0].get("id").and_then(Value::as_str).expect("P id");
    let q = results[1].get("id").and_then(Value::as_str).expect("Q id");
    assert_eq!((p, q), ("b1", "b2"), "新框 id 按最小空号");
    let ids = results[2]
        .get("ids")
        .and_then(Value::as_arr)
        .expect("edge ids");
    assert_eq!(ids.len(), 1, "事务里能用刚建的 id 连边");
    let _ = std::fs::remove_file(&path);
}

/// 4. set 批量改,返回 changed / unchanged
#[test]
fn 批量set返回改动的id() {
    let path = fixture("set");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"set","ids":["a","B"],"attr":"color","value":"#ef4b4b"}
{"id":2,"op":"set","ids":["a","B"],"attr":"color","value":"#ef4b4b"}"##,
    );
    assert_eq!(code, 0);
    let changed = ok(&resp[0])
        .get("changed")
        .and_then(Value::as_arr)
        .expect("changed");
    assert_eq!(changed.len(), 2);
    // 同值再来一次:都进 unchanged
    assert_eq!(
        ok(&resp[1])
            .get("changed")
            .and_then(Value::as_arr)
            .map(|a| a.len()),
        Some(0)
    );
    let _ = std::fs::remove_file(&path);
}

/// 4b. 文字底色与边底色各走各的,并且能存进 .gtui
#[test]
fn 底色独立设置并落盘() {
    let path = fixture("bg");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"set","ids":["a"],"attr":"textbg","value":"#334455"}
{"id":2,"op":"set","ids":["e1"],"attr":"edgebg","value":"#552211"}
{"id":3,"op":"save"}
{"id":4,"op":"get","target":"a"}
{"id":5,"op":"get","target":"e1"}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let t = ok(&resp[3]).get("text").expect("text");
    assert_eq!(t.get("bg").and_then(Value::as_str), Some("#334455"));
    assert_eq!(
        ok(&resp[3]).get("fill"),
        Some(&Value::Null),
        "文字底色不碰框的填充"
    );
    assert_eq!(
        ok(&resp[4]).get("bg").and_then(Value::as_str),
        Some("#552211")
    );
    // 落盘后读回来还在
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = Document::load(&text).expect("读回来");
    let ai = doc.box_idx("a").unwrap();
    assert_eq!(doc.text(ai).unwrap().bg, Some((0x33, 0x44, 0x55)));
    assert_eq!(doc.edges[0].bg, Some((0x55, 0x22, 0x11)));
    let _ = std::fs::remove_file(&path);
}

/// 5. delete/cut 返回快照;paste 生成新 id
#[test]
fn 剪切快照可粘回来() {
    let path = fixture("cut");
    // cut 之后同一会话里 paste(不给快照 = 用会话剪贴板)
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"cut","ids":["B"]}
{"id":2,"op":"paste","at":[30,20]}
{"id":3,"op":"info"}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let snap = ok(&resp[0]).get("snapshot").cloned().expect("snapshot");
    assert!(snap.get("boxes").and_then(Value::as_arr).is_some());
    let new = ok(&resp[1])
        .get("boxes")
        .and_then(Value::as_arr)
        .expect("new boxes");
    assert_eq!(new.len(), 1);
    assert_ne!(new[0].as_str(), Some("B"), "粘出来的该是新 id");
    assert_eq!(int(ok(&resp[2]), "boxes"), 3, "少一个多一个");
    // 剪掉的那条边因为另一头不在了被跳过(A->B 与 B->C 都连着 B)
    let _ = std::fs::remove_file(&path);
}

/// 6. 事务里任一失败 → 文档完全不变
#[test]
fn 事务失败整组回滚() {
    let path = fixture("rollback");
    let before = std::fs::read_to_string(&path).unwrap();
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"transaction","ops":[{"op":"add_box","label":"Z","at":[40,0]},{"op":"add_edges","sources":["a"],"targets":["nope"]}]}
{"id":2,"op":"info"}"##,
    );
    assert_eq!(code, 5, "事务回滚的退出码");
    let e = err(&resp[0]);
    assert_eq!(
        e.get("code").and_then(Value::as_str),
        Some("rollback"),
        "{e:?}"
    );
    assert!(
        e.get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .starts_with("ops[1]"),
        "{e:?}"
    );
    assert_eq!(int(ok(&resp[1]), "boxes"), 3, "回滚后框数不变");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "文件不该被写"
    );
    let _ = std::fs::remove_file(&path);
}

/// 7. dry_run:算结果但不动文档
#[test]
fn dry_run不动文档() {
    let path = fixture("dry");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"add_box","dry_run":true,"label":"D","at":[40,0]}
{"id":2,"op":"info"}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    assert!(ok(&resp[0]).get("id").is_some(), "dry_run 也要回新 id");
    assert_eq!(resp[0].get("dry_run").and_then(Value::as_bool), Some(true));
    assert_eq!(int(ok(&resp[1]), "boxes"), 3, "dry_run 不该落盘");
    let _ = std::fs::remove_file(&path);
}

/// 8. validate 失败带路径
#[test]
fn 校验错误带路径() {
    let path = fixture("validate-ok");
    // 正常文档:validate 通过
    let resp = one(&path, r##"{"id":1,"op":"validate"}"##);
    assert_eq!(ok(&resp).get("ok").and_then(Value::as_bool), Some(true));
    let _ = std::fs::remove_file(&path);
    // 坏文档:open 直接报 bad_doc
    let bad = temp("bad2");
    std::fs::write(&bad, "{\"type\":\"graph-tui\",\"version\":1}").unwrap();
    let cfg = Config {
        file: Some(bad.clone()),
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut errout: Vec<u8> = Vec::new();
    let code = headless::run_with(&cfg, &mut Cursor::new(""), &mut out, &mut errout);
    assert_eq!(code, 3, "文档坏了退出码是 3");
    assert!(!errout.is_empty(), "诊断走 stderr");
    assert!(out.is_empty(), "stdout 不该有东西");
    let _ = std::fs::remove_file(&bad);
}

/// 9/10. render text 与 ansi
#[test]
fn 渲染文本与ansi() {
    let path = fixture("render");
    let resp = one(
        &path,
        r##"{"id":1,"op":"render","format":"text","size":[40,12]}"##,
    );
    let r = ok(&resp);
    let lines = r.get("lines").and_then(Value::as_arr).expect("lines");
    assert_eq!(lines.len(), 12, "行数 == size 高度");
    let first = lines[0].as_str().unwrap_or("");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(first),
        40,
        "每行显示宽度等于列数"
    );
    assert!(lines.iter().any(|l| l.as_str().unwrap_or("").contains('a')));

    let resp = one(
        &path,
        r##"{"id":2,"op":"render","format":"json","size":[40,24]}"##,
    );
    let r = ok(&resp);
    let boxes = r.get("boxes").and_then(Value::as_arr).expect("boxes");
    assert_eq!(boxes.len(), 3, "每个框一个矩形");
    let rect = boxes[0].get("rect").and_then(Value::as_arr).expect("rect");
    assert_eq!(rect.len(), 4);
    let edges = r.get("edges").and_then(Value::as_arr).expect("edges");
    assert_eq!(edges.len(), 2, "两条边都有可见格");
    assert!(!edges[0]
        .get("cells")
        .and_then(Value::as_arr)
        .unwrap()
        .is_empty());

    let resp = one(
        &path,
        r##"{"id":3,"op":"render","format":"ansi","size":[20,6]}"##,
    );
    let ansi = ok(&resp).get("ansi").and_then(Value::as_str).expect("ansi");
    assert!(ansi.contains("\u{1b}[38;2;"), "ansi 该带真彩色转义");
    let _ = std::fs::remove_file(&path);
}

/// 10b. text 与 ansi 是同一帧:去掉转义后逐字相同;ansi 与 TUI --dump 共用同一个函数
#[test]
fn ansi与text是同一帧() {
    let path = fixture("ansi");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"render","format":"ansi","size":[40,12]}
{"id":2,"op":"render","format":"text","size":[40,12]}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let ansi = ok(&resp[0])
        .get("ansi")
        .and_then(Value::as_str)
        .expect("ansi");
    assert!(ansi.contains("\u{1b}[38;2;"), "ansi 该带真彩色转义");
    let strip = |t: &str| -> Vec<String> {
        t.lines()
            .map(|l| {
                let mut out = String::new();
                let mut it = l.chars().peekable();
                while let Some(c) = it.next() {
                    if c == '\u{1b}' {
                        for d in it.by_ref() {
                            if d == 'm' {
                                break;
                            }
                        }
                        continue;
                    }
                    out.push(c);
                }
                out
            })
            .collect()
    };
    let lines: Vec<String> = ok(&resp[1])
        .get("lines")
        .and_then(Value::as_arr)
        .expect("lines")
        .iter()
        .map(|l| l.as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(strip(ansi), lines, "text 与 ansi 该是同一帧");

    // TUI 的 --dump 用的是同一个 render::ansi:截一帧比比字形骨架
    let bin = env!("CARGO_BIN_EXE_graph-tui");
    let out = std::process::Command::new(bin)
        .args(["--dump", "--size", "40x12", &path])
        .output()
        .expect("run --dump");
    assert!(out.status.success());
    let dump = String::from_utf8_lossy(&out.stdout).into_owned();
    let dump_lines: Vec<String> = strip(&dump)
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let our: Vec<String> = strip(ansi)
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert!(
        dump_lines.len() >= our.len(),
        "--dump 至少包含整幅图:{} vs {}",
        dump_lines.len(),
        our.len()
    );
    for want in &our {
        assert!(
            dump_lines.iter().any(|l| l == want),
            "--dump 里该有这一行:{want:?}"
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// 9b. 宽字:文本行的显示宽度等于列数
#[test]
fn 宽字行按显示宽度补齐() {
    let path = fixture("wide");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"add_box","label":"新1","at":[30,0]}
{"id":2,"op":"render","format":"text","size":[44,14]}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let lines = ok(&resp[1])
        .get("lines")
        .and_then(Value::as_arr)
        .expect("lines");
    for (i, l) in lines.iter().enumerate() {
        let text = l.as_str().unwrap_or("");
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(text),
            44,
            "第 {i} 行显示宽度该是 44:{text:?}"
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// 11. inspect_cell:框 / 空 / 候选
#[test]
fn 查格子命中() {
    let path = fixture("cell");
    // a 在 (0,0) 4x3:文字格 (1,1) 落在框里
    let resp = one(&path, r##"{"id":1,"op":"inspect_cell","cell":[1,1]}"##);
    let r = ok(&resp);
    let pick = r.get("pick").expect("pick");
    assert_eq!(pick.get("type").and_then(Value::as_str), Some("box"));
    assert_eq!(pick.get("id").and_then(Value::as_str), Some("a"));
    // 空白处:pick 是 null
    let resp = one(&path, r##"{"id":2,"op":"inspect_cell","cell":[200,200]}"##);
    let r = ok(&resp);
    assert!(
        matches!(r.get("pick"), Some(Value::Null)),
        "空处该是 null:{r:?}"
    );
    let _ = std::fs::remove_file(&path);
}

/// 12. undo/redo 在会话内工作
#[test]
fn 会话内撤销重做() {
    let path = fixture("undo");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"set","ids":["a"],"attr":"style","value":"double"}
{"id":2,"op":"undo"}
{"id":3,"op":"get","target":"a"}
{"id":4,"op":"redo"}
{"id":5,"op":"get","target":"a"}"##,
    );
    assert_eq!(code, 0);
    let after_undo = ok(&resp[2]).get("style").and_then(Value::as_str);
    let after_redo = ok(&resp[4]).get("style").and_then(Value::as_str);
    assert_eq!(after_undo, Some("rounded"), "撤销该回到原样式");
    assert_eq!(after_redo, Some("double"), "重做该回到新样式");
    let _ = std::fs::remove_file(&path);
}

/// 13. 退出码与 stdout/stderr 分离
#[test]
fn 退出码与输出分离() {
    let path = fixture("codes");
    // 请求格式坏:2
    let (code, resp, stderr) = session(&path, "not json");
    assert_eq!(code, 2);
    assert_eq!(
        err(resp.first().expect("一行"))
            .get("code")
            .and_then(Value::as_str),
        Some("bad_request")
    );
    assert!(stderr.is_empty(), "协议错误也只在 stdout 里说: {stderr}");
    // 未知 op:2
    let (code, _, _) = session(&path, r##"{"id":1,"op":"nope"}"##);
    assert_eq!(code, 2);
    // IO:4(save 到不可写路径)
    let (code, resp, _) = session(&path, r##"{"id":1,"op":"save","path":"/proc/nope.gtui"}"##);
    assert_eq!(code, 4, "IO 失败的退出码");
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("io")
    );
    let _ = std::fs::remove_file(&path);
}

/// 14. 上限:批量建边与渲染尺寸
#[test]
fn 上限被拦住() {
    let path = fixture("limit");
    // 4096 条以上:先造 65 个框再 65x65 连边
    let mut reqs = String::new();
    for i in 0..65 {
        reqs.push_str(&format!(
            "{{\"id\":{},\"op\":\"add_box\",\"label\":\"n{i}\",\"at\":[{},0]}}\n",
            i + 1,
            i * 8
        ));
    }
    let (_, resp, _) = session(&path, &reqs);
    let ids: Vec<String> = resp
        .iter()
        .filter_map(|r| r.get("result")?.get("id")?.as_str().map(str::to_string))
        .collect();
    let sources = ids.join("\",\"");
    let (code, resp, _) = session(
        &path,
        &format!(
            r##"{{"id":1,"op":"add_edges","sources":["{sources}"],"targets":["{sources}"]}}"##
        ),
    );
    assert_eq!(code, 2);
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("limit")
    );
    let (code, resp, _) = session(
        &path,
        r##"{"id":2,"op":"render","format":"text","size":[1001,10]}"##,
    );
    assert_eq!(code, 2);
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("limit")
    );
    let _ = std::fs::remove_file(&path);
}

/// 15. TUI 的 set 与 headless 的 set 结果一致(同一个 `.gtui` 逐项相等)
#[test]
fn tui与headless的set结果一致() {
    let path_tui = fixture("parity-tui");
    let path_head = fixture("parity-head");
    // TUI 侧:App 里直接跑同样的命令
    let text = std::fs::read_to_string(&path_tui).unwrap();
    let doc = Document::load(&text).expect("load");
    let mut app = graph_tui::app::App::new(doc, graph_tui::app::Options::default()).expect("app");
    app.area = ratatui::layout::Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.cmd_apply("box double");
    app.cmd_apply("nodecolor #ef4b4b");
    // headless 侧:同样的意思,同一会话里改完存盘
    let (code, resp, _) = session(
        &path_head,
        r##"{"id":1,"op":"set","ids":["a"],"attr":"box","value":"double"}
{"id":2,"op":"set","ids":["a"],"attr":"color","value":"#ef4b4b"}
{"id":3,"op":"save"}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let head_text = std::fs::read_to_string(&path_head).unwrap();
    let head_doc = Document::load(&head_text).unwrap();
    let a_tui = &app.doc.boxes[app.doc.box_idx("a").unwrap()];
    let a_head = &head_doc.boxes[head_doc.box_idx("a").unwrap()];
    assert_eq!(a_tui.style, a_head.style, "框型");
    assert_eq!(a_tui.color, a_head.color, "颜色");
    assert_eq!(a_tui.size, a_head.size, "尺寸");
    let _ = std::fs::remove_file(&path_tui);
    let _ = std::fs::remove_file(&path_head);
}

/// 16. .dot 只导入;save 只写 .gtui
#[test]
fn dot能导入但只写gtui() {
    let dot = temp("import").replace(".gtui", ".dot");
    std::fs::write(
        &dot,
        "digraph g { node [box=single]; a [label=\"A\"]; b [label=\"B\"]; a -> b; }",
    )
    .unwrap();
    let cfg = Config {
        file: Some(dot.clone()),
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut errout: Vec<u8> = Vec::new();
    let code = headless::run_with(
        &cfg,
        &mut Cursor::new(r##"{"id":1,"op":"info"}"##),
        &mut out,
        &mut errout,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&errout));
    let resp = graph_tui::json::parse(String::from_utf8_lossy(&out).trim()).unwrap();
    assert_eq!(int(ok(&resp), "boxes"), 2);
    // save 不给路径:文件名是 .dot,不该写
    let (_, resp, _) = session(&dot, r##"{"id":2,"op":"save"}"##);
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("bad_arg"),
        "{resp:?}"
    );
    let _ = std::fs::remove_file(&dot);
}

/// 17. schema 是能解析的 JSON Schema
#[test]
fn schema可解析() {
    let path = fixture("schema");
    let resp = one(&path, r##"{"id":1,"op":"schema"}"##);
    let r = ok(&resp);
    assert_eq!(int(r, "protocol"), 1);
    for key in ["document", "request", "response"] {
        let part = r.get(key).expect(key);
        assert_eq!(
            part.get("type")
                .and_then(|t| t.get("type"))
                .and_then(Value::as_str),
            Some("object"),
            "{key} 该是 object schema"
        );
        assert!(part.get("properties").is_some(), "{key} 该有 properties");
    }
    let _ = std::fs::remove_file(&path);
}

/// 18. 没有 TTY:只靠 stdin/stdout 也能跑完
#[test]
fn 无终端也能跑() {
    let path = fixture("notty");
    let cfg = Config {
        file: Some(path.clone()),
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut errout: Vec<u8> = Vec::new();
    let code = headless::run_with(
        &cfg,
        &mut Cursor::new("{\"id\":1,\"op\":\"info\"}\n{\"id\":2,\"op\":\"info\"}\n"),
        &mut out,
        &mut errout,
    );
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&out).lines().count(), 2);
    let _ = std::fs::remove_file(&path);
}

/// --save-on-exit:会话结束把文档写回 --file
#[test]
fn 会话结束存盘() {
    let path = fixture("save-exit");
    let cfg = Config {
        file: Some(path.clone()),
        save_on_exit: true,
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut errout: Vec<u8> = Vec::new();
    let code = headless::run_with(
        &cfg,
        &mut Cursor::new("{\"id\":1,\"op\":\"add_box\",\"label\":\"S\",\"at\":[40,0]}\n"),
        &mut out,
        &mut errout,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&errout));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"S\""), "该写回文件");
    let doc = Document::load(&text).unwrap();
    assert_eq!(doc.boxes.len(), 4);
    let _ = std::fs::remove_file(&path);
}

/// batch 模式:默认整批,失败整批撤回
#[test]
fn 批量模式默认事务() {
    let path = fixture("batch");
    let ops = temp("ops").replace(".gtui", ".ndjson");
    std::fs::write(
        &ops,
        "{\"id\":1,\"op\":\"add_box\",\"label\":\"Z\",\"at\":[40,0]}\n{\"id\":2,\"op\":\"set\",\"ids\":[\"nope\"],\"attr\":\"color\",\"value\":\"#111111\"}\n",
    )
    .unwrap();
    let cfg = Config {
        file: Some(path.clone()),
        batch: Some(ops.clone()),
        ..Default::default()
    };
    let mut out: Vec<u8> = Vec::new();
    let mut errout: Vec<u8> = Vec::new();
    let code = headless::run_with(&cfg, &mut Cursor::new(""), &mut out, &mut errout);
    assert_eq!(code, Status::Rollback.code(), "整批失败退出码 5");
    let resp = one(&path, r##"{"id":9,"op":"info"}"##);
    assert_eq!(int(ok(&resp), "boxes"), 3, "第一条也不该留下");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&ops);
}

/// 图例:整体替换、增量、删除都进撤销栈;坏请求带错误码
#[test]
fn 图例的增删改与撤销() {
    let path = fixture("legend");
    let script = concat!(
        r##"{"id":1,"op":"legend_set","args":{"title":"转移","entries":[{"text":"CLOSED -> LISTEN","group":"g0"},{"text":"收到 RST","color":"#ef4b4b","glyph":"dashed"}]}}"##,
        "\n",
        r##"{"id":2,"op":"legend_add","args":{"text":"最后一行"}}"##,
        "\n",
        r##"{"id":3,"op":"legend"}"##,
        "\n",
        r##"{"id":4,"op":"legend_rm","args":{"index":2}}"##,
        "\n",
        r##"{"id":5,"op":"undo"}"##,
        "\n",
        r##"{"id":6,"op":"undo"}"##,
        "\n",
        r##"{"id":7,"op":"undo"}"##,
        "\n",
        r##"{"id":8,"op":"legend"}"##,
    );
    let (code, resp, stderr) = session(&path, script);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(resp.len(), 8, "{resp:?}");
    assert_eq!(int(ok(&resp[0]), "entries"), 2);
    assert_eq!(int(ok(&resp[1]), "count"), 3);
    let third = ok(&resp[2]);
    assert_eq!(third.get("title").and_then(Value::as_str), Some("转移"));
    assert_eq!(
        third
            .get("entries")
            .and_then(Value::as_arr)
            .map(|a| a.len()),
        Some(3)
    );
    assert_eq!(int(ok(&resp[3]), "count"), 2, "删掉第 2 行");
    // 三次真改动之后撤销三次,图例回到空
    assert_eq!(
        ok(&resp[7])
            .get("entries")
            .and_then(Value::as_arr)
            .map(|a| a.len()),
        Some(0),
        "撤销回到没有图例的状态:{:?}",
        resp[7]
    );

    // 坏请求:越界、悬空分组、未知字形(都会让会话以 2 退出)
    for bad_req in [
        r##"{"op":"legend_rm","args":{"index":9}}"##,
        r##"{"op":"legend_add","args":{"text":"x","group":"nope"}}"##,
        r##"{"op":"legend_add","args":{"text":"x","glyph":"wave"}}"##,
    ] {
        let (code, resp, _) = session(&path, bad_req);
        assert_eq!(code, 2, "坏请求该以 2 退出:{bad_req}");
        assert!(err(&resp[0]).get("message").is_some(), "{bad_req}");
    }
    let _ = std::fs::remove_file(&path);
}

/// 图例开关是视图状态:render 默认带,legend:false 就没有;info 报 view.legend
#[test]
fn 图例默认渲染并能关掉() {
    let path = fixture("legend-on");
    let script = concat!(
        r##"{"id":1,"op":"legend_add","args":{"text":"一行说明","glyph":"arrow"}}"##,
        "\n",
        r##"{"id":2,"op":"info"}"##,
        "\n",
        r##"{"id":3,"op":"render","format":"json","size":"auto"}"##,
        "\n",
        r##"{"id":4,"op":"render","format":"json","size":"auto","legend":false}"##,
        "\n",
        r##"{"id":5,"op":"view","args":{"legend":false}}"##,
        "\n",
        r##"{"id":6,"op":"info"}"##,
    );
    let (code, resp, stderr) = session(&path, script);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(resp.len(), 6, "{resp:?}");
    let info = ok(&resp[1]);
    assert_eq!(int(info.get("legend").unwrap(), "entries"), 1);
    assert_eq!(
        info.get("view")
            .and_then(|v| v.get("legend"))
            .and_then(Value::as_bool),
        Some(true),
        "新文档默认开"
    );
    assert!(ok(&resp[2]).get("legend").is_some(), "默认该有图例矩形");
    assert!(
        ok(&resp[3]).get("legend").is_none(),
        "关掉之后不该有图例矩形"
    );
    assert_eq!(
        ok(&resp[5])
            .get("view")
            .and_then(|v| v.get("legend"))
            .and_then(Value::as_bool),
        Some(false),
        "view 里的开关要落盘"
    );
    let _ = std::fs::remove_file(&path);
}

/// 边注释:set 建框 / get 报绑定 / render json 报注释文字与矩形
#[test]
fn 边注释的读写与渲染报告() {
    let path = fixture("enote");
    let script = concat!(
        r##"{"id":1,"op":"set","args":{"ids":["e1"],"attr":"note","value":"收到 SYN"}}"##,
        "\n",
        r##"{"id":2,"op":"get","args":{"id":"e1"}}"##,
        "\n",
        r##"{"id":3,"op":"render","format":"json","size":"auto"}"##,
        "\n",
        r##"{"id":4,"op":"set","args":{"ids":["e1"],"attr":"note","value":null}}"##,
        "\n",
        r##"{"id":5,"op":"list","args":{"kind":"boxes"}}"##,
    );
    let (code, resp, stderr) = session(&path, script);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(resp.len(), 5, "{resp:?}");
    let render = ok(&resp[2]);
    let noted = render
        .get("edges")
        .and_then(Value::as_arr)
        .and_then(|a| {
            a.iter()
                .find(|e| e.get("id").and_then(Value::as_str) == Some("e1"))
        })
        .expect("e1 该有格子");
    assert_eq!(
        noted.get("note").and_then(Value::as_str),
        Some("收到 SYN"),
        "{noted:?}"
    );
    assert!(
        noted.get("note_rect").and_then(Value::as_arr).is_some(),
        "注释框要报位置:{noted:?}"
    );
    // 清掉之后注释框也没了
    let boxes = ok(&resp[4]);
    assert_eq!(
        boxes.get("count").and_then(Value::as_f64),
        Some(3.0),
        "注释框删掉后只剩原来的三个节点:{boxes:?}"
    );

    // 坏值:多行、类型不对
    for bad in [
        r##"{"op":"set","args":{"ids":["e1"],"attr":"note","value":"a\nb"}}"##,
        r##"{"op":"set","args":{"ids":["e1"],"attr":"note","value":3}}"##,
    ] {
        let (code, resp, _) = session(&path, bad);
        assert_eq!(code, 2, "坏值该以 2 退出:{bad}");
        assert!(err(&resp[0]).get("message").is_some(), "{bad}");
    }
    let _ = std::fs::remove_file(&path);
}
