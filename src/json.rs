//! 极小的 JSON 读写,只够 JSON Canvas 用.
//!
//! 读是手写的递归下降:对象、数组、字符串(含 `\uXXXX` 与常见转义)、
//! 数字、true/false/null.报告错误时带上出错的字节位置.
//! 写只输出紧凑格式:不要多余空格,读回来逐项相等即可.

use std::fmt::Write as _;

/// 一棵 JSON 值.对象保持输入顺序(重复键也原样留着,取值取第一个)
///
/// `parse` 一律产出 `Num`;`Int` 是写文档时用的:坐标、尺寸、序号这些整数
/// 要原样写成 `10`,而视图里的 `zoom` / `center` 要写成 `1.0` / `20.0`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Int(i64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// 对象的某个键;不是对象或没有这个键就是 None
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// 必须是整数值,并且落在 i32 范围内
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Int(i) => i32::try_from(*i).ok(),
            Value::Num(n) => {
                if n.is_finite()
                    && n.fract() == 0.0
                    && *n >= i32::MIN as f64
                    && *n <= i32::MAX as f64
                {
                    Some(*n as i32)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.as_num()
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(xs) => Some(xs.as_slice()),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Obj(fields) => Some(fields.as_slice()),
            _ => None,
        }
    }
}

/// 解析一段 JSON 文本.末尾除空白外还有内容也算错.
pub fn parse(s: &str) -> Result<Value, String> {
    parse_impl(s, false)
}

/// 同 `parse`,但拒绝同一个对象里的重复键.错误带字节位置与键路径.
pub fn parse_strict(s: &str) -> Result<Value, String> {
    parse_impl(s, true)
}

fn parse_impl(s: &str, strict: bool) -> Result<Value, String> {
    let mut p = Parser {
        src: s,
        i: 0,
        depth: 0,
        strict,
        path: String::new(),
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    src: &'a str,
    i: usize,
    depth: usize,
    strict: bool,
    /// 当前值在文档里的位置,只用于重复键报错
    path: String,
}

/// 嵌套上限:坏输入不该把栈递归穿
const MAX_DEPTH: usize = 256;

/// 把键接到路径后面.普通键用 `.key`,带点/空格的键用 `["..."]`.
fn join_path(base: &str, key: &str) -> String {
    let simple = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if simple {
        if base.is_empty() {
            key.to_string()
        } else {
            let mut s = String::with_capacity(base.len() + key.len() + 1);
            s.push_str(base);
            s.push('.');
            s.push_str(key);
            s
        }
    } else {
        let mut s = String::with_capacity(base.len() + key.len() + 4);
        s.push_str(base);
        s.push('[');
        s.push_str(&escape(key));
        s.push(']');
        s
    }
}

/// 数组下标接路径
fn join_index(base: &str, i: usize) -> String {
    format!("{base}[{i}]")
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> String {
        format!("json: byte {}: {}", self.i, msg)
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.i).copied()
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("expected a value")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn literal(&mut self, word: &str, v: Value) -> Result<Value, String> {
        let end = self.i + word.len();
        if self.src.as_bytes().get(self.i..end) == Some(word.as_bytes()) {
            self.i = end;
            Ok(v)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.i += 1;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let mut out = Vec::new();
        self.ws();
        if self.eat(b'}') {
            self.depth -= 1;
            return Ok(Value::Obj(out));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let key_pos = self.i;
            let key = self.string()?;
            if self.strict && out.iter().any(|(k, _)| k == &key) {
                return Err(format!(
                    "json: byte {}: duplicate key {:?} at {}",
                    key_pos,
                    key,
                    join_path(&self.path, &key)
                ));
            }
            self.ws();
            if !self.eat(b':') {
                return Err(self.err("expected ':'"));
            }
            self.ws();
            let child = join_path(&self.path, &key);
            let saved = std::mem::replace(&mut self.path, child);
            let v = self.value()?;
            self.path = saved;
            out.push((key, v));
            self.ws();
            if self.eat(b'}') {
                self.depth -= 1;
                return Ok(Value::Obj(out));
            }
            if !self.eat(b',') {
                return Err(self.err("expected ',' or '}'"));
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.i += 1;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let mut out = Vec::new();
        self.ws();
        if self.eat(b']') {
            self.depth -= 1;
            return Ok(Value::Arr(out));
        }
        loop {
            self.ws();
            let child = join_index(&self.path, out.len());
            let saved = std::mem::replace(&mut self.path, child);
            let v = self.value()?;
            self.path = saved;
            out.push(v);
            self.ws();
            if self.eat(b']') {
                self.depth -= 1;
                return Ok(Value::Arr(out));
            }
            if !self.eat(b',') {
                return Err(self.err("expected ',' or ']'"));
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let mut n: u16 = 0;
        for _ in 0..4 {
            let b = self
                .peek()
                .ok_or_else(|| self.err("unexpected end in \\u escape"))?;
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u16,
                b'a'..=b'f' => (b - b'a' + 10) as u16,
                b'A'..=b'F' => (b - b'A' + 10) as u16,
                _ => return Err(self.err("invalid hex digit")),
            };
            n = n * 16 + d;
            self.i += 1;
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        // 调用处保证当前是引号
        self.i += 1;
        let mut out = String::new();
        loop {
            let b = match self.peek() {
                Some(b) => b,
                None => return Err(self.err("unterminated string")),
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = self
                        .peek()
                        .ok_or_else(|| self.err("unexpected end in escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xd800..=0xdbff).contains(&hi) {
                                let bytes = self.src.as_bytes();
                                if bytes.get(self.i) == Some(&b'\\')
                                    && bytes.get(self.i + 1) == Some(&b'u')
                                {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if !(0xdc00..=0xdfff).contains(&lo) {
                                        return Err(self.err("invalid surrogate pair"));
                                    }
                                    0x10000
                                        + (((hi as u32) - 0xd800) << 10)
                                        + ((lo as u32) - 0xdc00)
                                } else {
                                    return Err(self.err("unpaired high surrogate"));
                                }
                            } else if (0xdc00..=0xdfff).contains(&hi) {
                                return Err(self.err("unpaired low surrogate"));
                            } else {
                                hi as u32
                            };
                            let c =
                                char::from_u32(cp).ok_or_else(|| self.err("invalid code point"))?;
                            out.push(c);
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                }
                0x00..=0x1f => return Err(self.err("control character in string")),
                _ => {
                    // 输入的 &str 本身就是合法 UTF-8,按字符推进即可
                    let c = self.src[self.i..]
                        .chars()
                        .next()
                        .ok_or_else(|| self.err("unterminated string"))?;
                    out.push(c);
                    self.i += c.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        // 负号后面必须还有数字,交给下面统一判断
        self.eat(b'-');
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                while let Some(b'0'..=b'9') = self.peek() {
                    self.i += 1;
                }
            }
            _ => return Err(self.err("invalid number")),
        }
        if self.eat(b'.') {
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digits after '.'"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digits in exponent"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
        }
        let text = &self.src[start..self.i];
        let n = text
            .parse::<f64>()
            .map_err(|_| self.err("invalid number"))?;
        if !n.is_finite() {
            return Err(self.err("number out of range"));
        }
        Ok(Value::Num(n))
    }
}

/// 写一个 JSON 字符串(两侧引号一起给,转义好内部字符).
/// 非 ASCII 原样保留;控制字符写成 `\u00XX`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 写一段 JSON 文本(紧凑,不保证对象键顺序之外的东西)
pub fn write(v: &Value) -> String {
    let mut out = String::new();
    put(v, &mut out);
    out
}

fn put(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Num(n) => {
            if n.is_finite() {
                let _ = write!(out, "{n}");
            } else {
                // JSON 没有 NaN / inf
                out.push_str("null");
            }
        }
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Str(s) => out.push_str(&escape(s)),
        Value::Arr(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put(x, out);
            }
            out.push(']');
        }
        Value::Obj(fields) => {
            out.push('{');
            for (i, (k, x)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&escape(k));
                out.push(':');
                put(x, out);
            }
            out.push('}');
        }
    }
}

/// 两空格缩进的稳定输出.
///
/// 对象按 `Obj` 里的插入顺序写;数组一个元素一行,长度 0/1 也是多行;
/// 不含尾随空格,也不在末尾加换行(由调用方决定).
pub fn pretty(v: &Value) -> String {
    let mut out = String::new();
    pretty_put(v, 0, &mut out);
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn pretty_put(v: &Value, depth: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        // Num 是小数语义:整数也写成 `1.0`,和视图字段的规格一致
        Value::Num(n) => {
            if n.is_finite() {
                let _ = write!(out, "{n:?}");
            } else {
                out.push_str("null");
            }
        }
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Str(s) => out.push_str(&escape(s)),
        Value::Arr(xs) => {
            // 标量数组(坐标、尺寸、via 点、focus)写成一行:规格里的 `[10, 4]` 就是这样
            if xs
                .iter()
                .all(|x| !matches!(x, Value::Arr(_) | Value::Obj(_)))
            {
                out.push('[');
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    pretty_put(x, depth, out);
                }
                out.push(']');
                return;
            }
            out.push_str("[\n");
            for (i, x) in xs.iter().enumerate() {
                indent(out, depth + 1);
                pretty_put(x, depth + 1, out);
                if i + 1 < xs.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push(']');
        }
        Value::Obj(fields) => {
            out.push_str("{\n");
            for (i, (k, x)) in fields.iter().enumerate() {
                indent(out, depth + 1);
                out.push_str(&escape(k));
                out.push_str(": ");
                pretty_put(x, depth + 1, out);
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push('}');
        }
    }
}

/// 递归检查所有数字都是有限值.路径形如 `view.zoom`、`boxes[3].at`.
pub fn check_finite(v: &Value) -> Result<(), String> {
    match v {
        Value::Num(n) => {
            if n.is_finite() {
                Ok(())
            } else {
                Err("json: non-finite number at $".to_string())
            }
        }
        Value::Arr(xs) => {
            for (i, x) in xs.iter().enumerate() {
                check_finite_at(x, &join_index("", i))?;
            }
            Ok(())
        }
        Value::Obj(fields) => {
            for (k, x) in fields {
                check_finite_at(x, &join_path("", k))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_finite_at(v: &Value, path: &str) -> Result<(), String> {
    match v {
        Value::Num(n) => {
            if n.is_finite() {
                Ok(())
            } else {
                Err(format!("json: non-finite number at {path}"))
            }
        }
        Value::Arr(xs) => {
            for (i, x) in xs.iter().enumerate() {
                check_finite_at(x, &join_index(path, i))?;
            }
            Ok(())
        }
        Value::Obj(fields) => {
            for (k, x) in fields {
                check_finite_at(x, &join_path(path, k))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Value {
        Value::Str(x.to_string())
    }

    #[test]
    fn object_array_nested() {
        let v = parse(r#"{"a":1,"b":[true,null,"x"],"c":{"d":-2.5e1}}"#).unwrap();
        assert_eq!(v.get("a").and_then(Value::as_num), Some(1.0));
        assert_eq!(v.get("b").and_then(Value::as_arr).map(|a| a.len()), Some(3));
        assert_eq!(
            v.get("c").and_then(|c| c.get("d")).and_then(Value::as_num),
            Some(-25.0)
        );
        assert_eq!(v.get("b").and_then(Value::as_arr).unwrap()[2], s("x"));
    }

    #[test]
    fn strings_and_escapes() {
        let v = parse(r#""a\n\t\"\\\/\u4e2d""#).unwrap();
        assert_eq!(v, s("a\n\t\"\\/中"));
        assert_eq!(parse(r#""\ud83d\ude00""#).unwrap(), s("\u{1f600}"));
    }

    #[test]
    fn numbers() {
        assert_eq!(parse("-0").unwrap(), Value::Num(-0.0));
        assert_eq!(parse("1.25e2").unwrap(), Value::Num(125.0));
        assert_eq!(parse("0.5").unwrap(), Value::Num(0.5));
        assert!(parse("1.").is_err());
        assert!(parse("01").is_err());
        assert!(parse("1e").is_err());
    }

    #[test]
    fn error_has_byte_position() {
        let e = parse(r#"{"a": }"#).unwrap_err();
        assert!(e.contains("byte 6"), "{e}");
        let e = parse("[1,]").unwrap_err();
        assert!(e.contains("byte 3"), "{e}");
        let e = parse("{} x").unwrap_err();
        assert!(e.contains("trailing"), "{e}");
    }

    #[test]
    fn write_roundtrip() {
        let src = r#"{"a":[null,true,false,-3.5,"x\n中"],"b":{"c":1}}"#;
        let v = parse(src).unwrap();
        let back = parse(&write(&v)).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn escape_roundtrip() {
        let text = "he said \"hi\"\n\t中\\";
        let back = parse(&escape(text)).unwrap();
        assert_eq!(back, s(text));
    }

    #[test]
    fn 严格解析拒绝重复键并带路径() {
        let e = parse_strict(r#"{"a":1,"a":2}"#).unwrap_err();
        assert!(e.contains("duplicate key"), "{e}");
        assert!(e.contains("byte"), "{e}");
        assert!(e.contains("a"), "{e}");

        let e = parse_strict(r#"{"boxes":[{"id":"b","id":"c"}]}"#).unwrap_err();
        assert!(e.contains("boxes[0].id"), "{e}");

        // 非严格解析仍然保留重复键,取第一个
        let v = parse(r#"{"a":1,"a":2}"#).unwrap();
        assert_eq!(v.get("a").and_then(Value::as_num), Some(1.0));
    }

    #[test]
    fn 整数访问器只接受整数值() {
        assert_eq!(Value::Num(3.0).as_i32(), Some(3));
        assert_eq!(Value::Num(3.5).as_i32(), None);
        assert_eq!(Value::Num(1e20).as_i32(), None);
        assert_eq!(Value::Num(-2147483648.0).as_i32(), Some(i32::MIN));
        assert_eq!(Value::Int(7).as_i32(), Some(7));
        assert_eq!(Value::Int(i64::from(i32::MAX) + 1).as_i32(), None);
        assert_eq!(Value::Int(7).as_f64(), Some(7.0));
        assert_eq!(Value::Str("7".into()).as_i32(), None);
    }

    #[test]
    fn pretty_两空格缩进标量数组写一行() {
        let v = Value::Obj(vec![
            (
                "a".to_string(),
                Value::Arr(vec![Value::Int(1), Value::Int(2)]),
            ),
            ("b".to_string(), Value::Arr(Vec::new())),
            ("c".to_string(), Value::Num(1.0)),
            ("d".to_string(), Value::Obj(Vec::new())),
            // 嵌套数组(记录数组)还是要一行一个
            (
                "e".to_string(),
                Value::Arr(vec![Value::Obj(vec![("x".to_string(), Value::Int(1))])]),
            ),
        ]);
        let p = pretty(&v);
        assert_eq!(
            p,
            "{\n  \"a\": [1, 2],\n  \"b\": [],\n  \"c\": 1.0,\n  \"d\": {\n  },\n  \"e\": [\n    {\n      \"x\": 1\n    }\n  ]\n}"
        );
        assert!(p.lines().all(|line| !line.ends_with(' ')), "{p:?}");
        assert!(!p.ends_with('\n'));
        // 输出的仍然是合法 JSON
        assert!(parse(&p).is_ok());
    }

    #[test]
    fn finite_检查带路径() {
        let v = Value::Obj(vec![(
            "view".to_string(),
            Value::Obj(vec![
                (
                    "center".to_string(),
                    Value::Arr(vec![Value::Num(1.0), Value::Num(2.5)]),
                ),
                ("zoom".to_string(), Value::Num(0.0)),
            ]),
        )]);
        assert!(check_finite(&v).is_ok());

        let bad = Value::Obj(vec![(
            "view".to_string(),
            Value::Obj(vec![("zoom".to_string(), Value::Num(f64::INFINITY))]),
        )]);
        let e = check_finite(&bad).unwrap_err();
        assert!(e.contains("view.zoom"), "{e}");
    }
}
