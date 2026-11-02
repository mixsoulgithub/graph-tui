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
}

impl Span {
    fn push(&mut self, c: char) {
        self.text.push(c);
    }
}

/// 解析成同样式的小段
pub fn parse(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span {
        text: String::new(),
        bold: false,
        italic: false,
    };
    let mut i = 0;
    while i < chars.len() {
        // 粗体:**...**
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(close) = find(&chars, i + 2, &['*', '*']) {
                if close > i + 2 {
                    flush(&mut out, &mut cur);
                    let mut sp = Span {
                        text: chars[i + 2..close].iter().collect(),
                        bold: true,
                        italic: false,
                    };
                    // 粗体里允许再套一层斜体
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
                out.push(Span {
                    text: chars[i + 1..close].iter().collect(),
                    bold: false,
                    italic: true,
                });
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
    st
}

/// 把一组 span 写到 (x, y);最多占 max_w 列,超了就按整字符截断.返回实际列数
pub fn put(buf: &mut Buffer, x: u16, y: u16, spans: &[Span], base: Style, max_w: usize) -> u16 {
    let mut col = 0usize;
    for sp in spans {
        let st = style_of(base, sp);
        let mut piece = String::new();
        let mut pw = 0usize;
        for c in sp.text.chars() {
            let w = UnicodeWidthStr::width(c.to_string().as_str());
            if col + pw + w > max_w || x as usize + col + pw + w > buf.area.width as usize {
                break;
            }
            piece.push(c);
            pw += w;
        }
        if !piece.is_empty() {
            let sx = x + col as u16;
            buf.set_string(sx, y, &piece, st);
            // 宽字的续格 set_string 只写空白,不带底色;整段再补一次背景,框内填充才连成一片
            if let Some(bg) = st.bg {
                for k in 0..pw as u16 {
                    if let Some(cell) = buf.cell_mut((sx + k, y)) {
                        cell.set_bg(bg);
                    }
                }
            }
            col += pw;
        }
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
    fn 标记不占显示宽度() {
        assert_eq!(plain("**粗***斜*体"), "粗斜体");
        assert_eq!(plain_width("**粗***斜*体"), 6, "三个全角字");
        assert_eq!(plain_width("**abc**"), 3);
        assert_eq!(width_of(&parse("**ab**c")), 3);
    }
}
