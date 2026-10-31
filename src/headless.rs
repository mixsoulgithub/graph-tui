//! Headless 协议:一行一个 JSON 请求,一行一个 JSON 响应.
//!
//! 结构:
//!   `Engine`(文档 + 撤销 + 剪贴板) -> 这里只负责协议:解析请求、分发、包响应、
//!   定退出码. 所有语义都在 `crate::engine` 里.
//!
//! stdout 只放协议数据;诊断走 stderr. 没有 TTY 也能跑.

use crate::engine::{Engine, Error};
use crate::json::Value;
use std::io::{BufRead, Write};

/// 协议版本:加字段不升,不兼容才升
pub const PROTOCOL: i64 = 1;

/// 一次请求的结局(决定进程退出码)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    /// 2:请求格式错误
    BadRequest,
    /// 3:文档校验失败
    BadDoc,
    /// 4:IO 失败
    Io,
    /// 5:事务回滚
    Rollback,
}

impl Status {
    pub fn code(self) -> i32 {
        match self {
            Status::Ok => 0,
            Status::BadRequest => 2,
            Status::BadDoc => 3,
            Status::Io => 4,
            Status::Rollback => 5,
        }
    }
}

/// 会话:跨请求保留文档、撤销栈、剪贴板
pub struct Session {
    pub engine: Engine,
    pub limit: usize,
    pub seen: usize,
}

impl Session {
    pub fn new(engine: Engine, limit: usize) -> Session {
        Session {
            engine,
            limit,
            seen: 0,
        }
    }

    /// 处理一行请求文本,返回(响应 JSON, 结局)
    pub fn handle_line(&mut self, line: &str) -> (Value, Status) {
        let req = match crate::json::parse(line) {
            Ok(v) => v,
            Err(e) => {
                return (
                    error_response(Value::Null, &Error::new("bad_request", "request", e)),
                    Status::BadRequest,
                )
            }
        };
        if !matches!(req, Value::Obj(_)) {
            return (
                error_response(
                    Value::Null,
                    &Error::new("bad_request", "request", "request must be a JSON object"),
                ),
                Status::BadRequest,
            );
        }
        self.seen += 1;
        if self.seen > self.limit {
            return (
                error_response(
                    req.get("id").cloned().unwrap_or(Value::Null),
                    &Error::new(
                        "limit",
                        "request",
                        format!("session request limit {} reached", self.limit),
                    ),
                ),
                Status::BadRequest,
            );
        }
        self.handle(&req)
    }

    pub fn handle(&mut self, req: &Value) -> (Value, Status) {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let dry = req.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
        let before = if dry {
            Some(self.engine.doc.clone())
        } else {
            None
        };
        let past = self.engine.undo.counts().0;
        let out = self.dispatch(req);
        if let Some(doc) = before {
            // dry_run:把文档和撤销栈都回到原样,只把结果交出去
            self.engine.doc = doc;
            while self.engine.undo.counts().0 > past {
                self.engine.undo.drop_last();
            }
        }
        match out {
            Ok(result) => {
                let mut f = vec![
                    ("id".to_string(), id),
                    ("ok".to_string(), Value::Bool(true)),
                    ("protocol".to_string(), Value::Int(PROTOCOL)),
                    ("result".to_string(), result),
                ];
                if dry {
                    f.push(("dry_run".to_string(), Value::Bool(true)));
                }
                (Value::Obj(f), Status::Ok)
            }
            Err(e) => {
                let st = match e.code {
                    "bad_doc" => Status::BadDoc,
                    "io" => Status::Io,
                    "rollback" => Status::Rollback,
                    _ => Status::BadRequest,
                };
                (error_response(id, &e), st)
            }
        }
    }

    fn dispatch(&mut self, req: &Value) -> Result<Value, Error> {
        // 参数既可以直接放顶层,也可以收在 args 里
        let args = match req.get("args") {
            Some(Value::Obj(_)) => req.get("args").unwrap(),
            _ => req,
        };
        let op = req
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("bad_request", "op", "missing op"))?;
        match op {
            "open" => {
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::bad("path", "path is required"))?;
                self.engine.open(path)
            }
            "save" => self.engine.save(args.get("path").and_then(Value::as_str)),
            "info" => Ok(self.engine.info()),
            "validate" => self.engine.validate(),
            "schema" => Ok(schema()),
            "list" => self.engine.list(args),
            "get" => {
                // 请求自己的 `id` 是回执编号,所以目标 id 走 target / args.id
                let id = req
                    .get("args")
                    .and_then(|a| a.get("id"))
                    .and_then(Value::as_str)
                    .or_else(|| req.get("target").and_then(Value::as_str))
                    .or_else(|| req.get("get").and_then(Value::as_str))
                    .ok_or_else(|| {
                        Error::bad("target", "get needs a target id (use target or args.id)")
                    })?;
                self.engine.get(id)
            }
            "inspect_cell" => {
                let cell = args
                    .get("cell")
                    .and_then(|v| {
                        let a = v.as_arr()?;
                        if a.len() != 2 {
                            return None;
                        }
                        Some((a[0].as_i32()?, a[1].as_i32()?))
                    })
                    .ok_or_else(|| Error::bad("cell", "cell [x, y] is required"))?;
                self.engine.inspect_cell(cell)
            }
            "add_box" => self.engine.add_box(args),
            "add_edges" => self.engine.add_edges(args),
            "set" => self.engine.set(args),
            "move" => self.engine.move_ids(args),
            "delete" | "cut" => self.engine.cut(args),
            "paste" => {
                if args.get("snapshot").is_some() {
                    self.engine.paste(args)
                } else if self.engine.clip.is_some() {
                    // 不给快照就用会话里的剪贴板(上一次 cut 的)
                    let clip = self.engine.clip.clone().unwrap();
                    let snap = clip_to_value(&clip);
                    let mut merged = vec![("snapshot".to_string(), snap)];
                    if let Some(at) = args.get("at") {
                        merged.push(("at".to_string(), at.clone()));
                    }
                    self.engine.paste(&Value::Obj(merged))
                } else {
                    Err(Error::bad(
                        "snapshot",
                        "no snapshot given and the clipboard is empty",
                    ))
                }
            }
            "layer" => self.engine.layer(args),
            "text" => self.engine.text(args),
            "layout" => self.engine.layout(args),
            "orient" => self.engine.orient(args),
            "rotate" => self.engine.rotate(args),
            "view" => self.engine.view(args),
            "legend" => Ok(self.engine.legend()),
            "legend_set" => self.engine.legend_set(args),
            "legend_add" => self.engine.legend_add(args),
            "legend_rm" => self.engine.legend_rm(args),
            "render" => self.engine.render(args),
            "export" => self.engine.export(args),
            "undo" => self.engine.undo(),
            "redo" => self.engine.redo(),
            "transaction" => self.transaction(args),
            other => Err(Error::new(
                "bad_request",
                "op",
                format!("unknown op: {other}"),
            )),
        }
    }

    /// 一组操作:全成或者全不成,只算一步撤销
    fn transaction(&mut self, args: &Value) -> Result<Value, Error> {
        let ops = args
            .get("ops")
            .and_then(Value::as_arr)
            .ok_or_else(|| Error::bad("ops", "ops must be an array"))?
            .to_vec();
        let label = args
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("transaction")
            .to_string();
        let mut results: Vec<Value> = Vec::new();
        let engine = &mut self.engine;
        let out = engine.batch(&label, |e| {
            for (k, op) in ops.iter().enumerate() {
                match run_op(e, op) {
                    Ok(v) => results.push(v),
                    Err(err) => {
                        return Err(Error {
                            code: "rollback",
                            path: format!("ops[{k}].{}", err.path),
                            message: err.message,
                        })
                    }
                }
            }
            Ok(())
        });
        match out {
            Ok(()) => Ok(Value::Obj(vec![
                ("ops".to_string(), Value::Int(results.len() as i64)),
                ("results".to_string(), Value::Arr(results)),
            ])),
            Err(e) => Err(e),
        }
    }
}

/// 事务里的一个子步骤:只认会改文档的操作
fn run_op(e: &mut Engine, op: &Value) -> Result<Value, Error> {
    let op_name = op
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("bad_request", "op", "missing op"))?;
    match op_name {
        "add_box" => e.add_box(op),
        "add_edges" => e.add_edges(op),
        "set" => e.set(op),
        "move" => e.move_ids(op),
        "delete" | "cut" => e.cut(op),
        "paste" => e.paste(op),
        "layer" => e.layer(op),
        "text" => e.text(op),
        "layout" => e.layout(op),
        "orient" => e.orient(op),
        "rotate" => e.rotate(op),
        "view" => e.view(op),
        "legend_set" => e.legend_set(op),
        "legend_add" => e.legend_add(op),
        "legend_rm" => e.legend_rm(op),
        other => Err(Error::new(
            "bad_request",
            "op",
            format!("{other} is not allowed inside a transaction"),
        )),
    }
}

fn error_response(id: Value, e: &Error) -> Value {
    Value::Obj(vec![
        ("id".to_string(), id),
        ("ok".to_string(), Value::Bool(false)),
        ("protocol".to_string(), Value::Int(PROTOCOL)),
        ("error".to_string(), e.to_value()),
    ])
}

fn clip_to_value(clip: &crate::app::Clipboard) -> Value {
    // 和 engine::cut 的 snapshot 同构
    let mut boxes = Vec::new();
    for b in &clip.boxes {
        boxes.push(Value::Obj(vec![
            ("id".into(), Value::Str(b.id.clone())),
            (
                "at".into(),
                Value::Arr(vec![Value::Int(b.at.0 as i64), Value::Int(b.at.1 as i64)]),
            ),
            (
                "size".into(),
                Value::Arr(vec![
                    Value::Int(b.size.0 as i64),
                    Value::Int(b.size.1 as i64),
                ]),
            ),
            ("style".into(), Value::Str(b.style.name().to_string())),
            (
                "color".into(),
                b.color
                    .map(|c| Value::Str(format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)))
                    .unwrap_or(Value::Null),
            ),
            (
                "fill".into(),
                b.fill
                    .map(|c| Value::Str(format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)))
                    .unwrap_or(Value::Null),
            ),
            ("layer".into(), Value::Str(b.layer.clone())),
            (
                "terms".into(),
                Value::Arr(b.terms.iter().cloned().map(Value::Str).collect()),
            ),
        ]));
    }
    let mut texts = Vec::new();
    for t in &clip.texts {
        texts.push(Value::Obj(vec![
            ("id".into(), Value::Str(t.id.clone())),
            ("text".into(), Value::Str(t.text.clone())),
            (
                "at".into(),
                Value::Arr(vec![Value::Int(t.at.0 as i64), Value::Int(t.at.1 as i64)]),
            ),
            ("box".into(), Value::Str(t.box_id.clone())),
            (
                "bg".into(),
                t.bg.map(|c| Value::Str(format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)))
                    .unwrap_or(Value::Null),
            ),
            ("align".into(), Value::Str(t.align.name().to_string())),
        ]));
    }
    let mut edges = Vec::new();
    for e in &clip.edges {
        edges.push(Value::Obj(vec![
            ("id".into(), Value::Str(e.id.clone())),
            ("from".into(), Value::Str(e.from.clone())),
            ("to".into(), Value::Str(e.to.clone())),
            ("dir".into(), Value::Str(e.dir.name().to_string())),
            ("group".into(), Value::Str(e.group.clone())),
            ("style".into(), Value::Str(e.style.name().to_string())),
            (
                "bg".into(),
                e.bg.map(|c| Value::Str(format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)))
                    .unwrap_or(Value::Null),
            ),
            (
                "via".into(),
                Value::Arr(
                    e.via
                        .iter()
                        .map(|(x, y)| {
                            Value::Arr(vec![Value::Int(*x as i64), Value::Int(*y as i64)])
                        })
                        .collect(),
                ),
            ),
        ]));
    }
    let mut groups = Vec::new();
    for g in &clip.groups {
        groups.push(Value::Obj(vec![
            ("id".into(), Value::Str(g.id.clone())),
            ("label".into(), Value::Str(g.label.clone())),
            (
                "color".into(),
                Value::Str(format!(
                    "#{:02x}{:02x}{:02x}",
                    g.color.0, g.color.1, g.color.2
                )),
            ),
        ]));
    }
    Value::Obj(vec![
        (
            "origin".into(),
            Value::Arr(vec![
                Value::Int(clip.origin.0 as i64),
                Value::Int(clip.origin.1 as i64),
            ]),
        ),
        ("boxes".into(), Value::Arr(boxes)),
        ("texts".into(), Value::Arr(texts)),
        ("edges".into(), Value::Arr(edges)),
        ("groups".into(), Value::Arr(groups)),
    ])
}

/// 协议与文档的 JSON Schema(够工具做校验和补全;权威校验仍是 `validate`)
fn schema() -> Value {
    let hex = "{\"type\":[\"string\",\"null\"],\"pattern\":\"^#[0-9a-fA-F]{6}$\"}";
    let s = |k: &str, v: &str| format!("\"{k}\":{v}");
    let m = |parts: Vec<String>| format!("{{{}}}", parts.join(","));
    let doc =
        m(vec![
        s("type", "{\"type\":\"object\"}"),
        s(
            "required",
            "[\"type\",\"version\",\"groups\",\"layers\",\"boxes\",\"texts\",\"edges\",\"view\"]",
        ),
        s(
            "properties",
            &m(vec![
                s("type", "{\"const\":\"graph-tui\"}"),
                s("version", "{\"type\":\"integer\",\"minimum\":1,\"maximum\":3}"),
                s("source", "{\"type\":\"string\"}"),
                s("unit", "{\"const\":\"cell\"}"),
                s("groups", "{\"type\":\"array\"}"),
                s("layers", "{\"type\":\"array\"}"),
                s("boxes", "{\"type\":\"array\"}"),
                s("texts", "{\"type\":\"array\"}"),
                s("edges", "{\"type\":\"array\"}"),
                s("view", "{\"type\":\"object\"}"),
                s("legend", "{\"type\":\"object\"}"),
            ]),
        ),
        s("additionalProperties", "true"),
    ]);
    let request = m(vec![
        s("type", "{\"type\":\"object\"}"),
        s("required", "[\"op\"]"),
        s(
            "properties",
            &m(vec![
                s("id", "{\"type\":[\"integer\",\"string\"]}"),
                s(
                    "op",
                    "{\"type\":\"string\",\"enum\":[\"open\",\"save\",\"info\",\"validate\",\"schema\",\"list\",\"get\",\"inspect_cell\",\"add_box\",\"add_edges\",\"set\",\"move\",\"delete\",\"cut\",\"paste\",\"layer\",\"text\",\"layout\",\"orient\",\"rotate\",\"view\",\"legend\",\"legend_set\",\"legend_add\",\"legend_rm\",\"render\",\"export\",\"undo\",\"redo\",\"transaction\"]}",
                ),
                s("args", "{\"type\":\"object\"}"),
                s("dry_run", "{\"type\":\"boolean\"}"),
            ]),
        ),
        s("additionalProperties", "true"),
    ]);
    let response = m(vec![
        s("type", "{\"type\":\"object\"}"),
        s("required", "[\"ok\",\"protocol\"]"),
        s(
            "properties",
            &m(vec![
                s("id", "{\"type\":[\"integer\",\"string\",\"null\"]}"),
                s("ok", "{\"type\":\"boolean\"}"),
                s("protocol", "{\"const\":1}"),
                s("result", "{}"),
                s(
                    "error",
                    "{\"type\":\"object\",\"required\":[\"code\",\"path\",\"message\"],\"properties\":{\"code\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"},\"message\":{\"type\":\"string\"}}}",
                ),
            ]),
        ),
        s("additionalProperties", "true"),
    ]);
    let text = format!(
        "{{\"protocol\":{PROTOCOL},\"document\":{doc},\"request\":{request},\"response\":{response},"
    );
    let color = format!("\"color\":{hex}");
    let _ = color;
    // 拼好的字符串本身是 JSON,顺手解析回来(保证输出一定是合法 JSON)
    let closed = format!("{text}\"schemas\":{{\"color\":{hex}}}}}");
    crate::json::parse(&closed).unwrap_or(Value::Null)
}

// ---- 命令行 ----

#[derive(Debug, Clone)]
pub struct Config {
    pub file: Option<String>,
    pub op: Option<String>,
    pub batch: Option<String>,
    pub save_on_exit: bool,
    pub partial: bool,
    pub limit: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            file: None,
            op: None,
            batch: None,
            save_on_exit: false,
            partial: false,
            limit: 100_000,
        }
    }
}

pub fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config {
        limit: 100_000,
        ..Default::default()
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--file" => {
                cfg.file = Some(
                    it.next()
                        .ok_or_else(|| "--file needs a path".to_string())?
                        .clone(),
                )
            }
            "--op" => {
                cfg.op = Some(
                    it.next()
                        .ok_or_else(|| "--op needs a JSON object".to_string())?
                        .clone(),
                )
            }
            "--batch" => {
                cfg.batch = Some(
                    it.next()
                        .ok_or_else(|| "--batch needs a path".to_string())?
                        .clone(),
                )
            }
            "--save-on-exit" => cfg.save_on_exit = true,
            "--partial" => cfg.partial = true,
            "--limit" => {
                let v = it
                    .next()
                    .ok_or_else(|| "--limit needs a number".to_string())?;
                cfg.limit = v.parse().map_err(|_| format!("bad --limit value {v}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(cfg)
}

/// 跑一次 headless 会话,返回进程退出码
pub fn run(cfg: &Config) -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    run_with(cfg, &mut stdin.lock(), &mut out, &mut err)
}

pub fn run_with(
    cfg: &Config,
    stdin: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // 初始文档:--file 给了就打开,否则空白文档
    let (doc, source) = match &cfg.file {
        Some(path) => {
            let path = crate::paths::expand(path);
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    let _ = writeln!(err, "cannot read {path}: {e}");
                    return Status::Io.code();
                }
            };
            let lower = path.to_ascii_lowercase();
            let doc = if lower.ends_with(".gtui") {
                crate::document::Document::load(&text)
            } else if lower.ends_with(".canvas") {
                crate::canvas_io::import(&text)
            } else {
                crate::dot::parse_document(&text, crate::document::ViewState::default(), &path)
                    .map_err(|e| format!("{path}: {e}"))
            };
            match doc {
                Ok(d) => (d, Some(path)),
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    return Status::BadDoc.code();
                }
            }
        }
        None => (crate::document::Document::scratch(), None),
    };
    let mut session = Session::new(Engine::new(doc, source), cfg.limit);
    let mut status = Status::Ok;

    // 1) 一次性请求
    if let Some(op) = &cfg.op {
        let (resp, st) = session.handle_line(op);
        let _ = writeln!(out, "{}", crate::json::write(&resp));
        let _ = out.flush();
        status = st;
    } else if let Some(path) = &cfg.batch {
        // 2) 批量:默认整批事务
        let text = match std::fs::read_to_string(crate::paths::expand(path)) {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(err, "cannot read {path}: {e}");
                return Status::Io.code();
            }
        };
        let doc_before = session.engine.doc.clone();
        let mut failed = false;
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (resp, st) = session.handle_line(line);
            let _ = writeln!(out, "{}", crate::json::write(&resp));
            if st != Status::Ok && !failed {
                failed = true;
                status = st;
                let _ = writeln!(err, "batch failed at line {}", n + 1);
                if !cfg.partial {
                    session.engine.doc = doc_before;
                    status = Status::Rollback;
                    break;
                }
            }
        }
        let _ = out.flush();
    } else {
        // 3) NDJSON:一行一个请求
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    let _ = writeln!(err, "cannot read stdin: {e}");
                    return Status::Io.code();
                }
            }
            let text = line.trim_end_matches(['\r', '\n']);
            if text.trim().is_empty() {
                continue;
            }
            let (resp, st) = session.handle_line(text);
            let _ = writeln!(out, "{}", crate::json::write(&resp));
            let _ = out.flush();
            if st != Status::Ok && status == Status::Ok {
                status = st;
            }
        }
    }

    if cfg.save_on_exit {
        match session.engine.save(None) {
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(err, "save failed: {} ({})", e.message, e.path);
                if status == Status::Ok {
                    status = if e.code == "io" {
                        Status::Io
                    } else {
                        Status::BadDoc
                    };
                }
            }
        }
    }
    status.code()
}
