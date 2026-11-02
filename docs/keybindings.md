English | [中文](keybindings.zh.md)

# Keys and interaction

Every key, mouse action and `:` verb. `?` shows the short list; this is the full one.
Keys can be rebound — see [config.md](config.md) for the file format and the action names.

- All coordinates are terminal cells; the cursor is a **cell**, not "the nth box".
- What sits under the cursor cell is what is selected: a box, an edge, or nothing.
- Verbs that change geometry or attributes need `1:1`: when zoomed in/out they first switch
  back to `1:1` anchored on the cursor, then run; the status line shows `VIEW` (`VIEW 150%` when
  zoomed, plus `LOD` when detail is reduced).

## Who eats a key first

Top to bottom; while a higher layer is open the lower ones get nothing:

1. Overlays: `?` help, the `:e` file picker, the right-click menu.
2. Bottom-line input: the `i` / `t` prompt (`Ctrl+G` opens an external editor), the `:`
   command line, `/` search.
3. Normal keys.

`Ctrl` / `Alt` combinations only exist for the ones listed below; anything else is swallowed
(it never degrades into the bare key).

## Moving and selecting

| Key | What it does |
| --- | --- |
| `h j k l` / arrows | move the cursor **one cell**; a count prefix is the number of cells, `5l` = five to the right |
| `Ctrl-d` / `Ctrl-u` | jump half a screen down / up to the nearest node |
| `Ctrl-o` / `Tab` (= `Ctrl-i`) | back / forward in the jump history (cells, not boxes) |
| `v` / `Ctrl-q` | start a block selection: the anchor is the cursor cell (empty space works too), moving the cursor grows it; boxes are judged by rectangle intersection, edges by their visible cells |
| `Enter` | **confirm** the selection: the far end is pinned, later cursor moves no longer change it; with an empty selection it says "empty selection" |
| `Esc` | clear the selection if there is one, otherwise nothing (quitting is `:q` / `:wq` only) |
| `E` | neighbour view: light up the **cursor node**, its neighbours and the edges between them, dim everything else; move to another node and it follows; `E` or `Esc` exits (with no node under the cursor it just says so) |
| `Ctrl-e` | pick an edge: on a node it cycles the edges incident to that node; with stacked boxes it cycles the boxes first; off a node it cycles the edge candidates on that cell |
| `V` | block selection (same as `v` / `Ctrl-q`) |
| `L` | cycle the layer filter (all → each layer → all); other layers are dimmed |
| `Ctrl-h j k l` | move the selected boxes (or the cursor box) by one cell; a count works (`10` + `Ctrl-l` = ten cells, one undo step). Cursor and selection follow; the view follows when the cursor leaves the margin |

## View

| Key | What it does |
| --- | --- |
| `+` / `-` / `0` | zoom in / out (anchored on the cursor, 1.25 per press, snapping to `100%` within 6%) / back to `1:1` centred (the canvas is infinite, panning has no bounds) |
| `f` | fit the focused group (or the cursor node's group) to the screen |
| `F` | fit the whole document (zoom caps at `1:1`, it never magnifies) |
| `z` / `space` | centre the cursor cell |
| `b` | point-node name labels on/off |
| `/` | search names, aliases, initials; a bare number focuses that group |
| `n` / `N` | next / previous search hit |
| `r` | cycle the four layouts (this is one undo step) |
| `?` | key reference (any key closes it) |
| `:q` / `:wq` | quit / save and quit (`q` / `Esc` / `Ctrl-c` do not quit; a failing `:wq` keeps you in) |

## Building and editing

| Key | What it does |
| --- | --- |
| `i` | with boxes under the cursor or in the selection: **edit their text** (one undo step); with edges selected: create an **edge note** and open the prompt (an empty box is dropped on `Esc` or empty `Enter`); with nothing there: create a box on the cursor cell (shifted right if it would collide). Bottom-line prompt, `Enter` applies, `Esc` cancels |
| `a` | edge notes on/off (same as `:notes`). Hidden notes are not drawn, not obstacles, take no space and cannot be clicked; edges re-route |
| `s` | style menu: boxes get box style / border colour / fill / text colour / alignment / layer / text; edges get line style / direction / corner / colour / group / waypoints / layer |
| `c` | colour menu: the box's border colour or the edge colour (`auto` + 12 colours, the current one is marked `*`) |
| `t` | edit the selected box text (same path as `i`, every box in the selection); with edges selected it points at `i` |
| `d` | cut with edges: the box, its text and its edges are removed, and the clipboard keeps the edges. Refused if the whole document would disappear. The cursor stays put |
| `x` | cut box and text only: incident edges are still removed from the document but do not go into the clipboard (pasting has no edges). The cursor stays put |
| `p` | paste at the cursor (new ids, groups and layers reused); collisions move right / down / down-right, up to 32 tries before stacking |
| `e` | two-phase link: source (selection or cursor box) → pick a target → `Enter` creates, `Esc` cancels; source and target may be the same box (self-loop) |
| `u` / `Ctrl-r` | undo / redo |
| `R` | reload the source file from disk (see *External changes*); with unsaved edits the first press only warns, the second drops them |
| `:` | command line (verbs below) |

The prompt line (`box: ` / `text: ` / `link: `) takes the whole line like `:`, starts at the
far left and covers the mode marker and node name.

`Ctrl+G` in the prompt writes the current content to a temp file and hands it to `$VISUAL` /
`$EDITOR` (falling back to `vi`). When the editor exits the file is read back into the line:
if it changed, the whole string is replaced (multi-line content shows as `first line [+N lines]`);
if not (`:q!`, plain exit) it counts as cancelled; a non-zero exit code or non-UTF-8 / CR / NUL
keeps the line as it was and says why. `Enter` still commits. The `:` line and the `/` query are
single-line: `Ctrl+G` only prints a hint there. The editor must **block** — GUI editors need
`--wait` (`code -w`), otherwise identical content is treated as a cancel.

Text may be multi-line (`\n`): a framed box grows by the line count and widens to the longest
line, markdown's `**bold**` / `*italic*` and alignment apply per line; a point node holds one name
only. Markup is only interpreted in **node text**: the `:` line, `/` search and the prompt line are
raw text, so `/examples/` or `*a*b*` there never lose characters (the cursor counts the raw width).

## Command-line completion

While typing a `:`, `Tab` completes and `Shift-Tab` goes back; a candidate popup appears above
the status line with the current one highlighted.

- Before the first space: complete the **verb** (`fi` + Tab → `fill`).
- After a verb: complete its **value** — `box` (5 styles), `style` (3), `dir` (4), `corner`,
  `textalign`, `layout`, `lang`, `legend`, `rotate`, `cycle`.
- `layer` / `tolayer` complete layer names; `link` completes box ids and texts.
- `e` / `w` / `save` / `export` complete **real paths**: the argument is the whole rest of the
  line, so a name with spaces survives (`:w /tmp/a b.gtui` + Tab keeps both words). Directories
  come first and end in `/`, so `Tab` steps into a subdirectory; `~/` works. `e` only lists
  `.gtui` / `.dot` / `.canvas`, `w` / `export` list everything. Hidden entries need a typed `.`.
  The filesystem is scanned on `Tab` only, never while rendering.
- `:w` / `:wq` / `:save` with no argument: `Tab` fills the default save path (absolute) so you
  can keep editing it. A scratch buffer gets "no default path" instead of a guess.
- Free text (`text …`) is never completed. Typing narrows the popup; `Tab` re-filters for the
  current prefix.

The right side of the status line shows where a `:w` / `:e` / `:export` is heading while you
type — the resolved absolute path, `(overwrite)` for a `.gtui` source and
`(source .dot stays untouched)` for an imported one. It is truncated from the left, keeping the
file name, and never touches the input or the cursor.

## Selections and group operations

`v` (or `Ctrl-q`) pulls out a rectangle and `Enter` confirms it; after that, moving the cursor
no longer changes it and commands apply to **every** box and edge inside.

- Status line: `selection N boxes M edges · Enter to confirm` / `· confirmed`.
- Selected boxes and edges blink in the same red/white breathing phase as the cursor, with a
  continuous grey border around the rectangle. Nothing is filled.
- If everything in the selection disappears (deleted, undone), the selection is dropped, exactly
  as if you had clicked outside it.
- While unconfirmed, every document-changing command (`d` / `x` / `p` / style verbs /
  `Ctrl-hjkl`) is refused with "press Enter to confirm the selection" and no undo step.
- Once confirmed:
  - `d` / `x` / `:del` remove all selected boxes and edges as one undo step, and fill the clipboard;
  - `box` / `nodecolor` / `fill` / `textcolor` / `textalign` / `text` apply to every selected box;
  - `style` / `edgecolor` / `dir` / `corner` / `via` apply to every selected edge;
  - `Ctrl-hjkl` moves only the selected boxes (with only edges it says there is nothing to move);
  - every group operation costs **one** undo step.
- Deleting clears the selection; undo/redo clear it too.

## Clipboard

- One slot, alive for this session only: not the system clipboard, not saved into `.gtui`,
  not shared between processes.
- Only a successful cut replaces it; refused cuts, empty cuts and pasting an empty clipboard
  leave it alone.
- `p` pastes offset by `cursor - clipboard origin` (the origin uses boxes and texts, not edges);
  boxes, texts and edges all get new ids. Collisions try "shift one full copy right +1",
  "down +1", "down-right", then keep stepping, up to 32 candidates. The cursor lands on the
  first pasted box and the selection points at it. Groups and layers are reused by id/label
  (created when missing); an edge whose both ends were pasted is re-pointed, one whose other end
  still exists is kept, a dangling one is skipped and counted.
- Undo/redo never touch the clipboard.

## Undo and redo

Snapshot-based: each step stores the document as it was before the change.

- Depth defaults to 200 (`--history N`).
- Setting the same value twice is not a step; a whole drag is one step; each `Ctrl-hjkl` is one;
- Zoom, pan, focus, search, layer and label toggles are **view-only** and never enter the stack;
- A new change after an undo clears the redo stack; `:e` clears both;
- Undo/redo do not require `1:1` (they restore the model, not geometry editing).

## Mouse

| Action | What it does |
| --- | --- |
| left double-click (empty) | same as `i`: open the "new box" prompt and create it there on `Enter` (default style `none`) |
| left click | the cursor lands **immediately** on the clicked cell (no snapping to a node's corner); a box selects the box, an edge selects the edge; several edges on one cell cycle on repeated clicks. With a selection: clicking **inside** keeps it, clicking **outside** drops it |
| left drag on a node's **border** | link: drop on another box to create the edge (`Enter`-free; one undo step; dropping on empty space cancels; a plain click still just places the cursor). Only boxed nodes have a border, `none` / point nodes still use `e` |
| left drag on a node's **interior** | move those boxes (the whole selection if there is one); the whole drag is **one** undo step, and it switches to `1:1` first if needed |
| left drag on a selected edge's **endpoint** | reconnect that end: drop on another box to move `from` / `to`; colour, group, style and the note stay; empty space cancels |
| drag near the viewport edge | auto-pan while dragging (wider drags scroll faster, up to 3 cells per event) |

Overlapping edges are drawn one over the other (no `┼` junction): the cell shows the topmost edge's
own stroke and the covered one ends there. The first drawn edge is on top; picking an edge (clicking
a cell it covers again cycles the candidates, or `Ctrl-e`) raises it above the rest.
| left drag on empty space | **block selection**: the pressed cell is the anchor, the grey border follows, and **releasing the button confirms** (the status line reports boxes/edges). A plain click is not a drag, and an empty rectangle is dropped. Dragging again replaces the previous selection |
| `Ctrl` + left drag / middle drag | pan |
| wheel | zoom anchored on the pointer (1.1 per notch); `Ctrl` + wheel pans |
| right click | action menu at that cell (below) |

The menu is a **preview**: the first item is applied to the element the moment it opens, and
hovering / arrow keys re-preview live. One left click (or `Enter`) actually applies it, costing a
single undo step; cancelling or clicking outside rolls the document back exactly.

- Left click on an item = apply; outside the menu = close and roll back; right click = close and roll back;
- `↑ ↓` (or `j k`) move within the current level;
- `l` / `→` enter the next level (no-op when the item has no submenu); `h` / `←` / `Esc` go back one level, and close at the top;
- `Enter` applies (or enters a submenu); a digit applies item n directly;
- Previews never enter the undo stack and never move the cursor (applying uses the position from when the menu opened).

The menu contents depend on what is under the cursor:

- box: box style (5), fill, border colour, text colour, alignment, edit text…, delete;
- edge: line style, direction, corner, colour, clear waypoints, delete;
- empty: new box…, centre.

Every item runs the matching `:` verb, so a mouse-only workflow covers "pick an element →
change a style → save".

## Verbs

Open with `:`, run with `Enter`. Document-changing verbs cost one undo step.

| Verb | Effect |
| --- | --- |
| `:new <text>` | create a box on the cursor cell (no text → the new box id) |
| `:link <target>` | link from the cursor box to the target (id or unique text); the keyboard entry is `e`, this one is for naming a target exactly |
| `:del` | delete the cursor box (with text and edges); with an edge selected it deletes that edge (= `d`, clipboard keeps the edges) |
| `:box point\|none\|single\|rounded\|double\|heavy` | box style (`none` draws no frame and no node glyph, only text — it is the default) |
| `:nodecolor <#rrggbb\|auto>` | border colour |
| `:fill <#rrggbb\|none>` | box fill |
| `:textbg <#rrggbb\|none>` | text background (only under the glyphs; independent of the box fill and the edge background) |
| `:edgebg <#rrggbb\|none>` | edge background (only on the cells the edge crosses) |
| `:textcolor <#rrggbb\|auto>` | text colour |
| `:textalign <left\|center\|right>` | alignment inside the box |
| `:text <rest of line>` | the cursor box's text (taken verbatim, `**bold**` / `*italic*` included); on an edge it edits that edge's **note box**, an empty string removes the box |
| `:style <solid\|dashed\|dotted>` | line style of the selected edges |
| `:dir <none\|forward\|back\|both>` | direction |
| `:corner <rounded\|sharp>` | corner shape |
| `:edgecolor <#rrggbb\|auto>` | edge colour |
| `:via <x,y x,y\|off>` | manual waypoints (canvas coordinates) |
| `:layer <name\|all>` | view and edit only this layer |
| `:tolayer <name>` | move the selected nodes to that layer |
| `:notes [on\|off]` | edge notes on/off (on by default; bare toggles, same as `a`) |
| `:relabel [all]` | put note boxes back next to their edge (bare = only the selected notes) |
| `:legend [on\|off]` | legend panel on/off (on by default; bare toggles) |
| `:legend title <text>` | set the legend title (undoable) |
| `:legend add [--group g0] [--color #rrggbb] [--glyph solid\|dashed\|dotted\|arrow] <text>` | add a line; without group/colour it borrows the cursor node's group colour |
| `:legend rm <n>` | remove line n (1-based) |
| `:rotate [auto\|0\|90\|180\|270]` | rotate: bare = 90° clockwise; `auto` returns control to the layout (next `r`) |
| `:layout <auto\|geo\|relax\|force\|layered\|tree\|chain>` | switch layout (`auto` picks by structure; `r` cycles the six concrete ones) |
| `:side <factor>` / `:cycle <factor\|auto>` | the two tuning knobs for direction choice (the cell cursor no longer takes part in movement) |
| `:lang <zh\|en>` | UI language |
| `:w [path]` / `:save [path]` | save; without a path it writes the current document, a `.dot` source writes `<stem>.gtui` next to it |
| `:e <path>` / `:e` | open another document / show the picker (type to filter, `↑↓` to choose, `Enter`, `Esc` cancels) |
| `:fromside [auto\|left\|right\|up\|down]` | pin the side the selected edge leaves from |
| `:toside [...]` | pin the side it arrives at |
| `:port from=<side> to=<side>` | set both at once (either key may be omitted) |
| `:region new [label]` | create a region around the selected boxes (or the box under the cursor) |
| `:region add [id]` / `:region rm [id]` | add / remove the selected boxes from a region |
| `:region del [id]` | delete the region, keeping its boxes |
| `:region collapse [id]` / `:region expand [id]` | hide / show the members (display only) |
| `:region label <text>` / `:region color <c\|auto>` / `:region style <s>` | edit the region of the box under the cursor |
| `:region` | list regions and their member counts |
| `:e! [path]` | force a reload from disk, dropping local edits (`R` does the same after its confirmation) |
| `:export <path>` | export `.svg` / `.png` / `.canvas` / `.dot` (lossy) |
| `:export-canvas <path>` | same, writing JSON Canvas |
| `:undo` / `:redo` / `:history` | undo / redo / report the remaining steps |
| `:q` / `:quit` | quit (no save) |
| `:wq [path]` | save as `.gtui` and quit; a failing save (no name / not `.gtui`) keeps you in |

A leading `~` in a path is expanded; parent directories are created before writing.

### External changes

The source file is checked on every idle poll (a single `stat`, ~4 times a second). If its size
or mtime moved:

- **no local edits** - the document is replaced with the version on disk and the status line says
  so. A file that fails to parse (being written right now) is not retried in a loop; it falls back
  to the warning below.
- **local edits** - nothing is replaced. The right side of the status line keeps
  `file changed on disk: R reloads (drops edits), :w overwrites` until you act: `R` (twice, the
  first press only warns) or `:e!` reloads, `:w` overwrites the file, and an edit of your own
  clears nothing but keeps the warning.
- **file deleted** - the warning says the source is gone: `R` will fail, `:w` recreates it.

## Overlays

| Overlay | Open | Keys |
| --- | --- | --- |
| Key reference | `?` | any key closes it |
| File picker | `:e` | type to filter, `↑ ↓` to choose (`j k` are filter text), `Enter` opens, `Esc` cancels |
| Right-click menu | right mouse button | see above |
| Prompt line | `i` `t` | `Enter` applies, `Esc` cancels, backspace deletes; `Ctrl+G` hands over to the external editor (multi-line content shows as `first line [+N lines]`) |

## Cursor and status line

- Interactive mode uses the **real terminal cursor**: it sits on the cursor cell (even without a box); with `:`, `/` or a prompt open it goes to the end of the input; overlays hide it.
- `--drawn-cursor` paints the cursor into the picture instead: point nodes get a solid `●` / `◉`, framed nodes get a breathing border. `--dump` and exports always paint it.
- Where an edge meets a framed node: the start border gets a `┬ ┴ ├ ┤` connector, the arrowhead is embedded in the border cell; for point nodes the arrow sits one cell before the node.
- The selected element blinks **red/white**, without a background: framed nodes blink their border, point nodes blink their name/glyph.
- A block selection also gets a continuous grey border; elements inside it are shown in reverse video.
- The bottom is one line: on the left the current element (box name or "empty selection"), on the right the command line / echo / hint, empty when idle.
