# Usage: CLI, rendering, layouts

## Opening documents

```bash
graph-tui metro.gtui                     # open a native document (draws its stored coordinates)
graph-tui new.gtui                       # does not exist yet: new file, `:w` writes it (like nvim)
graph-tui examples/pipeline.dot          # import .dot; the first `w` writes pipeline.gtui next to it
graph-tui                                # no file: scratch buffer (memory only, gone on exit)
graph-tui --export - --area 0,0,120,36 a.gtui  # a 120x36 window from canvas (0,0)
graph-tui x.gtui > /tmp/x.txt            # stdout is not a terminal: one frame, plain text
graph-tui x.gtui | nvim -                # same thing: stdout is a pipe
graph-tui tutor                          # built-in tutorial: a scratch copy
```

**Tabs**:
Several files open as tabs (`graph-tui a.gtui b.dot`); each tab keeps its own document, cursor,
zoom, undo history and file stamp, while the clipboard is shared. `Ctrl-t` / `Ctrl-n` / `Ctrl-p` /
`Ctrl-w` (or `:tabnew` / `:tabnext` / `:tabprev` / `:tabclose`) manage them, and the status line
shows `[2/4] name *`. `:tabclose` refuses a tab with unsaved changes unless you add `!`; `:e` also
refuses to replace a dirty document.

**Hot-reload**:
The file is watched while open: a clean document reloads automatically when another program
rewrites it; a modified one gets a persistent warning (`R` reloads and drops the edits, `:w`
overwrites). Details in [keybindings](keybindings.md#external-changes).

**Export**:
Picture exports are 1:1, with no status line, cursor or selection. `--export -` writes one frame
to stdout instead of entering the canvas; a non-terminal stdout does the same, so
`graph-tui a.gtui > out` and `graph-tui a.gtui | nvim -` need no flag. The format follows the
destination: plain text for a pipe or a file (or `NO_COLOR` / `TERM=dumb`), true-colour ANSI
otherwise; `--format ansi` / `--format text` overrides it. Use `--ascii` or
`[prefs] render = ascii` for terminals without box-drawing glyphs.

- `--area x,y,w,h` for exact export. `x,y` is the
  canvas cell that lands at the picture's top-left (`auto` centres the content), `w,h` is the
  output size in cells (`auto` uses the content box + padding). `--area auto` is the same as no
  flag, `--area auto,auto,120,36` centres the content in a 120x36 output, `--area 0,0,120,36`
  anchors the window at the canvas origin. Still 1:1: **it crops when the content does not fit**, it never
  shrinks, and says so on stderr.
- `--no-node-labels` omits point-node names; dump draws them by default.
- `--export x.svg` / `x.png`; PNG still goes through SVG + `rsvg-convert`.
  One cell is 9x18 px and the font is 15 px, so the text advance and the cell width agree. Border
  and line glyphs (`─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼ ╭ ╮ ╰ ╯`, heavy variants too) are emitted as `<path>`
  segments between cell edges, never as font characters, so strokes stay joined no matter which
  font the viewer picks; every other glyph is centred in its cell (`text-anchor="middle"`), so a
  font whose advance is not exactly 9 px cannot drift across a row.
- Edge labels are content too: they follow `view.edge_labels`, and `--no-edge-labels` removes them (hidden labels
  are not obstacles and take no space).

```bash
graph-tui --export - --format ansi graph.gtui > graph.ansi
graph-tui --export - --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## Layouts

| name | what it does | 
| --- | --- | 
| `auto` | **default**: read the structure and pick one of the layouts below, pack disconnected components and fall back to `relax` with a `dense_graph` warning on clique-like graphs | 
| `tree` | tidy tree: parents above children and centred over their subtree, siblings in their original order; extra edges are routed, polytrees use the first parent. A pure star (one hub, every other node degree 1) is drawn as a **fan** instead: leaves in up to four vertical columns of ten, two per side, hub on the vertical middle line — 40 leaves become a `61x19` block rather than a `527`-cell-wide row (resolved profile `star`) |
| `chain` | longest path as a straight line, branches as spurs below it; a pure cycle breaks one deterministic edge | 
| `geo` | keep current positions (only separate overlapping boxes) | 
| `relax` | spring relaxation from current positions; evens gaps and snaps directions to `- \| / \`. Gaps are measured per axis (a box is one row tall, so stacked boxes sit rows apart, not box-widths apart), and an anchor keeps the result near a `sqrt(n)`-cells square instead of letting it creep outward | 
| `force` | Fruchterman-Reingold; snaps to horizontal/vertical/45° at the end | 
| `layered` | Sugiyama-style layering: back edges are cut, long edges get dummy nodes so the middle layers can order around them, then alternating barycentre sweeps plus a transpose pass pick the order with the fewest crossings; best for directed dependency/flow graphs |

`auto` is what new documents and `.dot` / `.canvas` imports use (`view.layout` stores the name, the
resolved profile is not written back). `r` cycles the six concrete layouts `geo -> relax -> force ->
layered -> tree -> chain -> geo`; `auto` is only reachable with `:layout auto` or `--layout auto`.
The status line reports the resolved profile (`layout = auto(tree)`, `layout = auto(star)`), with
warnings for `dense_graph`, `components_packed`, `layout_tree_approx`, `layout_chain_approx` and
`edges_reversed`.
The fan only applies to a hub that is the **source** of its spokes (or to undirected spokes); when
the arrows all point *into* one node the ports would have to pile up on one side, so that case stays
on the normal tree.

"Pack disconnected components" is the last step of `auto`, after the chosen concrete layout has run
on the whole document:

- Two boxes are in the same component when an edge connects them, ignoring the edge direction;
  an isolated box is a component of its own. The count is `components` in the report.
- Each component is treated as a rigid block: its current bounding box is measured, blocks are
  sorted by area (largest first), then laid out shelf-style left to right, starting a new row when
  the shelf width is exceeded, with a 2-cell gutter between blocks.
- Packing only translates boxes; relative positions inside a component, and therefore its local
  layout, are unchanged. It does not re-run routing or the layout per component.
- Only `auto` does this. `geo`, `relax`, `force`, `layered`, `tree` and `chain` leave disconnected
  components where their own layout put them.
- The report contains `components_packed` when there was more than one component.

Orientation (`view.orientation`): with `auto`, `force` / `layered` rotate the graph 90° when that
needs less zoom (no rotation if the difference is under 5%, so it does not flicker at the
threshold); `geo` and `relax` keep the input orientation because the coordinates carry meaning.
Opening a `.gtui` does not recompute coordinates, it only remembers this value. An explicit
`:rotate` changes coordinates (requires 1:1) and does not swap box width and height.

All layouts produce **integer cell** coordinates and write them back to `box.at`, moving the
attached text by the same amount; any two boxes are at least one cell apart. They are
**deterministic** (the same document twice gives identical points) and **changing the layout
never moves the cursor**; search and focus follow boxes.

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
:text hello world         # replace the text (the whole line is taken literally; **bold** *italic* work; the box resizes)
Ctrl-e                    # select an edge; then :dir both / :corner sharp / :head triangles / :style dashed
:w name.gtui              # save; without a path nothing is written (:w only echoes a hint)
:w ~/figs/name.gtui       # leading ~ expands to the home directory (command-line args too)
:q                        # quit (only :q quits); unsaved work is gone, no temp files left behind
:wq                       # save as .gtui and quit; on failure it stays open
```

`del` deletes the cursor box (with its text and edges); if an edge is selected it deletes that
edge. The last box cannot be deleted. New nodes default to `none` (one line of text, width follows
the text); `:box` adds a frame. A new box copies the style, colours and alignment of the box you
last had selected — or of one you just restyled with the `s` menu / `:box` / a colour verb, which
also counts as selecting it, so the template survives moving the cursor away. A block selection
does not count.

Changing a box's style keeps the **text** where it is and lets the frame follow the style: the four
framed styles all have the same size, so switching between them changes nothing, and switching to
`none` shrinks the box down to the text itself — the result is byte-for-byte the same picture as if
the node had always been `none` (edges re-route to the text, as they should). `point` puts the node
glyph on the first character's cell and lets the name be placed beside it. Going the other way, a
frame grows around the text without moving it. Typing instead of restyling still grows a `none` node
to the right, like an editor.

`e` is the two-phase link: pick the source (selection or cursor box), then the target, `Enter`
creates the edge and `Esc` cancels.
1->1 means "press `e` on A, move to B, press `Enter`"; when source and target are the same box you
get a self-loop (out one side, back in on an adjacent side). 1->N means "press `e`, block-select
multiple targets with `v`, `Enter`"; N->M means block-select the sources first.

## Editing

**Regions** (`:region`) group boxes under a titled frame; `:region collapse` hides the members
behind a title bar. Structure and `.dot` import/export are in [format.md](format.md#regions-v7).

Editing commands and typing require `1:1`, then snap back anchored on the cursor; `:layout` / `r`
and mouse dragging work at any zoom. See [keybindings.md](keybindings.md#view) and
[limits.md](limits.md).

Edge endpoints can be pinned with `:fromside` / `:toside` / `:port`; the rules are in
[format.md](format.md#edge-ports-v8).

Linking also works by mouse: drag a box border or a selected edge endpoint. See
[keybindings.md](keybindings.md#mouse).

`Ctrl+G` hands the prompt or the cursor box to `$VISUAL` / `$EDITOR`; see
[keybindings.md](keybindings.md).

On a selected edge, `i` creates or edits its edge-label box; `:edge-labels` toggles them and
`:relabel [all]` re-anchors them. See [format.md](format.md) and [keybindings.md](keybindings.md).

The style overview is `examples/styles-all.gtui` (six box styles, bold/italic, text colour, fills,
three alignments, three line styles, four directions, two corner styles on one page); import
examples are in `examples/styles.dot`, edge labels in `examples/edge-notes.gtui`, and multi-line
text in `examples/multiline.gtui`. Every verb is in [keybindings.md](keybindings.md).

Undo history defaults to 200 steps (`--history N`); see
[keybindings.md](keybindings.md#undo-and-redo).

Layers stack boxes and edges; `L` cycles the filter and items outside the current layer are dimmed.
Moving a box changes `box.at` and its `text.at`; see [format.md](format.md).

## CLI reference

This section *is* `--help`: the program embeds the text between the markers below and
prints it rendered for the terminal (Markdown tables become aligned columns, heading
markers and inline code ticks disappear), so the same text reads well here and in the
shell. Keys are not repeated: `--help` appends the table the `?` overlay uses, and
[keybindings.md](keybindings.md) is the long version.

<!-- cli-help:begin -->

### graph-tui

```text
usage: graph-tui [options] [file.gtui | file.dot | file.canvas]
```

With no file: if stdin is a pipe, the document is read from it; on a terminal you get an
empty scratch document (one box, never written to disk -- save it yourself with
`:w <path.gtui>`). `.gtui` is the native, editable document; `.dot` and `.canvas` are
imports (the first save writes a `.gtui` next to them).

#### Options

| Flag | What it does |
| --- | --- |
| `--file <path>` | read a graph file (same as giving it as an argument; `-` = stdin) |
| `--layout <name>` | `geo`, `relax`, `force` or `layered` (see below) |
| `--export <path>` | write out and exit: `-` for the picture on stdout (1:1, centred, padded, no status line and no cursor; size fits the content unless `--area` is given); otherwise a file: `.gtui` (native save), `.svg` (self-contained picture), `.png` (rasterised by `rsvg-convert`), `.canvas` (JSON Canvas) or `.dot` (best-effort view, lossy). A picture still comes out with no flags at all when stdout is not a terminal, so `graph-tui a.gtui > out` and `graph-tui a.gtui \| nvim -` just work |
| `--area <x,y,w,h>` | window to draw: `x,y` is the canvas cell under the picture's top-left (`auto` = centre the content), `w,h` the output size (`auto` = content + padding). Each field may be `auto`; `--area auto` = the whole content. Crops, never scales |
| `--node-labels` | start with every point-node name drawn (same as pressing `B`) |
| `--no-node-labels` | do not draw point-node names in the picture / `--export` |
| `--no-edge-labels` | do not draw edge labels |
| `--format <ansi\|text>` | output for the picture: `ansi` (true colour) or `text` (no escapes). Unset picks per destination: plain text when stdout is not a terminal, or with `NO_COLOR` set, or with `TERM=dumb`; ANSI otherwise |
| `--history <N>` | undo steps to keep (default 200) |
| `--ascii` | pure ASCII glyphs (frames, junctions, points, arrowheads) for terminals without box-drawing characters; Unicode is the default. It also forces `^ v < >` arrowheads, even on edges that store `head` |
| `--lane-gap <N>` | layered layout: minimum columns between boxes in one layer (default 3) |
| `--layer-gap <N>` | layered layout: minimum rows between layers (default 3) |
| `--layout-margin <N>` | leave N cells of margin around the laid-out picture (default 0) |
| `--drawn-cursor` | mark the cursor inside the picture instead of using the terminal cursor (TUI only; the picture and `--export` never draw a cursor) |
| `--at <name>` | start with the cursor on that node |
| `--find <text>` | run a search at startup (name / alias / group number) |
| `--lang <zh\|en>` | language of the app's own text (default: the config's `[prefs] lang`, else a document's `view.lang`, else the locale from `LC_ALL` / `LC_MESSAGES` / `LANGUAGE` / `LANG`, else `en`). Overrides the document; this help and the errors stay English |
| `--config <path>` | read this config file (default: `$GRAPH_TUI_CONFIG` or `$XDG_CONFIG_HOME/graph-tui/config.ini`). A missing explicit file is an error |
| `--no-config` | ignore every config file (built-in defaults) |
| `--print-config` | print the full default config (prefs + keys) and exit |
| `--init-config [p]` | write the default config (to `[p]` or the default path); needs `--force` to overwrite |
| `--force` | allow `--init-config` to overwrite an existing file |
| `-v`, `--version` | print the version and exit |
| `-h`, `--help` | show this help and exit |

#### Layouts

| Name | What it means |
| --- | --- |
| `geo` | use the positions from the input as they are |
| `relax` | spring-relax those positions so spacing evens out and runs snap to `-`, `\|`, `/` and `\\` |
| `force` | force-directed, for graphs with no positions at all |
| `layered` | layered (Sugiyama), for directed dependency graphs and flowcharts |

The default is `geo` when every node has a position, otherwise `layered` for acyclic graphs
and `force` otherwise; `layout` only affects the picture, not the stored positions.

### tutor

`graph-tui tutor` opens the built-in walkthrough. It is a scratch copy: edit it freely,
`:q` leaves without writing anything, and running the command again resets it.

### input (`.dot` subset)

```text
digraph deps {
  node [box=single];                    default attributes for nodes / edges
  "a" [pos="0,0", terms="alpha|ay", box="rounded", color="#4c8ef8"];
  "a" -> "b" [group="deps", color="#4c8ef8", style="dashed", via="10,4 20,4"];
}
```

A `.canvas` file (JSON Canvas / Obsidian) is read by extension instead of `.dot`. The
attribute lists, `pos_unit` and the `.canvas` mapping are in `docs/format.md`.
<!-- cli-help:end -->
