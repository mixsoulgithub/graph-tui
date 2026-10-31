//! 无 TUI 的语义操作层:文档 + 撤销 + 剪贴板,全部按 id.
//!
//! headless 协议(`src/headless.rs`)只是把这层包成 JSON;这里不碰 `View`,
//! 也不碰终端. 渲染类操作直接调 `render`, 导出直接调 `svg` / `dot_write` /
//! `canvas_io`, 和 TUI 用的是同一批函数.

use crate::app::View;
use crate::app::{Clipboard, History};
use crate::document::Rgb;
use crate::document::{Box, DocEdge, Document, Group, Layer, Text};
use crate::json::Value;
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// 协议错误:机器可读的 code + 出问题的路径 + 人读的消息
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Error {
        Error {
            code,
            path: path.into(),
            message: message.into(),
        }
    }

    /// 请求本身不合法
    pub fn bad(path: &str, message: impl Into<String>) -> Error {
        Error::new("bad_arg", path, message)
    }

    /// 引用不存在(id 之类)
    pub fn missing(path: &str, message: impl Into<String>) -> Error {
        Error::new("bad_ref", path, message)
    }

    pub fn to_value(&self) -> Value {
        Value::Obj(vec![
            ("code".into(), Value::Str(self.code.into())),
            ("path".into(), Value::Str(self.path.clone())),
            ("message".into(), Value::Str(self.message.clone())),
        ])
    }
}

/// 请求与文档之间的状态. headless 会话持有一个, 跨请求保留撤销栈与剪贴板
pub struct Engine {
    pub doc: Document,
    pub undo: History,
    pub clip: Option<Clipboard>,
    pub source: Option<String>,
    /// 图例开关:`.gtui` 里不存,会话内单独记
    pub legend: bool,
    /// 注释开关(同上,会话内单独记)
    pub notes: bool,
    /// 事务里连着做几步时, 内层不再各压一步撤销
    batch: bool,
}

/// 上限:批量建边、渲染尺寸、单次请求里的 id 数
pub const MAX_EDGES: usize = 4096;
pub const MAX_RENDER: u16 = 1000;

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn hex_or_null(c: Option<Rgb>) -> Value {
    match c {
        Some(c) => Value::Str(hex(c)),
        None => Value::Null,
    }
}

/// `#rrggbb`(或 `null`/`none`/`auto`)→ 颜色
fn parse_color(v: &Value) -> Option<Option<Rgb>> {
    match v {
        Value::Null => Some(None),
        Value::Str(s) => {
            let t = s.trim().to_ascii_lowercase();
            if t == "none" || t == "auto" || t.is_empty() {
                return Some(None);
            }
            let hexpart = t.strip_prefix('#')?;
            if hexpart.len() != 6 {
                return None;
            }
            let r = u8::from_str_radix(&hexpart[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hexpart[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hexpart[4..6], 16).ok()?;
            Some(Some((r, g, b)))
        }
        _ => None,
    }
}

fn pair(v: &Value) -> Option<(i32, i32)> {
    let a = v.as_arr()?;
    if a.len() != 2 {
        return None;
    }
    Some((a[0].as_i32()?, a[1].as_i32()?))
}

fn str_list(v: &Value) -> Option<Vec<String>> {
    let a = v.as_arr()?;
    a.iter().map(|x| x.as_str().map(str::to_string)).collect()
}

impl Engine {
    pub fn new(doc: Document, source: Option<String>) -> Engine {
        Engine {
            doc,
            undo: History::new(200),
            clip: None,
            source,
            legend: true,
            notes: true,
            batch: false,
        }
    }

    // ---- 事务 ----

    /// 包一层:里面所有改动合成一步撤销
    pub fn batch<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Engine) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let before = self.doc.clone();
        let past_before = self.undo.counts().0;
        self.batch = true;
        let out = f(self);
        self.batch = false;
        match out {
            Ok(v) => {
                if self.undo.counts().0 == past_before {
                    self.undo.begin(label, &before);
                }
                self.undo.prune(&self.doc);
                Ok(v)
            }
            Err(e) => {
                self.doc = before;
                // 事务失败:把这次压的快照丢掉
                while self.undo.counts().0 > past_before {
                    self.undo.drop_last();
                }
                Err(e)
            }
        }
    }

    fn begin(&mut self, label: &str) {
        if !self.batch {
            self.undo.begin(label, &self.doc);
        }
    }

    fn end(&mut self) {
        if !self.batch {
            self.undo.prune(&self.doc);
        }
    }

    // ---- 文档 ----

    pub fn open(&mut self, path: &str) -> Result<Value, Error> {
        let path = crate::paths::expand(path);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new("io", "path", format!("cannot read {path}: {e}")))?;
        let lower = path.to_ascii_lowercase();
        let doc = if lower.ends_with(".gtui") {
            Document::load(&text).map_err(|e| Error::new("bad_doc", "document", e))?
        } else if lower.ends_with(".canvas") {
            crate::canvas_io::import(&text).map_err(|e| Error::new("bad_doc", "document", e))?
        } else if lower.ends_with(".dot") || lower.ends_with(".gv") {
            crate::dot::parse_document(&text, crate::document::ViewState::default(), &path)
                .map_err(|e| Error::new("bad_doc", "document", e))?
        } else {
            return Err(Error::bad(
                "path",
                format!("unsupported extension: {path} (want .gtui/.dot/.canvas)"),
            ));
        };
        self.doc = doc;
        self.source = Some(path);
        self.undo = History::new(200);
        self.clip = None;
        Ok(self.info())
    }

    pub fn save(&mut self, path: Option<&str>) -> Result<Value, Error> {
        let target = match path {
            Some(p) => p.trim().to_string(),
            None => self.source.clone().ok_or_else(|| {
                Error::bad("path", "no file name yet: pass a path ending in .gtui")
            })?,
        };
        let target = crate::paths::expand(&target);
        if !target.to_ascii_lowercase().ends_with(".gtui") {
            return Err(Error::bad(
                "path",
                format!("only .gtui is written, not {target}"),
            ));
        }
        self.doc
            .validate()
            .map_err(|e| Error::new("bad_doc", "document", e))?;
        self.doc.source = format!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        let body = self.doc.save();
        crate::paths::write_atomic(&target, &body).map_err(|e| Error::new("io", "path", e))?;
        self.source = Some(target.clone());
        Ok(obj(vec![("path", Value::Str(target))]))
    }

    pub fn info(&self) -> Value {
        let (x0, y0, x1, y1) = self.doc.bbox();
        let layers: Vec<Value> = self
            .doc
            .layers
            .iter()
            .map(|l| {
                obj(vec![
                    ("name", Value::Str(l.name.clone())),
                    ("order", Value::Int(l.order as i64)),
                ])
            })
            .collect();
        let groups: Vec<Value> = self
            .doc
            .groups
            .iter()
            .map(|g| {
                obj(vec![
                    ("id", Value::Str(g.id.clone())),
                    ("label", Value::Str(g.label.clone())),
                    ("color", Value::Str(hex(g.color))),
                ])
            })
            .collect();
        obj(vec![
            ("boxes", Value::Int(self.doc.boxes.len() as i64)),
            ("edges", Value::Int(self.doc.edges.len() as i64)),
            ("texts", Value::Int(self.doc.texts.len() as i64)),
            (
                "bbox",
                Value::Arr(vec![
                    Value::Int(x0 as i64),
                    Value::Int(y0 as i64),
                    Value::Int(x1 as i64),
                    Value::Int(y1 as i64),
                ]),
            ),
            (
                "source",
                self.source.clone().map(Value::Str).unwrap_or(Value::Null),
            ),
            ("layers", Value::Arr(layers)),
            ("groups", Value::Arr(groups)),
            ("view", view_to_value(&self.doc.view)),
            (
                "legend",
                obj(vec![
                    (
                        "title",
                        self.doc
                            .legend
                            .title
                            .clone()
                            .map(Value::Str)
                            .unwrap_or(Value::Null),
                    ),
                    ("entries", Value::Int(self.doc.legend.entries.len() as i64)),
                ]),
            ),
        ])
    }

    pub fn validate(&self) -> Result<Value, Error> {
        match self.doc.validate() {
            Ok(()) => Ok(obj(vec![
                ("ok", Value::Bool(true)),
                ("errors", Value::Arr(vec![])),
            ])),
            Err(e) => {
                let (path, message) = split_validation(&e);
                Err(Error {
                    code: "bad_doc",
                    path,
                    message,
                })
            }
        }
    }

    pub fn list(&self, req: &Value) -> Result<Value, Error> {
        let kind = req.get("kind").and_then(Value::as_str).unwrap_or("boxes");
        let layer = req.get("layer").and_then(Value::as_str);
        let group = req.get("group").and_then(Value::as_str);
        let ids = req.get("ids").and_then(str_list);
        let keep = |id: &str| {
            ids.as_ref()
                .map(|v| v.iter().any(|x| x == id))
                .unwrap_or(true)
        };
        let out: Vec<Value> = match kind {
            "boxes" => self
                .doc
                .boxes
                .iter()
                .filter(|b| keep(&b.id))
                .filter(|b| layer.map(|l| b.layer == l).unwrap_or(true))
                .map(|b| self.box_value(b, false))
                .collect(),
            "edges" => self
                .doc
                .edges
                .iter()
                .filter(|e| keep(&e.id))
                .filter(|e| layer.map(|l| e.layer == l).unwrap_or(true))
                .filter(|e| group.map(|g| e.group == g).unwrap_or(true))
                .map(edge_value)
                .collect(),
            "groups" => self
                .doc
                .groups
                .iter()
                .filter(|g| keep(&g.id))
                .map(|g| {
                    obj(vec![
                        ("id", Value::Str(g.id.clone())),
                        ("label", Value::Str(g.label.clone())),
                        ("color", Value::Str(hex(g.color))),
                    ])
                })
                .collect(),
            "layers" => self
                .doc
                .layers
                .iter()
                .filter(|l| keep(&l.name))
                .map(|l| {
                    obj(vec![
                        ("name", Value::Str(l.name.clone())),
                        ("order", Value::Int(l.order as i64)),
                    ])
                })
                .collect(),
            other => {
                return Err(Error::bad(
                    "kind",
                    format!("unknown kind {other} (boxes/edges/groups/layers)"),
                ))
            }
        };
        let n = out.len();
        Ok(obj(vec![
            ("kind", Value::Str(kind.to_string())),
            ("count", Value::Int(n as i64)),
            ("items", Value::Arr(out)),
        ]))
    }

    pub fn get(&self, id: &str) -> Result<Value, Error> {
        if let Some(i) = self.doc.box_idx(id) {
            return Ok(self.box_value(&self.doc.boxes[i], true));
        }
        if let Some(i) = self.doc.edges.iter().position(|e| e.id == id) {
            return Ok(edge_value(&self.doc.edges[i]));
        }
        if let Some(g) = self.doc.groups.iter().find(|g| g.id == id) {
            return Ok(obj(vec![
                ("type", Value::Str("group".into())),
                ("id", Value::Str(g.id.clone())),
                ("label", Value::Str(g.label.clone())),
                ("color", Value::Str(hex(g.color))),
            ]));
        }
        if self.doc.layers.iter().any(|l| l.name == id) {
            let order = self.doc.layer_order(id);
            return Ok(obj(vec![
                ("type", Value::Str("layer".into())),
                ("name", Value::Str(id.to_string())),
                ("order", Value::Int(order as i64)),
            ]));
        }
        Err(Error::missing("id", format!("no such id: {id}")))
    }

    /// 某一格下面是什么(按 1:1 渲染一帧再查命中表)
    pub fn inspect_cell(&self, cell: (i32, i32)) -> Result<Value, Error> {
        let (w, h) = self.render_size();
        let area = Rect::new(0, 0, w, h);
        let view = View {
            drawn: false,
            ..Default::default()
        };
        let mut buf = Buffer::empty(area);
        let (p, hit) = crate::render::render_hit(
            &mut buf,
            area,
            &self.doc,
            &view,
            &crate::render::FrameHit::default(),
        );
        let screen = p.proj.cell((cell.0 as f64, cell.1 as f64));
        let mut out = obj(vec![(
            "cell",
            Value::Arr(vec![Value::Int(cell.0 as i64), Value::Int(cell.1 as i64)]),
        )]);
        let mut cands: Vec<Value> = Vec::new();
        for i in self.doc.boxes_at_cell(cell) {
            cands.push(obj(vec![
                ("type", Value::Str("box".into())),
                ("id", Value::Str(self.doc.boxes[i].id.clone())),
            ]));
        }
        for e in hit.candidates_at(screen).iter().rev() {
            if let Some(edge) = self.doc.edges.get(*e) {
                cands.push(obj(vec![
                    ("type", Value::Str("edge".into())),
                    ("id", Value::Str(edge.id.clone())),
                ]));
            }
        }
        if cands.is_empty() {
            if let Some(e) = hit.edge_at(screen).and_then(|e| self.doc.edges.get(e)) {
                cands.push(obj(vec![
                    ("type", Value::Str("edge".into())),
                    ("id", Value::Str(e.id.clone())),
                ]));
            }
        }
        let pick = cands.first().cloned().unwrap_or(Value::Null);
        out = match out {
            Value::Obj(mut f) => {
                f.push(("pick".into(), pick));
                f.push(("candidates".into(), Value::Arr(cands)));
                Value::Obj(f)
            }
            other => other,
        };
        Ok(out)
    }

    // ---- 编辑 ----

    pub fn add_box(&mut self, req: &Value) -> Result<Value, Error> {
        let label = req.get("label").and_then(Value::as_str).unwrap_or("");
        let at = req.get("at").and_then(pair).unwrap_or((0, 0));
        let style = match req.get("style").and_then(Value::as_str) {
            None => BoxStyle::default(),
            Some(s) => BoxStyle::parse(s)
                .ok_or_else(|| Error::bad("style", format!("unknown box style {s}")))?,
        };
        self.begin("add_box");
        let i = self
            .doc
            .add_box(label, at, style)
            .map_err(|e| Error::new("bad_arg", "label", e))?;
        let id = self.doc.boxes[i].id.clone();
        if let Some(size) = req.get("size").and_then(pair) {
            self.doc.boxes[i].size = (size.0.max(1) as u16, size.1.max(1) as u16);
        }
        if let Some(v) = req.get("color") {
            self.doc.boxes[i].color = parse_color(v)
                .ok_or_else(|| Error::bad("color", "colour must be #rrggbb or null"))?;
        }
        if let Some(v) = req.get("fill") {
            self.doc.boxes[i].fill =
                parse_color(v).ok_or_else(|| Error::bad("fill", "fill must be #rrggbb or null"))?;
        }
        if let Some(l) = req.get("layer").and_then(Value::as_str) {
            self.ensure_layer(l);
            self.doc.boxes[i].layer = l.to_string();
        }
        if let Some(terms) = req.get("terms").and_then(str_list) {
            self.doc.boxes[i].terms = terms;
        }
        if let Some(t) = req.get("textcolor") {
            let c = parse_color(t)
                .ok_or_else(|| Error::bad("textcolor", "colour must be #rrggbb or null"))?;
            if let Some(ti) = self.doc.text_of(i) {
                self.doc.texts[ti].color = c;
            }
        }
        if let Some(a) = req.get("align").and_then(Value::as_str) {
            let al =
                Align::parse(a).ok_or_else(|| Error::bad("align", format!("unknown align {a}")))?;
            if let Some(ti) = self.doc.text_of(i) {
                self.doc.texts[ti].align = al;
            }
        }
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![
            ("id", Value::Str(id)),
            ("box", self.get(&self.doc.boxes[i].id.clone())?),
        ]))
    }

    pub fn add_edges(&mut self, req: &Value) -> Result<Value, Error> {
        let sources = req
            .get("sources")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("sources", "sources must be an array of box ids"))?;
        let targets = req
            .get("targets")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("targets", "targets must be an array of box ids"))?;
        let group = req
            .get("group")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let dir = match req.get("dir").and_then(Value::as_str) {
            None => Dir::Forward,
            Some(d) => {
                Dir::parse(d).ok_or_else(|| Error::bad("dir", format!("unknown dir {d}")))?
            }
        };
        if sources.len() * targets.len() > MAX_EDGES {
            return Err(Error::new(
                "limit",
                "sources",
                format!(
                    "{} x {} edges exceeds {MAX_EDGES}",
                    sources.len(),
                    targets.len()
                ),
            ));
        }
        let group = if group.is_empty() {
            self.doc
                .groups
                .first()
                .map(|g| g.id.clone())
                .ok_or_else(|| Error::missing("group", "the document has no group"))?
        } else {
            if self.doc.group_idx(&group).is_none() {
                return Err(Error::missing("group", format!("no such group: {group}")));
            }
            group
        };
        // 端点必须都存在:引用错了是硬错误(事务要能整组回滚)
        for (k, id) in sources.iter().enumerate() {
            if self.doc.box_idx(id).is_none() {
                return Err(Error {
                    code: "bad_ref",
                    path: format!("sources[{k}]"),
                    message: format!("no such box: {id}"),
                });
            }
        }
        for (k, id) in targets.iter().enumerate() {
            if self.doc.box_idx(id).is_none() {
                return Err(Error {
                    code: "bad_ref",
                    path: format!("targets[{k}]"),
                    message: format!("no such box: {id}"),
                });
            }
        }
        self.begin("add_edges");
        let mut ids = Vec::new();
        let mut skipped = Vec::new();
        for s in &sources {
            for t in &targets {
                match self.doc.add_edge(s, t, &group, dir) {
                    Ok(i) => ids.push(Value::Str(self.doc.edges[i].id.clone())),
                    Err(e) => skipped.push(obj(vec![
                        ("from", Value::Str(s.clone())),
                        ("to", Value::Str(t.clone())),
                        ("reason", Value::Str(e)),
                    ])),
                }
            }
        }
        self.end();
        Ok(obj(vec![
            ("ids", Value::Arr(ids)),
            ("skipped", Value::Arr(skipped)),
        ]))
    }

    /// 图例读出来(标题 + 逐条)
    pub fn legend(&self) -> Value {
        obj(vec![
            (
                "title",
                self.doc
                    .legend
                    .title
                    .clone()
                    .map(Value::Str)
                    .unwrap_or(Value::Null),
            ),
            (
                "entries",
                Value::Arr(
                    self.doc
                        .legend
                        .entries
                        .iter()
                        .map(legend_entry_value)
                        .collect(),
                ),
            ),
        ])
    }

    /// 整块替换图例(`legend_set`):`title` 可省,`entries` 整份覆盖
    pub fn legend_set(&mut self, req: &Value) -> Result<Value, Error> {
        let mut next = crate::document::Legend::default();
        match req.get("title") {
            None | Some(Value::Null) => {}
            Some(Value::Str(t)) => next.title = Some(t.clone()),
            Some(_) => return Err(Error::bad("title", "title must be a string or null")),
        }
        if let Some(v) = req.get("entries") {
            let list = match v.as_arr() {
                Some(a) => a,
                None => return Err(Error::bad("entries", "entries must be an array")),
            };
            for (i, e) in list.iter().enumerate() {
                next.entries.push(self.legend_entry(e, i)?);
            }
        }
        if next == self.doc.legend {
            return Ok(obj(vec![("changed", Value::Bool(false))]));
        }
        self.begin("legend_set");
        self.doc.legend = next;
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![
            ("changed", Value::Bool(true)),
            ("entries", Value::Int(self.doc.legend.entries.len() as i64)),
        ]))
    }

    /// 加一行(`legend_add`):`text` 必给,`group`/`color`/`glyph` 可选
    pub fn legend_add(&mut self, req: &Value) -> Result<Value, Error> {
        let entry = self.legend_entry(req, 0)?;
        self.begin("legend_add");
        self.doc.legend.entries.push(entry);
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![
            ("index", Value::Int(self.doc.legend.entries.len() as i64)),
            ("count", Value::Int(self.doc.legend.entries.len() as i64)),
        ]))
    }

    /// 删一行(`legend_rm`),`index` 从 1 数,和界面一致
    pub fn legend_rm(&mut self, req: &Value) -> Result<Value, Error> {
        let n = req
            .get("index")
            .and_then(Value::as_i32)
            .ok_or_else(|| Error::bad("index", "index is required"))?;
        let total = self.doc.legend.entries.len();
        if n < 1 || n as usize > total {
            return Err(Error::bad(
                "index",
                format!("index must be 1..={total}, got {n}"),
            ));
        }
        self.begin("legend_rm");
        self.doc.legend.entries.remove(n as usize - 1);
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![
            ("removed", Value::Int(n as i64)),
            ("count", Value::Int(self.doc.legend.entries.len() as i64)),
        ]))
    }

    /// 一条图例行:文本 + 可选的 group / color / glyph
    fn legend_entry(&self, v: &Value, i: usize) -> Result<crate::document::LegendEntry, Error> {
        let path = |k: &str| format!("entries[{i}].{k}");
        let text = v
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::bad(&path("text"), "text is required"))?
            .to_string();
        let group = match v.get("group") {
            None | Some(Value::Null) => None,
            Some(g) => {
                let id = g
                    .as_str()
                    .ok_or_else(|| Error::bad(&path("group"), "group must be a string"))?;
                if self.doc.group_idx(id).is_none() {
                    return Err(Error::bad(
                        &path("group"),
                        format!("group {id:?} does not exist"),
                    ));
                }
                Some(id.to_string())
            }
        };
        let color = match v.get("color") {
            None | Some(Value::Null) => None,
            Some(_) => parse_color(v.get("color").unwrap())
                .ok_or_else(|| Error::bad(&path("color"), "colour must be #rrggbb or null"))?,
        };
        let glyph = match v.get("glyph") {
            None | Some(Value::Null) => None,
            Some(g) => {
                let name = g
                    .as_str()
                    .ok_or_else(|| Error::bad(&path("glyph"), "glyph must be a string"))?;
                Some(
                    crate::document::LegendGlyph::from_name(name).ok_or_else(|| {
                        Error::bad(
                            &path("glyph"),
                            format!("unknown glyph {name} (solid/dashed/dotted/arrow)"),
                        )
                    })?,
                )
            }
        };
        Ok(crate::document::LegendEntry {
            text,
            group,
            color,
            glyph,
        })
    }

    pub fn set(&mut self, req: &Value) -> Result<Value, Error> {
        let ids = req
            .get("ids")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("ids", "ids must be an array"))?;
        let attr = req
            .get("attr")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::bad("attr", "attr is required"))?;
        let value = req
            .get("value")
            .ok_or_else(|| Error::bad("value", "value is required"))?;
        if ids.len() > MAX_EDGES {
            return Err(Error::new(
                "limit",
                "ids",
                format!("too many ids (max {MAX_EDGES})"),
            ));
        }
        self.begin(&format!("set {attr}"));
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        for id in &ids {
            let touched = self.apply_attr(id, attr, value)?;
            if touched {
                changed.push(Value::Str(id.clone()));
            } else {
                unchanged.push(Value::Str(id.clone()));
            }
        }
        self.end();
        Ok(obj(vec![
            ("changed", Value::Arr(changed)),
            ("unchanged", Value::Arr(unchanged)),
        ]))
    }

    fn apply_attr(&mut self, id: &str, attr: &str, value: &Value) -> Result<bool, Error> {
        let path = format!("ids[{id}].{attr}");
        if let Some(i) = self.doc.box_idx(id) {
            return match attr {
                "style" | "box" => {
                    let s = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "style must be a string"))?;
                    let st = BoxStyle::parse(s)
                        .ok_or_else(|| Error::bad(&path, format!("unknown box style {s}")))?;
                    if self.doc.boxes[i].style == st {
                        return Ok(false);
                    }
                    self.doc.boxes[i].style = st;
                    self.doc.fit_box(i);
                    self.doc.rebuild();
                    Ok(true)
                }
                "color" | "border" | "nodecolor" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "colour must be #rrggbb or null"))?;
                    if self.doc.boxes[i].color == c {
                        return Ok(false);
                    }
                    self.doc.boxes[i].color = c;
                    Ok(true)
                }
                "fill" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "fill must be #rrggbb or null"))?;
                    if self.doc.boxes[i].fill == c {
                        return Ok(false);
                    }
                    self.doc.boxes[i].fill = c;
                    Ok(true)
                }
                "layer" => {
                    let l = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "layer must be a string"))?;
                    if self.doc.boxes[i].layer == l {
                        return Ok(false);
                    }
                    self.ensure_layer(l);
                    self.doc.boxes[i].layer = l.to_string();
                    Ok(true)
                }
                "terms" => {
                    let t = str_list(value)
                        .ok_or_else(|| Error::bad(&path, "terms must be an array of strings"))?;
                    if self.doc.boxes[i].terms == t {
                        return Ok(false);
                    }
                    self.doc.boxes[i].terms = t;
                    Ok(true)
                }
                // 注释框绑定:只有 none 框能绑,边必须存在
                "edge" | "note_of" => {
                    let id = match value {
                        Value::Null => None,
                        Value::Str(s) => {
                            if !self.doc.edges.iter().any(|e| &e.id == s) {
                                return Err(Error::bad(&path, format!("no such edge {s:?}")));
                            }
                            Some(s.clone())
                        }
                        _ => return Err(Error::bad(&path, "edge must be an edge id or null")),
                    };
                    if id.is_some() && self.doc.boxes[i].style != crate::model::BoxStyle::None {
                        return Err(Error::bad(&path, "only a text-only (none) box can hold a note"));
                    }
                    if id.is_some()
                        && self
                            .doc
                            .boxes
                            .iter()
                            .enumerate()
                            .any(|(j, b)| j != i && b.edge == id)
                    {
                        return Err(Error::bad(&path, "that edge already has a note box"));
                    }
                    if self.doc.boxes[i].edge == id {
                        return Ok(false);
                    }
                    self.doc.boxes[i].edge = id;
                    Ok(true)
                }
                "text" => {
                    let t = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "text must be a string"))?;
                    let same = self.doc.text(i).map(|x| x.text == t).unwrap_or(false);
                    if same {
                        return Ok(false);
                    }
                    self.doc.set_text(i, t);
                    Ok(true)
                }
                "textcolor" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "colour must be #rrggbb or null"))?;
                    let Some(ti) = self.doc.text_of(i) else {
                        return Ok(false);
                    };
                    if self.doc.texts[ti].color == c {
                        return Ok(false);
                    }
                    self.doc.texts[ti].color = c;
                    Ok(true)
                }
                "textbg" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "colour must be #rrggbb or null"))?;
                    let Some(ti) = self.doc.text_of(i) else {
                        return Ok(false);
                    };
                    if self.doc.texts[ti].bg == c {
                        return Ok(false);
                    }
                    self.doc.texts[ti].bg = c;
                    Ok(true)
                }
                "align" | "textalign" => {
                    let s = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "align must be a string"))?;
                    let a = Align::parse(s)
                        .ok_or_else(|| Error::bad(&path, format!("unknown align {s}")))?;
                    let Some(ti) = self.doc.text_of(i) else {
                        return Ok(false);
                    };
                    if self.doc.texts[ti].align == a {
                        return Ok(false);
                    }
                    self.doc.texts[ti].align = a;
                    Ok(true)
                }
                other => Err(Error::bad(&path, format!("unknown box attribute {other}"))),
            };
        }
        if let Some(e) = self.doc.edges.iter().position(|e| e.id == id) {
            return match attr {
                "style" => {
                    let s = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "style must be a string"))?;
                    let st = EdgeStyle::parse(s)
                        .ok_or_else(|| Error::bad(&path, format!("unknown line style {s}")))?;
                    if self.doc.edges[e].style == st {
                        return Ok(false);
                    }
                    self.doc.edges[e].style = st;
                    Ok(true)
                }
                "dir" => {
                    let s = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "dir must be a string"))?;
                    let d = Dir::parse(s)
                        .ok_or_else(|| Error::bad(&path, format!("unknown dir {s}")))?;
                    if self.doc.edges[e].dir == d {
                        return Ok(false);
                    }
                    self.doc.edges[e].dir = d;
                    Ok(true)
                }
                "corner" => {
                    let s = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "corner must be a string"))?;
                    let c = Corner::parse(s)
                        .ok_or_else(|| Error::bad(&path, format!("unknown corner {s}")))?;
                    if self.doc.edges[e].corner == c {
                        return Ok(false);
                    }
                    self.doc.edges[e].corner = c;
                    Ok(true)
                }
                "color" | "edgecolor" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "colour must be #rrggbb or null"))?;
                    if self.doc.edges[e].color == c {
                        return Ok(false);
                    }
                    self.doc.edges[e].color = c;
                    Ok(true)
                }
                "bg" | "edgebg" => {
                    let c = parse_color(value)
                        .ok_or_else(|| Error::bad(&path, "colour must be #rrggbb or null"))?;
                    if self.doc.edges[e].bg == c {
                        return Ok(false);
                    }
                    self.doc.edges[e].bg = c;
                    Ok(true)
                }
                "group" => {
                    let g = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "group must be a group id"))?;
                    if self.doc.group_idx(g).is_none() {
                        return Err(Error::missing(&path, format!("no such group: {g}")));
                    }
                    if self.doc.edges[e].group == g {
                        return Ok(false);
                    }
                    self.doc.edges[e].group = g.to_string();
                    Ok(true)
                }
                "layer" => {
                    let l = value
                        .as_str()
                        .ok_or_else(|| Error::bad(&path, "layer must be a string"))?;
                    if self.doc.edges[e].layer == l {
                        return Ok(false);
                    }
                    self.ensure_layer(l);
                    self.doc.edges[e].layer = l.to_string();
                    Ok(true)
                }
                "via" => {
                    let pts = match value {
                        Value::Null => Vec::new(),
                        Value::Arr(a) => {
                            let mut out = Vec::new();
                            for (k, p) in a.iter().enumerate() {
                                let q = pair(p).ok_or_else(|| {
                                    Error::bad(&path, format!("via[{k}] must be [x, y]"))
                                })?;
                                out.push(q);
                            }
                            out
                        }
                        _ => {
                            return Err(Error::bad(&path, "via must be an array of [x, y] or null"))
                        }
                    };
                    if self.doc.edges[e].via == pts {
                        return Ok(false);
                    }
                    self.doc.edges[e].via = pts;
                    Ok(true)
                }
                // 边注释:空串/null = 删掉注释框,否则新建或改写那个框
                "note" | "label" | "elabel" => {
                    let text = match value {
                        Value::Null => String::new(),
                        Value::Str(t) => {
                            if t.contains('\n') || t.contains('\r') || t.contains('\t') {
                                return Err(Error::bad(&path, "note must be a single line"));
                            }
                            t.clone()
                        }
                        _ => return Err(Error::bad(&path, "note must be a string or null")),
                    };
                    let before = self
                        .doc
                        .note_of_edge(&self.doc.edges[e].id)
                        .and_then(|b| self.doc.text(b).map(|t| t.text.clone()))
                        .unwrap_or_default();
                    if before == text {
                        return Ok(false);
                    }
                    self.doc.set_note(e, &text);
                    Ok(true)
                }
                other => Err(Error::bad(&path, format!("unknown edge attribute {other}"))),
            };
        }
        Err(Error::missing("ids", format!("no such box or edge: {id}")))
    }

    pub fn move_ids(&mut self, req: &Value) -> Result<Value, Error> {
        let ids = req
            .get("ids")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("ids", "ids must be an array"))?;
        let at = req.get("at").and_then(pair);
        let (dx, dy) = match (at, req.get("dx"), req.get("dy")) {
            (Some(_), _, _) => (0, 0),
            _ => (
                req.get("dx").and_then(Value::as_i32).unwrap_or(0),
                req.get("dy").and_then(Value::as_i32).unwrap_or(0),
            ),
        };
        if at.is_some() && ids.len() != 1 {
            return Err(Error::bad("at", "at only works with a single id"));
        }
        self.begin("move");
        let mut moved = Vec::new();
        for id in &ids {
            let Some(i) = self.doc.box_idx(id) else {
                self.undo.prune(&self.doc);
                return Err(Error::missing("ids", format!("no such box: {id}")));
            };
            match at {
                Some(p) => {
                    let cur = self.doc.boxes[i].at;
                    self.doc.move_box(i, p.0 - cur.0, p.1 - cur.1);
                }
                None => self.doc.move_box(i, dx, dy),
            }
            moved.push(Value::Str(id.clone()));
        }
        self.end();
        Ok(obj(vec![("moved", Value::Arr(moved))]))
    }

    pub fn layer(&mut self, req: &Value) -> Result<Value, Error> {
        let ids = req
            .get("ids")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("ids", "ids must be an array"))?;
        let name = req
            .get("layer")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::bad("layer", "layer is required"))?;
        self.begin("layer");
        self.ensure_layer(name);
        let mut moved = Vec::new();
        for id in &ids {
            if let Some(i) = self.doc.box_idx(id) {
                self.doc.boxes[i].layer = name.to_string();
                if let Some(ti) = self.doc.text_of(i) {
                    self.doc.texts[ti].layer = name.to_string();
                }
                moved.push(Value::Str(id.clone()));
            } else if let Some(e) = self.doc.edges.iter().position(|e| e.id == *id) {
                self.doc.edges[e].layer = name.to_string();
                moved.push(Value::Str(id.clone()));
            } else {
                self.undo.prune(&self.doc);
                return Err(Error::missing("ids", format!("no such id: {id}")));
            }
        }
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![("moved", Value::Arr(moved))]))
    }

    pub fn text(&mut self, req: &Value) -> Result<Value, Error> {
        let ids = req
            .get("ids")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("ids", "ids must be an array"))?;
        let text = req
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::bad("text", "text is required"))?;
        self.begin("text");
        let mut changed = Vec::new();
        for id in &ids {
            let Some(i) = self.doc.box_idx(id) else {
                self.undo.prune(&self.doc);
                return Err(Error::missing("ids", format!("no such box: {id}")));
            };
            if self.doc.text(i).map(|t| t.text == text).unwrap_or(false) {
                continue;
            }
            self.doc.set_text(i, text);
            changed.push(Value::Str(id.clone()));
        }
        self.end();
        Ok(obj(vec![("changed", Value::Arr(changed))]))
    }

    // ---- 删除 / 剪贴板 ----

    /// 删掉这些 id,返回可移植的快照(和 TUI 的 `d` 同一个结构)
    pub fn cut(&mut self, req: &Value) -> Result<Value, Error> {
        let ids = req
            .get("ids")
            .and_then(str_list)
            .ok_or_else(|| Error::bad("ids", "ids must be an array"))?;
        if ids.is_empty() {
            return Err(Error::bad("ids", "ids is empty"));
        }
        if ids.len() > MAX_EDGES {
            return Err(Error::new(
                "limit",
                "ids",
                format!("too many ids (max {MAX_EDGES})"),
            ));
        }
        let mut boxes = Vec::new();
        let mut edges = Vec::new();
        for id in &ids {
            if let Some(i) = self.doc.box_idx(id) {
                boxes.push(i);
            } else if let Some(e) = self.doc.edges.iter().position(|e| e.id == *id) {
                edges.push(e);
            } else {
                return Err(Error::missing("ids", format!("no such id: {id}")));
            }
        }
        if !boxes.is_empty() && boxes.len() >= self.doc.boxes.len() {
            return Err(Error::bad(
                "ids",
                "cannot remove every box (the document would be empty)",
            ));
        }
        let mut tx = Vec::new(); // 要删掉的边(显式 + 沾到被删框的)
        for &e in &edges {
            tx.push(e);
        }
        let names: std::collections::HashSet<&str> = boxes
            .iter()
            .map(|&i| self.doc.boxes[i].id.as_str())
            .collect();
        for (e, edge) in self.doc.edges.iter().enumerate() {
            if (names.contains(edge.from.as_str()) || names.contains(edge.to.as_str()))
                && !tx.contains(&e)
            {
                tx.push(e);
            }
        }
        self.begin("cut");
        let mut clip_boxes = Vec::new();
        let mut clip_texts = Vec::new();
        let mut clip_edges = Vec::new();
        let mut clip_groups: Vec<Group> = Vec::new();
        let mut origin = self.doc.bbox();
        origin = (origin.0, origin.1, 0, 0);
        let mut first = true;
        for &i in &boxes {
            let b = self.doc.boxes[i].clone();
            if first {
                origin.0 = b.at.0;
                origin.1 = b.at.1;
                first = false;
            } else {
                origin.0 = origin.0.min(b.at.0);
                origin.1 = origin.1.min(b.at.1);
            }
            if let Some(t) = self.doc.text(i) {
                clip_texts.push(t.clone());
            }
            clip_boxes.push(b);
        }
        for &e in &tx {
            let edge = self.doc.edges[e].clone();
            if let Some(g) = self.doc.groups.iter().find(|g| g.id == edge.group) {
                if !clip_groups.iter().any(|x: &Group| x.id == g.id) {
                    clip_groups.push(g.clone());
                }
            }
            clip_edges.push(edge);
        }
        // 先把 id 抄下来再删:删一条后面下标就全移了
        let edge_ids: Vec<String> = tx.iter().map(|&e| self.doc.edges[e].id.clone()).collect();
        for id in edge_ids {
            let _ = self.doc.remove_edge(&id);
        }
        for &i in boxes.iter().rev() {
            let id = self.doc.boxes[i].id.clone();
            let _ = self.doc.remove_box(&id);
        }
        self.end();
        let clip = Clipboard {
            origin: (origin.0, origin.1),
            boxes: clip_boxes,
            texts: clip_texts,
            edges: clip_edges,
            groups: clip_groups,
        };
        let snapshot = clipboard_to_value(&clip);
        self.clip = Some(clip);
        Ok(obj(vec![
            ("deleted_boxes", Value::Int(boxes.len() as i64)),
            ("deleted_edges", Value::Int(tx.len() as i64)),
            ("snapshot", snapshot),
        ]))
    }

    /// 粘一份新的:新 id,按 `at` 定位(给的是快照原点的落点)
    pub fn paste(&mut self, req: &Value) -> Result<Value, Error> {
        let snap = req
            .get("snapshot")
            .or_else(|| req.get("clip"))
            .ok_or_else(|| Error::bad("snapshot", "snapshot is required (use cut's result)"))?;
        let clip = clipboard_from_value(snap).map_err(|e| Error::bad("snapshot", e))?;
        let at = req.get("at").and_then(pair);
        let (dx, dy) = match at {
            Some(p) => (p.0 - clip.origin.0, p.1 - clip.origin.1),
            None => (0, 0),
        };
        self.begin("paste");
        match paste_clip(&mut self.doc, &clip, (dx, dy)) {
            Ok(out) => {
                self.end();
                Ok(out)
            }
            Err(e) => {
                self.undo.prune(&self.doc);
                Err(e)
            }
        }
    }

    // ---- 布局 / 方向 / 视图 ----

    pub fn layout(&mut self, req: &Value) -> Result<Value, Error> {
        let name = req
            .get("layout")
            .and_then(Value::as_str)
            .or_else(|| req.get("name").and_then(Value::as_str))
            .ok_or_else(|| Error::bad("layout", "layout is required"))?;
        let l = crate::layout::Layout::parse(name)
            .ok_or_else(|| Error::bad("layout", format!("unknown layout {name}")))?;
        self.begin("layout");
        let before: Vec<(i32, i32)> = self.doc.boxes.iter().map(|b| b.at).collect();
        crate::layout::apply(&mut self.doc, l).map_err(|e| Error::new("bad_doc", "layout", e))?;
        if let Some(o) = req.get("orientation") {
            self.apply_orientation(o)?;
        }
        let moved = before
            .iter()
            .zip(self.doc.boxes.iter())
            .filter(|(a, b)| **a != b.at)
            .count();
        self.doc.rebuild();
        self.end();
        Ok(obj(vec![
            ("layout", Value::Str(l.name().to_string())),
            ("moved", Value::Int(moved as i64)),
        ]))
    }

    fn apply_orientation(&mut self, v: &Value) -> Result<(), Error> {
        let o = match v {
            Value::Str(s) => crate::layout::Orientation::parse(s),
            Value::Int(i) => crate::layout::Orientation::parse(&i.to_string()),
            Value::Num(n) => crate::layout::Orientation::parse(&format!("{}", *n as i64)),
            _ => None,
        }
        .ok_or_else(|| Error::bad("orientation", "orientation is auto|0|90|180|270"))?;
        self.doc.view.orientation = o.name().to_string();
        Ok(())
    }

    /// 只改方向, 不动坐标
    pub fn orient(&mut self, req: &Value) -> Result<Value, Error> {
        let v = req
            .get("orientation")
            .or_else(|| req.get("value"))
            .ok_or_else(|| Error::bad("orientation", "orientation is required"))?;
        self.apply_orientation(v)?;
        Ok(obj(vec![(
            "orientation",
            Value::Str(self.doc.view.orientation.clone()),
        )]))
    }

    /// 把坐标整体转 90 度(和 TUI 的 :rotate 一致)
    pub fn rotate(&mut self, req: &Value) -> Result<Value, Error> {
        let q = req.get("quarter").and_then(Value::as_i32).unwrap_or(1);
        self.begin("rotate");
        self.doc.rotate_quarters(q);
        self.end();
        Ok(obj(vec![
            ("quarter", Value::Int(q as i64)),
            ("boxes", Value::Int(self.doc.boxes.len() as i64)),
        ]))
    }

    pub fn view(&mut self, req: &Value) -> Result<Value, Error> {
        if let Some(c) = req.get("center") {
            if matches!(c, Value::Null) {
                // 文档里的 center 是个点,没有"空";给 null 就回到包围盒中心
                let (x0, y0, x1, y1) = self.doc.bbox_shown(self.notes);
                self.doc.view.center = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
            } else {
                let p = pair(c).ok_or_else(|| Error::bad("center", "center must be [x, y]"))?;
                self.doc.view.center = (p.0 as f64, p.1 as f64);
            }
        }
        if let Some(z) = req.get("zoom") {
            let z = z
                .as_f64()
                .ok_or_else(|| Error::bad("zoom", "zoom must be a number"))?;
            if !(0.05..=64.0).contains(&z) {
                return Err(Error::bad("zoom", "zoom must be within 0.05..64"));
            }
            self.doc.view.zoom = z;
        }
        if let Some(b) = req.get("legend") {
            // 图例开关是视图状态:内存里立刻生效,存盘时跟着 view 一起写
            let on = b.as_bool().unwrap_or(false);
            self.legend = on;
            self.doc.view.legend = on;
        }
        if let Some(b) = req.get("labels") {
            self.doc.view.labels = b.as_bool().unwrap_or(false);
        }
        if let Some(v) = req.get("layer") {
            self.doc.view.layer = v.as_str().map(str::to_string);
        }
        if let Some(l) = req.get("lang").and_then(Value::as_str) {
            if crate::lang::Lang::parse(l).is_none() {
                return Err(Error::bad("lang", format!("unknown language {l}")));
            }
        }
        Ok(obj(vec![("view", view_to_value(&self.doc.view))]))
    }

    // ---- 渲染 / 导出 ----

    fn render_size(&self) -> (u16, u16) {
        let (x0, y0, x1, y1) = self.doc.bbox_shown(self.notes);
        let w = ((x1 - x0 + 3).max(8) as u16).min(MAX_RENDER);
        let h = ((y1 - y0 + 3).max(4) as u16).min(MAX_RENDER);
        (w, h)
    }

    pub fn render(&self, req: &Value) -> Result<Value, Error> {
        let format = req.get("format").and_then(Value::as_str).unwrap_or("text");
        // labels 默认开(和 --dump 一致),要干净图就 labels:false
        let labels = req.get("labels").and_then(Value::as_bool).unwrap_or(true);
        let auto = match req.get("size") {
            None => true,
            Some(Value::Str(s)) => s.eq_ignore_ascii_case("auto"),
            Some(_) => false,
        };
        let mut view = View {
            zoom: req.get("zoom").and_then(Value::as_f64).unwrap_or(1.0),
            labels,
            legend: req
                .get("legend")
                .and_then(Value::as_bool)
                .unwrap_or(self.legend),
            notes: req
                .get("notes")
                .and_then(Value::as_bool)
                .unwrap_or(self.notes),
            // headless 默认不画光标
            drawn: false,
            ..Default::default()
        };
        if let Some(c) = req.get("center").and_then(pair) {
            view.center = Some((c.0 as f64, c.1 as f64));
        }
        if let Some(c) = req.get("cursor").and_then(pair) {
            view.cursor = c;
            view.drawn = true;
        }
        // 尺寸:auto(或不给)按内容定,给了就精确尺寸、居中、超出裁掉
        let size = if auto {
            None
        } else {
            let a = req
                .get("size")
                .and_then(pair)
                .ok_or_else(|| Error::bad("size", "size must be [cols, rows] or \"auto\""))?;
            if a.0 <= 0 || a.1 <= 0 {
                return Err(Error::bad("size", "size must be positive"));
            }
            if a.0 as u32 > MAX_RENDER as u32 || a.1 as u32 > MAX_RENDER as u32 {
                return Err(Error::new(
                    "limit",
                    "size",
                    format!("size is capped at {MAX_RENDER}x{MAX_RENDER}"),
                ));
            }
            Some((a.0 as u16, a.1 as u16))
        };
        let frame = crate::dump::frame(&self.doc, labels, size, &view);
        let (w, h) = (frame.buf.area.width, frame.buf.area.height);
        match format {
            "text" | "json" => {
                let lines = crate::render::plain_lines(&frame.buf);
                let mut out = obj(vec![
                    (
                        "size",
                        Value::Arr(vec![Value::Int(w as i64), Value::Int(h as i64)]),
                    ),
                    (
                        "lines",
                        Value::Arr(lines.into_iter().map(Value::Str).collect()),
                    ),
                ]);
                if format == "json" {
                    // 坐标都换算到最终缓冲:探测坐标 - 内容原点 + 留白
                    let boxes: Vec<Value> = self
                        .doc
                        .boxes
                        .iter()
                        .zip(frame.rects.iter())
                        .filter(|(_, r)| r.2 > 0 && r.3 > 0)
                        .map(|(b, r)| {
                            let (x, y) = frame.map((r.0, r.1));
                            obj(vec![
                                ("id", Value::Str(b.id.clone())),
                                (
                                    "rect",
                                    Value::Arr(vec![
                                        Value::Int(x as i64),
                                        Value::Int(y as i64),
                                        Value::Int(r.2 as i64),
                                        Value::Int(r.3 as i64),
                                    ]),
                                ),
                            ])
                        })
                        .collect();
                    let mut cells: Vec<(usize, Vec<(i32, i32)>)> = frame
                        .hit
                        .edge_cells
                        .iter()
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    cells.sort_by_key(|(k, _)| *k);
                    let edges: Vec<Value> = cells
                        .into_iter()
                        .filter_map(|(e, mut cs)| {
                            let edge = self.doc.edges.get(e)?;
                            cs.sort_unstable();
                            // 注释框在画面里的位置:普通框,位置就是它自己的矩形
                            let rect = self
                                .doc
                                .note_of_edge(&edge.id)
                                .and_then(|b| frame.rects.get(b).copied())
                                .map(|(x, y, w, h)| {
                                    Value::Arr(vec![
                                        Value::Int(x as i64),
                                        Value::Int(y as i64),
                                        Value::Int(w as i64),
                                        Value::Int(h as i64),
                                    ])
                                });
                            let note = self
                                .doc
                                .note_of_edge(&edge.id)
                                .and_then(|b| self.doc.text(b).map(|t| t.text.clone()));
                            Some(obj(vec![
                                ("id", Value::Str(edge.id.clone())),
                                (
                                    "note",
                                    note.map(Value::Str).unwrap_or(Value::Null),
                                ),
                                ("note_rect", rect.unwrap_or(Value::Null)),
                                (
                                    "cells",
                                    Value::Arr(
                                        cs.into_iter()
                                            .map(|p| {
                                                let (x, y) = frame.map(p);
                                                Value::Arr(vec![
                                                    Value::Int(x as i64),
                                                    Value::Int(y as i64),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ),
                            ]))
                        })
                        .collect();
                    out = match out {
                        Value::Obj(mut f) => {
                            f.push(("boxes".into(), Value::Arr(boxes)));
                            f.push(("edges".into(), Value::Arr(edges)));
                            if frame.legend.width > 0 {
                                // 图例矩形已经是最终缓冲坐标
                                let (lx, ly) = (frame.legend.x, frame.legend.y);
                                // 图例区逐行文本(照着缓冲读,和画面上看到的一致)
                                // 逐格读,宽字跳掉续格:不然中文会变成"被 动 打 开"
                                let rows: Vec<Value> = (ly..ly + frame.legend.height)
                                    .map(|y| {
                                        let mut line = String::new();
                                        let mut x = lx;
                                        while x < lx + frame.legend.width {
                                            let sym = frame.buf[(x, y)].symbol();
                                            line.push_str(sym);
                                            let adv = crate::render::width(sym).max(1) as u16;
                                            x += adv;
                                        }
                                        Value::Str(line.trim_end().to_string())
                                    })
                                    .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                                    .collect();
                                f.push((
                                    "legend".into(),
                                    obj(vec![
                                        (
                                            "rect",
                                            Value::Arr(vec![
                                                Value::Int(lx as i64),
                                                Value::Int(ly as i64),
                                                Value::Int(frame.legend.width as i64),
                                                Value::Int(frame.legend.height as i64),
                                            ]),
                                        ),
                                        ("rows", Value::Arr(rows)),
                                    ]),
                                ));
                            }
                            Value::Obj(f)
                        }
                        other => other,
                    };
                }
                Ok(out)
            }
            "ansi" => Ok(obj(vec![(
                "ansi",
                Value::Str(crate::render::ansi(&frame.buf)),
            )])),
            "svg" | "png" => {
                let body = crate::svg::of_buffer(&frame.buf);
                match req.get("path").and_then(Value::as_str) {
                    Some(path) => {
                        let path = crate::paths::expand(path);
                        let res = if format == "png" {
                            crate::svg::write_png(&body, &path)
                        } else {
                            crate::paths::write_atomic(&path, &body)
                        };
                        res.map_err(|e| Error::new("io", "path", e))?;
                        Ok(obj(vec![
                            ("path", Value::Str(path)),
                            ("format", Value::Str(format.to_string())),
                        ]))
                    }
                    None => Ok(obj(vec![
                        ("format", Value::Str(format.to_string())),
                        ("base64", Value::Str(base64(body.as_bytes()))),
                    ])),
                }
            }
            other => Err(Error::bad(
                "format",
                format!("unknown format {other} (text/ansi/svg/png/json)"),
            )),
        }
    }

    pub fn export(&mut self, req: &Value) -> Result<Value, Error> {
        let path = req
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::bad("path", "path is required"))?;
        let path = crate::paths::expand(path);
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".dot") {
            let (body, warnings) = crate::dot_write::export(&self.doc)
                .map_err(|e| Error::new("bad_doc", "document", e))?;
            crate::paths::write_atomic(&path, &body).map_err(|e| Error::new("io", "path", e))?;
            return Ok(obj(vec![
                ("path", Value::Str(path)),
                (
                    "warnings",
                    Value::Arr(warnings.into_iter().map(Value::Str).collect()),
                ),
            ]));
        }
        if lower.ends_with(".canvas") {
            let (body, warnings) = crate::canvas_io::export(&self.doc);
            crate::paths::write_atomic(&path, &body).map_err(|e| Error::new("io", "path", e))?;
            return Ok(obj(vec![
                ("path", Value::Str(path)),
                (
                    "warnings",
                    Value::Arr(warnings.into_iter().map(Value::Str).collect()),
                ),
            ]));
        }
        if lower.ends_with(".svg") || lower.ends_with(".png") {
            // 和 --dump 同一套自动尺寸
            let view = View {
                drawn: false,
                ..Default::default()
            };
            let frame = crate::dump::frame(&self.doc, true, None, &view);
            let body = crate::svg::of_buffer(&frame.buf);
            if lower.ends_with(".png") {
                crate::svg::write_png(&body, &path).map_err(|e| Error::new("io", "path", e))?;
            } else {
                crate::paths::write_atomic(&path, &body)
                    .map_err(|e| Error::new("io", "path", e))?;
            }
            return Ok(obj(vec![("path", Value::Str(path))]));
        }
        Err(Error::bad(
            "path",
            format!("unsupported export extension: {path}"),
        ))
    }

    // ---- 撤销 ----

    pub fn undo(&mut self) -> Result<Value, Error> {
        match self.undo.undo(&mut self.doc) {
            Some(label) => Ok(obj(vec![
                ("undo", Value::Str(label)),
                ("undoable", Value::Int(self.undo.counts().0 as i64)),
            ])),
            None => Err(Error::new("bad_op", "undo", "nothing to undo")),
        }
    }

    pub fn redo(&mut self) -> Result<Value, Error> {
        match self.undo.redo(&mut self.doc) {
            Some(label) => Ok(obj(vec![
                ("redo", Value::Str(label)),
                ("redoable", Value::Int(self.undo.counts().1 as i64)),
            ])),
            None => Err(Error::new("bad_op", "redo", "nothing to redo")),
        }
    }

    // ---- 小工具 ----

    fn ensure_layer(&mut self, name: &str) {
        if self.doc.layers.iter().any(|l| l.name == name) {
            return;
        }
        let order = self.doc.layers.iter().map(|l| l.order).max().unwrap_or(0) + 1;
        self.doc.layers.push(Layer {
            name: name.to_string(),
            order,
        });
    }

    fn box_value(&self, b: &Box, with_text: bool) -> Value {
        let mut f = vec![
            ("type".to_string(), Value::Str("box".into())),
            ("id".to_string(), Value::Str(b.id.clone())),
            (
                "at".to_string(),
                Value::Arr(vec![Value::Int(b.at.0 as i64), Value::Int(b.at.1 as i64)]),
            ),
            (
                "size".to_string(),
                Value::Arr(vec![
                    Value::Int(b.size.0 as i64),
                    Value::Int(b.size.1 as i64),
                ]),
            ),
            ("style".to_string(), Value::Str(b.style.name().to_string())),
            ("color".to_string(), hex_or_null(b.color)),
            ("fill".to_string(), hex_or_null(b.fill)),
            ("layer".to_string(), Value::Str(b.layer.clone())),
            (
                "terms".to_string(),
                Value::Arr(b.terms.iter().cloned().map(Value::Str).collect()),
            ),
        ];
        if with_text {
            if let Some(i) = self.doc.box_idx(&b.id) {
                if let Some(t) = self.doc.text(i) {
                    f.push((
                        "text".to_string(),
                        obj(vec![
                            ("id", Value::Str(t.id.clone())),
                            ("text", Value::Str(t.text.clone())),
                            (
                                "at",
                                Value::Arr(vec![
                                    Value::Int(t.at.0 as i64),
                                    Value::Int(t.at.1 as i64),
                                ]),
                            ),
                            ("color", hex_or_null(t.color)),
                            ("bg", hex_or_null(t.bg)),
                            ("align", Value::Str(t.align.name().to_string())),
                            ("layer", Value::Str(t.layer.clone())),
                        ]),
                    ));
                }
            }
        }
        Value::Obj(f)
    }
}

fn edge_value(e: &DocEdge) -> Value {
    obj(vec![
        ("type", Value::Str("edge".into())),
        ("id", Value::Str(e.id.clone())),
        ("from", Value::Str(e.from.clone())),
        ("to", Value::Str(e.to.clone())),
        ("dir", Value::Str(e.dir.name().to_string())),
        ("group", Value::Str(e.group.clone())),
        ("style", Value::Str(e.style.name().to_string())),
        ("color", hex_or_null(e.color)),
        ("bg", hex_or_null(e.bg)),
        (
            "via",
            Value::Arr(
                e.via
                    .iter()
                    .map(|(x, y)| Value::Arr(vec![Value::Int(*x as i64), Value::Int(*y as i64)]))
                    .collect(),
            ),
        ),
        ("corner", Value::Str(e.corner.name().to_string())),
        ("layer", Value::Str(e.layer.clone())),
    ])
}

fn legend_entry_value(e: &crate::document::LegendEntry) -> Value {
    obj(vec![
        ("text", Value::Str(e.text.clone())),
        (
            "group",
            e.group.clone().map(Value::Str).unwrap_or(Value::Null),
        ),
        (
            "color",
            e.color.map(|c| Value::Str(hex(c))).unwrap_or(Value::Null),
        ),
        (
            "glyph",
            e.glyph
                .map(|g| Value::Str(g.name().to_string()))
                .unwrap_or(Value::Null),
        ),
    ])
}

fn view_to_value(v: &crate::document::ViewState) -> Value {
    obj(vec![
        (
            "center",
            Value::Arr(vec![Value::Num(v.center.0), Value::Num(v.center.1)]),
        ),
        ("zoom", Value::Num(v.zoom)),
        ("layout", Value::Str(v.layout.clone())),
        ("orientation", Value::Str(v.orientation.clone())),
        ("labels", Value::Bool(v.labels)),
        (
            "layer",
            v.layer.clone().map(Value::Str).unwrap_or(Value::Null),
        ),
        ("lang", Value::Str(v.lang.clone())),
        ("legend", Value::Bool(v.legend)),
    ])
}

/// 校验错误里的 `path: message` 拆开(文档校验都用这个格式)
fn split_validation(e: &str) -> (String, String) {
    match e.split_once(": ") {
        Some((p, m)) if !p.contains(' ') => (p.to_string(), m.to_string()),
        _ => ("document".to_string(), e.to_string()),
    }
}

// ---- 剪贴板 JSON ----

fn clipboard_to_value(c: &Clipboard) -> Value {
    let boxes: Vec<Value> = c
        .boxes
        .iter()
        .map(|b| {
            obj(vec![
                ("id", Value::Str(b.id.clone())),
                (
                    "at",
                    Value::Arr(vec![Value::Int(b.at.0 as i64), Value::Int(b.at.1 as i64)]),
                ),
                (
                    "size",
                    Value::Arr(vec![
                        Value::Int(b.size.0 as i64),
                        Value::Int(b.size.1 as i64),
                    ]),
                ),
                ("style", Value::Str(b.style.name().to_string())),
                ("color", hex_or_null(b.color)),
                ("fill", hex_or_null(b.fill)),
                ("layer", Value::Str(b.layer.clone())),
                (
                    "terms",
                    Value::Arr(b.terms.iter().cloned().map(Value::Str).collect()),
                ),
            ])
        })
        .collect();
    let texts: Vec<Value> = c
        .texts
        .iter()
        .map(|t| {
            obj(vec![
                ("id", Value::Str(t.id.clone())),
                ("text", Value::Str(t.text.clone())),
                (
                    "at",
                    Value::Arr(vec![Value::Int(t.at.0 as i64), Value::Int(t.at.1 as i64)]),
                ),
                ("box", Value::Str(t.box_id.clone())),
                ("color", hex_or_null(t.color)),
                ("bg", hex_or_null(t.bg)),
                ("align", Value::Str(t.align.name().to_string())),
                ("layer", Value::Str(t.layer.clone())),
            ])
        })
        .collect();
    let edges: Vec<Value> = c.edges.iter().map(edge_value).collect();
    let groups: Vec<Value> = c
        .groups
        .iter()
        .map(|g| {
            obj(vec![
                ("id", Value::Str(g.id.clone())),
                ("label", Value::Str(g.label.clone())),
                ("color", Value::Str(hex(g.color))),
            ])
        })
        .collect();
    obj(vec![
        (
            "origin",
            Value::Arr(vec![
                Value::Int(c.origin.0 as i64),
                Value::Int(c.origin.1 as i64),
            ]),
        ),
        ("boxes", Value::Arr(boxes)),
        ("texts", Value::Arr(texts)),
        ("edges", Value::Arr(edges)),
        ("groups", Value::Arr(groups)),
    ])
}

fn clipboard_from_value(v: &Value) -> Result<Clipboard, String> {
    let origin = v
        .get("origin")
        .and_then(pair)
        .ok_or_else(|| "snapshot.origin must be [x, y]".to_string())?;
    let mut boxes = Vec::new();
    for (k, b) in v
        .get("boxes")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let id = b
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("boxes[{k}].id"))?;
        let at = b
            .get("at")
            .and_then(pair)
            .ok_or_else(|| format!("boxes[{k}].at"))?;
        let size = b
            .get("size")
            .and_then(pair)
            .ok_or_else(|| format!("boxes[{k}].size"))?;
        let style = b
            .get("style")
            .and_then(Value::as_str)
            .and_then(BoxStyle::parse)
            .ok_or_else(|| format!("boxes[{k}].style"))?;
        boxes.push(Box {
            id: id.to_string(),
            at,
            size: (size.0.max(1) as u16, size.1.max(1) as u16),
            style,
            color: b.get("color").and_then(parse_color).flatten(),
            fill: b.get("fill").and_then(parse_color).flatten(),
            layer: b
                .get("layer")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            terms: b.get("terms").and_then(str_list).unwrap_or_default(),
            edge: None,
        });
    }
    let mut texts = Vec::new();
    for (k, t) in v
        .get("texts")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let id = t
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("texts[{k}].id"))?;
        let at = t
            .get("at")
            .and_then(pair)
            .ok_or_else(|| format!("texts[{k}].at"))?;
        let box_id = t
            .get("box")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("texts[{k}].box"))?;
        texts.push(Text {
            id: id.to_string(),
            text: t
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            at,
            box_id: box_id.to_string(),
            color: t.get("color").and_then(parse_color).flatten(),
            bg: t.get("bg").and_then(parse_color).flatten(),
            align: t
                .get("align")
                .and_then(Value::as_str)
                .and_then(Align::parse)
                .unwrap_or(Align::Left),
            layer: t
                .get("layer")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    let mut edges = Vec::new();
    for (k, e) in v
        .get("edges")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let id = e
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edges[{k}].id"))?;
        let from = e
            .get("from")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edges[{k}].from"))?;
        let to = e
            .get("to")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edges[{k}].to"))?;
        let mut via = Vec::new();
        for (j, p) in e
            .get("via")
            .and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            via.push(pair(p).ok_or_else(|| format!("edges[{k}].via[{j}]"))?);
        }
        edges.push(DocEdge {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            dir: e
                .get("dir")
                .and_then(Value::as_str)
                .and_then(Dir::parse)
                .unwrap_or(Dir::Forward),
            group: e
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            style: e
                .get("style")
                .and_then(Value::as_str)
                .and_then(EdgeStyle::parse)
                .unwrap_or(EdgeStyle::Solid),
            color: e.get("color").and_then(parse_color).flatten(),
            bg: e.get("bg").and_then(parse_color).flatten(),
            via,
            corner: e
                .get("corner")
                .and_then(Value::as_str)
                .and_then(Corner::parse)
                .unwrap_or(Corner::Rounded),
            layer: e
                .get("layer")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    let mut groups = Vec::new();
    for (k, g) in v
        .get("groups")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let id = g
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("groups[{k}].id"))?;
        groups.push(Group {
            id: id.to_string(),
            label: g
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            color: g
                .get("color")
                .and_then(parse_color)
                .flatten()
                .unwrap_or((0x4c, 0x8e, 0xf8)),
        });
    }
    Ok(Clipboard {
        origin,
        boxes,
        texts,
        edges,
        groups,
    })
}

/// 把一份剪贴板内容按偏移贴进文档,返回新 id 与跳过的边
fn paste_clip(doc: &mut Document, clip: &Clipboard, d: (i32, i32)) -> Result<Value, Error> {
    // 分组:按 id 或 label 复用,没有就新建
    let mut group_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for g in &clip.groups {
        let reuse = doc
            .groups
            .iter()
            .find(|x| x.id == g.id || (!g.label.is_empty() && x.label == g.label))
            .map(|x| x.id.clone());
        let id = match reuse {
            Some(id) => id,
            None => {
                let mut n = doc.groups.len();
                let id = loop {
                    let id = format!("g{n}");
                    if doc.group_idx(&id).is_none() {
                        break id;
                    }
                    n += 1;
                };
                doc.groups.push(Group {
                    id: id.clone(),
                    label: g.label.clone(),
                    color: g.color,
                });
                id
            }
        };
        group_of.insert(g.id.clone(), id);
    }
    // 框与文字:新 id
    let mut box_ids: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut new_boxes = Vec::new();
    let mut n = 1;
    for b in &clip.boxes {
        let id = loop {
            let id = format!("b{n}");
            n += 1;
            if doc.box_idx(&id).is_none() && !box_ids.values().any(|v| *v == id) {
                break id;
            }
        };
        box_ids.insert(b.id.clone(), id.clone());
        let mut nb = b.clone();
        nb.id = id;
        nb.at = (b.at.0 + d.0, b.at.1 + d.1);
        let at = nb.at;
        doc.boxes.push(nb);
        new_boxes.push(Value::Str(doc.boxes.last().unwrap().id.clone()));
        if let Some(t) = clip.texts.iter().find(|t| t.box_id == b.id) {
            let mut nt = t.clone();
            nt.id = free_text_id(doc);
            nt.box_id = doc.boxes.last().unwrap().id.clone();
            nt.at = if b.style.is_boxed() {
                (at.0 + 1, at.1 + 1)
            } else {
                (t.at.0 + d.0, t.at.1 + d.1)
            };
            doc.texts.push(nt);
        }
    }
    // 边:两端能对上就重建
    let mut new_edges = Vec::new();
    let mut skipped = Vec::new();
    for e in &clip.edges {
        let resolve = |id: &str| -> Option<String> {
            box_ids.get(id).cloned().or_else(|| {
                if doc.box_idx(id).is_some() {
                    Some(id.to_string())
                } else {
                    None
                }
            })
        };
        let (Some(f), Some(t)) = (resolve(&e.from), resolve(&e.to)) else {
            skipped.push(Value::Str(e.id.clone()));
            continue;
        };
        let group = group_of
            .get(&e.group)
            .cloned()
            .or_else(|| {
                if doc.group_idx(&e.group).is_some() {
                    Some(e.group.clone())
                } else {
                    None
                }
            })
            .or_else(|| doc.groups.first().map(|g| g.id.clone()));
        let Some(group) = group else {
            skipped.push(Value::Str(e.id.clone()));
            continue;
        };
        let mut ne = e.clone();
        ne.id = free_edge_id(doc);
        ne.from = f;
        ne.to = t;
        ne.group = group;
        ne.via = e.via.iter().map(|&(x, y)| (x + d.0, y + d.1)).collect();
        if ne.from == ne.to {
            skipped.push(Value::Str(e.id.clone()));
            continue;
        }
        if doc
            .edges
            .iter()
            .any(|x| x.from == ne.from && x.to == ne.to && x.group == ne.group)
        {
            skipped.push(Value::Str(e.id.clone()));
            continue;
        }
        new_edges.push(Value::Str(ne.id.clone()));
        doc.edges.push(ne);
    }
    doc.rebuild();
    Ok(obj(vec![
        ("boxes", Value::Arr(new_boxes)),
        ("edges", Value::Arr(new_edges)),
        ("skipped_edges", Value::Arr(skipped)),
    ]))
}

fn free_text_id(doc: &Document) -> String {
    let mut n = 1;
    loop {
        let id = format!("t{n}");
        if !doc.texts.iter().any(|t| t.id == id) {
            return id;
        }
        n += 1;
    }
}

fn free_edge_id(doc: &Document) -> String {
    let mut n = 1;
    loop {
        let id = format!("e{n}");
        if !doc.edges.iter().any(|e| e.id == id) {
            return id;
        }
        n += 1;
    }
}

/// 不需要外部依赖的 base64(svg 走内联时用)
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
