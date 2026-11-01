English | [中文](usage.zh.md)

# Usage: CLI, rendering, layouts

## Opening documents

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
Ctrl-e                    # select an edge; then :dir both / :corner sharp / :style dashed
:w name.gtui              # save; without a path nothing is written (:w only echoes a hint)
:w ~/figs/name.gtui       # leading ~ expands to the home directory (command-line args too)
:q                        # quit (only :q quits); unsaved work is gone, no temp files left behind
:wq                       # save as .gtui and quit; on failure it stays open
```

`del` deletes the cursor box (with its text and edges); if an edge is selected it deletes that
edge. The last box cannot be deleted. New nodes default to `none` (one line of text, width follows
the text); `:box` adds a frame.

`Ctrl-e` selects an edge; `:` verbs change attributes; `:w` saves. `e` is the two-phase link: pick
the source (selection or cursor box), then the target, `Enter` creates the edge and `Esc` cancels.
1->1 means "press `e` on A, move to B, press `Enter`"; when source and target are the same box you
get a self-loop (out one side, back in on an adjacent side). 1->N means "press `e`, block-select
multiple targets with `v`, `Enter`"; N->M means block-select the sources first.

## Editing

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
