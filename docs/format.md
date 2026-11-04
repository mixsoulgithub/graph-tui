English | [中文](format.zh.md)

# Document format `.gtui`

A native document is a JSON object; `type` is used for self-validation and `unit` is always `cell`:

```json
{
  "type": "graph-tui",
  "version": 9,
  "source": "graph-tui 0.9.0",
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
     "from_side": "auto", "to_side": "auto",
     "via": [[12, 5], [12, 9]], "layer": ""}
  ],
  "view": {"center": [20, 10], "zoom": 1.0, "layout": "layered", "cursor": [11, 5],
           "focus": [], "labels": false, "lang": "zh", "layer": null,
           "side": 2.0, "cycle": null, "orientation": "auto",
           "notes": true},
  "regions": []
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
  longest line + 2, at least 4 wide and 3 tall). Inline styles are markdown's `**bold**` and
  `*italic*` (bold may contain italic; an unclosed marker is literal text); they and alignment apply per
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
  origin; one save upgrades it). The top-level `version` is 9; versions 1 through 8 are also
  read (a v4 `edge.label` is converted into a note box bound to the edge; v5 note boxes and v6
  multi-line text are the same structure; v7 adds regions; v8 adds explicit edge sides; v9 drops
  the legend).
- **v9 removed the legend.** `version: 8` files may still carry a top-level `legend` object and
  `view.legend`; both are read and **discarded** (the keys are accepted so old files open, and no
  content is migrated — there is no reliable mapping from a panel row to a box). Saving writes v9,
  which has neither key. If you want a key on the picture, draw it as content: text-only (`none`)
  boxes, a short edge showing the line style, group colours, and `geo` layout so nothing moves.
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
  itself; it is 0.9.0 now.
- The **`.gtui` format version** (`"version"` in the file) versions the document format; it is 9
  now. It only bumps on structural changes (v1 cursor as box id -> v2 cursor as cell -> v3 legend
  -> v4 edge labels -> v5 edge notes as bound boxes -> v6 multi-line text -> v7 regions -> v8
  explicit edge sides -> v9 legend removed), and old versions are always read. The two are
  independent.

## Regions (v7)

`regions` is a top-level array of cluster-like boxes. A region has no coordinates of its own: its
rectangle is the bounding box of its members plus `padding` (and it widens to fit `label`), so it
follows the boxes when they move.

```json
"regions": [
  {"id": "r1", "label": "connection setup", "color": "#22b46a", "style": "rounded",
   "layer": "", "padding": 1, "members": ["b1", "b2"], "collapsed": false}
]
```

- `members` are box ids or other region ids; a box may belong to at most one region, and nested
  regions must not form a cycle. Both are rejected on load.
- `style` must be a boxed style (`single` / `rounded` / `double` / `heavy`).
- A collapsed region draws as a small title bar with a `…` mark; its member boxes and the edges
  that touch them are not drawn (the data stays). Collapse is display-only.
- Regions are drawn under edges and boxes, so they never cover text; edges may cross a region
  border. The border and title are not picked by the cursor in v7 (commands address the region of
  the box under the cursor, or an explicit id).
- Files before v7 have no `regions` key and load with an empty list; saving always writes v8.

## Edge ports (v8)

An edge may pin which side of a box it leaves from / arrives at:

```json
{"id": "e1", "from": "b1", "to": "b2", "from_side": "left", "to_side": "up"}
```

- Values: `auto` (the default, written explicitly) / `left` / `right` / `up` / `down`. `auto`
  keeps the previous behaviour: the side is chosen from the difference of the two centres.
- A forced side only affects that one endpoint; the other may stay `auto`.
- Point nodes have no border, so a forced side is ignored for them (the TUI says so when you set
  one); `none` nodes do have a virtual border and honour it.
- Several edges pinned to the same side fill the free cells of that side in order (the existing
  `border_slot` rule); arrow heads still get a cell of their own.
- In the TUI: select the edge and use `:fromside` / `:toside` / `:port from=… to=…`, or open the
  style menu (`s`, or right-click) → **ports** → start / end → side; the menu previews live and
  applies with one undo step.
- Files before v8 load with `auto` on every edge; saving always writes v8.
