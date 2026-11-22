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

**Tutorial**:
`graph-tui tutor` is a tutorial.

**Others**:
`[prefs] render = ascii` makes it use only ascii to draw. 

**Tabs**:
Several files open as tabs (`graph-tui a.gtui b.dot`); each tab keeps its own document, cursor,
zoom, undo history and file stamp, while the clipboard is shared. `Ctrl-t` / `Ctrl-n` / `Ctrl-p` /
`Ctrl-w` (or `:tabnew` / `:tabnext` / `:tabprev` / `:tabclose`) manage them, and the status line
shows `[2/4] name *`. `:tabclose` refuses a tab with unsaved changes unless you add `!`; `:e` also
refuses to replace a dirty document.

**Hot-reload**:
The file on disk is watched while it is open: when another program rewrites it, an unmodified
document is reloaded automatically, and a modified one gets a persistent warning with two ways
out (`R` reloads and drops the edits, `:w` overwrites the file). 

Picture only, no UI: no status line, no cursor, no selection.

**Export**: 
Export is 1:1.
- `--export -` to force to export to stdout rather than enter canvas. 
Default export when stdout is not a terminal, like `graph-tui a.gtui > out` and `graph-tui a.gtui | nvim -`. 
The format follows the destination: plain text for a pipe or a file (and on a terminal
that sets `NO_COLOR` or `TERM=dumb`), true-colour ANSI otherwise; `--format ansi` / `text` override that either way. 
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
- The headless `render` op behaves the same: `size` may be omitted or `"auto"`, and the response
  carries `size`, `lines`, `labels_skipped` and `warnings`.

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
The status line reports the resolved profile (`layout = auto(tree)`, `layout = auto(star)`), and the
headless `layout` op returns `profile`, `components`, `crossings`, `reversed` and `warnings`
(`dense_graph`, `components_packed`, `layout_tree_approx`, `layout_chain_approx`, `edges_reversed`).
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

All layouts produce **integer cell** coordinates, write them back to `box.at`, and translate
the attached text by the same amount; any two boxes are at least one cell apart. `r` cycles the
six concrete ones in place. **Changing the layout never moves the cursor**; search and focus follow
boxes.

All layouts are **deterministic**: the same document run twice gives identical points
(no randomness).

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

`Ctrl-e` selects an edge; `:` verbs change attributes; `:w` saves. `e` is the two-phase link: pick
the source (selection or cursor box), then the target, `Enter` creates the edge and `Esc` cancels.
1->1 means "press `e` on A, move to B, press `Enter`"; when source and target are the same box you
get a self-loop (out one side, back in on an adjacent side). 1->N means "press `e`, block-select
multiple targets with `v`, `Enter`"; N->M means block-select the sources first.

## Editing

**Regions** (`:region`) group boxes under a titled frame. The frame is the bounding box of its
members plus padding, so it follows them; `:region collapse` replaces the frame with a title bar and
hides the members. `.dot` `subgraph cluster_*` becomes a region on import and is written back on
export (colour, style and collapse are lossy). Regions are drawn beneath edges and boxes, so they
never cover text.

**Commands and typing require `1:1`**: while zoomed you can only look (`VIEW 150%`, plus `LOD`
when detail is reduced). Typing an edit command first snaps the view back to `1:1` anchored on the
cursor, then runs the command; the status line shows `VIEW` (or `VIEW 150%` when zoomed). Mouse
dragging of boxes and `:layout` / `r` work at any zoom, and the box you were looking at stays where
it was.

Edge endpoints can be pinned: select an edge and use `:fromside left` / `:toside up` (or
`:port from=left to=up`) to make it leave or arrive on a specific side. `auto` is the default and
picks the side from the two centres, so nothing changes unless you ask. Point nodes have no border,
so a pinned side is ignored there.

Linking is also a mouse gesture: drag from a boxed node's **border** onto another box to create
the edge, and drag a selected edge's **endpoint** onto another box to reconnect it (colour, group,
style and note are kept). Dragging inside a box still moves it; near the viewport edge the drag
pans the view. Keys `e` / `:link` remain the equivalent path.

Zoom is anchored: the wheel keeps the cell under the pointer fixed and `+` / `-` keep the cursor
cell fixed (1.25 per press, 1.1 per wheel notch, and anything within 6% of `100%` snaps to it).
Below `60%` text is not drawn at all and the status line says `LOD`; below `30%` boxes become
solid blocks. The picture and image export always draw the full detail.

In the bottom-line prompt (new node / edit text) press **`Ctrl+G`** to hand the current content to
`$VISUAL` / `$EDITOR` (falling back to `vi`). When the editor exits the file is read back and
replaces the prompt: unchanged content (`:q!`, an immediate exit, a GUI editor that does not block)
counts as cancel; a non-zero exit keeps the original and reports the code; invalid UTF-8, CR or NUL
are rejected. You still press `Enter` to commit, `Esc` to discard. The editor must **block**
(`code -w` / `vim`); the command line and search do not accept this key (pressing it only shows a
hint). In normal mode `Ctrl+G` skips the prompt: it sends the cursor box (or the whole selection)
to the editor and writes the result straight back on exit, as one undo step.

On a selected edge, `i` creates or edits its edge-label box (an empty box is removed by `Esc` or an empty
Enter); `:edge-labels` shows or hides them all; `:relabel [all]` re-anchors edge-label boxes to their
  edges. A hidden edge label is not drawn, is not an obstacle, and takes no space.

With a selection, `box` / `nodecolor` / `fill` / `textcolor` / `textalign` / `text` apply to the
**whole selection**; when `L` shows a single layer, verbs only touch that layer. Every verb is in
[docs/keybindings.md](keybindings.md); the style overview is
`examples/styles-all.gtui` (six box styles, bold/italic, text colour, fills, three alignments,
three line styles, four directions, two corner styles on one page), import examples are in
`examples/styles.dot`; edge labels are shown in `examples/edge-notes.gtui` (five edges, each with a
edge-label box); multi-line text is in `examples/multiline.gtui` (a three-line framed box, a two-line
text-only node, and a two-line edge label).

Undo/redo is snapshot-based, 200 steps by default (`--history N`); setting the same value does not
consume a step, a whole drag is one step, and view-only actions never enter the stack. Details in
[docs/keybindings.md](keybindings.md).

Layers stack overlapping items: boxes and edges both carry a `layer`, drawing follows layer order
with the current layer on top, and items outside the current layer are dimmed. Moving a box changes
`box.at` and its `text.at` (integers); positions are fixed once saved, and only `r` re-lays them out.

## CLI reference

This is what `graph-tui --help` prints; the text lives here so the two cannot drift.

<!-- cli-help:begin -->
```text
graph-tui - a universal graph renderer for the terminal
(box-drawing edges, one true colour per group, orthogonal routing, interactive pan/zoom)

usage: graph-tui [options] [file.gtui | file.dot | file.canvas]

With no file: if stdin is a pipe, the document is read from it; on a terminal you
get an empty scratch document (one box, never written to disk -- save it yourself
with `:w <path.gtui>`). `.gtui` is the native, editable document; `.dot` and
`.canvas` are imports (the first save writes a `.gtui` next to them).

options:
  --file <path>       read a graph .dot file (same as giving it as an argument; '-' = stdin)
  --layout <name>     geo | relax | force | layered. Default: geo when every node has a
                      position, otherwise layered for acyclic graphs and force otherwise
  --export <path>     write out and exit: `-` for the picture on stdout (1:1, centred,
                      padded, no status line and no cursor; size fits the content
                      unless --area is given); otherwise a file: .gtui (native save),
                      .svg (self-contained picture), .png (rasterised by
                      rsvg-convert), .canvas (JSON Canvas) or .dot (best-effort view,
                      lossy). A picture still comes out with no flags at all when
                      stdout is not a terminal, so `graph-tui a.gtui > out` and
                      `graph-tui a.gtui | nvim -` just work
  --area <x,y,w,h>    window to draw: `x,y` is the canvas cell under the picture's
                      top-left (`auto` = centre the content), `w,h` the output size
                      (`auto` = content + padding). Each field may be `auto`;
                      `--area auto` = the whole content. Crops, never scales;
  --node-labels       start with every point-node name drawn (same as pressing B)
  --format <ansi|text>  output for the picture: `ansi` (true colour) or `text` (no escapes).
                      Unset picks per destination: plain text when stdout is not a
                      terminal, or with NO_COLOR set, or with TERM=dumb; ANSI otherwise
  --no-node-labels    do not draw point-node names in the picture / --export
  --no-edge-labels    do not draw edge labels
  --history <N>       undo steps to keep (default 200)
  --ascii             pure ASCII glyphs (frames, junctions, points) for terminals without
                      box-drawing characters; --unicode is the default
  --lane-gap <N>      layered layout: minimum columns between boxes in one layer (default 3)
  --layer-gap <N>     layered layout: minimum rows between layers (default 3)
  --layout-margin <N> leave N cells of margin around the laid-out picture (default 0)
  --arrow <style>     arrow glyphs: ascii (^ v < >) | arrows (up/down/left/right) |
                      triangles (solid triangles). Default ascii
  --drawn-cursor      mark the cursor inside the picture instead of using the terminal cursor
                      (TUI only; the picture and --export never draw a cursor)
  --at <name>         start with the cursor on that node
  --find <text>       run a search at startup (name / alias / group number)
  --lang <zh|en>      language of the app's own text (default: the config's
                      `[prefs] lang`, else a document's `view.lang`, else the locale
                      from LC_ALL / LC_MESSAGES / LANGUAGE / LANG, else en). Overrides
                      the document; this help and the errors stay English
  --config <path>     read this config file (default: $GRAPH_TUI_CONFIG or
                      $XDG_CONFIG_HOME/graph-tui/config.ini). A missing explicit file is an error
  --no-config         ignore every config file (built-in defaults)
  --print-config      print the full default config (prefs + keys) and exit
  --init-config [p]   write the default config (to [p] or the default path); needs --force to
                      overwrite
  --force             allow --init-config to overwrite an existing file
  -v, --version       print the version and exit
  -h, --help          show this help and exit

tutor (built-in, editable walkthrough):
  graph-tui tutor                 open the tutorial
      It is a scratch copy: edit it freely, `:q` leaves without writing anything,
      and running the command again resets it.

headless (no TTY, JSON in / JSON out):
  graph-tui headless --file a.gtui --op '{"op":"info"}'
  graph-tui headless --file a.gtui < requests.ndjson > responses.ndjson
  graph-tui headless --file a.gtui --batch ops.ndjson --save-on-exit [--partial]
      --file <path>      open .gtui / .dot / .canvas first
      --op <json>        run one request and exit
      --batch <path>     run one request per line (atomic unless --partial)
      --save-on-exit     write the document back to --file at the end
      --limit <n>        max requests per session (default 100000)

input (.dot subset):
  digraph deps {                          graph or digraph (digraph: edges get an arrowhead)
    node [box=single];                    default attributes for nodes / edges
    "a" [pos="0,0", terms="alpha|ay", box="rounded", color="#4c8ef8"];
    "a" -> "b" [group="deps", color="#4c8ef8", style="dashed", via="10,4 20,4"];
  }
  a .canvas file (JSON Canvas / Obsidian) is read by extension instead of .dot
  node: pos (x right / y up; ours is written in cells with `pos_unit="cell"` and uses the
        box id as the node name, `node_names="id"`) | label (the text) | terms (search aliases)
        | box=point|none|single|rounded|double|heavy | color (border and text)
        | size=WxH | textcolor | textbg | edge_label=<edge number> | align=left|center|right
        edge: group | color | style=solid|dashed|dotted | via (points the line is forced
        through) | corner=rounded|sharp | head=ascii|arrows|triangles | bg
        | from_side / to_side = left|right|up|down
  cluster: label | color | box | padding | collapsed | layer (regions)
  labels take **bold**, *italic*, ~~strike~~, <u>, <dim>, <fg=#rrggbb>, <bg=#rrggbb>

layouts:
  geo      use the positions from the input as they are
  relax    spring-relax those positions so spacing evens out and runs snap to - | / \\
  force    force-directed, for graphs with no positions at all
  layered  layered (Sugiyama), for directed dependency graphs and flowcharts

keys:
  h j k l / arrows   move to the nearest node on screen in that direction
  H K / J L          step one node along the group (its own order, not geography)
  + - / 0            zoom in / out / back to 1:1 (the canvas is infinite: pan has no bounds)
  z                  centre the view on the cursor node
  f / F              fit the focused group / fit the whole graph
  b                  toggle node labels
  /                  search names or aliases; a bare number focuses that group
  n / N              next / previous search hit
  V                  rectangle select (same as v / Ctrl-q)
  r                  cycle the layout
  Ctrl-d / Ctrl-u    jump half a screen down / up
  Ctrl-o / Ctrl-i    back / forward through the nodes you jumped to
  ?                  key reference (any key closes it)
  v                  rectangle select from the cursor node (move the cursor to grow it, esc to drop)
  Ctrl-h j k l       nudge the selection (or the cursor node) by one screen cell
  L                  cycle the layer under the cursor (other layers are dimmed)
  e                  two-phase link: sources, then move to a target and press Enter
  E                  cycle the edge selection through the edges at the cursor node
  s / c              style menu / colour menu (Enter applies, esc cancels)
  :                  command line: box <style> | nodecolor <#rrggbb|auto> | style <solid|dashed
                     |dotted> | edgecolor <#rrggbb|auto> | via <x,y x,y|off> | text <rest of line>
                     | layer <name|all> | tolayer <name> | export <path.svg|png|canvas>
                     | export-canvas <path> | save [path] | layout <name> | side <x>
                     | cycle <x> | lang <zh|en>
  mouse              click = jump; drag on a node = move it, drag elsewhere = pan;
                     wheel = zoom, Ctrl+wheel = pan
  :q / :wq           quit / save and quit (q and esc do not quit)
```
<!-- cli-help:end -->
