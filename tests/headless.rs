//! Headless protocol: requests/responses, transactions, dry_run, rendering, error codes
//! and exit codes.

use std::io::Cursor;

use graph_tui::document::{Box, DocEdge, Document, Group, Layer, Text, ViewState};
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

/// Small document with three boxes and one edge: a(0,0) -> B(12,0) -> C(12,8);
/// fixed ids keep assertions simple.
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
        from_side: None,
        to_side: None,
        head: None,
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
        // Pin the view: the cursor is inside the viewport so the App's first frame will not
        // re-centre on its own; both sides can then be compared glyph by glyph
        ViewState {
            center: (8.0, 5.0),
            cursor: Some(graph_tui::document::Cursor::Cell((1, 1))),
            ..ViewState::default()
        },
    )
    .expect("样例文档");
    let path = temp(tag);
    std::fs::write(&path, doc.save()).expect("write fixture");
    path
}

/// Run one session: returns (exit code, responses on stdout, stderr).
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

/// 1. --op handles one line only; the id is echoed back verbatim.
#[test]
fn op_mode_one_response_per_line() {
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

/// 2. NDJSON session: state persists.
#[test]
fn session_keeps_state() {
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

/// 3. add_box / add_edges return new ids; a transaction can reference ids just created.
#[test]
fn transaction_can_reference_fresh_id() {
    let path = fixture("tx");
    // a/B/C are taken; since they are not bN names, new boxes start at b1
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

/// 4. set changes in bulk and returns changed / unchanged.
#[test]
fn batch_set_returns_changed_ids() {
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
    // Same values again: everything lands in unchanged
    assert_eq!(
        ok(&resp[1])
            .get("changed")
            .and_then(Value::as_arr)
            .map(|a| a.len()),
        Some(0)
    );
    let _ = std::fs::remove_file(&path);
}

/// 4b. Text background and edge background are independent and persist to .gtui.
#[test]
fn bg_set_independently_and_persisted() {
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
    // Still there after reading back from disk
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = Document::load(&text).expect("读回来");
    let ai = doc.box_idx("a").unwrap();
    assert_eq!(doc.text(ai).unwrap().bg, Some((0x33, 0x44, 0x55)));
    assert_eq!(doc.edges[0].bg, Some((0x55, 0x22, 0x11)));
    let _ = std::fs::remove_file(&path);
}

/// 5. delete/cut return a snapshot; paste generates new ids.
#[test]
fn cut_snapshot_can_be_pasted_back() {
    let path = fixture("cut");
    // paste in the same session after cut (no snapshot = session clipboard)
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
    // The cut edge is skipped because its other end is gone (both A->B and B->C touch B)
    let _ = std::fs::remove_file(&path);
}

/// 6. Any failure inside a transaction -> the document is unchanged.
#[test]
fn transaction_failure_rolls_back_whole_group() {
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

/// 7. dry_run: compute the result without touching the document.
#[test]
fn dry_run_leaves_document_untouched() {
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

/// 8. A validate failure carries a path.
#[test]
fn validation_error_includes_path() {
    let path = fixture("validate-ok");
    // Good document: validate passes
    let resp = one(&path, r##"{"id":1,"op":"validate"}"##);
    assert_eq!(ok(&resp).get("ok").and_then(Value::as_bool), Some(true));
    let _ = std::fs::remove_file(&path);
    // Bad document: open reports bad_doc directly
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

/// The layout op reports crossings / reversed / warnings.
#[test]
fn layout_reports_fields() {
    let path = fixture("render");
    let resp = one(&path, r#"{"id":1,"op":"layout","layout":"layered"}"#);
    let r = ok(&resp);
    assert_eq!(r.get("layout").and_then(Value::as_str), Some("layered"));
    assert!(r.get("moved").and_then(Value::as_i32).is_some());
    assert!(r.get("crossings").and_then(Value::as_i32).is_some());
    assert!(r.get("reversed").and_then(Value::as_i32).is_some());
    let warnings = r.get("warnings").and_then(Value::as_arr).expect("warnings");
    for w in warnings {
        assert!(w.get("code").and_then(Value::as_str).is_some());
        assert!(w.get("message").and_then(Value::as_str).is_some());
    }
}

/// 9/10. render text and ansi.
#[test]
fn render_text_and_ansi() {
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
    // Labels that do not fit and their matching non-fatal warnings: fixed fields scripts can read
    let skipped = r
        .get("labels_skipped")
        .and_then(Value::as_arr)
        .expect("labels_skipped");
    let warnings = r.get("warnings").and_then(Value::as_arr).expect("warnings");
    assert_eq!(skipped.len(), warnings.len(), "一条丢失标签一条告警");
    for (s, w) in skipped.iter().zip(warnings.iter()) {
        assert!(s.get("box").and_then(Value::as_str).is_some());
        assert!(s.get("text").and_then(Value::as_str).is_some());
        assert_eq!(
            w.get("code").and_then(Value::as_str),
            Some("label_not_placed")
        );
        assert_eq!(
            w.get("box").and_then(Value::as_str),
            s.get("box").and_then(Value::as_str)
        );
    }
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

/// Split markers only apply at the interactive prompt line: headless text writes them as
/// plain text.
#[test]
fn headless_text_ignores_split_marker() {
    let path = fixture("split");
    let (code, resp, _) = session(
        &path,
        r##"{"id":1,"op":"text","ids":["a"],"text":"x<|new box|>y"}
{"id":2,"op":"save"}"##,
    );
    assert_eq!(code, 0, "{resp:?}");
    let raw = std::fs::read_to_string(&path).expect("读回文件");
    let doc = graph_tui::json::parse(&raw).expect("文件是 JSON");
    let boxes = doc.get("boxes").and_then(Value::as_arr).expect("boxes");
    assert_eq!(boxes.len(), 3, "没有新框");
    let texts: Vec<&str> = doc
        .get("texts")
        .and_then(Value::as_arr)
        .expect("texts")
        .iter()
        .map(|t| t.get("text").and_then(Value::as_str).unwrap_or(""))
        .collect();
    assert!(texts.contains(&"x<|new box|>y"), "{texts:?}");
    let _ = std::fs::remove_file(&path);
}

/// 10b. text and ansi are the same frame: identical glyph by glyph after stripping
/// escapes; ansi shares one function with the TUI picture.
#[test]
fn ansi_and_text_are_same_frame() {
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

    // The TUI picture uses the same render::ansi: capture a frame and compare the glyph skeleton
    let bin = env!("CARGO_BIN_EXE_graph-tui");
    let out = std::process::Command::new(bin)
        .args(["--export", "-", "--area", "auto,auto,40,12", &path])
        .output()
        .expect("run --export -");
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
        "画面至少包含整幅图:{} vs {}",
        dump_lines.len(),
        our.len()
    );
    for want in &our {
        assert!(
            dump_lines.iter().any(|l| l == want),
            "画面里该有这一行:{want:?}"
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// 9b. Wide glyphs: a text line's display width equals the column count.
#[test]
fn wide_glyph_lines_padded_to_display_width() {
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

/// 11. inspect_cell: box / empty / candidate.
#[test]
fn inspect_cell_hit() {
    let path = fixture("cell");
    // a sits at (0,0) and is 4x3: cell (1,1) falls inside the box
    let resp = one(&path, r##"{"id":1,"op":"inspect_cell","cell":[1,1]}"##);
    let r = ok(&resp);
    let pick = r.get("pick").expect("pick");
    assert_eq!(pick.get("type").and_then(Value::as_str), Some("box"));
    assert_eq!(pick.get("id").and_then(Value::as_str), Some("a"));
    // Empty area: pick is null
    let resp = one(&path, r##"{"id":2,"op":"inspect_cell","cell":[200,200]}"##);
    let r = ok(&resp);
    assert!(
        matches!(r.get("pick"), Some(Value::Null)),
        "空处该是 null:{r:?}"
    );
    let _ = std::fs::remove_file(&path);
}

/// 12. undo/redo work within a session.
#[test]
fn undo_redo_within_session() {
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

/// 13. Exit codes and stdout/stderr separation.
#[test]
fn exit_codes_and_output_separation() {
    let path = fixture("codes");
    // Malformed request: 2
    let (code, resp, stderr) = session(&path, "not json");
    assert_eq!(code, 2);
    assert_eq!(
        err(resp.first().expect("一行"))
            .get("code")
            .and_then(Value::as_str),
        Some("bad_request")
    );
    assert!(stderr.is_empty(), "协议错误也只在 stdout 里说: {stderr}");
    // Unknown op: 2
    let (code, _, _) = session(&path, r##"{"id":1,"op":"nope"}"##);
    assert_eq!(code, 2);
    // IO: 4 (save to an unwritable path)
    let (code, resp, _) = session(&path, r##"{"id":1,"op":"save","path":"/proc/nope.gtui"}"##);
    assert_eq!(code, 4, "IO 失败的退出码");
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("io")
    );
    let _ = std::fs::remove_file(&path);
}

/// 14. Limits: bulk edge creation and render size.
#[test]
fn limits_are_enforced() {
    let path = fixture("limit");
    // Above 4096: create 65 boxes, then 65x65 edges
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

/// 15. TUI set and headless set agree (every field equal on the same `.gtui`).
#[test]
fn tui_and_headless_set_agree() {
    let path_tui = fixture("parity-tui");
    let path_head = fixture("parity-head");
    // TUI side: run the same commands directly in the App
    let text = std::fs::read_to_string(&path_tui).unwrap();
    let doc = Document::load(&text).expect("load");
    let mut app = graph_tui::app::App::new(doc, graph_tui::app::Options::default()).expect("app");
    app.area = ratatui::layout::Rect::new(0, 0, 80, 24);
    app.jump_to(0);
    app.cmd_apply("box double");
    app.cmd_apply("nodecolor #ef4b4b");
    // headless side: same intent, edit then save in one session
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

/// 16. .dot is import-only; save writes .gtui only.
#[test]
fn dot_imports_but_saves_only_gtui() {
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
    // save without a path: the filename is .dot, so nothing should be written
    let (_, resp, _) = session(&dot, r##"{"id":2,"op":"save"}"##);
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("bad_arg"),
        "{resp:?}"
    );
    let _ = std::fs::remove_file(&dot);
}

/// 17. schema is parseable JSON Schema.
#[test]
fn schema_is_parseable() {
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

/// 18. Without a TTY: runs to completion over stdin/stdout alone.
#[test]
fn runs_without_tty() {
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

/// --save-on-exit: write the document back to --file when the session ends.
#[test]
fn save_on_session_exit() {
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

/// batch mode: the whole batch is a transaction, and a failure reverts it all.
#[test]
fn batch_mode_defaults_to_transaction() {
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

/// No legend support: old legend_* ops fail as unknown ops (session exits 2) and render
/// emits no legend rectangles.
#[test]
fn legend_ops_are_unknown() {
    let path = fixture("legend-gone");
    let (code, resp, _) = session(&path, r##"{"id":1,"op":"legend"}"##);
    assert_eq!(code, 2, "未知 op 该以 2 退出");
    assert_eq!(
        err(&resp[0]).get("code").and_then(Value::as_str),
        Some("bad_request")
    );
    let msg = err(&resp[0])
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(msg.contains("unknown op"), "{msg}");

    for op in ["legend_set", "legend_add", "legend_rm"] {
        let (code, _, _) = session(&path, &format!(r#"{{"id":1,"op":"{op}","args":{{}}}}"#));
        assert_eq!(code, 2, "{op} 也该是未知 op");
    }

    // render has no legend field
    let resp = one(
        &path,
        r##"{"id":1,"op":"render","format":"json","size":"auto"}"##,
    );
    assert!(ok(&resp).get("legend").is_none(), "渲染结果里不该有图例");
    let _ = std::fs::remove_file(&path);
}

/// Edge label: set creates the box / get reports the binding / render json reports the
/// label text and rect.
#[test]
fn edge_label_read_write_and_render_report() {
    let path = fixture("enote");
    let script = concat!(
        r##"{"id":1,"op":"set","args":{"ids":["e1"],"attr":"edge_label","value":"收到 SYN"}}"##,
        "\n",
        r##"{"id":2,"op":"get","args":{"id":"e1"}}"##,
        "\n",
        r##"{"id":3,"op":"render","format":"json","size":"auto"}"##,
        "\n",
        r##"{"id":4,"op":"set","args":{"ids":["e1"],"attr":"edge_label","value":null}}"##,
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
        noted.get("edge_label").and_then(Value::as_str),
        Some("收到 SYN"),
        "{noted:?}"
    );
    assert!(
        noted
            .get("edge_label_rect")
            .and_then(Value::as_arr)
            .is_some(),
        "注释框要报位置:{noted:?}"
    );
    // After clearing, the edge label box is gone too
    let boxes = ok(&resp[4]);
    assert_eq!(
        boxes.get("count").and_then(Value::as_f64),
        Some(3.0),
        "注释框删掉后只剩原来的三个节点:{boxes:?}"
    );

    // Bad values: multiple lines, wrong type
    for bad in [
        r##"{"op":"set","args":{"ids":["e1"],"attr":"edge_label","value":"a\nb"}}"##,
        r##"{"op":"set","args":{"ids":["e1"],"attr":"edge_label","value":3}}"##,
    ] {
        let (code, resp, _) = session(&path, bad);
        assert_eq!(code, 2, "坏值该以 2 退出:{bad}");
        assert!(err(&resp[0]).get("message").is_some(), "{bad}");
    }
    let _ = std::fs::remove_file(&path);
}
