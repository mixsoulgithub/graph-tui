English | [中文](usage.zh.md)

# Usage: CLI, rendering, layouts

## Opening documents

```bash
graph-tui metro.gtui                     # open a native document (draws its stored coordinates)
graph-tui new.gtui                       # does not exist yet: new file, `:w` writes it (like nvim)
graph-tui examples/pipeline.dot          # import .dot; the first `w` writes pipeline.gtui next to it
graph-tui                                # no file: scratch buffer (memory only, gone on exit)
graph-tui --dump --size 120x36 x.gtui    # one frame and exit, for screenshots and docs
```

If the terminal lacks box-drawing glyphs (or draws them two cells wide), `--ascii` renders
everything with `- | + o @`. Corners, tees and crossings all become `+`, and double/heavy frames
collapse to single lines; colours and geometry are unchanged. `[prefs] render = ascii` makes it
the default.

Several files open as tabs (`graph-tui a.gtui b.dot`); each tab keeps its own document, cursor,
zoom, undo history and file stamp, while the clipboard is shared. `Ctrl-t` / `Ctrl-n` / `Ctrl-p` /
`Ctrl-w` (or `:tabnew` / `:tabnext` / `:tabprev` / `:tabclose`) manage them, and the status line
shows `[2/4] name *`. `:tabclose` refuses a tab with unsaved changes unless you add `!`; `:e` also
refuses to replace a dirty document.

Opening a file always starts at `1:1`, centred on the picture (the same as pressing `0`); a
stored `zoom` / `center` is used by `--dump` and export but not restored interactively. Press `F`
to fit the whole document.

With no file and a terminal on stdin you get a **scratch document**: one page in memory (one
group, one layer, one box `a`). No temporary file is written; it disappears on exit. Saving needs
an explicit path (`:w name.gtui`); without one you only get "no filename yet". If stdin is a pipe,
it is still read from the pipe.

A `.gtui` path that does **not** exist yet opens the same way `nvim newfile` does: an empty
document already bound to that name (the status line says `new file … (:w to write it)`), and `:w`
creates the file. `.dot` / `.canvas` are import-only, so a missing one is still an error, and
`--dump` / `--export` need real content. The same rule applies to `:e new.gtui` and
`:tabnew new.gtui`; on such a buffer `R` reports that the file does not exist yet instead of
reloading an empty document.

The file on disk is watched while it is open: when another program rewrites it, an unmodified
document is reloaded automatically, and a modified one gets a persistent warning with two ways
out (`R` reloads and drops the edits, `:w` overwrites the file). See
[keybindings](keybindings.md#external-changes).

Where a `:` verb will read or write is visible **before** you press Enter: the right side of the
status line resolves the path. `:w` on a `.gtui` shows the file plus `(overwrite)`; on an
imported `.dot` it shows the `<stem>.gtui` that will be created, plus
`(source .dot stays untouched)`. `Tab` after `:w` / `:wq` fills that default path into the line
so it can still be edited; `Tab` after `:e` / `:w` / `:export` completes real paths (directories
first, `~/` expanded, names with spaces kept).

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
--no-notes         do not draw edge notes (default follows the document's view.notes)
--drawn-cursor     draw the cursor inside the picture (default is the real terminal cursor;
                   TUI only; --dump/--export never draw a cursor)
--history <N>      undo depth (default 200)
--ascii            pure ASCII glyphs (frames, junctions, points) for terminals without
                   box-drawing characters; --unicode (the default) restores them
--lane-gap <N>     layered: minimum columns between boxes in one layer (default 3)
--layer-gap <N>    layered: minimum rows between layers (default 3)
--layout-margin <N>  leave N blank cells around the laid-out picture (default 0)
--arrow <style>    arrow glyphs: ascii|arrows|triangles (default ascii)
--at <name>        put the cursor on that node at startup
--find <text>      run a search at startup (name / alias / group number)
--lang <en|zh>     UI language (default en; command-line help and errors are always English)
--export <path>    write and exit: .gtui (native save) / .svg / .png (rasterised by
                   rsvg-convert) / .canvas (JSON Canvas) / .dot (best-effort lossy view,
                   warnings on stderr)
-v, --version      version
-h, --help         help
```

### Rendering: `--dump` / `--export`

Picture only, no UI: no status line, no cursor, no selection. The map fills the whole width.

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
- Edge notes are content too: they follow `view.notes`, and `--no-notes` removes them (hidden notes
  are not obstacles and take no space).
- There is no legend panel any more. When a picture needs a key, draw it as content: a `none` or
  framed box with the text, a short edge with the line style you want to explain, and the group
  colours you already have. Keep it in place with `geo` layout (or place it after the layout runs),
  because `auto` / `layered` move every box they see.
- The headless `render` op behaves the same: `size` may be omitted or `"auto"`, and the response
  carries `size`, `lines`, `labels_skipped` and `warnings`.

```bash
graph-tui --dump graph.gtui > graph.ansi
graph-tui --dump --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## Layouts

| name | what it does | requirements |
| --- | --- | --- |
| `auto` | **default**: read the structure and pick one of the layouts below (chain / tree / layered / relax), pack disconnected components and fall back to `relax` with a `dense_graph` warning on clique-like graphs | any document |
| `tree` | tidy tree: parents above children and centred over their subtree, siblings in their original order; extra edges are routed, polytrees use the first parent. A pure star (one hub, every other node degree 1) is drawn as a **fan** instead: leaves in up to four vertical columns of ten, two per side, hub on the vertical middle line — 40 leaves become a `61x19` block rather than a `527`-cell-wide row (resolved profile `star`) | any document (approximate for graphs that are not trees) |
| `chain` | longest path as a straight line, branches as spurs below it; a pure cycle breaks one deterministic edge | any document (approximate otherwise) |
| `geo` | keep current positions (only separate overlapping boxes) | any document |
| `relax` | spring relaxation from current positions; evens gaps and snaps directions to `- \| / \`. Gaps are measured per axis (a box is one row tall, so stacked boxes sit rows apart, not box-widths apart), and an anchor keeps the result near a `sqrt(n)`-cells square instead of letting it creep outward | any document |
| `force` | Fruchterman-Reingold; snaps to horizontal/vertical/45° at the end | any document |
| `layered` | Sugiyama-style layering: back edges are cut, long edges get dummy nodes so the middle layers can order around them, then alternating barycentre sweeps plus a transpose pass pick the order with the fewest crossings; best for directed dependency/flow graphs | any document (cycles included) |

`auto` is what new documents and `.dot` / `.canvas` imports use (`view.layout` stores the name, the
resolved profile is not written back). `r` cycles the six concrete layouts `geo -> relax -> force ->
layered -> tree -> chain -> geo`; `auto` is only reachable with `:layout auto` or `--layout auto`.
The status line reports the resolved profile (`layout = auto(tree)`, `layout = auto(star)`), and the
headless `layout` op returns `profile`, `components`, `crossings`, `reversed` and `warnings`
(`dense_graph`, `components_packed`, `layout_tree_approx`, `layout_chain_approx`, `edges_reversed`).
The fan only applies to a hub that is the **source** of its spokes (or to undirected spokes); when
the arrows all point *into* one node the ports would have to pile up on one side, so that case stays
on the normal tree.

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
Ctrl-e                    # select an edge; then :dir both / :corner sharp / :style dashed
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

**Editing only works at `1:1`**: while zoomed you can only look (`VIEW 150%`). Typing an edit
command first snaps the view back to `1:1` and centres it, then runs the command; the bottom line
shows `VIEW` (or `VIEW 150%` when zoomed). Switching back for an edit anchors on the cursor
instead of re-centring, so the box you were looking at stays where it was.

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
solid blocks. `--dump` and image export always draw the full detail.

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
[docs/keybindings.md](keybindings.md); the style overview is
`examples/styles-all.gtui` (six box styles, bold/italic, text colour, fills, three alignments,
three line styles, four directions, two corner styles on one page), import examples are in
`examples/styles.dot`; edge notes are shown in `examples/edge-notes.gtui` (five edges, each with a
note box); multi-line text is in `examples/multiline.gtui` (a three-line framed box, a two-line
text-only node, and a two-line edge note).

Undo/redo is snapshot-based, 200 steps by default (`--history N`); setting the same value does not
consume a step, a whole drag is one step, and view-only actions never enter the stack. Details in
[docs/keybindings.md](keybindings.md).

Layers stack overlapping items: boxes and edges both carry a `layer`, drawing follows layer order
with the current layer on top, and items outside the current layer are dimmed. Moving a box changes
`box.at` and its `text.at` (integers); positions are fixed once saved, and only `r` re-lays them out.
