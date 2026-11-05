//! `.gtui` native 文档的读写.
//!
//! 读的时候逐层手写校验:类型、范围、未知键、引用都要报错,并且错误里带
//! JSON 路径(`boxes[3].at`、`edges[0].via[1]`).写的时候键序固定、数字稳定,
//! 同一份文档重复保存得到的字节完全一样.

use std::collections::{HashMap, HashSet};

use crate::document::{
    Box as DocBox, Cursor, Dir, DocEdge, Document, Group, Layer, Rgb, Text, ViewState,
};
use crate::json::{self, Value};
use crate::model::{Align, BoxStyle, Corner, EdgeStyle};

/// v4 边标签:(边下标, 文字, 手工位置, 颜色)
type OldLabel = (usize, String, Option<(i32, i32)>, Option<Rgb>);

const TOP_KEYS: &[&str] = &[
    "type", "version", "source", "unit", "groups", "layers", "boxes", "texts", "edges", "view",
    "legend", "regions",
];

// ---- 读:小工具 ----

fn key_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn item_path(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Num(_) => "number",
        Value::Int(_) => "integer",
        Value::Str(_) => "string",
        Value::Arr(_) => "array",
        Value::Obj(_) => "object",
    }
}

fn field<'a>(o: &'a [(String, Value)], key: &str, path: &str) -> Result<&'a Value, String> {
    o.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("{}: missing key", key_path(path, key)))
}

/// 可选字段:旧文件里没有这个键时返回 None,由调用方按默认值处理
fn opt_field<'a>(o: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    o.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn check_unknown(o: &[(String, Value)], allowed: &[&str], path: &str) -> Result<(), String> {
    for (k, _) in o {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("{}: unknown key", key_path(path, k)));
        }
    }
    Ok(())
}

fn need_obj<'a>(v: &'a Value, path: &str) -> Result<&'a [(String, Value)], String> {
    v.as_obj()
        .ok_or_else(|| format!("{path}: expected an object, got {}", kind(v)))
}

fn need_arr<'a>(v: &'a Value, path: &str) -> Result<&'a [Value], String> {
    v.as_arr()
        .ok_or_else(|| format!("{path}: expected an array, got {}", kind(v)))
}

fn need_str<'a>(v: &'a Value, path: &str) -> Result<&'a str, String> {
    v.as_str()
        .ok_or_else(|| format!("{path}: expected a string, got {}", kind(v)))
}

fn need_bool(v: &Value, path: &str) -> Result<bool, String> {
    v.as_bool()
        .ok_or_else(|| format!("{path}: expected a boolean, got {}", kind(v)))
}

fn need_i32(v: &Value, path: &str) -> Result<i32, String> {
    v.as_i32()
        .ok_or_else(|| format!("{path}: expected an integer in i32 range, got {}", kind(v)))
}

fn need_u16(v: &Value, path: &str) -> Result<u16, String> {
    let n = need_i32(v, path)?;
    if !(0..=u16::MAX as i32).contains(&n) {
        return Err(format!("{path}: expected an integer in u16 range, got {n}"));
    }
    Ok(n as u16)
}

fn need_f64(v: &Value, path: &str) -> Result<f64, String> {
    let n = v
        .as_f64()
        .ok_or_else(|| format!("{path}: expected a number, got {}", kind(v)))?;
    if !n.is_finite() {
        return Err(format!("{path}: expected a finite number"));
    }
    Ok(n)
}

fn need_opt_str(v: &Value, path: &str) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        _ => Ok(Some(need_str(v, path)?.to_string())),
    }
}

fn need_opt_f64(v: &Value, path: &str) -> Result<Option<f64>, String> {
    match v {
        Value::Null => Ok(None),
        _ => Ok(Some(need_f64(v, path)?)),
    }
}

fn need_pair_i32(v: &Value, path: &str) -> Result<(i32, i32), String> {
    let xs = need_arr(v, path)?;
    if xs.len() != 2 {
        return Err(format!("{path}: expected two integers"));
    }
    Ok((
        need_i32(&xs[0], &item_path(path, 0))?,
        need_i32(&xs[1], &item_path(path, 1))?,
    ))
}

fn need_pair_u16(v: &Value, path: &str) -> Result<(u16, u16), String> {
    let xs = need_arr(v, path)?;
    if xs.len() != 2 {
        return Err(format!("{path}: expected two integers"));
    }
    Ok((
        need_u16(&xs[0], &item_path(path, 0))?,
        need_u16(&xs[1], &item_path(path, 1))?,
    ))
}

fn need_pair_f64(v: &Value, path: &str) -> Result<(f64, f64), String> {
    let xs = need_arr(v, path)?;
    if xs.len() != 2 {
        return Err(format!("{path}: expected two numbers"));
    }
    Ok((
        need_f64(&xs[0], &item_path(path, 0))?,
        need_f64(&xs[1], &item_path(path, 1))?,
    ))
}

fn parse_hex(s: &str) -> Option<Rgb> {
    if s.len() != 7 || !s.is_ascii() || !s.starts_with('#') {
        return None;
    }
    let mut out = [0u8; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let part = &s[1 + i * 2..1 + i * 2 + 2];
        if !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    Some((out[0], out[1], out[2]))
}

fn need_color(v: &Value, path: &str) -> Result<Option<Rgb>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Str(s) => parse_hex(s)
            .map(Some)
            .ok_or_else(|| format!("{path}: expected #rrggbb, got {s:?}")),
        _ => Err(format!("{path}: expected #rrggbb or null, got {}", kind(v))),
    }
}

fn need_box_style(s: &str, path: &str) -> Result<BoxStyle, String> {
    match s {
        "point" => Ok(BoxStyle::Point),
        "none" | "empty" => Ok(BoxStyle::None),
        "single" => Ok(BoxStyle::Single),
        "rounded" => Ok(BoxStyle::Rounded),
        "double" => Ok(BoxStyle::Double),
        "heavy" => Ok(BoxStyle::Heavy),
        _ => Err(format!(
            "{path}: expected point|none|single|rounded|double|heavy, got {s:?}"
        )),
    }
}

fn need_edge_style(s: &str, path: &str) -> Result<EdgeStyle, String> {
    match s {
        "solid" => Ok(EdgeStyle::Solid),
        "dashed" => Ok(EdgeStyle::Dashed),
        "dotted" => Ok(EdgeStyle::Dotted),
        _ => Err(format!("{path}: expected solid|dashed|dotted, got {s:?}")),
    }
}

fn need_align(s: &str, path: &str) -> Result<Align, String> {
    match s {
        "left" => Ok(Align::Left),
        "center" => Ok(Align::Center),
        "right" => Ok(Align::Right),
        _ => Err(format!("{path}: expected left|center|right, got {s:?}")),
    }
}

fn need_dir(s: &str, path: &str) -> Result<Dir, String> {
    match s {
        "none" => Ok(Dir::None),
        "forward" => Ok(Dir::Forward),
        "back" => Ok(Dir::Back),
        "both" => Ok(Dir::Both),
        _ => Err(format!(
            "{path}: expected none|forward|back|both, got {s:?}"
        )),
    }
}

fn need_corner(s: &str, path: &str) -> Result<Corner, String> {
    match s {
        "rounded" => Ok(Corner::Rounded),
        "sharp" => Ok(Corner::Sharp),
        _ => Err(format!("{path}: expected rounded|sharp, got {s:?}")),
    }
}

fn claim_id(used: &mut HashMap<String, String>, id: &str, path: &str) -> Result<(), String> {
    if let Some(prev) = used.get(id) {
        return Err(format!(
            "{path}: duplicate id {id:?} (already used at {prev})"
        ));
    }
    used.insert(id.to_string(), path.to_string());
    Ok(())
}

// ---- 读 ----

impl Document {
    /// 从 `.gtui` 文本建文档.所有错误都带 JSON 路径.
    pub fn load(text: &str) -> Result<Document, String> {
        let root = json::parse_strict(text).map_err(|e| format!("document: {e}"))?;
        json::check_finite(&root).map_err(|e| format!("document: {e}"))?;
        let top = need_obj(&root, "")?;
        check_unknown(top, TOP_KEYS, "")?;

        let ty = need_str(field(top, "type", "")?, "type")?;
        if ty != "graph-tui" {
            return Err(format!("type: expected \"graph-tui\", got {ty:?}"));
        }
        let version = need_i32(field(top, "version", "")?, "version")?;
        // v1:光标存框 id;v2:光标存 [x, y];v3:图例;v4:边标签;v5:边注释改成绑定框;
        // v1..v8 都能读(v4 的边标签读进来转成注释框);v7 增加 regions(cluster),
        // v8 增加显式边端口.写出去一律 v8
        if !(1..=10).contains(&version) {
            return Err(format!("version: unsupported version {version}"));
        }
        let unit = need_str(field(top, "unit", "")?, "unit")?;
        if unit != "cell" {
            return Err(format!("unit: expected \"cell\", got {unit:?}"));
        }
        let source = need_str(field(top, "source", "")?, "source")?.to_string();

        // 全局 id 表:分组、框、文字、边共用一个命名空间
        let mut used: HashMap<String, String> = HashMap::new();

        let group_vals = need_arr(field(top, "groups", "")?, "groups")?;
        let mut groups = Vec::with_capacity(group_vals.len());
        for (i, gv) in group_vals.iter().enumerate() {
            let p = item_path("groups", i);
            let o = need_obj(gv, &p)?;
            check_unknown(o, &["id", "label", "color"], &p)?;
            let id = need_str(field(o, "id", &p)?, &key_path(&p, "id"))?.to_string();
            claim_id(&mut used, &id, &key_path(&p, "id"))?;
            let label = need_str(field(o, "label", &p)?, &key_path(&p, "label"))?.to_string();
            let color = need_color(field(o, "color", &p)?, &key_path(&p, "color"))?
                .ok_or_else(|| format!("{}: a group color is required", key_path(&p, "color")))?;
            groups.push(Group { id, label, color });
        }
        if groups.is_empty() {
            return Err("groups: at least one group is required".to_string());
        }
        let group_ids: HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();

        let layer_vals = need_arr(field(top, "layers", "")?, "layers")?;
        let mut layers = Vec::with_capacity(layer_vals.len());
        let mut layer_names: HashSet<String> = HashSet::new();
        for (i, lv) in layer_vals.iter().enumerate() {
            let p = item_path("layers", i);
            let o = need_obj(lv, &p)?;
            check_unknown(o, &["name", "order"], &p)?;
            let name = need_str(field(o, "name", &p)?, &key_path(&p, "name"))?.to_string();
            if !layer_names.insert(name.clone()) {
                return Err(format!(
                    "{}: duplicate layer {name:?}",
                    key_path(&p, "name")
                ));
            }
            let order = need_i32(field(o, "order", &p)?, &key_path(&p, "order"))?;
            layers.push(Layer { name, order });
        }
        if !layer_names.contains("") {
            return Err("layers: the default layer (\"\") is missing".to_string());
        }

        let box_vals = need_arr(field(top, "boxes", "")?, "boxes")?;
        let mut boxes = Vec::with_capacity(box_vals.len());
        // box id -> (左上角, 是否画框),文字校验用
        let mut box_at: HashMap<String, (i32, i32, bool)> = HashMap::new();
        for (i, bv) in box_vals.iter().enumerate() {
            let p = item_path("boxes", i);
            let o = need_obj(bv, &p)?;
            check_unknown(
                o,
                &[
                    "id", "at", "size", "style", "color", "fill", "layer", "terms", "edge",
                ],
                &p,
            )?;
            let id = need_str(field(o, "id", &p)?, &key_path(&p, "id"))?.to_string();
            claim_id(&mut used, &id, &key_path(&p, "id"))?;
            let at = need_pair_i32(field(o, "at", &p)?, &key_path(&p, "at"))?;
            let size = need_pair_u16(field(o, "size", &p)?, &key_path(&p, "size"))?;
            // 老文件与手写文件都可能没有 style:按默认样式(现在是 none)读
            let style = match opt_field(o, "style") {
                None | Some(Value::Null) => BoxStyle::default(),
                Some(v) => {
                    let s = need_str(v, &key_path(&p, "style"))?;
                    need_box_style(s, &key_path(&p, "style"))?
                }
            };
            let color = need_color(field(o, "color", &p)?, &key_path(&p, "color"))?;
            // 旧文件没有 fill,按 null 加载
            let fill = match opt_field(o, "fill") {
                Some(v) => need_color(v, &key_path(&p, "fill"))?,
                None => None,
            };
            let layer = need_str(field(o, "layer", &p)?, &key_path(&p, "layer"))?.to_string();
            if !layer_names.contains(&layer) {
                return Err(format!(
                    "{}: unknown layer {layer:?}",
                    key_path(&p, "layer")
                ));
            }
            let term_vals = need_arr(field(o, "terms", &p)?, &key_path(&p, "terms"))?;
            let mut terms = Vec::with_capacity(term_vals.len());
            for (j, tv) in term_vals.iter().enumerate() {
                terms.push(need_str(tv, &format!("{}.terms[{j}]", p))?.to_string());
            }
            if size.0 == 0 || size.1 == 0 {
                return Err(format!(
                    "{}: width and height must be positive",
                    key_path(&p, "size")
                ));
            }
            if style.is_boxed() {
                // 多行(v6)之后高度按行数来:只要求"至少三行、至少四列",
                // 文字装得下由文档校验统一查
                if size.1 < 3 || size.0 < 4 {
                    return Err(format!(
                        "{}: a boxed node is at least 3 rows tall and 4 wide",
                        key_path(&p, "size")
                    ));
                }
            } else if style.is_point() && size != (1, 1) {
                return Err(format!("{}: a point is 1x1", key_path(&p, "size")));
            }
            let edge = match opt_field(o, "edge") {
                None | Some(Value::Null) => None,
                Some(v) => Some(need_str(v, &key_path(&p, "edge"))?.to_string()),
            };
            if edge.is_some() && style != BoxStyle::None {
                return Err(format!(
                    "{}: only a text-only (none) node can be an edge note",
                    key_path(&p, "edge")
                ));
            }
            box_at.insert(id.clone(), (at.0, at.1, style.is_boxed()));
            boxes.push(DocBox {
                id,
                at,
                size,
                style,
                color,
                fill,
                layer,
                terms,
                edge,
            });
        }

        let text_vals = need_arr(field(top, "texts", "")?, "texts")?;
        let mut texts = Vec::with_capacity(text_vals.len());
        for (i, tv) in text_vals.iter().enumerate() {
            let p = item_path("texts", i);
            let o = need_obj(tv, &p)?;
            check_unknown(
                o,
                &["id", "text", "at", "box", "color", "bg", "align", "layer"],
                &p,
            )?;
            let id = need_str(field(o, "id", &p)?, &key_path(&p, "id"))?.to_string();
            claim_id(&mut used, &id, &key_path(&p, "id"))?;
            let text = need_str(field(o, "text", &p)?, &key_path(&p, "text"))?.to_string();
            // 多行用 \n;其余控制字符(含 \r \t)一律拒
            if text.chars().any(|c| c.is_control() && c != '\n') {
                return Err(format!(
                    "{}: control characters are not allowed (use \\n for lines)",
                    key_path(&p, "text")
                ));
            }
            let at = need_pair_i32(field(o, "at", &p)?, &key_path(&p, "at"))?;
            let box_id = need_str(field(o, "box", &p)?, &key_path(&p, "box"))?.to_string();
            if !box_at.contains_key(&box_id) {
                return Err(format!(
                    "{}: box {box_id:?} does not exist",
                    key_path(&p, "box")
                ));
            }
            let color = need_color(field(o, "color", &p)?, &key_path(&p, "color"))?;
            // 旧文件没有 align,按 left 加载
            let align = match opt_field(o, "align") {
                Some(v) => {
                    need_align(need_str(v, &key_path(&p, "align"))?, &key_path(&p, "align"))?
                }
                None => Align::Left,
            };
            let layer = need_str(field(o, "layer", &p)?, &key_path(&p, "layer"))?.to_string();
            if !layer_names.contains(&layer) {
                return Err(format!(
                    "{}: unknown layer {layer:?}",
                    key_path(&p, "layer")
                ));
            }
            let bg = match opt_field(o, "bg") {
                Some(v) => need_color(v, &key_path(&p, "bg"))?,
                None => None,
            };
            texts.push(Text {
                id,
                text,
                at,
                box_id,
                color,
                bg,
                align,
                layer,
            });
        }

        // 框与文字严格一对一
        for (bi, b) in boxes.iter().enumerate() {
            let n = texts.iter().filter(|t| t.box_id == b.id).count();
            if n != 1 {
                return Err(format!("boxes[{bi}]: needs exactly one text, got {n}"));
            }
        }
        for (i, t) in texts.iter().enumerate() {
            if let Some(&(x, y, boxed)) = box_at.get(&t.box_id) {
                if boxed && t.at != (x + 1, y + 1) {
                    return Err(format!(
                        "texts[{i}].at: a boxed node's text must be at [{}, {}]",
                        x + 1,
                        y + 1
                    ));
                }
            }
        }

        let edge_vals = need_arr(field(top, "edges", "")?, "edges")?;
        let mut edges = Vec::with_capacity(edge_vals.len());
        // v4 及更早:标签挂在边上;读的时候先攒起来,之后变成"绑在边上的文字框"
        let mut labels: Vec<OldLabel> = Vec::new();
        for (i, ev) in edge_vals.iter().enumerate() {
            let p = item_path("edges", i);
            let o = need_obj(ev, &p)?;
            check_unknown(
                o,
                &[
                    "id",
                    "from",
                    "to",
                    "dir",
                    "group",
                    "style",
                    "color",
                    "bg",
                    "via",
                    "corner",
                    "from_side",
                    "to_side",
                    "head",
                    "layer",
                    "label",
                    "label_at",
                    "label_color",
                ],
                &p,
            )?;
            let id = need_str(field(o, "id", &p)?, &key_path(&p, "id"))?.to_string();
            claim_id(&mut used, &id, &key_path(&p, "id"))?;
            let from = need_str(field(o, "from", &p)?, &key_path(&p, "from"))?.to_string();
            if !box_at.contains_key(&from) {
                return Err(format!(
                    "{}: box {from:?} does not exist",
                    key_path(&p, "from")
                ));
            }
            let to = need_str(field(o, "to", &p)?, &key_path(&p, "to"))?.to_string();
            if !box_at.contains_key(&to) {
                return Err(format!("{}: box {to:?} does not exist", key_path(&p, "to")));
            }
            // 旧文件没有 dir,按 forward 加载;新写出的文件必写 dir
            let dir = match opt_field(o, "dir") {
                Some(v) => {
                    let s = need_str(v, &key_path(&p, "dir"))?;
                    need_dir(s, &key_path(&p, "dir"))?
                }
                None => Dir::Forward,
            };
            let group = need_str(field(o, "group", &p)?, &key_path(&p, "group"))?.to_string();
            if !group_ids.contains(group.as_str()) {
                return Err(format!(
                    "{}: group {group:?} does not exist",
                    key_path(&p, "group")
                ));
            }
            let style_s = need_str(field(o, "style", &p)?, &key_path(&p, "style"))?;
            let style = need_edge_style(style_s, &key_path(&p, "style"))?;
            let color = need_color(field(o, "color", &p)?, &key_path(&p, "color"))?;
            let via_vals = need_arr(field(o, "via", &p)?, &key_path(&p, "via"))?;
            let mut via = Vec::with_capacity(via_vals.len());
            for (j, vv) in via_vals.iter().enumerate() {
                via.push(need_pair_i32(vv, &format!("{}.via[{j}]", p))?);
            }
            // 端口:v8 才有;缺省 / "auto" = 自动
            let side_arg = |name: &str| -> Result<Option<crate::model::Side>, String> {
                match opt_field(o, name) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => {
                        let sv = need_str(v, &key_path(&p, name))?;
                        if sv.eq_ignore_ascii_case("auto") {
                            Ok(None)
                        } else {
                            crate::model::Side::parse(sv).map(Some).ok_or_else(|| {
                                format!(
                                    "{}: expected auto|left|right|up|down, got {sv:?}",
                                    key_path(&p, name)
                                )
                            })
                        }
                    }
                }
            };
            let from_side = side_arg("from_side")?;
            let to_side = side_arg("to_side")?;
            // 箭头字形:老文件没有,跟全局设置走
            let head = match opt_field(o, "head") {
                None => None,
                Some(Value::Null) => None,
                Some(v) => {
                    let sv = need_str(v, &key_path(&p, "head"))?;
                    if sv == "auto" {
                        None
                    } else {
                        Some(crate::model::ArrowStyle::parse(sv).ok_or_else(|| {
                            format!(
                                "{}: expected auto|ascii|arrows|triangles, got {sv:?}",
                                key_path(&p, "head")
                            )
                        })?)
                    }
                }
            };
            // 旧文件没有 corner,按 rounded 加载
            let corner = match opt_field(o, "corner") {
                Some(v) => {
                    let s = need_str(v, &key_path(&p, "corner"))?;
                    need_corner(s, &key_path(&p, "corner"))?
                }
                None => Corner::Rounded,
            };
            let layer = need_str(field(o, "layer", &p)?, &key_path(&p, "layer"))?.to_string();
            if !layer_names.contains(&layer) {
                return Err(format!(
                    "{}: unknown layer {layer:?}",
                    key_path(&p, "layer")
                ));
            }
            let bg = match opt_field(o, "bg") {
                Some(v) => need_color(v, &key_path(&p, "bg"))?,
                None => None,
            };
            // 边标签(v4 及更早):按"注释框"的老写法读,稍后统一转成绑定框。
            // 多行照收(注释框能放多行),只拒 \r / \t
            let label = match opt_field(o, "label") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let t = need_str(v, &key_path(&p, "label"))?;
                    if t.contains('\r') || t.contains('\t') {
                        return Err(format!(
                            "{}: an edge label must not contain CR or TAB",
                            key_path(&p, "label")
                        ));
                    }
                    Some(t.to_string())
                }
            };
            let label_at = match opt_field(o, "label_at") {
                None | Some(Value::Null) => None,
                Some(v) => Some(need_pair_i32(v, &key_path(&p, "label_at"))?),
            };
            let label_color = match opt_field(o, "label_color") {
                None | Some(Value::Null) => None,
                Some(v) => need_color(v, &key_path(&p, "label_color"))?,
            };
            edges.push(DocEdge {
                id,
                from,
                to,
                dir,
                group,
                style,
                color,
                bg,
                via,
                corner,
                layer,
                from_side,
                to_side,
                head,
            });
            if let Some(text) = label {
                labels.push((edges.len() - 1, text, label_at, label_color));
            }
        }
        // 迁移:每条带标签的边补一个 none 框(+ 一条文字),框绑在这条边上
        for (ei, text, at, color) in labels {
            let e = &edges[ei];
            let at = match at {
                Some(p) => p,
                None => {
                    // 没有手工位置:两端框中心的中点往右一格(一次性、确定性)
                    let center = |id: &str| -> Option<(i32, i32)> {
                        box_at.get(id).map(|&(x, y, _)| (x, y))
                    };
                    match (center(&e.from), center(&e.to)) {
                        (Some(a), Some(b)) => ((a.0 + b.0) / 2 + 1, (a.1 + b.1) / 2),
                        _ => (0, 0),
                    }
                }
            };
            let mut n = boxes.len() + 1;
            let id = loop {
                let id = format!("n{n}");
                if !used.contains_key(&id) {
                    used.insert(id.clone(), "migrated label".to_string());
                    break id;
                }
                n += 1;
            };
            let mut t = texts.len() + 1;
            let tid = loop {
                let tid = format!("t{t}");
                if !used.contains_key(&tid) {
                    used.insert(tid.clone(), "migrated label".to_string());
                    break tid;
                }
                t += 1;
            };
            let (lines, widest) = crate::document::text_metrics(&text);
            boxes.push(DocBox {
                id: id.clone(),
                at,
                size: (widest.max(1) as u16, lines.max(1) as u16),
                style: BoxStyle::None,
                color: None,
                fill: None,
                layer: e.layer.clone(),
                terms: Vec::new(),
                edge: Some(e.id.clone()),
            });
            texts.push(Text {
                id: tid,
                text,
                at,
                box_id: id,
                color,
                bg: None,
                align: Align::Left,
                layer: e.layer.clone(),
            });
        }

        let vp = "view";
        let vo = need_obj(field(top, "view", "")?, vp)?;
        check_unknown(
            vo,
            &[
                "center",
                "zoom",
                "layout",
                "cursor",
                "focus",
                "labels",
                "lang",
                "layer",
                "side",
                "cycle",
                "orientation",
                "legend",
                "notes",
                // v4 的老名字(边标签开关),读进来当注释开关
                "elabels",
            ],
            vp,
        )?;
        let center = need_pair_f64(field(vo, "center", vp)?, &key_path(vp, "center"))?;
        let zoom = need_f64(field(vo, "zoom", vp)?, &key_path(vp, "zoom"))?;
        if zoom <= 0.0 {
            return Err("view.zoom: must be greater than 0".to_string());
        }
        let layout = need_str(field(vo, "layout", vp)?, &key_path(vp, "layout"))?.to_string();
        // v1 存框 id 的字符串,v2 存 [x, y] 的格子
        let cursor = match opt_field(vo, "cursor") {
            None | Some(Value::Null) => None,
            Some(Value::Str(id)) => {
                if !box_at.contains_key(id.as_str()) {
                    return Err(format!(
                        "{}: box {id:?} does not exist",
                        key_path(vp, "cursor")
                    ));
                }
                Some(Cursor::Box(id.clone()))
            }
            Some(v @ Value::Arr(_)) => {
                Some(Cursor::Cell(need_pair_i32(v, &key_path(vp, "cursor"))?))
            }
            Some(_) => {
                return Err(format!(
                    "{}: cursor must be a box id or [x, y]",
                    key_path(vp, "cursor")
                ))
            }
        };
        let focus_vals = need_arr(field(vo, "focus", vp)?, &key_path(vp, "focus"))?;
        let mut focus = Vec::with_capacity(focus_vals.len());
        for (j, fv) in focus_vals.iter().enumerate() {
            let id = need_str(fv, &format!("{vp}.focus[{j}]"))?.to_string();
            if !group_ids.contains(id.as_str()) {
                return Err(format!("{vp}.focus[{j}]: group {id:?} does not exist"));
            }
            focus.push(id);
        }
        let labels = need_bool(field(vo, "labels", vp)?, &key_path(vp, "labels"))?;
        let lang = need_str(field(vo, "lang", vp)?, &key_path(vp, "lang"))?.to_string();
        if lang != "zh" && lang != "en" {
            return Err(format!(
                "view.lang: expected \"zh\" or \"en\", got {lang:?}"
            ));
        }
        let layer = need_opt_str(field(vo, "layer", vp)?, &key_path(vp, "layer"))?;
        if let Some(name) = &layer {
            if !layer_names.contains(name) {
                return Err(format!("{}: unknown layer {name:?}", key_path(vp, "layer")));
            }
        }
        let side = need_f64(field(vo, "side", vp)?, &key_path(vp, "side"))?;
        let cycle = need_opt_f64(field(vo, "cycle", vp)?, &key_path(vp, "cycle"))?;
        // 方向:缺省 auto(老文件没有这一项)
        let orientation = match opt_field(vo, "orientation") {
            None | Some(Value::Null) => "auto".to_string(),
            Some(Value::Str(t)) if t == "auto" => "auto".to_string(),
            Some(Value::Int(d)) if [0, 90, 180, 270].contains(d) => d.to_string(),
            Some(Value::Num(n)) if n.fract() == 0.0 && [0.0, 90.0, 180.0, 270.0].contains(n) => {
                (*n as i32).to_string()
            }
            Some(other) => {
                return Err(format!(
                    "{}: orientation must be auto or 0/90/180/270, got {other:?}",
                    key_path(vp, "orientation")
                ))
            }
        };

        let view = ViewState {
            center,
            zoom,
            layout,
            cursor,
            focus,
            labels,
            lang,
            layer,
            side,
            cycle,
            orientation,
            notes: match opt_field(vo, "notes").or_else(|| opt_field(vo, "elabels")) {
                None | Some(Value::Null) => true,
                Some(v) => need_bool(
                    v,
                    &key_path(
                        vp,
                        if opt_field(vo, "notes").is_some() {
                            "notes"
                        } else {
                            "elabels"
                        },
                    ),
                )?,
            },
        };

        // 文字装不装得下:在 io 层再查一遍,报错带上 JSON 路径(box 层只知道 id)
        for (i, t) in texts.iter().enumerate() {
            let Some(b) = boxes.iter().find(|b| b.id == t.box_id) else {
                continue; // 悬空引用由文档校验报
            };
            let (lines, widest) = crate::document::text_metrics(&t.text);
            let ok = match b.style {
                BoxStyle::Point => lines <= 1,
                BoxStyle::None => lines <= b.size.1 as usize && widest <= b.size.0 as usize,
                _ => lines + 2 <= b.size.1 as usize && widest + 2 <= b.size.0 as usize,
            };
            if !ok {
                return Err(format!(
                    "{}: the text does not fit box {:?} ({} lines x {} cols; box {}x{})",
                    item_path("texts", i),
                    b.id,
                    lines,
                    widest,
                    b.size.0,
                    b.size.1
                ));
            }
        }

        // 区域(cluster):v7 才有,老文件当空表
        let mut regions: Vec<crate::document::Region> = Vec::new();
        if let Some(rv) = opt_field(top, "regions") {
            let arr = need_arr(rv, "regions")?;
            for (i, item) in arr.iter().enumerate() {
                let p = item_path("regions", i);
                let o = need_obj(item, &p)?;
                check_unknown(
                    o,
                    &[
                        "id",
                        "label",
                        "color",
                        "style",
                        "layer",
                        "padding",
                        "members",
                        "collapsed",
                    ],
                    &p,
                )?;
                let id = need_str(field(o, "id", &p)?, &key_path(&p, "id"))?.to_string();
                claim_id(&mut used, &id, &key_path(&p, "id"))?;
                let label = match opt_field(o, "label") {
                    None | Some(Value::Null) => String::new(),
                    Some(v) => need_str(v, &key_path(&p, "label"))?.to_string(),
                };
                let color = match opt_field(o, "color") {
                    None => None,
                    Some(v) => need_color(v, &key_path(&p, "color"))?,
                };
                let style = match opt_field(o, "style") {
                    None | Some(Value::Null) => BoxStyle::Single,
                    Some(v) => {
                        let sname = need_str(v, &key_path(&p, "style"))?;
                        need_box_style(sname, &key_path(&p, "style"))?
                    }
                };
                if !style.is_boxed() {
                    return Err(format!(
                        "{}: a region needs a boxed style",
                        key_path(&p, "style")
                    ));
                }
                let layer = match opt_field(o, "layer") {
                    None | Some(Value::Null) => String::new(),
                    Some(v) => need_str(v, &key_path(&p, "layer"))?.to_string(),
                };
                if !layer_names.contains(&layer) {
                    return Err(format!(
                        "{}: unknown layer {layer:?}",
                        key_path(&p, "layer")
                    ));
                }
                let padding = match opt_field(o, "padding") {
                    None | Some(Value::Null) => 1,
                    Some(v) => need_i32(v, &key_path(&p, "padding"))?.clamp(0, 16) as u16,
                };
                let members = match opt_field(o, "members") {
                    None => Vec::new(),
                    Some(v) => {
                        let vals = need_arr(v, &key_path(&p, "members"))?;
                        let mut out = Vec::with_capacity(vals.len());
                        for (j, mv) in vals.iter().enumerate() {
                            out.push(need_str(mv, &format!("{}.members[{j}]", p))?.to_string());
                        }
                        out
                    }
                };
                let collapsed = match opt_field(o, "collapsed") {
                    None | Some(Value::Null) => false,
                    Some(v) => match v {
                        Value::Bool(b) => *b,
                        other => {
                            return Err(format!(
                                "{}: expected true/false, got {other:?}",
                                key_path(&p, "collapsed")
                            ))
                        }
                    },
                };
                regions.push(crate::document::Region {
                    id,
                    label,
                    color,
                    style,
                    layer,
                    padding,
                    members,
                    collapsed,
                });
            }
        }
        Document::from_parts_with_regions(
            source, groups, layers, boxes, texts, edges, view, regions,
        )
        .map_err(|e| format!("document: {e}"))
    }
}

// ---- 写 ----

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn opt_color(c: Option<Rgb>) -> Value {
    match c {
        Some(c) => Value::Str(hex(c)),
        None => Value::Null,
    }
}

fn int(n: i32) -> Value {
    Value::Int(i64::from(n))
}

fn pair(a: i32, b: i32) -> Value {
    Value::Arr(vec![int(a), int(b)])
}

fn opt_str(s: &Option<String>) -> Value {
    match s {
        Some(s) => Value::Str(s.clone()),
        None => Value::Null,
    }
}

fn opt_float(n: Option<f64>) -> Value {
    match n {
        Some(n) => Value::Num(n),
        None => Value::Null,
    }
}

fn str_arr(xs: &[String]) -> Value {
    Value::Arr(xs.iter().map(|s| Value::Str(s.clone())).collect())
}

impl Document {
    /// 写成 `.gtui` 文本.键序固定,末尾恰好一个换行.
    pub fn save(&self) -> String {
        // 老文件光标存的是框 id:写出前换成格子(v2 的写法)
        let mut view = self.view.clone();
        if let Some(Cursor::Box(_)) = view.cursor {
            view.cursor = Some(Cursor::Cell(view.cursor_cell(self)));
        }
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                Value::Obj(vec![
                    ("id".to_string(), Value::Str(g.id.clone())),
                    ("label".to_string(), Value::Str(g.label.clone())),
                    ("color".to_string(), Value::Str(hex(g.color))),
                ])
            })
            .collect();

        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|l| {
                Value::Obj(vec![
                    ("name".to_string(), Value::Str(l.name.clone())),
                    ("order".to_string(), int(l.order)),
                ])
            })
            .collect();

        let boxes: Vec<Value> = self
            .boxes
            .iter()
            .map(|b| {
                Value::Obj(vec![
                    ("id".to_string(), Value::Str(b.id.clone())),
                    ("at".to_string(), pair(b.at.0, b.at.1)),
                    (
                        "size".to_string(),
                        pair(i32::from(b.size.0), i32::from(b.size.1)),
                    ),
                    ("style".to_string(), Value::Str(b.style.name().to_string())),
                    ("color".to_string(), opt_color(b.color)),
                    ("fill".to_string(), opt_color(b.fill)),
                    ("layer".to_string(), Value::Str(b.layer.clone())),
                    (
                        "edge".to_string(),
                        b.edge.clone().map(Value::Str).unwrap_or(Value::Null),
                    ),
                    ("terms".to_string(), str_arr(&b.terms)),
                ])
            })
            .collect();

        let texts: Vec<Value> = self
            .texts
            .iter()
            .map(|t| {
                Value::Obj(vec![
                    ("id".to_string(), Value::Str(t.id.clone())),
                    ("text".to_string(), Value::Str(t.text.clone())),
                    ("at".to_string(), pair(t.at.0, t.at.1)),
                    ("box".to_string(), Value::Str(t.box_id.clone())),
                    ("color".to_string(), opt_color(t.color)),
                    ("bg".to_string(), opt_color(t.bg)),
                    ("align".to_string(), Value::Str(t.align.name().to_string())),
                    ("layer".to_string(), Value::Str(t.layer.clone())),
                ])
            })
            .collect();

        let edges: Vec<Value> = self
            .edges
            .iter()
            .map(|e| {
                Value::Obj(vec![
                    ("id".to_string(), Value::Str(e.id.clone())),
                    ("from".to_string(), Value::Str(e.from.clone())),
                    ("to".to_string(), Value::Str(e.to.clone())),
                    ("dir".to_string(), Value::Str(e.dir.name().to_string())),
                    ("group".to_string(), Value::Str(e.group.clone())),
                    ("style".to_string(), Value::Str(e.style.name().to_string())),
                    ("color".to_string(), opt_color(e.color)),
                    ("bg".to_string(), opt_color(e.bg)),
                    (
                        "via".to_string(),
                        Value::Arr(e.via.iter().map(|&(x, y)| pair(x, y)).collect()),
                    ),
                    (
                        "corner".to_string(),
                        Value::Str(e.corner.name().to_string()),
                    ),
                    (
                        "from_side".to_string(),
                        Value::Str(
                            e.from_side
                                .map(|s| s.name().to_string())
                                .unwrap_or_else(|| "auto".to_string()),
                        ),
                    ),
                    (
                        "to_side".to_string(),
                        Value::Str(
                            e.to_side
                                .map(|s| s.name().to_string())
                                .unwrap_or_else(|| "auto".to_string()),
                        ),
                    ),
                    (
                        "head".to_string(),
                        Value::Str(
                            e.head
                                .map(|h| h.name().to_string())
                                .unwrap_or_else(|| "auto".to_string()),
                        ),
                    ),
                    ("layer".to_string(), Value::Str(e.layer.clone())),
                ])
            })
            .collect();

        let view = Value::Obj(vec![
            (
                "center".to_string(),
                Value::Arr(vec![
                    Value::Num(self.view.center.0),
                    Value::Num(self.view.center.1),
                ]),
            ),
            ("zoom".to_string(), Value::Num(self.view.zoom)),
            ("layout".to_string(), Value::Str(self.view.layout.clone())),
            (
                "cursor".to_string(),
                match &view.cursor {
                    Some(Cursor::Cell((x, y))) => {
                        Value::Arr(vec![Value::Int(*x as i64), Value::Int(*y as i64)])
                    }
                    Some(Cursor::Box(id)) => Value::Str(id.clone()),
                    None => Value::Null,
                },
            ),
            ("focus".to_string(), str_arr(&self.view.focus)),
            ("labels".to_string(), Value::Bool(self.view.labels)),
            ("lang".to_string(), Value::Str(self.view.lang.clone())),
            ("layer".to_string(), opt_str(&self.view.layer)),
            ("side".to_string(), Value::Num(self.view.side)),
            ("cycle".to_string(), opt_float(self.view.cycle)),
            (
                "orientation".to_string(),
                match self.view.orientation.as_str() {
                    "auto" => Value::Str("auto".to_string()),
                    d => Value::Int(d.parse().unwrap_or(0)),
                },
            ),
            ("notes".to_string(), Value::Bool(self.view.notes)),
        ]);

        let regions: Vec<Value> = self
            .regions
            .iter()
            .map(|r| {
                let color = match r.color {
                    Some((a, b, c)) => Value::Str(format!("#{a:02x}{b:02x}{c:02x}")),
                    None => Value::Null,
                };
                let members: Vec<Value> = r.members.iter().map(|m| Value::Str(m.clone())).collect();
                Value::Obj(vec![
                    ("id".to_string(), Value::Str(r.id.clone())),
                    ("label".to_string(), Value::Str(r.label.clone())),
                    ("color".to_string(), color),
                    ("style".to_string(), Value::Str(r.style.name().to_string())),
                    ("layer".to_string(), Value::Str(r.layer.clone())),
                    ("padding".to_string(), Value::Int(r.padding as i64)),
                    ("members".to_string(), Value::Arr(members)),
                    ("collapsed".to_string(), Value::Bool(r.collapsed)),
                ])
            })
            .collect();
        let root = Value::Obj(vec![
            ("type".to_string(), Value::Str("graph-tui".to_string())),
            ("version".to_string(), Value::Int(10)),
            ("source".to_string(), Value::Str(self.source.clone())),
            ("unit".to_string(), Value::Str("cell".to_string())),
            ("groups".to_string(), Value::Arr(groups)),
            ("layers".to_string(), Value::Arr(layers)),
            ("boxes".to_string(), Value::Arr(boxes)),
            ("texts".to_string(), Value::Arr(texts)),
            ("edges".to_string(), Value::Arr(edges)),
            ("view".to_string(), view),
            ("regions".to_string(), Value::Arr(regions)),
        ]);

        // save 的签名不能返回错误;非有限值属于调用方违约,直接暴露出来,
        // 不能静默写成 null.
        if let Err(e) = json::check_finite(&root) {
            panic!("document save: {e}");
        }

        let mut out = json::pretty(&root);
        out.push('\n');
        out
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;

    fn fixture() -> Document {
        let groups = vec![
            Group {
                id: "g0".to_string(),
                label: "一号线".to_string(),
                color: (76, 142, 248),
            },
            Group {
                id: "g1".to_string(),
                label: String::new(),
                color: (240, 120, 80),
            },
        ];
        let layers = vec![
            Layer {
                name: String::new(),
                order: 0,
            },
            Layer {
                name: "base".to_string(),
                order: 1,
            },
        ];
        let boxes = vec![
            DocBox {
                id: "b1".to_string(),
                at: (10, 4),
                // 文字 30 列宽:框要装得下(加载时校验这一点)
                size: (34, 3),
                style: BoxStyle::Rounded,
                color: Some((76, 142, 248)),
                fill: Some((20, 30, 40)),
                layer: "base".to_string(),
                terms: vec!["central".to_string(), "zx".to_string()],
                edge: None,
            },
            DocBox {
                id: "b2".to_string(),
                at: (20, 10),
                size: (1, 1),
                style: BoxStyle::Point,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            },
            DocBox {
                id: "b3".to_string(),
                at: (30, 20),
                size: (6, 3),
                style: BoxStyle::Single,
                color: None,
                fill: Some((1, 2, 3)),
                layer: "base".to_string(),
                terms: vec!["end".to_string()],
                edge: None,
            },
        ];
        let texts = vec![
            Text {
                id: "t1".to_string(),
                text: "中央 \"引号\" \\反斜杠\\ **粗体** *斜体*".to_string(),
                at: (11, 5),
                box_id: "b1".to_string(),
                color: Some((250, 250, 250)),
                bg: None,
                align: Align::Right,
                layer: "base".to_string(),
            },
            Text {
                id: "t2".to_string(),
                text: "点".to_string(),
                at: (20, 10),
                box_id: "b2".to_string(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
            Text {
                id: "t3".to_string(),
                text: "末尾".to_string(),
                at: (31, 21),
                box_id: "b3".to_string(),
                color: None,
                bg: None,
                align: Align::Center,
                layer: "base".to_string(),
            },
        ];
        let edges = vec![
            DocEdge {
                id: "e1".to_string(),
                from: "b1".to_string(),
                to: "b2".to_string(),
                dir: Dir::Both,
                group: "g0".to_string(),
                style: EdgeStyle::Dashed,
                color: Some((200, 200, 10)),
                bg: None,
                via: vec![(12, 6), (12, 9)],
                corner: Corner::Sharp,
                layer: "base".to_string(),
                from_side: None,
                to_side: None,
                head: None,
            },
            DocEdge {
                id: "e2".to_string(),
                from: "b1".to_string(),
                to: "b3".to_string(),
                dir: Dir::Forward,
                group: "g1".to_string(),
                style: EdgeStyle::Dotted,
                color: None,
                bg: None,
                via: Vec::new(),
                corner: Corner::Rounded,
                layer: String::new(),
                from_side: None,
                to_side: None,
                head: None,
            },
        ];
        let view = ViewState {
            center: (20.5, 10.25),
            zoom: 1.5,
            layout: "force".to_string(),
            cursor: Some(Cursor::Box("b2".to_string())),
            focus: vec!["g0".to_string(), "g1".to_string()],
            labels: true,
            lang: "en".to_string(),
            layer: Some("base".to_string()),
            side: 1.5,
            cycle: Some(30.0),
            orientation: "auto".to_string(),
            notes: true,
        };
        Document::from_parts("graph-tui 0.2.0", groups, layers, boxes, texts, edges, view).unwrap()
    }

    /// 拿一份合法文档,改一棵 JSON 树后再写回文本
    fn mutate(f: impl FnOnce(&mut Vec<(String, Value)>)) -> String {
        let text = fixture().save();
        let mut root = match json::parse(&text).unwrap() {
            Value::Obj(o) => o,
            _ => panic!("root is not an object"),
        };
        f(&mut root);
        json::pretty(&Value::Obj(root))
    }

    fn field<'a>(o: &'a mut [(String, Value)], key: &str) -> &'a mut Value {
        &mut o
            .iter_mut()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .1
    }

    fn as_arr(v: &mut Value) -> &mut Vec<Value> {
        match v {
            Value::Arr(a) => a,
            _ => panic!("not an array"),
        }
    }

    fn as_obj(v: &mut Value) -> &mut Vec<(String, Value)> {
        match v {
            Value::Obj(o) => o,
            _ => panic!("not an object"),
        }
    }

    fn load_err(text: &str) -> String {
        match Document::load(text) {
            Ok(_) => panic!("expected load to fail"),
            Err(e) => e,
        }
    }

    #[test]
    fn 全字段往返相等() {
        let d = fixture();
        let text = d.save();
        let back = Document::load(&text).unwrap();
        assert_eq!(back.source, d.source);
        assert_eq!(back.groups, d.groups);
        assert_eq!(back.layers, d.layers);
        assert_eq!(back.boxes, d.boxes);
        assert_eq!(back.texts, d.texts);
        assert_eq!(back.edges, d.edges);
        // 光标写出去统一换成格子(v2),比较解析后的位置
        assert_eq!(back.view.cursor_cell(&back), d.view.cursor_cell(&d));
        assert_eq!(
            ViewState {
                cursor: None,
                ..back.view.clone()
            },
            ViewState {
                cursor: None,
                ..d.view.clone()
            }
        );
    }

    #[test]
    fn 特殊文本往返正确() {
        let d = fixture();
        let back = Document::load(&d.save()).unwrap();
        assert_eq!(
            back.texts[0].text,
            "中央 \"引号\" \\反斜杠\\ **粗体** *斜体*"
        );
        assert_eq!(back.texts[1].text, "点");
    }

    #[test]
    fn 一个框挂两条文字时失败() {
        let text = mutate(|root| {
            let texts = as_arr(field(root, "texts"));
            let t = as_obj(&mut texts[1]);
            *field(t, "box") = Value::Str("b1".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("needs exactly one text"), "{e}");
    }

    #[test]
    fn 文字指向不存在的框时失败() {
        let text = mutate(|root| {
            let texts = as_arr(field(root, "texts"));
            let t = as_obj(&mut texts[1]);
            *field(t, "box") = Value::Str("nope".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("texts[1].box"), "{e}");
    }

    #[test]
    fn 重复_id_路径() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            let b = as_obj(&mut boxes[1]);
            *field(b, "id") = Value::Str("b1".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("boxes[1].id"), "{e}");
        assert!(e.contains("duplicate id"), "{e}");
    }

    #[test]
    fn 悬空引用路径() {
        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[0]);
            *field(e, "to") = Value::Str("nope".to_string());
        });
        let err = load_err(&text);
        assert!(err.contains("edges[0].to"), "{err}");
    }

    #[test]
    fn 未知图层路径() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            let b = as_obj(&mut boxes[0]);
            *field(b, "layer") = Value::Str("nope".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("boxes[0].layer"), "{e}");
    }

    #[test]
    fn 未知键路径() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            let b = as_obj(&mut boxes[0]);
            b.push(("bogus".to_string(), Value::Int(1)));
        });
        let e = load_err(&text);
        assert!(e.contains("boxes[0].bogus"), "{e}");
    }

    #[test]
    fn 重复键失败() {
        let e = load_err(r#"{"type":"graph-tui","type":"graph-tui"}"#);
        assert!(e.contains("duplicate key"), "{e}");
        assert!(e.contains("type"), "{e}");
    }

    #[test]
    fn 非整数坐标路径() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            let b = as_obj(&mut boxes[0]);
            let at = as_arr(field(b, "at"));
            at[0] = Value::Num(10.5);
        });
        let e = load_err(&text);
        assert!(e.contains("boxes[0].at"), "{e}");
    }

    #[test]
    fn 保存稳定且末尾只有一个换行() {
        let d = fixture();
        let a = d.save();
        assert_eq!(a, d.save());
        let b = Document::load(&a).unwrap().save();
        assert_eq!(a, b);

        assert!(a.ends_with('\n'));
        assert_eq!(a.chars().rev().take_while(|&c| c == '\n').count(), 1);
        assert!(a.lines().all(|line| !line.ends_with(' ')), "{a:?}");
        assert!(a.contains("\"type\": \"graph-tui\""));
        assert!(a.contains("\"unit\": \"cell\""));
    }

    #[test]
    fn 新字段保存加载后逐项相等() {
        let d = fixture();
        let back = Document::load(&d.save()).unwrap();
        assert_eq!(back.boxes[0].fill, Some((20, 30, 40)));
        assert_eq!(back.boxes[1].fill, None);
        assert_eq!(back.boxes[2].fill, Some((1, 2, 3)));
        assert_eq!(back.texts[0].align, Align::Right);
        assert_eq!(back.texts[1].align, Align::Left);
        assert_eq!(back.texts[2].align, Align::Center);
        assert_eq!(back.edges[0].dir, Dir::Both);
        assert_eq!(back.edges[1].dir, Dir::Forward);
        assert_eq!(back.edges[0].corner, Corner::Sharp);
        assert_eq!(back.edges[1].corner, Corner::Rounded);
    }

    #[test]
    /// v1 的光标是框 id:读进来还能用,存出去变成格子(v2)
    fn v1光标迁到格子() {
        let text = fixture().save();
        let mut root = match json::parse(&text).unwrap() {
            Value::Obj(o) => o,
            _ => panic!(),
        };
        // 造一份 v1:光标写框 id
        for (k, v) in root.iter_mut() {
            match (k.as_str(), v) {
                ("version", slot) => *slot = Value::Int(1),
                ("view", Value::Obj(vo)) => {
                    for (vk, vv) in vo.iter_mut() {
                        if vk == "cursor" {
                            *vv = Value::Str("b2".to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        let text = json::pretty(&Value::Obj(root));
        let doc = Document::load(&text).expect("v1 能读");
        let want = doc.text(doc.box_idx("b2").unwrap()).unwrap().at;
        assert_eq!(doc.view.cursor_cell(&doc), want, "v1 光标落到框的文字起点");

        let out = doc.save();
        let back = Document::load(&out).expect("v2 能读");
        assert_eq!(back.view.cursor, Some(Cursor::Cell(want)), "写出的是格子");
        assert!(out.contains("\"version\": 10"), "写出去该是 v10");
    }

    #[test]
    fn 旧文档缺新字段时按默认值加载() {
        // 手写的旧格式:boxes/texts/edges 都不带 fill/align/dir/corner
        let old = r##"{
  "type": "graph-tui",
  "version": 1,
  "source": "old",
  "unit": "cell",
  "groups": [{"id": "g0", "label": "", "color": "#4c8ef8"}],
  "layers": [{"name": "", "order": 0}],
  "boxes": [
    {"id": "a", "at": [0, 0], "size": [5, 3], "style": "single",
     "color": null, "layer": "", "terms": []},
    {"id": "b", "at": [10, 0], "size": [1, 1], "style": "point",
     "color": null, "layer": "", "terms": []}
  ],
  "texts": [
    {"id": "ta", "text": "A", "at": [1, 1], "box": "a",
     "color": null, "layer": ""},
    {"id": "tb", "text": "B", "at": [10, 0], "box": "b",
     "color": null, "layer": ""}
  ],
  "edges": [
    {"id": "e", "from": "a", "to": "b", "group": "g0", "style": "solid",
     "color": null, "via": [], "layer": ""}
  ],
  "view": {"center": [0, 0], "zoom": 1, "layout": "force", "cursor": null,
           "focus": [], "labels": false, "lang": "en", "layer": null,
           "side": 1, "cycle": null}
}"##;
        let d = Document::load(old).unwrap();
        assert!(d.view.notes, "老文件没有 view.notes:按开读");
        assert_eq!(d.boxes[0].fill, None);
        assert_eq!(d.texts[0].align, Align::Left);
        assert_eq!(d.edges[0].dir, Dir::Forward);
        assert_eq!(d.edges[0].corner, Corner::Rounded);

        // 兼容读取得到的文档再保存时把新字段补全
        let text = d.save();
        assert!(text.contains("\"dir\": \"forward\""));
        assert!(text.contains("\"fill\": null"));
        assert!(text.contains("\"align\": \"left\""));
        assert!(text.contains("\"corner\": \"rounded\""));
        let back = Document::load(&text).unwrap();
        assert_eq!(back.boxes, d.boxes);
        assert_eq!(back.texts, d.texts);
        assert_eq!(back.edges, d.edges);
    }

    /// v8 里的图例:读的时候接受,内容丢弃;存出去是 v9 且不再有 legend
    #[test]
    fn 旧图例读取丢弃并存成v10() {
        let text = fixture().save();
        let mut root = match json::parse(&text).unwrap() {
            Value::Obj(o) => o,
            _ => panic!(),
        };
        for (k, v) in root.iter_mut() {
            match (k.as_str(), v) {
                ("version", slot) => *slot = Value::Int(8),
                ("view", Value::Obj(vo)) => {
                    vo.push(("legend".to_string(), Value::Bool(false)));
                }
                _ => {}
            }
        }
        root.push((
            "legend".to_string(),
            json::parse(
                r#"{"title": "转移", "entries": [{"text": "CLOSED -> LISTEN", "group": "g0"}]}"#,
            )
            .unwrap(),
        ));
        let old = json::pretty(&Value::Obj(root));
        let want = fixture().boxes.len();
        let doc = Document::load(&old).expect("带图例的 v8 该能读");
        assert_eq!(doc.boxes.len(), want, "图例不变成框");
        let out = doc.save();
        assert!(out.contains("\"version\": 10"), "写出去该是 v10");
        assert!(!out.contains("legend"), "存出去不该再有 legend:{out}");
        assert!(!out.contains("转移"), "图例内容不迁移:{out}");
        // v9 里就算写了 legend 也忽略
        let mut root = match json::parse(&out).unwrap() {
            Value::Obj(o) => o,
            _ => panic!(),
        };
        root.push(("legend".to_string(), Value::Bool(true)));
        let back = Document::load(&json::pretty(&Value::Obj(root))).expect("v9 读到多余字段");
        assert_eq!(back.boxes.len(), want);
    }

    /// v6 文件没有 regions:读进来是空表,存出去升 v10 并带上空 regions
    #[test]
    fn 老版本没有区域() {
        let d = crate::document::Document::scratch();
        let text = d.save();
        // 把 "regions" 那一行连同上一行末尾的逗号一起删掉,再把版本改回 6
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let idx = lines
            .iter()
            .position(|l| l.trim_start().starts_with("\"regions\""))
            .expect("v8 写出应包含 regions");
        lines.remove(idx);
        if idx > 0 {
            let prev = lines[idx - 1].trim_end().trim_end_matches(',').to_string();
            lines[idx - 1] = prev;
        }
        let old = lines
            .join("\n")
            .replace("\"version\": 10", "\"version\": 6");
        let back = crate::document::Document::load(&old).expect("v6 该能读");
        assert!(back.regions.is_empty(), "v6 没有区域");
        let out = back.save();
        assert!(out.contains("\"version\": 10"), "存出去升 v10");
        assert!(out.contains("\"regions\": []"), "写空数组:{out}");
        let again = crate::document::Document::load(&out).expect("回读");
        assert_eq!(again.regions, back.regions);
    }

    /// v6:多行文字往返一致;\r 拒
    #[test]
    fn 多行文字往返一致() {
        let mut d = fixture();
        d.set_text(0, "第一行\n第二行");
        let text = d.save();
        assert!(text.contains("\\n"), "换行要写成 JSON 的 \\n:{text}");
        let back = Document::load(&text).unwrap();
        assert_eq!(back.text(0).unwrap().text, "第一行\n第二行");
        assert_eq!(back.boxes[0].size, d.boxes[0].size, "框高跟着行数");
        assert_eq!(back.save(), text, "重复保存字节相同");

        let bad = mutate(|root| {
            let texts = as_arr(field(root, "texts"));
            let t = as_obj(&mut texts[0]);
            *field(t, "text") = Value::Str("a\rb".to_string());
        });
        let e = load_err(&bad);
        assert!(e.contains("texts[0].text"), "{e}");
    }

    /// 每条边的箭头字形存进文件;auto 写 auto,读回来是 None
    #[test]
    fn 边箭头往返() {
        let mut d = fixture();
        d.edges[0].head = Some(crate::model::ArrowStyle::Triangles);
        d.edges[1].head = None;
        let text = d.save();
        assert!(text.contains("\"head\": \"triangles\""), "{text}");
        assert!(text.contains("\"head\": \"auto\""), "{text}");
        let back = Document::load(&text).expect("读回来");
        assert_eq!(
            back.edges[0].head,
            Some(crate::model::ArrowStyle::Triangles)
        );
        assert_eq!(back.edges[1].head, None);
        assert_eq!(back.save(), text, "重复保存字节相同");
        // 坏值要报错,不是静默当成 auto
        let bad = text.replace("\"head\": \"triangles\"", "\"head\": \"square\"");
        let Err(err) = Document::load(&bad) else {
            panic!("坏 head 该报错");
        };
        assert!(err.contains("head") && err.contains("square"), "{err}");
    }

    /// v5:注释框的 edge 绑定往返一致(位置、颜色都存)
    #[test]
    fn 注释框往返一致() {
        let mut d = fixture();
        let eid = d.edges[0].id.clone();
        let b = d.new_note(0).expect("该能建注释框");
        d.set_text(b, "收到 SYN");
        let text = d.save();
        assert!(text.contains("\"version\": 10"), "写出去该是 v10");
        assert!(text.contains("\"edge\": \""), "{text}");
        let back = Document::load(&text).unwrap();
        let nb = back.note_of_edge(&eid).expect("读回来该还有注释框");
        assert_eq!(back.text(nb).unwrap().text, "收到 SYN");
        let again = back.save();
        eprintln!("len {} vs {}", text.len(), again.len());
        if again != text {
            for (i, (a, b)) in text.lines().zip(again.lines()).enumerate() {
                if a != b {
                    eprintln!("line {i}:\n  first: {a}\n  again: {b}");
                }
            }
        }
        assert_eq!(again, text, "重复保存字节相同");
    }

    /// v4 老文件:边上的 label 读进来变成注释框,再存成 v5
    #[test]
    fn v4边标签迁成注释框() {
        let text = mutate(|root| {
            *field(root, "version") = Value::Int(4);
            let edges = as_arr(field(root, "edges"));
            let o = as_obj(&mut edges[0]);
            o.push(("label".to_string(), Value::Str("收到 SYN".to_string())));
            o.push((
                "label_at".to_string(),
                Value::Arr(vec![Value::Int(7), Value::Int(9)]),
            ));
            o.push(("label_color".to_string(), Value::Null));
        });
        let d = Document::load(&text).expect("v4 该能读");
        let b = d.note_of_edge(&d.edges[0].id).expect("该迁成注释框");
        assert_eq!(d.text(b).unwrap().text, "收到 SYN");
        assert_eq!(d.boxes[b].at, (7, 9), "label_at 就是框的位置");
        let out = d.save();
        assert!(out.contains("\"version\": 10"), "迁移后写 v10");
        assert!(!out.contains("\"label_at\":"), "边上不再有 label 键");
    }

    /// v3 老文件没有标签键:照读,不会凭空多出注释框
    #[test]
    fn v3没有标签就不建注释框() {
        let text = mutate(|root| {
            *field(root, "version") = Value::Int(3);
            let edges = as_arr(field(root, "edges"));
            for e in edges.iter_mut() {
                let o = as_obj(e);
                o.retain(|(k, _)| k != "label" && k != "label_at" && k != "label_color");
            }
        });
        let d = Document::load(&text).expect("v3 该能读");
        assert!(d.boxes.iter().all(|b| b.edge.is_none()));
    }

    /// 框缺 style 时按默认样式(只有文字)读,size 也就按一行算
    #[test]
    fn 框缺样式按默认读() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            for b in boxes.iter_mut() {
                as_obj(b).retain(|(k, _)| k != "style");
            }
            // 默认样式是一行,尺寸要跟着改,否则会被尺寸校验拒掉
            // (b1 的文字 30 列宽,框要留够)
            let ats = [(2, 5), (20, 10), (30, 20)];
            for (i, b) in boxes.iter_mut().enumerate() {
                let o = as_obj(b);
                let w = if i == 0 { 34 } else { 6 };
                *field(o, "size") = Value::Arr(vec![Value::Int(w), Value::Int(1)]);
                *field(o, "at") = Value::Arr(vec![Value::Int(ats[i].0), Value::Int(ats[i].1)]);
            }
            // 文字位置:只有文字时就是框自己的左上角
            let texts = as_arr(field(root, "texts"));
            for (i, t) in texts.iter_mut().enumerate() {
                let o = as_obj(t);
                *field(o, "at") = Value::Arr(vec![Value::Int(ats[i].0), Value::Int(ats[i].1)]);
            }
        });
        let d = Document::load(&text).expect("缺 style 该按默认读");
        assert!(d
            .boxes
            .iter()
            .all(|b| b.style == crate::model::BoxStyle::None));
    }

    /// 边标签的坏值都带 JSON 路径
    #[test]
    fn 边标签坏值带路径() {
        // 多行标签现在是合法的(注释框放得下),只有回车不收
        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[0]);
            e.push(("label".to_string(), Value::Str("a\rb".to_string())));
        });
        let e = load_err(&text);
        assert!(e.contains("edges[0].label"), "{e}");

        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[0]);
            e.push(("label".to_string(), Value::Int(3)));
        });
        let e = load_err(&text);
        assert!(e.contains("edges[0].label"), "{e}");

        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[0]);
            e.push(("label_at".to_string(), Value::Str("x".to_string())));
        });
        let e = load_err(&text);
        assert!(e.contains("edges[0].label_at"), "{e}");

        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[0]);
            e.push(("label_color".to_string(), Value::Str("red".to_string())));
        });
        let e = load_err(&text);
        assert!(e.contains("edges[0].label_color"), "{e}");
    }

    /// 图例已经不再解析:里面的坏值(悬空分组、未知字形、多行)都不再报错,直接丢弃
    #[test]
    fn 旧图例坏值不再报错() {
        let text = mutate(|root| {
            root.push((
                "legend".to_string(),
                Value::Obj(vec![
                    ("title".to_string(), Value::Str("a\nb".to_string())),
                    (
                        "entries".to_string(),
                        Value::Arr(vec![Value::Obj(vec![
                            ("text".to_string(), Value::Str("x".to_string())),
                            ("group".to_string(), Value::Str("nope".to_string())),
                            ("glyph".to_string(), Value::Str("wave".to_string())),
                        ])]),
                    ),
                ]),
            ));
        });
        let d = Document::load(&text).expect("图例字段只读不校验");
        assert!(!d.save().contains("legend"), "存出去不该再有 legend");
    }

    #[test]
    fn 新字段坏值带路径() {
        let text = mutate(|root| {
            let boxes = as_arr(field(root, "boxes"));
            let b = as_obj(&mut boxes[0]);
            *field(b, "fill") = Value::Str("red".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("boxes[0].fill"), "{e}");

        let text = mutate(|root| {
            let texts = as_arr(field(root, "texts"));
            let t = as_obj(&mut texts[0]);
            *field(t, "align") = Value::Str("middle".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("texts[0].align"), "{e}");

        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[1]);
            *field(e, "dir") = Value::Str("sideways".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("edges[1].dir"), "{e}");

        let text = mutate(|root| {
            let edges = as_arr(field(root, "edges"));
            let e = as_obj(&mut edges[1]);
            *field(e, "corner") = Value::Str("hard".to_string());
        });
        let e = load_err(&text);
        assert!(e.contains("edges[1].corner"), "{e}");
    }

    #[test]
    fn 保存时_forward_也写出_dir() {
        let text = fixture().save();
        assert!(text.contains("\"dir\": \"forward\""), "{text}");
    }
}
