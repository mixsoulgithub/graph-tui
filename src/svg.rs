//! 把一帧 ratatui buffer 导出成自包含 SVG(图片导出用).
//! 一个字符格是 8x16 px 的等宽格子;同一行里颜色与修饰符都相同的连续格子合成一个
//! `<text>`,同色的连续底色合成一个 `<rect>`.这一层只读 buffer,不认识终端.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

/// 一格宽(px).9 = 等宽字体 15 px 时的步进(DejaVu/Noto 都是 0.6 em),字号和格子宽度
/// 对上了连出来的框线才不会一格格错开
const CW: u32 = 9;
/// 一行高(px)
const CH: u32 = 18;
/// 字号(px)
const FS: u32 = 15;
/// 基线离行顶的距离(px),15 号字落在 18 px 行里大约是这个位置
const BASELINE: u32 = 13;
/// 制表符线段的粗细(px).终端里是 1 px 线,这里按格子放大
const STROKE: f32 = 1.6;
/// 圆角拐弯的半径(px)
const ROUND: f32 = 3.5;

/// 默认前景色(与 dump 的默认前景一致)
const FG: (u8, u8, u8) = (212, 218, 232);
/// 默认底色(与 dump 的默认底色一致)
const BG: (u8, u8, u8) = (9, 10, 17);

/// 字体族:等宽里挑有制表符/中文的,最后一档交给系统
const FONT: &str =
    "DejaVu Sans Mono, Noto Sans Mono CJK SC, Noto Sans CJK SC, Liberation Mono, monospace";

/// 文字的样式:颜色加上会被写成属性的修饰符
#[derive(Clone, Copy, PartialEq, Eq)]
struct Ink {
    fg: (u8, u8, u8),
    bold: bool,
    italic: bool,
    underline: bool,
    dim: bool,
}

fn rgb_of(c: Color, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
    }
}

fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn width_of(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 一个格子最终怎么画:文字颜色/修饰,以及它贡献的底色.
/// REVERSED 在这里就把前景背景换过来,后面两趟都不用再管.
fn ink_of(cell: &Cell) -> (Ink, (u8, u8, u8)) {
    let mut fg = rgb_of(cell.fg, FG);
    let mut bg = rgb_of(cell.bg, BG);
    let m = cell.modifier;
    if m.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    (
        Ink {
            fg,
            bold: m.contains(Modifier::BOLD),
            italic: m.contains(Modifier::ITALIC),
            underline: m.contains(Modifier::UNDERLINED),
            dim: m.contains(Modifier::DIM),
        },
        bg,
    )
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 制表符的方向位(和 canvas 一致:左 1 右 2 上 4 下 8)、是否圆角、是否粗线
fn art_of(ch: char) -> Option<(u8, bool, bool)> {
    const L: u8 = 1;
    const R: u8 = 2;
    const U: u8 = 4;
    const D: u8 = 8;
    Some(match ch {
        '─' => (L | R, false, false),
        '│' => (U | D, false, false),
        '┌' => (D | R, false, false),
        '┐' => (D | L, false, false),
        '└' => (U | R, false, false),
        '┘' => (U | L, false, false),
        '├' => (U | D | R, false, false),
        '┤' => (U | D | L, false, false),
        '┬' => (L | R | D, false, false),
        '┴' => (L | R | U, false, false),
        '┼' => (L | R | U | D, false, false),
        '╭' => (D | R, true, false),
        '╮' => (D | L, true, false),
        '╰' => (U | R, true, false),
        '╯' => (U | L, true, false),
        '━' => (L | R, false, true),
        '┃' => (U | D, false, true),
        '┏' => (D | R, false, true),
        '┓' => (D | L, false, true),
        '┗' => (U | R, false, true),
        '┛' => (U | L, false, true),
        '┣' => (U | D | R, false, true),
        '┫' => (U | D | L, false, true),
        '┳' => (L | R | D, false, true),
        '┻' => (L | R | U, false, true),
        '╋' => (L | R | U | D, false, true),
        _ => return None,
    })
}

/// 把一格制表符画成线段.线段都画到格子边界/正中间,相邻两格自然接上,
/// 不依赖字体里有没有制表符、也不受字宽影响
fn draw_art(out: &mut String, row: u16, cx: u16, ink: Ink, ch: char) {
    const L: u8 = 1;
    const R: u8 = 2;
    const U: u8 = 4;
    const D: u8 = 8;
    let Some((mask, rounded, heavy)) = art_of(ch) else {
        return;
    };
    let x0 = (cx as u32 * CW) as f32;
    let y0 = (row as u32 * CH) as f32;
    let x1 = x0 + CW as f32;
    let y1 = y0 + CH as f32;
    let mx = x0 + CW as f32 / 2.0;
    let my = y0 + CH as f32 / 2.0;
    let mut d = String::new();
    let straight = |d: &mut String, ax: f32, ay: f32, bx: f32, by: f32| {
        d.push_str(&format!("M{ax:.1},{ay:.1}L{bx:.1},{by:.1}"));
    };
    if rounded && (mask & (L | R | U | D)).count_ones() == 2 {
        // 圆角:横竖两段用一段二次曲线接起来
        let r = ROUND;
        let (hx, hy) = if mask & R != 0 { (x1, my) } else { (x0, my) };
        let (vx, vy) = if mask & D != 0 { (mx, y1) } else { (mx, y0) };
        let cy = if mask & D != 0 { my + r } else { my - r };
        d.push_str(&format!("M{hx:.1},{hy:.1}Q{mx:.1},{my:.1} {mx:.1},{cy:.1}"));
        straight(&mut d, mx, cy, vx, vy);
    } else {
        if mask & L != 0 {
            straight(&mut d, x0, my, mx, my);
        }
        if mask & R != 0 {
            straight(&mut d, mx, my, x1, my);
        }
        if mask & U != 0 {
            straight(&mut d, mx, y0, mx, my);
        }
        if mask & D != 0 {
            straight(&mut d, mx, my, mx, y1);
        }
    }
    if d.is_empty() {
        return;
    }
    if ink.dim {
        out.push_str("<g opacity=\"0.55\">");
    }
    out.push_str(&format!(
        "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"butt\"/>\n",
        hex(ink.fg),
        if heavy { STROKE * 2.0 } else { STROKE }
    ));
    if ink.dim {
        out.push_str("</g>\n");
    }
}

/// 先铺一行的底色块.
/// 宽字在 buffer 里的后半格会被 reset,底色得按字宽补过去,否则色块只盖住一半.
fn row_bg(buf: &Buffer, area: Rect, row: u16, out: &mut String) {
    let w = area.width as usize;
    if w == 0 {
        return;
    }
    let mut cols: Vec<Option<(u8, u8, u8)>> = vec![None; w];
    let mut cx = 0u16;
    while cx < area.width {
        let cell = &buf[(area.x.saturating_add(cx), area.y.saturating_add(row))];
        let (_, bg) = ink_of(cell);
        let cw = width_of(cell.symbol());
        if bg != BG {
            cols[cx as usize] = Some(bg);
            // 宽字占的其余列也涂成同一色
            for k in 1..cw as u16 {
                let at = cx.saturating_add(k);
                if at < area.width {
                    cols[at as usize] = Some(bg);
                }
            }
        }
        cx = cx.saturating_add(cw.max(1) as u16);
    }

    let mut start = 0usize;
    while start < w {
        if let Some(c) = cols[start] {
            let mut end = start + 1;
            while end < w && cols[end] == Some(c) {
                end += 1;
            }
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
                start as u32 * CW,
                row as u32 * CH,
                (end - start) as u32 * CW,
                CH,
                hex(c)
            ));
            start = end;
        } else {
            start += 1;
        }
    }
}

/// 一个字一个 `<text>`:字体自己的字宽不是整 8px,连成一段会在后面几格漂掉;
/// 每个字都钉在自己的列上,框线才接得上
fn glyph(out: &mut String, row: u16, cx: u16, ink: Ink, sym: &str) {
    let y = row as u32 * CH + BASELINE;
    // 居中对齐:字体的实际字宽和 9 px 有出入时,误差只平均落在左右两侧,
    // 不会像左对齐那样一路累积
    let half = (width_of(sym).max(1) as f32) * (CW as f32) / 2.0;
    let x = cx as f32 * CW as f32 + half;
    out.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"{y}\" text-anchor=\"middle\" fill=\"{}\"",
        hex(ink.fg)
    ));
    if ink.bold {
        out.push_str(" font-weight=\"bold\"");
    }
    if ink.italic {
        out.push_str(" font-style=\"italic\"");
    }
    if ink.underline {
        out.push_str(" text-decoration=\"underline\"");
    }
    if ink.dim {
        out.push_str(" opacity=\"0.55\"");
    }
    out.push('>');
    out.push_str(&esc(sym));
    out.push_str("</text>\n");
}

/// 再把一行的字压上去.
/// 空格不输出;宽字按两列前进,它后面的续格直接跳过.
fn row_text(buf: &Buffer, area: Rect, row: u16, out: &mut String) {
    let mut cx = 0u16;
    while cx < area.width {
        let cell = &buf[(area.x.saturating_add(cx), area.y.saturating_add(row))];
        let sym = cell.symbol();
        if !sym.is_empty() && sym != " " {
            let (ink, _) = ink_of(cell);
            let chars: Vec<char> = sym.chars().collect();
            if chars.len() == 1 && art_of(chars[0]).is_some() {
                draw_art(out, row, cx, ink, chars[0]);
            } else {
                glyph(out, row, cx, ink, sym);
            }
        }
        cx = cx.saturating_add(width_of(sym).max(1) as u16);
    }
}

/// 把一帧 buffer 变成一份自包含 SVG.
pub fn of_buffer(buf: &Buffer) -> String {
    let area = *buf.area();
    let px_w = area.width as u32 * CW;
    let px_h = area.height as u32 * CH;
    let mut out = String::new();
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    // 字体写在根上:text 全部继承,一个文件里省下几千份 font-family
    out.push_str(&format!(
        " width=\"{px_w}\" height=\"{px_h}\" viewBox=\"0 0 {px_w} {px_h}\" xml:space=\"preserve\" font-family=\"{FONT}\" font-size=\"{FS}\">\n"
    ));
    out.push_str(&format!(
        "<rect width=\"{px_w}\" height=\"{px_h}\" fill=\"{}\"/>\n",
        hex(BG)
    ));
    for row in 0..area.height {
        row_bg(buf, area, row, &mut out);
    }
    for row in 0..area.height {
        row_text(buf, area, row, &mut out);
    }
    out.push_str("</svg>\n");
    out
}

/// 把一段 SVG 落成 PNG:终端自己没法栅格化字体,所以借系统里的 rsvg-convert
/// (没装就明确报错,不静默生成一个坏文件).先写临时 .svg,转完删掉.
pub fn write_png(svg_text: &str, path: &str) -> Result<(), String> {
    let tmp = format!("{path}.svg.tmp");
    std::fs::write(&tmp, svg_text).map_err(|e| format!("cannot write {tmp}: {e}"))?;
    let out = std::process::Command::new("rsvg-convert")
        .args(["-o", path, &tmp])
        .output();
    let _ = std::fs::remove_file(&tmp);
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "rsvg-convert failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(_) => Err("PNG export needs rsvg-convert (librsvg) on PATH".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// 每个字都在自己的列上:字体的字宽不是整 8px,连成一段后半行会漂
    #[test]
    fn 每个字都钉在自己的列上() {
        let mut b = buf(4, 1);
        for (x, ch) in [(0u16, "a"), (1, "b"), (2, "c")] {
            b[(x, 0)].set_symbol(ch);
            b[(x, 0)].fg = Color::Rgb(255, 0, 0);
        }
        let s = of_buffer(&b);
        assert!(s.starts_with("<svg"));
        assert_eq!(s.matches("<text").count(), 3);
        for x in [4.5, 13.5, 22.5] {
            assert!(
                s.contains(&format!("<text x=\"{x}\"")),
                "第 {x} 像素处该有一个字:\n{s}"
            );
        }
        assert_eq!(s.matches("fill=\"#ff0000\"").count(), 3);
    }

    /// 制表符画成线段:字体缺制表符、或者字宽不是整 9px 都不再影响框线
    #[test]
    fn 框线是线段不是字形() {
        let mut b = buf(3, 2);
        b[(0, 0)].set_symbol("─");
        b[(1, 0)].set_symbol("┼");
        b[(0, 1)].set_symbol("│");
        b[(0, 0)].fg = Color::Rgb(0, 128, 0);
        b[(1, 0)].fg = Color::Rgb(0, 128, 0);
        b[(0, 1)].fg = Color::Rgb(0, 128, 0);
        let s = of_buffer(&b);
        assert!(!s.contains("<text"), "制表符不该再当文字画:\n{s}");
        assert_eq!(s.matches("<path").count(), 3);
        assert!(s.contains("stroke=\"#008000\" stroke-width=\"1.6\""));
        // 横线从格子左边界到右边界,相邻两格自然接上
        assert!(s.contains("M0.0,9.0L4.5,9.0M4.5,9.0L9.0,9.0"), "\n{s}");
        // 竖线从格顶到格底
        assert!(s.contains("M4.5,18.0L4.5,27.0M4.5,27.0L4.5,36.0"), "\n{s}");
    }

    /// 圆角拐弯是一段曲线,不是四个散落的角
    #[test]
    fn 圆角是曲线() {
        let mut b = buf(1, 1);
        b[(0, 0)].set_symbol("╭");
        let s = of_buffer(&b);
        assert!(s.contains("Q4.5,9.0"), "圆角该用二次曲线:\n{s}");
        assert!(!s.contains("<text"));
    }

    #[test]
    fn escapes_markup() {
        let mut b = buf(7, 1);
        for (x, ch) in ["a", "<", "b", "&", "c", "\"", "d"].iter().enumerate() {
            b[(x as u16, 0)].set_symbol(ch);
        }
        let s = of_buffer(&b);
        for want in ["&lt;", "&amp;", "&quot;"] {
            assert!(s.contains(want), "该转义成 {want}:\n{s}");
        }
        assert!(!s.contains("<b&c"));
    }

    #[test]
    fn modifiers_become_attributes() {
        let mut b = buf(3, 1);
        b[(0, 0)].set_symbol("B");
        b[(0, 0)].modifier = Modifier::BOLD;
        b[(1, 0)].set_symbol("I");
        b[(1, 0)].modifier = Modifier::ITALIC;
        b[(2, 0)].set_symbol("U");
        b[(2, 0)].modifier = Modifier::UNDERLINED;
        let s = of_buffer(&b);
        assert!(s.contains("font-weight=\"bold\""));
        assert!(s.contains("font-style=\"italic\""));
        assert!(s.contains("text-decoration=\"underline\""));
    }

    #[test]
    fn reversed_swaps_fg_and_bg() {
        let mut b = buf(1, 1);
        b[(0, 0)].set_symbol("R");
        b[(0, 0)].fg = Color::Rgb(255, 0, 0);
        b[(0, 0)].bg = Color::Rgb(0, 0, 255);
        b[(0, 0)].modifier = Modifier::REVERSED;
        let s = of_buffer(&b);
        // 字用原来的底色,块用原来的前景色
        assert!(s.contains("fill=\"#0000ff\""));
        assert!(s.contains("<rect x=\"0\" y=\"0\" width=\"9\" height=\"18\" fill=\"#ff0000\"/>"));
    }

    #[test]
    fn background_runs_merge() {
        let mut b = buf(3, 1);
        for x in 0..3u16 {
            b[(x, 0)].set_symbol(" ");
            b[(x, 0)].bg = Color::Rgb(1, 2, 3);
        }
        let s = of_buffer(&b);
        assert!(s.contains("<rect x=\"0\" y=\"0\" width=\"27\" height=\"18\" fill=\"#010203\"/>"));
        assert!(!s.contains("<text"));
    }

    #[test]
    fn empty_buffer_is_still_svg() {
        let s = of_buffer(&Buffer::empty(Rect::new(0, 0, 0, 0)));
        assert!(s.starts_with("<svg"));
        assert!(s.trim_end().ends_with("</svg>"));
        let s = of_buffer(&buf(3, 2));
        assert!(s.starts_with("<svg"));
        assert!(!s.contains("<text"));
    }

    #[test]
    fn wide_char_is_emitted_once() {
        let mut b = buf(4, 1);
        b.set_stringn(0, 0, "中a", 4, Style::default());
        let s = of_buffer(&b);
        assert_eq!(s.matches('中').count(), 1);
        // 宽字占两列,它的中心在两列交界处;后面的字从第三列的中心开始
        assert!(s.contains("<text x=\"9.0\""), "宽字该跨两列居中:\n{s}");
        assert!(
            s.contains("<text x=\"22.5\""),
            "宽字后面的字该从第二格起:\n{s}"
        );
    }
}
