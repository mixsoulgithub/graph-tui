//! Render glyphs: Unicode box-drawing characters by default, plus a pure ASCII fallback.
//!
//! On terminals whose font lacks box-drawing characters, or draws them two cells wide,
//! Unicode misaligns; `--ascii` swaps in a full ASCII glyph set (corners, tees and
//! crossings are all `+`, double and heavy lines degrade to single). Colors and topology
//! are unchanged; only the glyphs differ.

use crate::model::{BoxStyle, Corner};

/// One glyph set. Constant table, `Copy`, no allocation on the render hot path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Glyphs {
    /// true = pure ASCII (`+ - | o @`)
    pub ascii: bool,
}

impl Glyphs {
    /// Default: Unicode box-drawing characters.
    pub const UNICODE: Glyphs = Glyphs { ascii: false };
    /// Pure ASCII.
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

    /// Direction mask -> stroke glyph (horizontal/vertical + tees/crosses + corners). A
    /// lone direction bit falls back to a horizontal/vertical line.
    pub fn line(&self, mask: u8, corner: Corner) -> char {
        if !self.ascii {
            return crate::canvas::box_char(mask, corner);
        }
        // Direction bits follow canvas's definition: LEFT=1, RIGHT=2, UP=4, DOWN=8
        const L: u8 = 1;
        const R: u8 = 2;
        const U: u8 = 4;
        const D: u8 = 8;
        match mask & 0b1111 {
            0 => ' ',
            // Single direction: the segment itself
            R | L => '-',
            U | D => '|',
            // Left/right and up/down: straight line; any other combination (corner, tee,
            // cross) is '+'
            m if m == L | R => '-',
            m if m == U | D => '|',
            _ => '+',
        }
    }

    /// Junction on a box border (horizontal plus the side it exits through).
    pub fn junction(&self, style: BoxStyle, mask: u8) -> char {
        if self.ascii {
            return self.line(mask, Corner::Sharp);
        }
        crate::canvas::junction_glyph(style, mask)
    }

    /// The six glyphs of a box style: horizontal, vertical, top-left, top-right,
    /// bottom-left, bottom-right.
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

    /// Point node glyph: hub / ordinary.
    pub fn node(&self, hub: bool) -> char {
        match (self.ascii, hub) {
            (true, true) => '@',
            (true, false) => 'o',
            (false, true) => '◎',
            (false, false) => '○',
        }
    }

    /// Point node under the cursor: solid glyph.
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
    fn both_glyph_sets_are_consistent() {
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
        // Corner: Unicode has a direction, ASCII is always '+'.
        assert_eq!(u.line(10, Corner::Sharp), '┌');
        assert_eq!(a.line(10, Corner::Sharp), '+');
        // Double and heavy lines degrade to single under ASCII.
        assert_eq!(
            a.box_chars(BoxStyle::Double),
            ['-', '|', '+', '+', '+', '+']
        );
        assert_eq!(u.box_chars(BoxStyle::Double)[0], '═');
        assert_eq!(a.box_chars(BoxStyle::None), [' '; 6]);
        // Name round trip.
        assert_eq!(Glyphs::parse("ASCII"), Some(a));
        assert_eq!(Glyphs::parse("unicode"), Some(u));
        assert_eq!(Glyphs::parse("nope"), None);
        assert_eq!(a.name(), "ascii");
    }
}
