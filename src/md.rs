//! Markdown -> terminal text, just enough for the CLI help that lives in
//! `docs/usage.md`: headings, tables, lists, fenced code and inline code / emphasis /
//! links. The output is plain text wrapped to a fixed width, so the same markdown can be
//! read on GitHub and printed by `--help`.
//!
//! Deliberately small: no nesting, no HTML, no auto-links. Unknown constructs pass
//! through as text.

use unicode_width::UnicodeWidthStr;

/// Render `src` as terminal text: tables become aligned columns, heading markers and
/// inline markup are dropped, paragraphs are wrapped to `width`.
pub fn to_terminal(src: &str, width: usize) -> String {
    let width = width.max(1);
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            push_blank(&mut out);
            i += 1;
            continue;
        }
        if let Some(fence) = fence_open(trimmed) {
            i += 1;
            while i < lines.len() && !is_fence_close(lines[i], fence) {
                out.push(lines[i].trim_end().to_string());
                i += 1;
            }
            i += 1;
            continue;
        }
        if let Some(text) = heading(trimmed) {
            push_blank(&mut out);
            out.push(inline(text));
            i += 1;
            continue;
        }
        if is_table_row(trimmed) {
            let start = i;
            while i < lines.len() && is_table_row(lines[i].trim_end()) {
                i += 1;
            }
            out.extend(table(&lines[start..i], width));
            continue;
        }
        if let Some((marker, rest)) = list_item(trimmed) {
            let text = paragraph(lines_before_block(&lines[i + 1..]), rest);
            let rest_width = width.saturating_sub(marker.len());
            let wrapped = wrap(&text, rest_width);
            let mut first = true;
            for chunk in wrapped {
                if first {
                    out.push(format!("{marker}{chunk}"));
                    first = false;
                } else {
                    out.push(format!("{}{chunk}", " ".repeat(marker.len())));
                }
            }
            // the continuation lines were consumed as part of the paragraph
            i += 1 + lines_before_block(&lines[i + 1..]).len();
            continue;
        }
        // Paragraph: join lines until a blank line or the start of another block
        let block = lines_before_block(&lines[i..]);
        let text = paragraph(block, "");
        out.extend(wrap(&text, width));
        i += block.len();
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// Lines that belong to the current paragraph: up to the next blank line or block start.
fn lines_before_block<'a, 'b>(lines: &'a [&'b str]) -> &'a [&'b str] {
    let mut n = 0;
    for line in lines {
        let t = line.trim_end();
        if t.trim().is_empty() || heading(t).is_some() || is_table_row(t) || fence_open(t).is_some()
        {
            break;
        }
        if n > 0 && list_item(t).is_some() {
            break;
        }
        n += 1;
    }
    &lines[..n]
}

fn push_blank(out: &mut Vec<String>) {
    if !out.is_empty() && !out.last().is_some_and(|l| l.is_empty()) {
        out.push(String::new());
    }
}

fn fence_open(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with("```") {
        Some("```")
    } else if t.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn is_fence_close(line: &str, fence: &str) -> bool {
    line.trim_start().starts_with(fence)
}

/// `## Title` -> `Title` (ATX headings only).
fn heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        Some(line[hashes..].trim_start())
    } else {
        None
    }
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// `- text`, `* text`, `+ text` or `12. text`
fn list_item(line: &str) -> Option<(&'static str, &str)> {
    let t = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(marker) {
            return Some(("- ", rest));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && t[digits..].starts_with(". ") {
        return Some(("1. ", &t[digits + 2..]));
    }
    None
}

fn paragraph(block: &[&str], first: &str) -> String {
    let mut text = String::from(first.trim());
    for line in block {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
            text.push_str(t);
        } else {
            text.push_str(t);
        }
    }
    inline(&text)
}

/// Drop inline markdown: code ticks, emphasis markers, link syntax (the text stays, the
/// target does not), and unescape `\|`.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '`' => i += 1,
            '*' | '~' if i + 1 < chars.len() && chars[i + 1] == c => i += 2,
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '[' => {
                // [text](target) -> text
                if let Some(close) = find(&chars, i + 1, ']') {
                    if close + 1 < chars.len() && chars[close + 1] == '(' {
                        if let Some(end) = find(&chars, close + 2, ')') {
                            out.push_str(&inline(&chars[i + 1..close].iter().collect::<String>()));
                            i = end + 1;
                            continue;
                        }
                    }
                }
                out.push(c);
                i += 1;
            }
            '<' => {
                // Only the inline style tags carry no meaning in plain text; angle
                // brackets in metavars (`<path>`, `<x,y,w,h>`) stay.
                if let Some(close) = find(&chars, i + 1, '>') {
                    let body: String = chars[i + 1..close].iter().collect();
                    if is_style_tag(&body) {
                        i = close + 1;
                        continue;
                    }
                }
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Inline style tags the app understands (`<u>`, `<dim>`, `<fg=#rrggbb>`, ...)
fn is_style_tag(body: &str) -> bool {
    let tag = body.trim_start_matches('/');
    matches!(tag, "u" | "dim") || tag.starts_with("fg=") || tag.starts_with("bg=")
}

fn find(chars: &[char], from: usize, want: char) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, c)| **c == want)
        .map(|(i, _)| i)
}

/// Word wrap on single spaces; long words are kept whole.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let extra = if line.is_empty() {
            width_of(word)
        } else {
            width_of(word) + 1
        };
        if !line.is_empty() && width_of(&line) + extra > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn width_of(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Split a table row into cells, honouring `\|` and stripping the outer pipes.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('\\');
                cur.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.iter().map(|c| inline(c.trim())).collect()
}

fn is_separator(row: &[String]) -> bool {
    !row.is_empty()
        && row.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Markdown table -> aligned columns. Columns shrink (and cells wrap) when the natural
/// width does not fit, so a long description stays inside the terminal.
fn table(rows: &[&str], width: usize) -> Vec<String> {
    let mut body: Vec<Vec<String>> = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        let cells = cells(row);
        if n == 1 && is_separator(&cells) {
            continue;
        }
        body.push(cells);
    }
    if body.is_empty() {
        return Vec::new();
    }
    let cols = body.iter().map(Vec::len).max().unwrap_or(0);
    let indent = 2;
    let gap = 2;
    let mut widths = vec![0usize; cols];
    for row in &body {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(width_of(cell));
        }
    }
    let budget = width.saturating_sub(indent + gap * cols.saturating_sub(1));
    while widths.iter().sum::<usize>() > budget {
        let Some((widest, _)) = widths
            .iter()
            .enumerate()
            .max_by_key(|(c, w)| (**w, std::cmp::Reverse(*c)))
        else {
            break;
        };
        if widths[widest] <= 12 || budget < 12 {
            break;
        }
        widths[widest] -= 1;
    }
    let mut out = Vec::new();
    for row in &body {
        let wrapped: Vec<Vec<String>> = row
            .iter()
            .enumerate()
            .map(|(c, cell)| wrap(cell, widths[c]))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let mut text = " ".repeat(indent);
            for (c, w) in widths.iter().enumerate() {
                let cell = wrapped
                    .get(c)
                    .and_then(|v| v.get(line))
                    .cloned()
                    .unwrap_or_default();
                if c + 1 == cols {
                    text.push_str(&cell);
                } else {
                    text.push_str(&cell);
                    text.push_str(&" ".repeat(w.saturating_sub(width_of(&cell)) + gap));
                }
            }
            out.push(text.trim_end().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_and_markup_lose_their_syntax() {
        let text = to_terminal(
            "## Options\n\nUse `--ascii` with **bold** and [docs](docs/x.md).\n",
            80,
        );
        assert_eq!(text, "Options\n\nUse --ascii with bold and docs.");
    }

    #[test]
    fn table_becomes_aligned_columns() {
        let text = to_terminal(
            "| Flag | What |\n| --- | --- |\n| `--at` | jump |\n| `--find` | search |\n",
            80,
        );
        let lines: Vec<&str> = text.lines().collect();
        let at = |name: &str| lines.iter().position(|l| l.contains(name)).expect(name);
        // every description starts in the same column
        let col = lines[at("What")].find("What").unwrap();
        assert_eq!(lines[at("jump")].find("jump"), Some(col));
        assert_eq!(lines[at("search")].find("search"), Some(col));
        assert!(!text.contains('|'));
    }

    #[test]
    fn long_table_cells_wrap_inside_the_width() {
        let text = to_terminal(
            "| K | Description |\n| --- | --- |\n| a | one two three four five six seven eight |\n",
            40,
        );
        assert!(text.lines().all(|l| width_of(l) <= 40), "{text}");
        assert!(text.contains("seven"), "{text}");
        assert!(text.lines().count() >= 2);
    }

    #[test]
    fn escaped_pipe_stays_in_the_cell() {
        let text = to_terminal("| A | B |\n| --- | --- |\n| x | <left\\|right> |\n", 80);
        assert!(text.contains("<left|right>"), "{text}");
        assert_eq!(text.lines().last().unwrap().matches('|').count(), 1);
    }

    #[test]
    fn fenced_code_is_kept_verbatim() {
        let text = to_terminal("```text\nusage: x  [a | b]\n```\n", 80);
        assert_eq!(text, "usage: x  [a | b]");
    }

    #[test]
    fn lists_keep_a_bullet_and_hang_continuations() {
        let text = to_terminal("- one two three four five six seven\n", 24);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("- one"));
        assert!(lines[1].starts_with("  "), "{text}");
        assert!(lines.iter().all(|l| width_of(l) <= 24));
    }

    #[test]
    fn paragraphs_are_rewrapped() {
        let text = to_terminal("one two\nthree four\n", 10);
        assert_eq!(text, "one two\nthree four");
    }
}
