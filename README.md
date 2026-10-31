English | [中文](README.zh.md)

# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

*Graph renderer and editor for the terminal: orthogonal box-drawing edges, one colour per group,
automatic crossings, pan/zoom, multi-line nodes and edge notes. Rust, `ratatui`.*

A general-purpose graph renderer and a small editor for the terminal. Give it a graph
(nodes + edges + edge groups) and it draws a grid of characters. Edges are orthogonal polylines
with rounded corners (`╭╮╯╰`); where two edges cross, one passes under the other (a single cell
is broken). Each cell holds exactly one colour, and each group gets one colour. Where an edge
meets a framed node, the start is a `┬ ┴ ├ ┤` connector (double `╦╩╠╣`, heavy `┳┻┣┫`), and the
arrowhead is **embedded in the border cell** — nothing is drawn outside the frame. Undirected
edges get connectors at both ends. Arrow glyphs default to `^ v < >`;
`--arrow ascii|arrows|triangles` switches to `↑↓←→` / `▲▼◀▶` (glyphs wider than one cell fall
back to ASCII).

Routing is **port-aligned**: each end picks the facing side from the difference between the two
centres, the connector cell is the middle one adjusted for odd/even width and height, and the
line only travels to the cell just outside the connector. Line, connector and arrowhead therefore
always sit on the same orthogonal axis. Bends are tried from fewest to most: straight if possible,
one orthogonal bend if that connects, and a small **jog** when the connector cells are off by one —
the jog hugs the target end so long straight runs stay clean. If two frames are too close for a
jog, the line runs the long way from the source port and then shifts over. Routes never pass
through a frame; when blocked they go around the outside, and if there is no way around they stop
at the last reachable cell.

The canvas is **infinite**: panning has no bounds, and `1:1` means one canvas coordinate equals
one column. Nodes can be framed (rounded/single/double/heavy) and carry their own colour; edges
have their own colour, line style (solid/dashed/dotted) and manual waypoints; text supports
`**bold**` and `/italic/` — all of it is editable in the UI.

Documents are native `.gtui` files (JSON, coordinates are integer cells). `.dot` and `.canvas`
are **import** formats only; `.dot` can additionally be exported as a best-effort lossy view
(not a source document).

```
                   North Gate ◎────────────╮
                              │            │
                              │            │
                              │────────────○────────────╮
                              │         Museum          │
                              │                         │  Harbour    groups
                              │                         │     ○       ── 1
                              │                         │     │       ── 2
 West End                     │                         │     │       ── 3
     ○────────────────────────◉─────────────────────────◎─────╯
                              │                     East Dock         ○ node
                              │                                       ◎ hub
                              │                                       > directed edge
                              │
                              ○ South Park
7 nodes  8 edges  3 groups  1:1  cursor Central  1  2  3
```

(The frame above is `--dump` output for `examples/metro.dot`, with blank lines trimmed;
`--dump` prints one true-colour ANSI frame and exits.)

## Install

```bash
cargo install graph-tui                 # crates.io
brew install mixsoulgithub/tap/graph-tui
```

Prebuilt archives (`graph-tui-<target>.tar.gz`, or `.zip` on Windows, each with a `.sha256`)
are on [Releases](https://github.com/mixsoulgithub/graph-tui/releases); unpack and
`install -m755 graph-tui ~/.local/bin/`.

### Build from source

```bash
cargo build --release
cargo install --path .          # installs to ~/.cargo/bin
```

Requires Rust 1.88 or newer (the MSRV of `ratatui 0.30`). The only dependencies are `ratatui` /
`crossterm` / `unicode-width`; at runtime there is no network and no database.

## Usage

```bash
graph-tui metro.gtui                     # open a native document (draws its stored coordinates)
graph-tui examples/pipeline.dot          # import .dot; the first `w` writes pipeline.gtui next to it
graph-tui                                # no file: scratch buffer (memory only, gone on exit)
graph-tui --dump --size 120x36 x.gtui    # one frame and exit, for screenshots and docs
```

With no file and a terminal on stdin you get a **scratch document**: one page in memory (one
group, one layer, one box `a`). No temporary file is written; it disappears on exit. Saving needs
an explicit path (`:w name.gtui`); without one you only get "no filename yet". If stdin is a pipe,
it is still read from the pipe.

```
--file <path>      read a document (`-` = stdin; a bare path argument works too)
--layout <name>    geo | relax | force | layered; default is picked from the graph
--dump             draw one true-colour ANSI frame to stdout and exit
                   format is chosen by extension: `.gtui` native, `.canvas` JSON Canvas,
                   everything else is treated as .dot
--size <WxH|auto>  canvas size for --dump / --export (default auto: content + padding;
                   smaller sizes crop instead of scaling)
--labels           start with every point-node name drawn (same as pressing b)
--no-labels        do not draw point-node names (--dump / --export)
--format <ansi|text>  --dump output: true-colour ANSI (default) or plain text
--legend           force the right-hand legend on (otherwise the document decides)
--no-legend        force it off (mutually exclusive with --legend)
--no-notes         do not draw edge notes (default follows the document's view.notes)
--drawn-cursor     draw the cursor inside the picture (default is the real terminal cursor;
                   TUI only; --dump/--export never draw a cursor)
--history <N>      undo depth (default 200)
--arrow <style>    arrow glyphs: ascii|arrows|triangles (default ascii)
--at <name>        put the cursor on that node at startup
--find <text>      run a search at startup (name / alias / group number)
--lang <zh|en>     UI language (default zh; command-line help and errors stay English)
--export <path>    write and exit: .gtui (native save) / .svg / .png (rasterised by
                   rsvg-convert) / .canvas (JSON Canvas) / .dot (best-effort lossy view,
                   warnings on stderr)
-v, --version      version
-h, --help         help
```

### Rendering: `--dump` / `--export`

Picture only, no UI: no status line, no cursor, no selection. The legend is part of the picture
and follows the document by default.

- **1:1**, never scaled; the picture is centred with padding (2 columns left/right, 1 row top/bottom).
- **Auto size**: without `--size`, one probe frame is drawn at 1:1, the rectangle that actually
  contains ink is measured (boxes, text, edges, T-connectors, arrowheads, point-node labels), and
  that region is copied out with padding. Nothing is redrawn, so auto-placed labels do not move
  between probe and output.
- `--size WxH` is exact, still 1:1 and still centred; **it crops when it does not fit**, it never
  shrinks, and says so on stderr. `--size auto` means no size.
- `--format text` prints plain text (no escape sequences); `--format ansi` (default) is true colour.
  stdout contains only the picture, with a single trailing newline.
- `--no-labels` omits point-node names; dump draws them by default.
- `--export x.svg` / `x.png` use the same auto size; PNG still goes through SVG + `rsvg-convert`.
- The legend is part of the picture: it follows `view.legend` and is drawn to the right of the map
  (at least 2 columns away, vertically centred); `--no-legend` removes it. Its width is capped at
  `min(terminal_width / 3, 44)` and overlong rows are truncated with `...`.
- Edge notes are content too: they follow `view.notes`, and `--no-notes` removes them (hidden notes
  are not obstacles and take no space).
- The headless `render` op behaves the same: `size` may be omitted or `"auto"`; `"legend": false`
  turns the legend off, and the JSON response includes `legend.rect`.

```bash
graph-tui --dump graph.gtui > graph.ansi
graph-tui --dump --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## Layouts

| name | what it does | requirements |
| --- | --- | --- |
| `geo` | keep current positions (only separate overlapping boxes) | any document |
| `relax` | spring relaxation from current positions; evens gaps and snaps directions to `- \| / \` | any document |
| `force` | Fruchterman-Reingold; snaps to horizontal/vertical/45° at the end | any document |
| `layered` | Sugiyama-style layering; best for directed dependency/flow graphs | any document (cycles included) |

Orientation (`view.orientation`): with `auto`, `force` / `layered` rotate the graph 90° when that
needs less zoom (no rotation if the difference is under 5%, so it does not flicker at the
threshold); `geo` and `relax` keep the input orientation because the coordinates carry meaning.
Opening a `.gtui` does not recompute coordinates, it only remembers this value. An explicit
`:rotate` changes coordinates (requires 1:1) and does not swap box width and height.

All four layouts produce **integer cell** coordinates, write them back to `box.at`, and translate
the attached text by the same amount; any two boxes are at least one cell apart. `r` cycles the
four in place. **Changing the layout never moves the cursor**; search and focus follow boxes.

All four layouts are **deterministic**: the same document run twice gives identical points
(no randomness).

## Keys

The full key map, mouse actions and every `:` verb are in **[docs/keybindings.md](docs/keybindings.md)**;
the most used ones:

| key | what it does |
| --- | --- |
| `h j k l` / arrows | move the cursor one cell (`5l` = five); selects the box or edge under it |
| `i` `t` `p` | create a box or edit the selected box text / edit text / paste (offsets on overlap) |
| `s` `c` | style menu (box style/colour/align…) / colour menu (box = border, edge = line) |
| `d` `x` | cut with edges / cut box and text only (both remove the connected edges) |
| `e` / `Ctrl-e` | two-phase link (source, then target; source = target makes a self-loop) / cycle through the edges adjacent to the cursor node |
| `E` | neighbour highlight: light only the cursor node, its neighbours and the connecting edges; follows the cursor; press `E` or `Esc` to leave |
| `Ctrl-q` / `v` | block selection (grey dashed range); `Enter` confirms and commands apply to the block |
| `Ctrl-hjkl` | move the selected boxes (or the cursor box) by one cell; numeric prefix works (`10`+`Ctrl-l` = ten cells, one undo step); the cursor follows and the view scrolls at the edge |
| `+` `-` `0` `f` `F` `z` | zoom, fit, centre |
| `/` `n` `N` `L` `b` `r` | search (= focus a group), change layer, toggle names, change layout |
| `u` / `Ctrl-r` | undo / redo |
| `:` `?` | command line (`Tab` completes verbs and values, `:w` saves) / quick reference |
| mouse | left click: cursor lands on that cell, repeated clicks cycle coincident edges (clicking outside a selection clears it); **double click on empty space = `i` new node**; drag on a node moves it, drag on empty space makes a block selection (release to finish, drag again for a new one); `Ctrl`+left or middle drag pans; wheel zooms; right click opens the action menu |
| `:q` `:wq` | quit / save and quit (use `:wq` if you want to keep it) |

The cursor is the real terminal cursor; selected elements pulse red/white (framed boxes pulse their
border, unframed ones pulse their text, no background fill). `--drawn-cursor` goes back to drawing
the cursor inside the buffer.

The bottom line is one row: the current element on the left, the command line / messages on the
right; it takes no space when idle.

## Document format `.gtui`

A native document is a JSON object; `type` is used for self-validation and `unit` is always `cell`:

```json
{
  "type": "graph-tui",
  "version": 6,
  "source": "graph-tui 0.1.0",
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

## Drawing from scratch

You do not need a `.dot` file first: start `graph-tui` with a scratch buffer and build everything
with `:` verbs.

```
graph-tui                 # scratch buffer, starts with one node a (default style none: one line of text)
:new B                    # new node on the cursor cell, text B, cursor follows
:box single               # give it a frame explicitly
:link a                   # draw an edge from the cursor node (B) to a; target can be an id (b1) or text
:box double               # switch the cursor box to a double border
:fill #1b2440             # interior fill
:textcolor #f59e0b        # text colour
:text hello world         # replace the text (the whole line is taken literally; **bold** /italic/ work; the box resizes)
E                         # select an edge; then :dir both / :corner sharp / :style dashed
:w name.gtui              # save; without a path nothing is written (:w only echoes a hint)
:w ~/figs/name.gtui       # leading ~ expands to the home directory (command-line args too)
:q                        # quit (only :q quits); unsaved work is gone, no temp files left behind
:wq                       # save as .gtui and quit; on failure it stays open
```

`del` deletes the cursor box (with its text and edges); if an edge is selected it deletes that
edge. The last box cannot be deleted. New nodes default to `none` (one line of text, width follows
the text); `:box` adds a frame.

## Editing

`Ctrl-e` selects an edge; `:` verbs change attributes; `:w` saves. `e` is the two-phase link: pick
the source (selection or cursor box), then the target, `Enter` creates the edge and `Esc` cancels.
1->1 means "press `e` on A, move to B, press `Enter`"; when source and target are the same box you
get a self-loop (out one side, back in on an adjacent side). 1->N means "press `e`, block-select
multiple targets with `v`, `Enter`"; N->M means block-select the sources first.

**Editing only works at `1:1`**: while zoomed you can only look (`VIEW x2.0`). Typing an edit
command first snaps the view back to `1:1` and centres it, then runs the command; the bottom line
shows `VIEW` (or `VIEW x2` when zoomed).

In the bottom-line prompt (new node / edit text) press **`Ctrl+G`** to hand the current content to
`$VISUAL` / `$EDITOR` (falling back to `vi`). When the editor exits the file is read back and
replaces the prompt: unchanged content (`:q!`, an immediate exit, a GUI editor that does not block)
counts as cancel; a non-zero exit keeps the original and reports the code; invalid UTF-8, CR or NUL
are rejected. You still press `Enter` to commit, `Esc` to discard. The editor must **block**
(`code -w` / `vim`); the command line and search do not accept this key (pressing it only shows a
hint).

On a selected edge, `i` creates or edits its note box (an empty box is removed by `Esc` or an empty
Enter); `a` or `:notes` collapses/expands all notes; `:relabel [all]` re-anchors note boxes to
their edges. When notes are collapsed they are not drawn, are not obstacles, and take no space.

With a selection, `box` / `nodecolor` / `fill` / `textcolor` / `textalign` / `text` apply to the
**whole selection**; when `L` shows a single layer, verbs only touch that layer. Every verb is in
[docs/keybindings.md](docs/keybindings.md#动词); the style overview is
`examples/styles-all.gtui` (six box styles, bold/italic, text colour, fills, three alignments,
three line styles, four directions, two corner styles on one page), import examples are in
`examples/styles.dot`; edge notes are shown in `examples/edge-notes.gtui` (five edges, each with a
note box); multi-line text is in `examples/multiline.gtui` (a three-line framed box, a two-line
text-only node, and a two-line edge note).

Undo/redo is snapshot-based, 200 steps by default (`--history N`); setting the same value does not
consume a step, a whole drag is one step, and view-only actions never enter the stack. Details in
[docs/keybindings.md](docs/keybindings.md#撤销与重做).

Layers stack overlapping items: boxes and edges both carry a `layer`, drawing follows layer order
with the current layer on top, and items outside the current layer are dimmed. Moving a box changes
`box.at` and its `text.at` (integers); positions are fixed once saved, and only `r` re-lays them out.

## Headless (for scripts and AI)

No TUI, no TTY: one JSON request per line, one JSON response per line. stdout carries protocol data
only, diagnostics go to stderr, and state (document, undo stack, clipboard) persists within a
session.

```bash
graph-tui headless --file a.gtui --op '{"op":"info"}'
graph-tui headless --file a.gtui < requests.ndjson > responses.ndjson
graph-tui headless --file a.gtui --batch ops.ndjson --save-on-exit [--partial]
```

- Request: `{"id":1,"op":"add_box","label":"A","at":[10,4]}` (arguments may sit at the top level or
  inside `args`).
- Response: `{"id":1,"ok":true,"protocol":1,"result":{...}}`; failures look like
  `{"ok":false,"error":{"code":"bad_ref","path":"targets[0]","message":"no such box: b9"}}`.
- Exit codes: `0` all succeeded, `2` bad request, `3` invalid document, `4` IO failure,
  `5` transaction rolled back.
- Ops: `info` `list` `get` `inspect_cell` `add_box` `add_edges` `set` `move` `delete`/`cut`
  `paste` `layer` `text` `layout` `orient` `rotate` `view` `render` `export` `undo` `redo`
  `validate` `schema` `open` `save` `transaction`. Everything uses **stable ids**, never array
  indices.
- `transaction`: commits only if the whole group succeeds, otherwise rolls back entirely and
  consumes one undo step; `add_edges` inside it can use ids returned by earlier `add_box` calls.
- `dry_run: true`: computes the result and returns ids without changing the document.
- `render format=text|ansi|json`: text is plain monospace cells (no colour); json additionally
  reports each box's screen rectangle and each edge's visible cells and note (`note` / `note_rect`);
  `ansi` uses the same renderer as `--dump`.
- Writes only accept `.gtui` (atomic); `.dot` / `.canvas` are import-only. Batches are capped at
  4096 edges and renders at 1000x1000.

## Relation to metro-tui

The renderer (direction-aware glyph composition, crossings with underpasses, one colour per cell,
label placement) and the `relax` spring routine were lifted from the same author's
[metro-tui](https://github.com/mixsoulgithub/metro-tui), where the metro semantics (`line` /
`station` / `interchange`) were generalised into `group` / `node` / `hub`. metro-tui is a special
case of this engine. Both are MIT.

## Stress cases (examples/limits/)

These graphs deliberately push the engine to its limits:

| file | what it stresses |
| --- | --- |
| `star40.dot` | a hub with 40 leaves: one cell has only four directions, fan-out at the limit |
| `k12.dot` | K12 complete graph: 66 edges crammed onto 12 points |
| `grid20.dot` | 20x20 grid, 400 points, 760 edges: density and timing |
| `chain60.dot` | a 60-long chain through `layered`: 3-cell layer height x 60 layers, stacks when the window is too short |
| `ring.dot` | a directed cycle; layered layout must break one back edge |
| `parallel.dot` | four edges on one node pair across four groups: each gets its own parallel line (no border cells left to shift into, so lane offsets do it) |
| `selfloop.dot` | self-loop: out one side, back in on an adjacent side |
| `isolated.dot` | isolated node plus one connected component |
| `components.dot` | four disconnected components |
| `inarrow.dot` | 12 incoming edges to one point: arrowheads only have four directions |
| `collinear.dot` | eight collinear nodes |
| `samepos.dot` | five nodes with identical coordinates |
| `outlier.dot` | one distant outlier expands the bounding box |
| `dupname.dot` | two nodes with the same name (one node in .dot semantics) |
| `longlabel.dot` / `cjk.dot` | very long labels and CJK label width |

They measure the edges of the engine: after projection nodes can land on the same cell, lines can
run out of room, labels may not fit — everything is still drawn (overlaps stay overlapped) and the
status line does not report these details; that space belongs to commands and messages.

## Real-world graphs (examples/real/)

These are not invented: each was converted directly from real data lying around, and the file
header records the source and how to regenerate it.

| file | data source | size |
| --- | --- | --- |
| `modules.dot` | this crate's own module dependencies (`crate::xxx` references in the source) | 11 modules, 18 edges |
| `cargo-deps.dot` | this crate's `Cargo.lock`, one level deep | 21 packages, 18 edges |
| `schema.dot` | foreign keys of the local Elisa music player database (`pragma foreign_key_list`) | 12 tables, 8 foreign keys |
| `systemd.dot` | reverse dependencies of the local `dbus.socket` (`systemctl show` Requires / Wants / After) | 21 units, 51 edges |
| `tcp.dot` | the TCP connection state machine (RFC 793) | 11 states, 26 directed edges |
| `tcp.gtui` | the native version of the above: hand-placed coordinates, all nodes text-only (`none`), the `pair` edge is dashed, with a legend, three edge notes and seven self-loops | 11 nodes, 26 edges |

Those five graphs (six files; `tcp` has both `.dot` and `.gtui`) all sit at 11–21 nodes — not a
coincidence: that is about the size a terminal can draw and a person can read. Beyond that (say,
fully expanding the transitive dependencies of `Cargo.lock`, a hundred-plus packages) it turns into
a knot, and a GUI would not help; that is not a terminal problem.

The "stay in place" actions in `tcp.dot` (retransmit, timeout, resend FIN in the RFC diagram) are
drawn as self-loops: out one side, back in on an adjacent side, in their own orange dashed group,
listed one by one in the legend.

## Known trade-offs

- `.png` export relies on `rsvg-convert` installed on the system (a terminal cannot rasterise
  fonts); without it you get a clear error.
- `.dot` export is lossy: same-name nodes merge, `via`/`layer`/`terms` mean different things in
  Graphviz, and camera state cannot be expressed.
- Editing requires `1:1`: to place things precisely, zoom in to see, then press `0` to return to 1:1.
- Zoom scales **geometry only**; glyphs are always one cell per character. `f` / `F` fit at most to
  `1:1`; going further just spreads boxes apart with the same glyph size, and below one cell per
  glyph characters get clipped — that is only a navigation overview.
- Opening a document does not auto-fit (zoom is document state; a new document is `1:1`); press `F`
  to see everything.
- `cargo clippy --all-targets` is warning-free; the two routing primitives take many arguments and
  explicitly `allow(too_many_arguments)` with the reason written down.
- `via` points are **canvas coordinates**: after a layout change everything else moves and the
  waypoints stay put, so they need resetting.
- Dashed/dotted styles only break straight runs; corners and crossings are always solid (otherwise
  you cannot see where a line turns); a cell already taken by a solid line prefers solid.
- Italic depends on terminal support (`Modifier::ITALIC`); some terminals render it as normal text
  or reverse video.
- Saving rewrites node ids to their labels: for graphs that used a separate `label=`, the old id
  moves into `terms` after one save.
- Routing is orthogonal only: a 45° edge becomes an L on screen; there are no diagonal strokes
  (`╱ ╲ ╳`) because East Asian widths are uncertain.
- The `Ctrl+G` external editor is launched through `sh -c` and needs a Unix shell; the Windows build
  works, but that key reports it is unavailable.
- Glyphs such as `○ ◎ ─` are two cells wide in some CJK fonts, which shifts the picture.
- Nodes have no shape or size concept and labels have no leader lines; labels are dropped when
  positions collide.
- `layered` is Sugiyama-lite: layering, barycentre ordering and back-edge breaking are there, but
  there is no crossing-minimising orthogonal router, so deep graphs look very "vertical".
- `geo` requires every node to have coordinates; without them it falls back to `force` or `layered`.
- Scale: 6000 nodes with no coordinates take about 3.5 s in `force` (release); beyond that the
  iteration cap keeps it bounded.

## Versioning

Two version numbers, do not mix them:

- The **crate version** (`version` in `Cargo.toml`, `graph-tui --version`) versions the program
  itself; it is 0.1.0 now.
- The **`.gtui` format version** (`"version"` in the file) versions the document format; it is 6
  now. It only bumps on structural changes (v1 cursor as box id -> v2 cursor as cell -> v3 legend
  -> v4 edge labels -> v5 edge notes as bound boxes -> v6 multi-line text), and old versions are
  always read. The two are independent.

## License

MIT, see [LICENSE](LICENSE).
