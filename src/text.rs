//! 标签里的内联样式:markdown 的写法 —— `**粗体**` 与 `*斜体*`.
//!
//! 只认这两种;标记没闭合、或者 `*` 后面紧跟空白,都当普通字符.
//! 标记本身不占显示宽度,所以加了样式不会把标签撑长,布局与占位都不受影响.
//! 输入行(命令行 / 搜索 / 提示行)不走这里:那些是原样文本,`/` 与 `*` 都不会被解释.

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// 一段同样式文字
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// `[文字](目标)` 里的目标;普通文字是 None.目标不占显示宽度
    pub link: Option<String>,
}

impl Span {
    fn push(&mut self, c: char) {
        self.text.push(c);
    }
}

/// 解析成同样式的小段
pub fn parse(s: &str) -> Vec<Span> {
    parse_inner(s, true)
}

/// `links = false` 时 `[x](y)` 按普通文字处理(链接里套链接就是这个效果)
fn parse_inner(s: &str, links: bool) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span {
        text: String::new(),
        bold: false,
        italic: false,
        link: None,
    };
    let mut i = 0;
    while i < chars.len() {
        // 链接:[显示文字](目标).目标认不出来就整段当普通文字
        if links && chars[i] == '[' {
            if let Some((label, target, next)) = link_at(&chars, i) {
                flush(&mut out, &mut cur);
                // 显示文字里还能有粗体斜体,但链接里不能再套链接
                for mut sp in parse_inner(&label, false) {
                    sp.link = Some(target.clone());
                    out.push(sp);
                }
                i = next;
                continue;
            }
        }
        // 粗体:**...**
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(close) = find(&chars, i + 2, &['*', '*']) {
                if close > i + 2 {
                    flush(&mut out, &mut cur);
                    let mut sp = Span {
                        text: chars[i + 2..close].iter().collect(),
                        bold: true,
                        italic: false,
                        link: None,
                    };
                    // 粗体里允许再套一层斜体,也允许整段是个链接
                    let inner = parse(&sp.text);
                    if inner.iter().any(|x| x.italic) {
                        out.extend(inner.into_iter().map(|mut x| {
                            x.bold = true;
                            x
                        }));
                    } else {
                        sp.text = inner.into_iter().map(|x| x.text).collect();
                        out.push(sp);
                    }
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
                // 斜体里也可能整段是链接(`*[x](y)*`):照常解析,再统一标成斜体
                let inner: String = chars[i + 1..close].iter().collect();
                for mut sp in parse_inner(&inner, true) {
                    sp.italic = true;
                    out.push(sp);
                }
                i = close + 1;
                continue;
            }
        }
        cur.push(chars[i]);
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
        out.push(cur.clone());
        cur.text.clear();
    }
    cur.bold = false;
    cur.italic = false;
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

/// span 落到基准样式上
pub fn style_of(base: Style, sp: &Span) -> Style {
    let mut st = base;
    if sp.bold {
        st = st.add_modifier(Modifier::BOLD);
    }
    if sp.italic {
        st = st.add_modifier(Modifier::ITALIC);
    }
    if sp.link.is_some() {
        // 链接只加下划线,不换颜色:免得和搜索黄、选中呼吸色打架
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    st
}

/// 把一组 span 写到 (x, y);最多占 max_w 列,超了就按整字符截断.返回实际列数
pub fn put(buf: &mut Buffer, x: u16, y: u16, spans: &[Span], base: Style, max_w: usize) -> u16 {
    put_link(buf, x, y, spans, base, max_w, |_, _, _| {})
}

/// 同上,但把链接盖到的每一格报出去(屏幕格):调用方换成画布格记进命中表.
/// 宽字的第二格也算(点它一样能打开)
pub fn put_link<F: FnMut(u16, u16, &str)>(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    spans: &[Span],
    base: Style,
    max_w: usize,
    on_cell: F,
) -> u16 {
    let clip = (0, buf.area.width as i32);
    put_link_clip(buf, x as i32, y as i32, spans, base, max_w, clip, on_cell)
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
        let st = style_of(base, sp);
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

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<(String, bool, bool)> {
        spans
            .iter()
            .map(|s| (s.text.clone(), s.bold, s.italic))
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
        assert!(sp[0].bold);
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
        assert!(sp[0].italic);
        assert_eq!(sp[0].link.as_deref(), Some("/tmp"));
        assert_eq!(plain("*[it](/tmp)*"), "it");
    }

    #[test]
    fn 链接带下划线() {
        let sp = parse("[a](https://x)");
        let st = style_of(Style::default(), &sp[0]);
        assert!(
            st.add_modifier == Modifier::UNDERLINED
                || st.add_modifier.contains(Modifier::UNDERLINED)
        );
        let plain_st = style_of(Style::default(), &parse("a")[0]);
        assert!(!plain_st.add_modifier.contains(Modifier::UNDERLINED));
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
