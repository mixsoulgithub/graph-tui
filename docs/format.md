English | [中文](format.zh.md)

# Document format `.gtui`

A native document is a JSON object; `type` is used for self-validation and `unit` is always `cell`:

```json
{
  "type": "graph-tui",
  "version": 6,
  "source": "graph-tui 0.1.1",
  "unit": "cell",
  "groups": [{"id": "g0", "label": "", "color": "#4c8ef8"}],
  "layers": [{"name": "", "order": 0}],
  "boxes": [
    {"id": "b1", "at": [10, 4], "size": [9, 3], "style": "rounded",
     "color": null, "fill": "#1b2440", "layer": "", "terms": ["central", "zx"],
     "edge": null},
    {"id": "b2", "at": [30, 4], "size": [8, 1], "style": "none",
     "color": null, "fill": null, "layer": "", "terms": [], "edge": "e1"}
  ],
  "texts": [
    {"id": "t1", "text": "Central\nsecond line", "at": [11, 5], "box": "b1", "color": null,
     "bg": null, "align": "center", "layer": ""}
  ],
  "edges": [
    {"id": "e1", "from": "b1", "to": "b2", "dir": "both", "group": "g0",
     "style": "solid", "color": null, "bg": null, "corner": "sharp",
     "via": [[12, 5], [12, 9]], "layer": ""}
  ],
  "view": {"center": [20, 10], "zoom": 1.0, "layout": "layered", "cursor": [11, 5],
           "focus": [], "labels": false, "lang": "zh", "layer": null,
           "side": 2.0, "cycle": null, "orientation": "auto", "legend": true,
           "notes": true},
  "legend": {
    "title": "transitions",
    "entries": [
      {"text": "CLOSED -> LISTEN      passive open", "group": "g0"},
      {"text": "SYN-SENT -> CLOSED    got RST", "color": "#ef4b4b", "glyph": null},
      {"text": ""}
    ]
  }
}
```

Key points:

- Boxes, texts and edges are three record types; boxes and texts are **strictly one-to-one** —
  a text always belongs to exactly one box.
- Coordinates are integer cells and `at` is the **top-left corner**. `style` has six values:
  `single` / `rounded` / `double` / `heavy` draw a frame; `point` occupies one cell and draws a
  node glyph; `none` **draws nothing** — the area holds text only (width follows the longest line,
  height follows the line count). Edges still route around it but never draw T-connectors or
  arrowheads on top of it — the arrowhead lands on the cell just outside.
  Ports still follow the border rules: several edges at one node each take their own entry/exit
  point (they are just not drawn), so they never pile onto the same cell.
  **`style` may be omitted and defaults to `none`**: a node with no style is just one line of text
  (`i` / `:new` / headless `add_box` create that by default).
- Text can be **multi-line**: `\n` splits lines; `\r` / `\t` are rejected. A box grows with the
  line count (`none`: height = lines, width = longest line; framed: height = lines + 2, width =
  longest line + 2, at least 4 wide and 3 tall). `**bold**` / `/italic/` and alignment apply per
  line. A point node has a single name; newlines are invalid there. The canonical form keeps no
  trailing blank line (the editor bridge strips exactly one trailing `\n`; interior blank lines are
  preserved); `"\n"` is two lines.
- Loading validates everything: unknown keys, duplicate keys, duplicate ids, dangling references,
  unknown layers, non-integer coordinates, two texts on one box, and text that does not fit the box
  (`lines > height - 2` or `longest line > width - 2`) are all rejected with a JSON path.
- Colours are independent layers: `box.color` is the **border**, `box.fill` the **interior fill**
  (ignored for points and `none`), `text.color` the **glyphs**, `text.bg` the **text background**
  (only under the glyph cells, takes priority over `fill`), `edge.color` the **line**, and
  `edge.bg` the **line background** (only on cells the edge passes through). `text.bg` / `edge.bg`
  are optional (old files without the key still load) and accept `null`. `text.align` is
  `left|center|right`; the left/right padding is only visible when the box is wider than the text.
- `edge.dir` is four-state `none|forward|back|both` (no longer just directed/undirected);
  `edge.corner` is `rounded|sharp`. Defaults are `forward` and `rounded`.
- An edge may **connect a node to itself** (`from` equals `to`): it is drawn as a small hook that
  leaves one side and comes back on an adjacent side, with the arrowhead on the returning end. It
  is not part of a "line" (it does not join series) and layouts do not layer it, but it can be
  selected, restyled with `:style` / `:text`, and exported like any other edge.
- `view` is view state only (centre, zoom, cursor cell, focus, layer, language, orientation…);
  it does not affect geometry, and **zoom is never multiplied into coordinates** when saving.
  `view.cursor` is a `[x, y]` cell in v2 (a box id in older files is converted to that box's text
  origin; one save upgrades it). The top-level `version` is 6; versions 1 / 2 / 3 / 4 / 5 are also
  read (a v4 `edge.label` is converted into a note box bound to the edge; v5 note boxes and v6
  multi-line text are the same structure).
- `legend` is a document-owned legend: an optional `title` plus any number of rows. Each row has a
  single-line `text`; `group` (borrow the group colour) and `color` (`#rrggbb`) are alternatives and
  `color` wins; the optional `glyph` is `solid|dashed|dotted|arrow` and draws a matching prefix
  sample; empty text is a blank row. With no `entries` the renderer falls back to the derived form
  (title `groups` + colours + three glyph explanations), which is what old files show. The on/off
  switch is not stored here — it is `view.legend` (missing key reads as `true`); content and view
  stay separate.
- The legend is not content: it does not take part in the bounding box, layout, hit-testing or
  selection, and camera pan/zoom does not scale it.
- An edge note is **a text box bound to an edge**: a box with `style: none` plus
  `"edge": "<edge id>"`. The text is the note and the box's own `at` is its position (no per-frame
  placement). It is strictly one-to-one: one edge has at most one note box, one box binds to at
  most one edge. Deleting the edge deletes the note box too (one undo step). Rendering never moves
  it; `i` on a selected edge creates it (suggested position = midpoint of the two centres shifted
  right by one cell, avoiding other boxes, computed once), and `:relabel [all]` re-anchors it.
  Its text colour follows the edge by default. `view.notes` is the switch (missing key reads as
  `true`): when off the note is not drawn, is not an obstacle, takes no space, and cannot be hit.
- Saves write `.tmp` and rename; a failed validation never overwrites the original, and saving the
  same document twice produces identical bytes.

### Importing `.dot` / `.canvas`

- `.dot` is parsed as a Graphviz subset and converted to native: framed nodes get a box sized from
  their text, point nodes become `1x1` points; metric `pos` is projected to cells once on this path
  (`geo` only makes sense there). Node style: `box=` wins; otherwise Graphviz `shape=point`
  (point) and `shape=none|plaintext` (text only) are recognised; with neither, the default is
  `none`. On export point nodes are written as `shape="point"` and other styles as the custom
  `box=` key. `.dot` escapes: `\n` / `\l` / `\r` all mean a line break, `\\` / `\"` are unescaped,
  and any other `\x` keeps its backslash (it used to be swallowed); export writes line breaks as
  `\n`. The node name goes into `terms` and `label` becomes the text. A `.dot` without `pos` is
  laid out once on import (otherwise every node sits at the origin). Style attributes use custom
  keys Graphviz ignores: `fill`, `align=left|center|right`, `corner=rounded|sharp`. Edge direction
  understands `->` `<-` `--` and `dir=none|forward|back|both` (`dir=<name>` has priority).
- `.canvas` (Obsidian) converts at 8x16 pixels per cell; a canvas has no direction semantics, so
  every edge imports as `forward`.
- Both formats are **import only**: `.gtui` is the editable source.
- After import there is no native source path: **the first `w` writes `<stem>.gtui` next to the
  input**, leaving the original `.dot` untouched.
- `.dot` export happens only through an explicit `--export x.dot` / `:export x.dot`; the file
  header says it is a lossy view and every lossy item is reported.
- All path arguments (`:w` / `:e` / `:export`, `--file` / `--export`) expand a leading `~`, and
  missing parent directories are **created automatically** (`:w ~/figs/a/b.gtui` makes the dirs).

Two version numbers, do not mix them:

- The **crate version** (`version` in `Cargo.toml`, `graph-tui --version`) versions the program
  itself; it is 0.1.1 now.
- The **`.gtui` format version** (`"version"` in the file) versions the document format; it is 6
  now. It only bumps on structural changes (v1 cursor as box id -> v2 cursor as cell -> v3 legend
  -> v4 edge labels -> v5 edge notes as bound boxes -> v6 multi-line text), and old versions are
  always read. The two are independent.
