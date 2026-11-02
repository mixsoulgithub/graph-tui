//! 渲染字形:默认 Unicode 制表符,另有一套纯 ASCII 的退路.
//!
//! 字体缺制表符、或者把制表符画成两格宽的终端上,Unicode 会错位;`--ascii` 换成一整套
//! ASCII 字形(拐角/三通/交叉一律 `+`,双线与粗线退化成单线).颜色与拓扑不变,只有字形变.

use crate::model::{BoxStyle, Corner};

/// 一套字形.常量表,`Copy`,渲染热路径上不分配
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Glyphs {
    /// true = 纯 ASCII(`+ - | o @`)
    pub ascii: bool,
}

impl Glyphs {
    /// 默认:Unicode 制表符
    pub const UNICODE: Glyphs = Glyphs { ascii: false };
    /// 纯 ASCII
    pub const ASCII: Glyphs = Glyphs { ascii: true };

    pub fn parse(s: &str) -> Option<Glyphs> {
        match s.trim().to_ascii_lowercase().as_str() {
            "unicode" | "utf8" | "utf-8" => Some(Glyphs::UNICODE),
            "ascii" | "plain" => Some(Glyphs::ASCII),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        if self.ascii {
            "ascii"
        } else {
            "unicode"
        }
    }

    /// 方向位 -> 笔画字形(横竖 + 三通四通 + 拐角).独立的一格方向位落到横线/竖线
    pub fn line(&self, mask: u8, corner: Corner) -> char {
        if !self.ascii {
            return crate::canvas::box_char(mask, corner);
        }
        // 方向位沿用 canvas 的定义:LEFT=1, RIGHT=2, UP=4, DOWN=8
        const L: u8 = 1;
        const R: u8 = 2;
        const U: u8 = 4;
        const D: u8 = 8;
        match mask & 0b1111 {
            0 => ' ',
            // 只朝一个方向:线段本身
            R | L => '-',
            U | D => '|',
            // 左右 / 上下:直线;其它组合(拐角、三通、交叉)一律 '+'
            m if m == L | R => '-',
            m if m == U | D => '|',
            _ => '+',
        }
    }

    /// 边框上的连接点(横向 + 从哪一面出去)
    pub fn junction(&self, style: BoxStyle, mask: u8) -> char {
        if self.ascii {
            return self.line(mask, Corner::Sharp);
        }
        crate::canvas::junction_glyph(style, mask)
    }

    /// 一种框型的六个字形:横、竖、左上、右上、左下、右下
    pub fn box_chars(&self, style: BoxStyle) -> [char; 6] {
        if !self.ascii {
            let (h, v, tl, tr, bl, br) = style.glyphs();
            return [h, v, tl, tr, bl, br];
        }
        if style.is_boxed() {
            ['-', '|', '+', '+', '+', '+']
        } else {
            [' '; 6]
        }
    }

    /// 点节点字形:枢纽 / 普通
    pub fn node(&self, hub: bool) -> char {
        match (self.ascii, hub) {
            (true, true) => '@',
            (true, false) => 'o',
            (false, true) => '◎',
            (false, false) => '○',
        }
    }

    /// 光标所在的点节点:实心字形
    pub fn node_cursor(&self, hub: bool) -> char {
        if self.ascii {
            return if hub { '@' } else { '#' };
        }
        if hub {
            '◉'
        } else {
            '●'
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 两套字形都自洽() {
        let u = Glyphs::UNICODE;
        let a = Glyphs::ASCII;
        assert_eq!(u.node(false), '○');
        assert_eq!(u.node(true), '◎');
        assert_eq!(a.node(false), 'o');
        assert_eq!(a.node(true), '@');
        // LEFT|RIGHT = 3,UP|DOWN = 12,DOWN|RIGHT = 10
        assert_eq!(u.line(3, Corner::Rounded), '─');
        assert_eq!(a.line(3, Corner::Rounded), '-');
        assert_eq!(u.line(12, Corner::Sharp), '│');
        assert_eq!(a.line(12, Corner::Sharp), '|');
        // 拐角:Unicode 有方向,ASCII 统一 '+'
        assert_eq!(u.line(10, Corner::Sharp), '┌');
        assert_eq!(a.line(10, Corner::Sharp), '+');
        // 双线与粗线在 ASCII 下退化成单线
        assert_eq!(
            a.box_chars(BoxStyle::Double),
            ['-', '|', '+', '+', '+', '+']
        );
        assert_eq!(u.box_chars(BoxStyle::Double)[0], '═');
        assert_eq!(a.box_chars(BoxStyle::None), [' '; 6]);
        // 名字往返
        assert_eq!(Glyphs::parse("ASCII"), Some(a));
        assert_eq!(Glyphs::parse("unicode"), Some(u));
        assert_eq!(Glyphs::parse("nope"), None);
        assert_eq!(a.name(), "ascii");
    }
}
