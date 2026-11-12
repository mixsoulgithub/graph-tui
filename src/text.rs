//! 标签里的内联样式(GTUI inline markup v1):
//!
//! - `**粗体**` / `*斜体*` / `~~删除线~~` / `[文字](目标)`;
//! - `<u>` 下划线、`<dim>` 变暗、`<fg=#rrggbb>` / `<bg=#rrggbb>` 前景/背景,
//!   关闭用 `</u>` `</dim>` `</fg>` `</bg>`,`#rrggbb` 也可以写 `default`;
//! - 标记本身不占显示宽度,布局、对齐、宽字、搜索都不受影响;
//! - 未知标签、未闭合标签按普通文字处理;`<` 用 `\<` 转义。
//!
//! 输入行(命令行 / 搜索 / 提示行)不走这里:那些是原样文本.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// 一段同样式文字
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// 内联前景 / 背景(None = 跟基准样式)
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    /// 内联修饰(粗体 / 斜体 / 删除线 / 下划线 / 变暗)
    pub modifiers: Modifier,
    /// `[文字](目标)` 里的目标;普通文字是 None.目标不占显示宽度
    pub link: Option<String>,
}

impl Span {
    /// 纯文字、无内联样式的 span
    pub fn plain(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            fg: None,
            bg: None,
            modifiers: Modifier::empty(),
            link: None,
        }
    }

    fn with(state: State, text: String) -> Span {
        Span {
            text,
            fg: state.fg,
            bg: state.bg,
            modifiers: state.modifiers,
            link: None,
        }
    }
}

/// 临时 UI 状态(搜索命中 / 选中 / 光标脉冲):最后盖上去,内联颜色压不掉它
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextOverlay {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Modifier,
}

impl TextOverlay {
    /// 只换前景(搜索黄、光标呼吸色这类)
    pub fn fg(c: Color) -> TextOverlay {
        TextOverlay {
            fg: Some(c),
            ..TextOverlay::default()
        }
    }
}

/// 当前生效的内联样式(标签栈里存的就是它)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

/// 解析成同样式的小段
pub fn parse(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    parse_inner(&chars, true, State::default())
}

/// 内联标签
enum Tag {
    /// 开:栈里记的名字 + 效果
    Open(String, Kind),
    /// 关:名字
    Close(String),
}

/// 标签效果
enum Kind {
    Underline,
    Dim,
    Fg(Option<Color>),
    Bg(Option<Color>),
}

impl Kind {
    fn apply(&self, mut st: State) -> State {
        match self {
            Kind::Underline => st.modifiers |= Modifier::UNDERLINED,
            Kind::Dim => st.modifiers |= Modifier::DIM,
            Kind::Fg(c) => st.fg = *c,
            Kind::Bg(c) => st.bg = *c,
        }
        st
    }
}

/// 在 `<` 处认一个白名单标签:返回 (标签, 下一个位置).未知/不闭合的当普通文字
fn tag_at(chars: &[char], i: usize) -> Option<(Tag, usize)> {
    let close = (i + 1..(i + 40).min(chars.len())).find(|&k| chars[k] == '>')?;
    let raw: String = chars[i + 1..close].iter().collect::<String>();
    let raw = raw.trim().to_ascii_lowercase();
    let (shut, body) = match raw.strip_prefix('/') {
        Some(rest) => (true, rest.trim().to_string()),
        None => (false, raw.clone()),
    };
    let color = |v: &str| -> Option<Option<Color>> {
        if v.eq_ignore_ascii_case("default") {
            return Some(None);
        }
        let hex = v.strip_prefix('#')?;
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let b = |k: usize| u8::from_str_radix(&hex[k..k + 2], 16).ok();
        Some(Some(Color::Rgb(b(0)?, b(2)?, b(4)?)))
    };
    // 关闭标签只需要名字对得上
    if shut {
        return match body.as_str() {
            "u" | "dim" | "fg" | "bg" => Some((Tag::Close(body), close + 1)),
            _ => None,
        };
    }
    let (key, kind) = match body.as_str() {
        "u" => ("u", Kind::Underline),
        "dim" => ("dim", Kind::Dim),
        _ => {
            let (k, v) = body.split_once('=')?;
            match k.trim() {
                "fg" => ("fg", Kind::Fg(color(v.trim())?)),
                "bg" => ("bg", Kind::Bg(color(v.trim())?)),
                _ => return None,
            }
        }
    };
    Some((Tag::Open(key.to_string(), kind), close + 1))
}

/// 后面还有没有配对的 `</name>`:没有的话这个开标签就是普通文字
fn has_close(chars: &[char], from: usize, name: &str) -> bool {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some((Tag::Close(n), _)) = tag_at(chars, i) {
                if n == name {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// `links = false` 时 `[x](y)` 按普通文字处理(链接里套链接就是这个效果)
fn parse_inner(chars: &[char], links: bool, state: State) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span::with(state, String::new());
    // 开着哪些标签,以及开之前的状态;按栈匹配,禁止交叉
    let mut stack: Vec<(String, State)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // 转义:`\<` 就是一个字面的 `<`
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'<') {
            cur.text.push('<');
            i += 2;
            continue;
        }
        // 内联标签
        if chars[i] == '<' {
            if let Some((tag, next)) = tag_at(chars, i) {
                match tag {
                    Tag::Open(name, kind) => {
                        // 没闭合的标签按普通文字处理,不吞后面的内容
                        if !has_close(chars, next, &name) {
                            cur.text.extend(chars[i..next].iter());
                            i = next;
                            continue;
                        }
                        flush(&mut out, &mut cur);
                        stack.push((
                            name,
                            State {
                                fg: cur.fg,
                                bg: cur.bg,
                                modifiers: cur.modifiers,
                            },
                        ));
                        let st = kind.apply(State {
                            fg: cur.fg,
                            bg: cur.bg,
                            modifiers: cur.modifiers,
                        });
                        cur = Span::with(st, String::new());
                        i = next;
                        continue;
                    }
                    Tag::Close(name) => {
                        if stack.last().is_some_and(|(n, _)| *n == name) {
                            flush(&mut out, &mut cur);
                            let st = stack.pop().map(|(_, st)| st).unwrap_or(state);
                            cur = Span::with(st, String::new());
                            i = next;
                            continue;
                        }
                        // 和栈顶不符(交叉/多余):当普通文字
                    }
                }
            }
        }
        // 链接:[显示文字](目标).目标认不出来就整段当普通文字
        if links && chars[i] == '[' {
            if let Some((label, target, next)) = link_at(chars, i) {
                flush(&mut out, &mut cur);
                let st = State {
                    fg: cur.fg,
                    bg: cur.bg,
                    modifiers: cur.modifiers,
                };
                // 显示文字里还能有别的标记,但链接里不能再套链接
                let inner: Vec<char> = label.chars().collect();
                for mut sp in parse_inner(&inner, false, st) {
                    sp.link = Some(target.clone());
                    out.push(sp);
                }
                i = next;
                continue;
            }
        }
        // 删除线:~~...~~
        if chars[i] == '~' && chars.get(i + 1) == Some(&'~') {
            if let Some(close) = find(chars, i + 2, &['~', '~']) {
                if close > i + 2 {
                    flush(&mut out, &mut cur);
                    let mut st = State {
                        fg: cur.fg,
                        bg: cur.bg,
                        modifiers: cur.modifiers,
                    };
                    st.modifiers |= Modifier::CROSSED_OUT;
                    out.extend(parse_inner(&chars[i + 2..close], links, st));
                    i = close + 2;
                    continue;
                }
            }
        }
        // 粗体:**...**
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(close) = find(chars, i + 2, &['*', '*']) {
                if close > i + 2 {
                    flush(&mut out, &mut cur);
                    let mut st = State {
                        fg: cur.fg,
                        bg: cur.bg,
                        modifiers: cur.modifiers,
                    };
                    st.modifiers |= Modifier::BOLD;
                    out.extend(parse_inner(&chars[i + 2..close], links, st));
                    i = close + 2;
                    continue;
                }
            }
        }
        // 斜体:*...*(markdown):开头 `*` 后面不能是空白或另一个 `*`,
        // 结尾 `*` 前面不能是 `*`(那是粗体的尾巴),也不吞空白
        if chars[i] == '*'
            && chars
                .get(i + 1)
                .is_some_and(|c| !c.is_whitespace() && *c != '*')
        {
            let mut close = None;
            let mut j = i + 2;
            while j < chars.len() {
                if chars[j] == '*'
                    && chars[j - 1] != '*'
                    && chars.get(j + 1) != Some(&'*')
                    && !chars[j - 1].is_whitespace()
                {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(close) = close {
                flush(&mut out, &mut cur);
                let mut st = State {
                    fg: cur.fg,
                    bg: cur.bg,
                    modifiers: cur.modifiers,
                };
                st.modifiers |= Modifier::ITALIC;
                out.extend(parse_inner(&chars[i + 1..close], links, st));
                i = close + 1;
                continue;
            }
        }
        cur.text.push(chars[i]);
        i += 1;
    }
    flush(&mut out, &mut cur);
    out
}

/// 在 `[` 处尝试解析一个链接:返回 (显示文字, 目标, 下一个位置)
fn link_at(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let close = matching(chars, i, '[', ']')?;
    if close == i + 1 || chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = matching(chars, close + 1, '(', ')')?;
    let label: String = chars[i + 1..close].iter().collect();
    let target: String = chars[close + 2..end].iter().collect();
    if label.trim().is_empty() || target.trim().is_empty() {
        return None;
    }
    crate::link::classify(&target)?;
    Some((label, target, end + 1))
}

/// 从开括号处找配对的闭括号(按层数,目标里的成对括号能保留)
fn matching(chars: &[char], open: usize, o: char, c: char) -> Option<usize> {
    let mut depth = 0usize;
    for (k, ch) in chars.iter().enumerate().skip(open) {
        if *ch == o {
            depth += 1;
        } else if *ch == c {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
    }
    None
}

/// 从 from 开始找 pat(长度 1 或 2),返回它的起点
fn find(chars: &[char], from: usize, pat: &[char]) -> Option<usize> {
    let n = pat.len();
    if chars.len() < n {
        return None;
    }
    (from..=chars.len() - n).find(|&i| &chars[i..i + n] == pat)
}

fn flush(out: &mut Vec<Span>, cur: &mut Span) {
    if !cur.text.is_empty() {
        let state = State {
            fg: cur.fg,
            bg: cur.bg,
            modifiers: cur.modifiers,
        };
        out.push(cur.clone());
        *cur = Span::with(state, String::new());
    }
}

/// 去掉标记之后的纯文本(搜索、状态行、写文件时用得上)
pub fn plain(s: &str) -> String {
    parse(s).into_iter().map(|x| x.text).collect()
}

/// 显示宽度(全角算两格),按去掉标记之后算
pub fn plain_width(s: &str) -> usize {
    UnicodeWidthStr::width(plain(s).as_str())
}

/// 一组 span 的显示宽度
pub fn width_of(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|x| UnicodeWidthStr::width(x.text.as_str()))
        .sum()
}

/// span 落到基准样式上:基准 -> 内联 -> 链接下划线 -> 临时 UI(overlay 最高)
pub fn style_of(base: Style, sp: &Span, overlay: &TextOverlay) -> Style {
    let mut st = base;
    st = st.add_modifier(sp.modifiers);
    if let Some(c) = sp.fg {
        st = st.fg(c);
    }
    if let Some(c) = sp.bg {
        st = st.bg(c);
    }
    if sp.link.is_some() {
        // 链接只加下划线,不换颜色:免得和搜索黄、选中呼吸色打架
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    st = st.add_modifier(overlay.modifiers);
    if let Some(c) = overlay.fg {
        st = st.fg(c);
    }
    if let Some(c) = overlay.bg {
        st = st.bg(c);
    }
    st
}

/// 把一组 span 写到 (x, y);最多占 max_w 列,超了就按整字符截断.返回实际列数
pub fn put(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    spans: &[Span],
    base: Style,
    overlay: &TextOverlay,
    max_w: usize,
) -> u16 {
    put_link(buf, x, y, spans, base, overlay, max_w, |_, _, _| {})
}

/// 同上,但把链接盖到的每一格报出去(屏幕格):调用方换成画布格记进命中表.
/// 宽字的第二格也算(点它一样能打开)
#[allow(clippy::too_many_arguments)] // 位置、内容、基准样式、临时 UI、宽度、回调各是一件事
pub fn put_link<F: FnMut(u16, u16, &str)>(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    spans: &[Span],
    base: Style,
    overlay: &TextOverlay,
    max_w: usize,
    on_cell: F,
) -> u16 {
    let clip = (0, buf.area.width as i32);
    put_link_clip(
        buf, x as i32, y as i32, spans, base, overlay, max_w, clip, on_cell,
    )
}

/// 同 `put_link`,但只写落在 `clip`(列范围 [min, max))里的格子.
/// 框/文字被画面边缘裁掉时,剩下那部分照样画出来,不再整块消失.
#[allow(clippy::too_many_arguments)] // 位置、内容、裁剪范围本来就是几件事
pub fn put_link_clip<F: FnMut(u16, u16, &str)>(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    spans: &[Span],
    base: Style,
    overlay: &TextOverlay,
    max_w: usize,
    clip: (i32, i32),
    mut on_cell: F,
) -> u16 {
    let mut col = 0usize;
    let rows = buf.area.height as i32;
    if y < 0 || y >= rows {
        return 0;
    }
    for sp in spans {
        let st = style_of(base, sp, overlay);
        let mut pw = 0usize;
        for c in sp.text.chars() {
            let w = UnicodeWidthStr::width(c.to_string().as_str()).max(1);
            if col + pw + w > max_w {
                break;
            }
            let cx = x + (col + pw) as i32;
            let inside = cx >= clip.0 && cx + w as i32 <= clip.1;
            if inside {
                let mut one = [0u8; 4];
                let sym = c.encode_utf8(&mut one);
                if let Some(cell) = buf.cell_mut((cx as u16, y as u16)) {
                    cell.set_symbol(sym);
                    cell.set_style(st);
                }
                // 宽字的续格:set_symbol 会留下"空",底色得自己补
                for k in 1..w as i32 {
                    if let Some(cell) = buf.cell_mut(((cx + k) as u16, y as u16)) {
                        // 续格用空格(和 set_string 一致):空串会让"有没有字"的判断出错
                        cell.set_symbol(" ");
                        cell.set_style(st);
                    }
                }
                if let Some(t) = &sp.link {
                    for k in 0..w as i32 {
                        on_cell((cx + k) as u16, y as u16, t);
                    }
                }
            }
            pw += w;
        }
        col += pw;
        if col >= max_w {
            break;
        }
    }
    col as u16
}

/// 同 `put_link_clip`,但带一张"已经写过字的格子"表:
/// 别的框的文字先占住的格子不再覆盖(宽字的续格空格也不铺上去,否则会把别人的字擦掉),
/// 自己写过的格子记进表里,后面的框也就擦不掉它.
#[allow(clippy::too_many_arguments)]
pub fn put_link_clip_kept<F: FnMut(u16, u16, &str)>(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    spans: &[Span],
    base: Style,
    overlay: &TextOverlay,
    max_w: usize,
    clip: (i32, i32),
    kept: &mut std::collections::HashSet<(i32, i32)>,
    mut on_cell: F,
) -> u16 {
    let mut col = 0usize;
    let rows = buf.area.height as i32;
    if y < 0 || y >= rows {
        return 0;
    }
    for sp in spans {
        let st = style_of(base, sp, overlay);
        let mut pw = 0usize;
        for c in sp.text.chars() {
            let w = UnicodeWidthStr::width(c.to_string().as_str()).max(1);
            if col + pw + w > max_w {
                break;
            }
            let cx = x + (col + pw) as i32;
            let inside = cx >= clip.0 && cx + w as i32 <= clip.1;
            // 要占的格子被别人先写了:整个字让出去,不写一半
            let taken = (0..w as i32).any(|k| kept.contains(&(cx + k, y)));
            if inside && !taken {
                let mut one = [0u8; 4];
                let sym = c.encode_utf8(&mut one);
                if let Some(cell) = buf.cell_mut((cx as u16, y as u16)) {
                    cell.set_symbol(sym);
                    cell.set_style(st);
                }
                for k in 1..w as i32 {
                    if let Some(cell) = buf.cell_mut(((cx + k) as u16, y as u16)) {
                        cell.set_symbol(" ");
                        cell.set_style(st);
                    }
                }
                for k in 0..w as i32 {
                    kept.insert((cx + k, y));
                }
                if let Some(t) = &sp.link {
                    for k in 0..w as i32 {
                        on_cell((cx + k) as u16, y as u16, t);
                    }
                }
            }
            pw += w;
        }
        col += pw;
        if col >= max_w {
            break;
        }
    }
    col as u16
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<(String, bool, bool)> {
        spans
            .iter()
            .map(|s| {
                (
                    s.text.clone(),
                    s.modifiers.contains(Modifier::BOLD),
                    s.modifiers.contains(Modifier::ITALIC),
                )
            })
            .collect()
    }

    #[test]
    fn 粗体与斜体() {
        assert_eq!(
            texts(&parse("a **b** c *d* e")),
            vec![
                ("a ".to_string(), false, false),
                ("b".to_string(), true, false),
                (" c ".to_string(), false, false),
                ("d".to_string(), false, true),
                (" e".to_string(), false, false),
            ]
        );
        assert_eq!(texts(&parse("**b**")), vec![("b".to_string(), true, false)]);
        assert_eq!(texts(&parse("*d*")), vec![("d".to_string(), false, true)]);
    }

    #[test]
    fn 粗体里套斜体() {
        assert_eq!(
            texts(&parse("**a *b* c**")),
            vec![
                ("a ".to_string(), true, false),
                ("b".to_string(), true, true),
                (" c".to_string(), true, false),
            ]
        );
    }

    #[test]
    fn 没闭合的标记与单独的星号都是普通字符() {
        for s in ["**未闭合", "*未闭合", "a*b", "2*3", "**", "*", "a * b"] {
            assert_eq!(texts(&parse(s)), vec![(s.to_string(), false, false)], "{s}");
        }
        // 中文里用斜体是常见的:紧贴汉字也算
        assert_eq!(texts(&parse("中文*斜体*中文")).len(), 3);
    }

    #[test]
    fn 链接解析() {
        let sp = parse("[site](https://example.com)");
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].text, "site");
        assert_eq!(sp[0].link.as_deref(), Some("https://example.com"));
        // 粗体 + 链接
        let sp = parse("[**文档**](./a.md)");
        assert_eq!(sp.len(), 1);
        assert!(sp[0].modifiers.contains(Modifier::BOLD));
        assert_eq!(sp[0].link.as_deref(), Some("./a.md"));
        // 目标里的成对括号保留
        let sp = parse("[x](./a(b).md)");
        assert_eq!(sp[0].link.as_deref(), Some("./a(b).md"));
        // 白名单以外的目标、未闭合的括号都还是普通文字
        for s in [
            "[x](notes.md)",
            "[x](javascript:alert(1))",
            "[x](https://a",
            "[x] (https://a)",
            "[](https://a)",
        ] {
            assert!(
                parse(s).iter().all(|x| x.link.is_none()),
                "{s} 不该变成链接"
            );
        }
        // 显示与宽度只算文字
        assert_eq!(plain("[site](https://example.com)!"), "site!");
        assert_eq!(plain_width("[site](https://example.com)"), 4);
        // 链接里再写链接:里面那段按普通文字
        let sp = parse("[a [b](https://x) c](./y.md)");
        assert_eq!(plain("[a [b](https://x) c](./y.md)"), "a [b](https://x) c");
        assert!(sp.iter().all(|x| x.link.as_deref() == Some("./y.md")));
    }

    #[test]
    fn 斜体里的链接() {
        let sp = parse("*[it](/tmp)*");
        assert_eq!(sp.len(), 1);
        assert!(sp[0].modifiers.contains(Modifier::ITALIC));
        assert_eq!(sp[0].link.as_deref(), Some("/tmp"));
        assert_eq!(plain("*[it](/tmp)*"), "it");
    }

    #[test]
    fn 链接带下划线() {
        let sp = parse("[a](https://x)");
        let st = style_of(Style::default(), &sp[0], &TextOverlay::default());
        assert!(
            st.add_modifier == Modifier::UNDERLINED
                || st.add_modifier.contains(Modifier::UNDERLINED)
        );
        let plain_st = style_of(Style::default(), &parse("a")[0], &TextOverlay::default());
        assert!(!plain_st.add_modifier.contains(Modifier::UNDERLINED));
    }

    fn mods(s: &str) -> Vec<Modifier> {
        parse(s).into_iter().map(|x| x.modifiers).collect()
    }

    #[test]
    fn 删除线与下划线变暗() {
        assert_eq!(mods("~~x~~"), vec![Modifier::CROSSED_OUT]);
        assert_eq!(mods("<u>x</u>"), vec![Modifier::UNDERLINED]);
        assert_eq!(mods("<dim>x</dim>"), vec![Modifier::DIM]);
        assert_eq!(
            mods("<u>~~x~~</u>"),
            vec![Modifier::UNDERLINED | Modifier::CROSSED_OUT],
            "标签可以叠加"
        );
        assert_eq!(plain("a<u>b</u>c"), "abc");
        assert_eq!(plain_width("<u>中文</u>"), 4);
    }

    #[test]
    fn 颜色标签() {
        let sp = parse("<fg=#ff0000>红</fg><bg=#001122>底</bg>");
        assert_eq!(sp[0].fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(sp[0].bg, None);
        assert_eq!(sp[1].fg, None);
        assert_eq!(sp[1].bg, Some(Color::Rgb(0, 17, 34)));
        // default = 回到默认(前面的颜色不再生效)
        let sp = parse("<fg=#ff0000>a<fg=default>b</fg>c</fg>");
        let by = |t: &str| sp.iter().find(|x| x.text == t).expect("有这一段").clone();
        assert_eq!(by("a").fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(by("b").fg, None, "<fg=default> 把颜色还回去");
        assert_eq!(by("c").fg, Some(Color::Rgb(255, 0, 0)), "关掉内层回到外层");
        // 只认 #rrggbb 与 default
        for bad in [
            "<fg=red>x</fg>",
            "<fg=#f00>x</fg>",
            "<fg=#ff00zz>x</fg>",
            "<fg=>x</fg>",
            "<color=#ff0000>x</color>",
        ] {
            assert!(
                parse(bad).iter().all(|x| x.fg.is_none()),
                "{bad} 不该被当成颜色标签"
            );
            assert_eq!(plain(bad), bad, "整段按普通文字:{bad}");
        }
    }

    #[test]
    fn 未知未闭合交叉与转义() {
        // 未知标签按文字
        assert_eq!(plain("<b>x</b>"), "<b>x</b>");
        assert_eq!(plain("<|new box|>"), "<|new box|>");
        // 未闭合的标签不吞后面的内容
        assert_eq!(plain("<u>x"), "<u>x");
        assert_eq!(plain("a<fg=#ff0000>b"), "a<fg=#ff0000>b");
        // 交叉不成立:第二个关闭被当成文字,但第一个仍然生效
        let sp = parse("<u>a<dim>b</u>");
        assert_eq!(plain("<u>a<dim>b</u>"), "a<dim>b");
        assert_eq!(sp.len(), 1, "没闭合的 <dim> 只是文字,不切段");
        assert_eq!(sp[0].text, "a<dim>b");
        assert_eq!(sp[0].modifiers, Modifier::UNDERLINED);
        // 转义
        assert_eq!(plain("\\<u>x\\</u>"), "<u>x</u>");
        assert_eq!(parse("\\<u>").len(), 1);
        assert!(parse("\\<u>x</u>")[0].modifiers.is_empty());
        // 未闭合的 ~~ 是普通文字
        assert_eq!(plain("~~x"), "~~x");
    }

    #[test]
    fn 标签与链接混用() {
        // 标签里套链接:链接文字带内联颜色
        let sp = parse("<fg=#ff0000>[a](https://x)</fg>");
        assert_eq!(sp[0].link.as_deref(), Some("https://x"));
        assert_eq!(sp[0].fg, Some(Color::Rgb(255, 0, 0)));
        // 链接里套标签
        let sp = parse("[<u>a</u>](https://x)");
        assert_eq!(plain("[<u>a</u>](https://x)"), "a");
        assert_eq!(sp[0].modifiers, Modifier::UNDERLINED);
        assert_eq!(sp[0].link.as_deref(), Some("https://x"));
    }

    #[test]
    fn 临时_ui_盖过内联颜色() {
        let sp = parse("<fg=#ff0000>红</fg>")[0].clone();
        let base = Style::default().fg(Color::Rgb(1, 2, 3));
        let plain_st = style_of(base, &sp, &TextOverlay::default());
        assert_eq!(plain_st.fg, Some(Color::Rgb(255, 0, 0)), "内联颜色盖基准");
        let hit = TextOverlay {
            fg: Some(Color::Rgb(255, 255, 0)),
            bg: None,
            modifiers: Modifier::BOLD,
        };
        let st = style_of(base, &sp, &hit);
        assert_eq!(st.fg, Some(Color::Rgb(255, 255, 0)), "搜索命中压过内联颜色");
        assert!(st.add_modifier.contains(Modifier::BOLD));
        // 链接下划线与 <u> 不冲突:重复加同一个 modifier 还是一个
        let sp = parse("<u>[a](https://x)</u>")[0].clone();
        let st = style_of(Style::default(), &sp, &TextOverlay::default());
        assert!(st.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn 位置后缀的链接也解析() {
        for raw in [
            "[a](self:10,10)",
            "[a](./x.md:3,4)",
            "[a](./x.gtui:1,2)",
            "[a](custom://y:1,2)",
        ] {
            let sp = parse(raw);
            assert_eq!(plain(raw), "a");
            assert_eq!(sp[0].link.as_deref(), sp[0].link.as_deref());
            assert!(sp[0].link.is_some(), "{raw} 该是链接:{:?}", sp[0].link);
        }
    }

    #[test]
    fn 标记不占显示宽度() {
        assert_eq!(plain("**粗***斜*体"), "粗斜体");
        assert_eq!(plain_width("**粗***斜*体"), 6, "三个全角字");
        assert_eq!(plain_width("**abc**"), 3);
        assert_eq!(width_of(&parse("**ab**c")), 3);
    }
}
