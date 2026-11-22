//! Inline styles inside labels (GTUI inline markup v1):
//!
//! - `**bold**` / `*italic*` / `~~strikethrough~~` / `[text](target)`;
//! - `<u>` underline, `<dim>` dim, `<fg=#rrggbb>` / `<bg=#rrggbb>` foreground/background,
//!   closed with `</u>` `</dim>` `</fg>` `</bg>`; `#rrggbb` may also be written `default`;
//! - markup takes no display width: layout, align, wide glyphs and search are unaffected;
//! - unknown and unclosed tags are plain text; `<` is escaped as `\<`.
//!
//! Input lines (command line / search / prompt line) bypass this: they are raw text.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A run of text with one style
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// Inline foreground / background (None = follow the base style)
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    /// Inline modifiers (bold / italic / strikethrough / underline / dim)
    pub modifiers: Modifier,
    /// Target of `[text](target)`; None for plain text. The target takes no display width
    pub link: Option<String>,
}

impl Span {
    /// Plain-text span with no inline style
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

/// Transient UI state (search hit / selection / cursor pulse): applied last, so
/// inline colours cannot override it
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextOverlay {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Modifier,
}

impl TextOverlay {
    /// Override the foreground only (cursor pulse colour and the like)
    pub fn fg(c: Color) -> TextOverlay {
        TextOverlay {
            fg: Some(c),
            ..TextOverlay::default()
        }
    }
}

/// Inline style currently in effect (what the tag stack stores)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

/// Parse into runs of uniform style
pub fn parse(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    parse_inner(&chars, true, State::default())
}

/// Inline tag
enum Tag {
    /// Open: the name recorded on the stack plus its effect
    Open(String, Kind),
    /// Close: the name
    Close(String),
}

/// Tag effect
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

/// Recognise a whitelisted tag at `<`: returns (tag, next position). Unknown and
/// unclosed tags are plain text
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
    // a close tag only needs its name to match
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

/// Is there a matching `</name>` further on? Without one the open tag is plain text
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

/// With `links = false`, `[x](y)` is plain text (this is how nested links behave)
fn parse_inner(chars: &[char], links: bool, state: State) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span::with(state, String::new());
    // which tags are open, with the state from before each; stack-matched, no crossing
    let mut stack: Vec<(String, State)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // escape: `\<` is a literal `<`
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'<') {
            cur.text.push('<');
            i += 2;
            continue;
        }
        // inline tag
        if chars[i] == '<' {
            if let Some((tag, next)) = tag_at(chars, i) {
                match tag {
                    Tag::Open(name, kind) => {
                        // an unclosed tag is plain text and swallows nothing
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
                        // does not match the stack top (crossed/redundant): plain text
                    }
                }
            }
        }
        // link: [label](target). If the target is not recognised, all of it is plain text
        if links && chars[i] == '[' {
            if let Some((label, target, next)) = link_at(chars, i) {
                flush(&mut out, &mut cur);
                let st = State {
                    fg: cur.fg,
                    bg: cur.bg,
                    modifiers: cur.modifiers,
                };
                // the label may carry other markup, but links never nest
                let inner: Vec<char> = label.chars().collect();
                for mut sp in parse_inner(&inner, false, st) {
                    sp.link = Some(target.clone());
                    out.push(sp);
                }
                i = next;
                continue;
            }
        }
        // strikethrough: ~~...~~
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
        // bold: **...**
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
        // italic: *...* (markdown): the opening `*` must not be followed by whitespace
        // or another `*`, and the closing `*` must not follow a `*` (that is bold's
        // tail) nor swallow whitespace
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

/// Try to parse a link at `[`: returns (label, target, next position)
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

/// Find the matching close bracket from an open one (by depth, so balanced
/// parentheses in the target survive)
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

/// Find pat (length 1 or 2) starting at from, returning its start index
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

/// Plain text with markup removed (used by search, status line and file writing)
pub fn plain(s: &str) -> String {
    parse(s).into_iter().map(|x| x.text).collect()
}

/// Positions of the query within a line ([start, end) in chars). Matching rules are
/// those of `Document::search`: visible text with inline markup removed, case-insensitive;
/// if case folding changes the char count, give up (it would not line up with cells)
pub fn find_ranges(line: &str, query: &str) -> Vec<(usize, usize)> {
    let visible = plain(line);
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    let hay: Vec<char> = visible.to_lowercase().chars().collect();
    if needle.is_empty() || hay.len() != visible.chars().count() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            out.push((i, i + needle.len()));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Columns taken by the first `n` display chars (wide glyphs count two): hit positions
/// go from char index to cell offset
pub fn width_of_chars(s: &str, n: usize) -> usize {
    s.chars()
        .take(n)
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1).max(1))
        .sum()
}

/// Display width (wide glyphs count two), measured after markup is removed
pub fn plain_width(s: &str) -> usize {
    UnicodeWidthStr::width(plain(s).as_str())
}

/// Display width of a set of spans
pub fn width_of(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|x| UnicodeWidthStr::width(x.text.as_str()))
        .sum()
}

/// Resolve a span onto the base style: base -> inline -> link underline -> overlay
/// (overlay wins)
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
        // links only add an underline, never a colour, so they do not fight the search
        // background or the selection pulse
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

/// Write a set of spans at (x, y); at most max_w columns, truncated on whole chars.
/// Returns the number of columns written
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

/// Same as above, but report every cell a link covers (screen cells): the caller maps
/// them to canvas cells for its hit table. The second cell of a wide glyph counts too
/// (clicking it opens the link as well)
#[allow(clippy::too_many_arguments)] // position, content, base style, overlay, width, callback
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

/// Like `put_link`, but writes only cells inside `clip` (column range [min, max)).
/// When a box or text is cropped by the screen edge, the visible part is still drawn
/// instead of vanishing as a whole.
#[allow(clippy::too_many_arguments)] // position, content and clip range are inherently separate
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
                // continuation cells of a wide glyph: set_symbol leaves them "empty",
                // so the background is filled here
                for k in 1..w as i32 {
                    if let Some(cell) = buf.cell_mut(((cx + k) as u16, y as u16)) {
                        // continuation cells get a space (as set_string does): an empty
                        // string breaks the "has content" check
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

/// Like `put_link_clip`, but with a table of cells that already hold text: cells claimed
/// by another box's text are left alone (not even the space of a wide glyph's
/// continuation cell, which would erase the other glyph), and cells written here are
/// recorded so later boxes cannot erase them.
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
            // the cells we need are already taken: give up the whole glyph, never half
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
#[allow(clippy::field_reassign_with_default)] // tests tweak a default; clearer than a long literal
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
    fn bold_and_italic() {
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
    fn italic_nested_in_bold() {
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
    fn unclosed_markup_and_lone_stars_are_plain() {
        for s in ["**未闭合", "*未闭合", "a*b", "2*3", "**", "*", "a * b"] {
            assert_eq!(texts(&parse(s)), vec![(s.to_string(), false, false)], "{s}");
        }
        // italic next to CJK is common, so adjacent CJK still counts
        assert_eq!(texts(&parse("中文*斜体*中文")).len(), 3);
    }

    #[test]
    fn link_parsing() {
        let sp = parse("[site](https://example.com)");
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].text, "site");
        assert_eq!(sp[0].link.as_deref(), Some("https://example.com"));
        // bold + link
        let sp = parse("[**文档**](./a.md)");
        assert_eq!(sp.len(), 1);
        assert!(sp[0].modifiers.contains(Modifier::BOLD));
        assert_eq!(sp[0].link.as_deref(), Some("./a.md"));
        // balanced parentheses in the target survive
        let sp = parse("[x](./a(b).md)");
        assert_eq!(sp[0].link.as_deref(), Some("./a(b).md"));
        // targets outside the whitelist and unclosed brackets stay plain text
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
        // display and width count the text only
        assert_eq!(plain("[site](https://example.com)!"), "site!");
        assert_eq!(plain_width("[site](https://example.com)"), 4);
        // a link inside a link: the inner one stays plain text
        let sp = parse("[a [b](https://x) c](./y.md)");
        assert_eq!(plain("[a [b](https://x) c](./y.md)"), "a [b](https://x) c");
        assert!(sp.iter().all(|x| x.link.as_deref() == Some("./y.md")));
    }

    #[test]
    fn link_inside_italic() {
        let sp = parse("*[it](/tmp)*");
        assert_eq!(sp.len(), 1);
        assert!(sp[0].modifiers.contains(Modifier::ITALIC));
        assert_eq!(sp[0].link.as_deref(), Some("/tmp"));
        assert_eq!(plain("*[it](/tmp)*"), "it");
    }

    #[test]
    fn links_are_underlined() {
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
    fn strike_underline_and_dim() {
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
    fn color_tags() {
        let sp = parse("<fg=#ff0000>红</fg><bg=#001122>底</bg>");
        assert_eq!(sp[0].fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(sp[0].bg, None);
        assert_eq!(sp[1].fg, None);
        assert_eq!(sp[1].bg, Some(Color::Rgb(0, 17, 34)));
        // default resets to the default (earlier colour no longer applies)
        let sp = parse("<fg=#ff0000>a<fg=default>b</fg>c</fg>");
        let by = |t: &str| sp.iter().find(|x| x.text == t).expect("有这一段").clone();
        assert_eq!(by("a").fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(by("b").fg, None, "<fg=default> 把颜色还回去");
        assert_eq!(by("c").fg, Some(Color::Rgb(255, 0, 0)), "关掉内层回到外层");
        // only #rrggbb and default are accepted
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
    fn unknown_unclosed_crossed_and_escaped() {
        // unknown tags are text
        assert_eq!(plain("<b>x</b>"), "<b>x</b>");
        assert_eq!(plain("<|new box|>"), "<|new box|>");
        // unclosed tags swallow nothing
        assert_eq!(plain("<u>x"), "<u>x");
        assert_eq!(plain("a<fg=#ff0000>b"), "a<fg=#ff0000>b");
        // crossing does not parse: the second close is text, the first still applies
        let sp = parse("<u>a<dim>b</u>");
        assert_eq!(plain("<u>a<dim>b</u>"), "a<dim>b");
        assert_eq!(sp.len(), 1, "没闭合的 <dim> 只是文字,不切段");
        assert_eq!(sp[0].text, "a<dim>b");
        assert_eq!(sp[0].modifiers, Modifier::UNDERLINED);
        // escaping
        assert_eq!(plain("\\<u>x\\</u>"), "<u>x</u>");
        assert_eq!(parse("\\<u>").len(), 1);
        assert!(parse("\\<u>x</u>")[0].modifiers.is_empty());
        // an unclosed ~~ is plain text
        assert_eq!(plain("~~x"), "~~x");
    }

    #[test]
    fn tags_and_links_mixed() {
        // link nested in a tag: the link text carries the inline colour
        let sp = parse("<fg=#ff0000>[a](https://x)</fg>");
        assert_eq!(sp[0].link.as_deref(), Some("https://x"));
        assert_eq!(sp[0].fg, Some(Color::Rgb(255, 0, 0)));
        // tag nested in a link
        let sp = parse("[<u>a</u>](https://x)");
        assert_eq!(plain("[<u>a</u>](https://x)"), "a");
        assert_eq!(sp[0].modifiers, Modifier::UNDERLINED);
        assert_eq!(sp[0].link.as_deref(), Some("https://x"));
    }

    #[test]
    fn overlay_beats_inline_color() {
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
        // link underline and <u> coexist: adding the same modifier twice is a no-op
        let sp = parse("<u>[a](https://x)</u>")[0].clone();
        let st = style_of(Style::default(), &sp, &TextOverlay::default());
        assert!(st.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn links_with_position_suffix_parse() {
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
    fn markup_has_no_display_width() {
        assert_eq!(plain("**粗***斜*体"), "粗斜体");
        assert_eq!(plain_width("**粗***斜*体"), 6, "三个全角字");
        assert_eq!(plain_width("**abc**"), 3);
        assert_eq!(width_of(&parse("**ab**c")), 3);
    }
}
