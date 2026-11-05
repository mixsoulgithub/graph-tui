# 更新日志

格式照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。`.gtui` 的存档格式版本(`"version"`,现在 9)
和 crate 版本各自独立,见 README 的「版本号」。

## [Unreleased]

### Changed

- SVG export draws border glyphs as vector `<path>` segments and centres every character in its
  cell, on a 9x18 px cell with a 15 px font. Before, the font's advance (8.42 px at 14 px) did not
  match the 8 px cell, so glyphs drifted about 0.4 px per column and box corners broke up.
- The jump list is global instead of per document: entries remember the tab as well as the cell, so
  `Ctrl-o` after `gf` (or any tab switch) goes back to the previous buffer and position, and
  `Ctrl-i` (`Tab`) walks forward again. Closing a tab drops its entries and shifts the rest.

### Added

- `i` edits the cursor box's text **in the box**: no prompt line, the frame grows as you type, a
  reversed cell is the caret, `Enter` inserts a newline, `Esc` commits (one undo step) and
  `Ctrl-C` drops the draft. Left/Right/Home/End move by character and line, `Alt` + arrow by word,
  `Up`/`Down` keep the column. `t` still edits through the prompt (multi-selection and
  `<|new box|>` splitting), and a `point` node keeps the prompt because a one-cell name has no
  room for a draft.
- `Ctrl+G` (`edit_text_external`) edits the cursor box's text in `$VISUAL` / `$EDITOR` and writes
  the result straight into the box when the editor exits, as one undo step; `<|new box|>` splitting
  applies. The prompt's `Ctrl+G` also applies its result immediately now instead of leaving the
  text in the prompt for a second `Enter`.
- Custom URI schemes are configurable: any `scheme://…` link is looked up in a new `[links]`
  section (`custom = my-reader`) and the raw target is appended as **one** argument, run blocking
  with the terminal suspended. An unconfigured scheme only says `no handler for scheme …`. The
  core carries no vendor-specific scheme any more (there is a source audit test for it), and
  `--print-config` shows an empty `[links]` example.
- Link targets are parsed into a handler chain (`self` -> URL -> `omp://` -> local -> unknown) and
  can end in a **position**: `[x](./src/app.rs:10,4)` opens `$VISUAL` / `$EDITOR` at line 10,
  column 4 (nvim/vim get `+call cursor(10,4)`, other editors get `+10` and a note that the column
  was dropped), `[x](self:10,4)` jumps the current document's cursor to cell `10,4` and
  `[x](./other.gtui:10,4)` switches to that tab (opening one if needed) and jumps. URLs never take
  a position, `.dot` ignores it, `omp://` is recognised but reports that it is unsupported, and
  every jump is recorded so `Ctrl-o` goes back across tabs. The terminal suspend/resume for a
  positioned edit is the `Ctrl+G` path; a `true` / `false` / `:` `$EDITOR` stub now counts as
  unset.
- Each edge can pick its own arrowhead glyphs: `head` is `auto|ascii|arrows|triangles` in the
  `.gtui` file (format v10), settable from the right-click menu's `arrowhead` submenu or
  `:head triangles`, one undo step, previewed live like every other menu item. `auto` keeps
  following the global `--arrow` / `[prefs] arrow`, a new edge inherits the last chosen one, and
  files from v9 and older load with `auto`.
- A link keeps its link inside italics (`*[label](target)*`); bold already did.
- Text can carry links: `[label](target)` renders as the underlined `label`, the target takes no
  width, and `Enter` or `gf` on it opens the target. `http(s)://` and `mailto:` go to the opener,
  `file://` / `~/` / `./` / `../` / `/` are resolved to a local path (relative ones against the
  document directory), and a local `.gtui` / `.dot` opens as a tab instead of starting another
  graph-tui. The opener is `[prefs] opener`, then `$OPENER`, then `opener` / `xdg-open`; when it is
  the `opener` helper it is asked with `--which` and the `edit` answer runs with the terminal
  suspended, so `nvim` can take over. `Enter` is now `open_link` (`confirm_selection` keeps no
  default key and can be rebound).

## [0.10.0] - 2026-11-04

### Added

- Right-drag pans like middle drag or `Ctrl` + left drag, and a right **click** opens the action menu
  when the button is released: a right press is ambiguous, so nothing happens until it moves. Under
  two cells of movement it counts as a click and the menu opens at the pressed cell; two cells or
  more pans the view and no menu appears (even if you drag back to where you started). With a menu
  already open a right press still only cancels it.

### Changed

- The style menu (`s` / right click) remembers its **stack** per kind (box, edge, colour): every
  level you walked through, and the item you stood on in each. Opening it again goes straight back
  down the same path with the cursor on the item you picked, and `h` walks back to the exact item
  you came from instead of resetting to the first one. So changing a colour twice is `s`, `l`,
  `Enter`. The `edit text…` entry is gone from the box menu (use `i` / `t`); `delete` stays as the
  last item.

## [0.9.0] - 2026-11-04

### Added

- `examples/fun/uk-dynasties.gtui` follows the English/British royal houses from Wessex to
  Windsor, with the Scottish Stewart line joining in 1603: 17 `point` nodes, one line each, and no
  edge labels. `examples/fun/dynasties.gtui` grows to 50 content nodes: 44 `point` regimes plus six
  one-line `none` labels for the Warring States (齐 / 楚 / 燕 / 韩 / 赵 / 魏), which branch from 东周
  and merge into 秦.
- Go-to sequences: `gg` puts the cursor on `0,0`, `{x},{y}G` jumps to that cell (read as `x,y`,
  like the status line) and `{n}G` to row `n` with the column unchanged; the view **centres on the
  landing cell** and the jump goes into the jump list. A jump that has to move the view now lands
  exactly on the cursor instead of covering only 85% of the distance, which used to leave a far
  jump looking like nothing happened. Counts (`5l`) still work, and any sequence that is still
  waiting (`12`, `100,100`, `g`) is echoed on the **right end of the bottom line** like nvim's
  `showcmd`.
- `/` searches the **text** in the document, not just node names: node labels (substring, any
  position), aliases and pinyin initials, group names and region titles. A hit on a group or a
  region collects the boxes inside it, so `n` / `N` walk real jump targets — and every matched box
  is drawn in search-yellow (frame, text, point glyph and its label) so you can see what matched.
  A text search no longer touches the group focus — that used to light up every other node in the
  same group, so one match looked like a whole chain; a bare number still focuses that group, and
  `Esc` restores the focus you had before searching. Ids are not searched, and the search prompt
  says what is.

### Fixed

- `Ctrl-e` (`E`) could not select an edge once the view was scrolled: the candidate table is keyed
  by **screen** cells but the cursor was passed in canvas cells, so the lookup only matched at the
  default centre. The cursor is projected first now (test `视图挪过也能选中边`).

- Selecting an edge no longer shows a stray arrowhead at its far end: when another edge's arrow
  sat on a cell the selected edge runs through, the glyph stayed the foreign arrow (so 金 -> 蒙古
  looked bidirectional because 辽 -> 金's `<` sat on the shared cell). A raised edge now wins the
  glyph on its own cells and keeps its stroke.

### Changed

- Point-node labels are no longer dropped when their four standard slots are blocked: a node that
  sits on a crossing (e.g. 东周 in `dynasties.gtui`) now gets its name on the offset cell above or
  below, which is usually free while the centred slot is not.
- Edge ports are fixed anchors instead of a greedy search for a free border cell: the anchor is
  measured from the node's **text**, a side carrying one edge (or three and more) uses the middle
  anchor, two edges get the two anchors nearest it, and a side never uses more than two. Adding an
  edge no longer reshuffles the others, `none` and framed styles keep the same anchor, and zooming
  cannot send an edge to another side. The trade-off: several incoming edges on one side share a
  single arrowhead.
- `0` only puts the zoom back to `1:1` and leaves the picture where it is: going to the origin is
  `gg` now, so resetting the view no longer jumps the map around. Opening a file still centres it.

## [0.8.0] - 2026-11-04

### Changed

- Keyboard cursor movement scrolls like nvim: the view stays put until the cursor reaches a
  two-cell margin, then every further step scrolls the picture by exactly one cell **immediately**
  while the cursor keeps its **screen line** (only its logical cell advances). Scrolling here is
  not eased — an animation would move the cursor down one cell and then pull it back, which reads
  as a jump. Jumps (search, `n` / `N`, click, jump history) stay animated, and all mouse gestures
  are unchanged.

### Fixed

- A point-node edge no longer routes straight along an edge that is already drawn: two edges that
  occupy the same cells collapse into one stroke, so the later edge looks invisible while the
  earlier one's arrowhead sits on it, which makes the selected edge read as bidirectional. The L
  now picks the other corner when the first one would duplicate an existing run of the same axis
  (crossings still share a single cell, as before).
- Edges keep their shape while the view moves: routing now looks at the whole document
  (every node is an obstacle, and the "this cell already has a stroke" state is kept outside the
  viewport too) instead of only at the visible cells, so panning and cursor scrolling are a pure
  translation of the picture and a long edge whose node has left the screen still shows the part
  that is on screen.

- `Ctrl-o` / `Tab` (`Ctrl-i`) walk a proper jump list: besides search hits, `n` / `N` and mouse
  clicks, every edit records its position — creating a box, editing text, changing a style — while
  plain cursor moves do not. `Ctrl-o` from anywhere first notes where you are and then goes back to
  the last place you worked.


### Added

- Six hand-placed joke graphs under `examples/fun/` (`owl`, `left-pad`, `git-graph`, `unix`,
  `invaders`, `dynasties`), listed in `docs/examples.md`. They use `geo` layout, so the stored
  coordinates are the picture; `invaders` is pure pixel art built from 46 single-cell text boxes,
  and `dynasties` is a 44-regime top-down Chinese-dynasty DAG of `point` nodes with one-line CJK
  labels placed around them: edges branch where regimes coexisted and merge where one power
  reunified, with no edge labels. The native example test walks `examples/fun/` as well.
- `<|new box|>` inside the text prompt splits one box into several: the text before the marker
  stays in the box, each following segment becomes a new box to its right (top aligned, one cell
  apart, nudged right/down when the space is taken) that copies the source box style, colours,
  fill, text alignment and layer. Edges and notes stay on the original box; the cursor does not
  move and the whole split is one undo step. Only single plain boxes split: multi-selections, edge
  notes and `point` nodes keep the marker as ordinary text, and so do `:text` and headless `text`.
  `\<|new box|>` is a literal.

## [0.7.0] - 2026-11-04

### Changed

- The bottom of the screen is **two lines** on terminals with at least 8 rows (shorter ones keep
  the single line): the upper line shows `[n/m] path *` + mode (`NORMAL` / `SELECT` / `LINK` /
  `COMMAND` / `SEARCH`) + zoom on the left and the working directory plus the cursor cell `x,y` on
  the right; the lower line carries the command/prompt/search input, the `:` path hint and the last
  message or error. Long paths truncate from the left so the file name survives, then the working
  directory is dropped. The map loses the extra row; `--dump`, `--export` and headless render do
  not.
- The default UI language is now **English** (`Lang::default` and `view.lang` in new documents).
  `--lang zh`, `:lang zh` and an existing document whose `view.lang` is `zh` still switch to
  Chinese. Style menu titles, help and the status line are fully translated, so an English UI
  shows no CJK.
- Status, command, prompt and search lines draw with a bright but **non-bold** ink; only cursors
  and selections stay bold.

## [0.6.0] - 2026-11-04

### Added

- `i` / `t` prefill the node's current text (cursor at the end), so renaming is editing rather than
  retyping; a multi-selection is prefilled only when all boxes share the same text.
- The prompt, the `:` command line and the `/` search share one line editor: left/right, Home/End,
  `Ctrl-A/B/E/F/K/U/W/D`, Delete, `Alt-Backspace` (kill the word before the cursor, like `Ctrl-W`),
  and word motion with `Alt-left`/`Alt-right` or `Alt-B`/`Alt-F`. Typing inserts at the cursor.
- Config: `edge_color` / `text_color` in `[prefs]` (`auto` = today's behaviour, or `#rrggbb`) set
  the ink for edges and text that do not carry a colour of their own; both `#ffffff` gives a
  monochrome picture. Explicit per-element colours still win.
- A new box copies the style, colours and alignment of the box you last had selected **or just
  restyled** through the `s` menu / `:box` / a colour verb; a new edge copies group / line style /
  corner / colour / background of the last selected edge. A block selection is not "a selected
  element", so it neither updates the template nor gets inherited.

### Fixed

- Changing a box style keeps the **text** where it is and reshapes the box around it. Switching to
  `none` now produces exactly the picture you would get if the node had always been `none` (the box
  shrinks to the text, edges attach to it, no phantom border), `point` puts the glyph on the first
  character's cell, and a frame grows around the text without moving it.

## [0.5.0] - 2026-11-03

### Added

- `graph-tui new.gtui` on a path that does not exist yet opens a **new file** the way `nvim` does:
  an empty document bound to that name, written by `:w`. The same applies to `:e` and `:tabnew`;
  missing `.dot` / `.canvas` files are still an error (import-only), as are `--dump` / `--export`
  on a missing input. `R` on such a buffer says the file does not exist yet.
- A POSIX `install.sh`: `curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh`
  downloads the matching release archive, verifies its `.sha256`, and installs to `~/.local/bin`
  without sudo. Supports `--version <v>` and `--dir <path>`.
- `examples/real/hangzhou-metro.gtui`, a 262-station Hangzhou metro map generated from the
  metro-tui OSM data, with line groups, interchanges, branches and searchable station names.

### Removed

- **图例面板整个移除(破坏性变更).** `.gtui` 格式升到 **v9**:v8 文件里残留的顶层 `legend`
  对象和 `view.legend` 仍然能读,但内容直接丢弃(不会转换成框),保存后这两个键都不再写出。
  一起移除的还有:`--legend` / `--no-legend`,`:legend` 命令(开关 / `title` / `add` / `rm`),
  headless 的 `legend` / `legend_set` / `legend_add` / `legend_rm` 操作及 `info` / `render`
  响应里的 `legend` 字段,以及右侧面板本身(地图现在铺满整行,所以 `--dump`、`F` 和导出的
  宽度都变大了). 需要图例就在画面里自己画:`none` 或带边框的框、一条示意边、分组颜色,
  位置固定就用 `geo` 布局.

## [0.4.0] - 2026-11-03

### Added

- Star-shaped graphs (one hub, all other nodes degree 1) get a "fan" layout in `tree` / `auto`:
  leaves split into up to four vertical columns of ten, two per side, hub on the vertical middle
  line. `examples/limits/star40.dot` goes from a 527-cell-wide row to a 61x19 block. The resolved
  profile is `star`; a reverse star (all arrows into the hub) keeps the normal tree.
- The edge style menu now exposes explicit ports: `s` on an edge -> ports -> from / to -> `auto` or
  `left` / `right` / `up` / `down`. It runs the same `fromside` / `toside` commands as the prompt,
  so live preview, one undo step and cancel behave like the rest of the menu. Mixed selections
  tick nothing.

### Fixed

- An arrowhead no longer disappears when another edge crosses its cell: arrows are drawn above
  every stroke and keep their own colour. One cell still holds a single arrow; the edge raised by
  a selection or by `E` wins over an arrow of a covered edge.
- `E` (neighbour view) now raises the lit edges: a highlighted edge stays continuous where other
  edges cross it instead of dropping a cell at every crossing.

## [0.3.0] - 2026-11-03

### Added

- Tabs: several documents in one process. `graph-tui a.gtui b.dot` opens one tab per file; each
  tab keeps its own document, view, undo history, source path and external-change stamp while the
  clipboard stays shared. `:tabnew [path]` / `:tab <n>` / `:tabnext` / `:tabprev` / `:tabclose [!]`
  / `:tabs`, plus `Ctrl-t` / `Ctrl-n` / `Ctrl-p` / `Ctrl-w`. The status line shows `[2/4] name *`;
  `:tabclose` and `:e` refuse to drop unsaved changes unless forced with `!`.
- Layout profiles: `auto` is the default for new documents and imports. It reads the structure
  (components, cycle rank, tree defect, density) and picks `chain` for a path or a pure cycle,
  `tree` for a tree/forest, `layered` otherwise, `relax` for clique-like graphs (with a
  `dense_graph` warning). Disconnected graphs are packed block by block (`components_packed`).
  New `tree` (tidy, parents centred over their subtree) and `chain` (straight main line, spurs
  below, deterministic cycle break) layouts; `r` now cycles geo -> relax -> force -> layered ->
  tree -> chain. `LayoutReport` gained `profile` / `components`, exposed by the headless `layout`
  op; the status line shows `auto(tree)`.

### Changed

- Dragging boxes works at any zoom (it used to snap the view back to `1:1` and swallow the first
  move). The cursor and the selection rectangle follow the same canvas delta, the rectangle is
  hidden while dragging, and the whole drag stays one undo step.
- Opening a document (`graph-tui file`, `:e`, `:e!`) resets the view like pressing `0`: `1:1`,
  centred on the picture. The `zoom` / `center` stored in the `.gtui` file are still saved and are
  still used by `--dump` / image export, but they are no longer restored when opening interactively.
- Overlapping edges no longer merge into junction glyphs (`+`, `|-`, `T`): a cell shows one edge's
  own stroke and the edge underneath ends there, so a crossing reads as a vertical stroke over a
  horizontal one. The first drawn edge stays on top and selecting an edge (click cycling or
  `Ctrl-e`) raises it so the covered one comes forward. Shape ownership is per edge now, not per
  group.

## [0.2.0] - 2026-11-02

### Added

- Edge ports (`from_side` / `to_side`, format v8): `:fromside`, `:toside` and
  `:port from=… to=…` pin which side of a box an edge leaves from / arrives at; `auto` (the
  default, and what every older file gets) keeps the old centre-based choice. `.dot` round-trips
  the same attributes; point nodes ignore a pinned side.
- Mouse gestures: drag from a boxed node's border onto another box to create an edge, drag a
  selected edge's endpoint to reconnect it (other attributes kept), and drags auto-pan near the
  viewport edge. Dragging inside a box still moves it; `e` / `:link` are unchanged.
- Regions (`regions` in the `.gtui` file, format v7): a titled frame derived from its members'
  bounding box (plus padding, widened for the label), nested up to any depth, `collapse` hides the
  members and their edges, `:region new/add/rm/del/collapse/expand/label/color/style` edit them.
  `.dot` `subgraph cluster_*` imports as a region and exports back (colour/style/collapse lossy).
  Older files load with no regions; saving always writes v8.
- `--ascii`: a pure-ASCII glyph set (`- | + o @`) for terminals without box-drawing characters.
  A single `Glyphs` table now owns frames, junctions, corners and point glyphs; double/heavy
  frames collapse to single lines and the legend samples follow the same table. Also available as
  `[prefs] render = ascii` and as `ascii: true` on the headless `render` op; `--arrow arrows` /
  `triangles` falls back to ASCII arrows with a warning. SVG/PNG export honours it.
- `layered` grew dummy nodes for long edges, alternating barycentre sweeps, a transpose pass and a
  best-of-two choice against the old ordering, all scored by the final geometry; the report
  carries the resulting crossings. Spacing is configurable (`lane_gap`, `layer_gap`, `margin` in
  `[prefs]`, or `--lane-gap` / `--layer-gap` / `--layout-margin`, or the same fields on the
  headless `layout` op).
- Labels are no longer dropped silently: the renderer records every point-node name that had no
  free slot (`FrameHit::labels_skipped`), the status line says `N label(s) did not fit`, `--dump`
  prints one stderr line, and headless `render` exposes `labels_skipped` plus
  `warnings: [{code: "label_not_placed", box}]`.
- `layout::apply` returns a `LayoutReport` (crossings, reversed edges, warnings); headless `layout`
  returns those fields and `edges_reversed` is a warning with a stable code.
- Zoom is anchored: the wheel keeps the cell under the pointer, `+` / `-` keep the cursor cell
  (1.25 and 1.1 per step, snapping to `100%` inside 6%). Lower detail levels: below 60% no text
  is drawn, below 30% boxes are solid blocks, and the status line shows the percentage plus
  `LOD`. `--dump` / export ignore LOD.
- The source file is watched for external changes (size + mtime, one `stat` per idle poll): an
  unmodified document is reloaded automatically; a modified one gets a persistent warning,
  `R` reloads (a second press drops unsaved edits), `:e!` forces it, `:w` overwrites.
- Path hint while typing a `:` verb: the status line's right side shows the resolved absolute
  path, plus `(overwrite)` for a `.gtui` source or `(source .dot stays untouched)` for an
  imported one. Truncated from the left, so the file name stays visible.
- `Tab` after `:w` / `:wq` / `:save` with no argument fills the default save path (still
  editable); `Tab` after `:e` / `:w` / `:save` / `:export` completes real paths - subdirectories
  end in `/`, `~/` works, hidden entries need a typed `.`, and the filesystem is only scanned on
  `Tab`. `:e` lists just `.gtui` / `.dot` / `.canvas`.

### Changed

- Entering an edit from a zoomed view snaps back to `1:1` anchored on the cursor instead of
  re-centring on the box (the old behaviour made the picture jump). `ZOOM_MAX` is 16, not 64:
  glyphs do not grow with zoom, so 64 only emptied the screen.

- Inline text styles are markdown now: `**bold**` and `*italic*` (the old `/italic/` form is gone;
  a `/` in a label is a plain character again). Bold may contain italic; an unclosed marker is text.

### Fixed

- `relax` (and the final polish pass) measured every gap with one number - the largest box
  width - in both axes, so a stack of one-row boxes was pushed tens of rows apart and the two
  clusters of a star graph ended up hundreds of columns from each other. Spacing is now per
  axis (box height vertically, box width horizontally), the polish grid follows the same
  ratio, and the anchor no longer re-scales to the current bounding box (which let the picture
  ratchet outward every 20 iterations). `examples/real/cargo-deps.dot` drops from 1665x126 to
  321x27; `systemd.dot` from 408x511 to 319x32.
- Completion replaced only the last space-separated word, so a path with spaces was cut apart.
  It now replaces the whole argument (everything after the verb).
- The terminal cursor in the `:` command line, `/` search and the prompt line is placed by the
  raw character width. `:w /examples/` used to stop one cell short of the final `/` because the
  path was parsed as an italic span. Markup is only interpreted in node text.

## [0.1.2] - 2026-11-01

### 修复

- 外部编辑器的临时文件名改成进程内单调递增:`Drop` 删掉文件后不再复用序号
  (并行单元测试里会撞上"另一次调用刚建好同名文件")。
- 外部编辑器的单元测试不再先落 `.sh` 脚本再执行(并行跑时偶发 `Text file busy`),
  直接把 shell 片段交给 `edit_with`。

## [0.1.1] - 2026-11-01

### 新增

- 键位与偏好配置:`config.ini` 支持 `[prefs]`(arrow / history / lang / cursor / mouse)与
  `[keys]`(normal 模式重绑、`none` 解绑);`--print-config`、`--init-config [--force]`、
  `--config`、`--no-config`;解析失败报 `path:line` 并以非零退出。
- 英文优先的 README 与 `docs/` 拆分(usage / format / headless / config / examples / limits),
  中文文档以 `*.zh.md` 并列保留;新增演示 GIF。
- `packaging/conda/meta.yaml`(个人 conda 频道用)。

### 变更

- `--dump` / `--export` 明确不画光标;帮助文本不再声称 dump 不带图例
  (`--drawn-cursor` 只影响 TUI)。
- crate 包排除 `*.gif`。

### 修复

- 修正帮助与文档里 `--dump` / `--drawn-cursor` 的错误描述。

## [0.1.0] - 2026-10-31

首个发布。

### 渲染

- 正交走线(端口对齐、jog、交叉下穿),框内文字的 `**粗体**` / `/斜体/` 与左/中/右对齐。
- 六种节点样式:`point` / `none`(只有文字)/ `single` / `rounded` / `double` / `heavy`;
  节点与边都能单独上色,边有实线/虚线/点线、四种方向、圆角/锐角拐角、手工折点 `via`。
- 分组一色一格,同格冲突按"光标所在分组 > 图层 > 画序"决定;图例面板(文档自带条目,
  没有就按分组派生)。
- 边注释:绑在边上的 `none` 文本框,严格一对一,删边级联删除;`a` / `:notes` 收起/展开,
  `:relabel` 重贴。
- 多行文字:框按行数长高、按最长行长宽,逐行对齐与粗斜体生效。
- 自环:从节点一侧出线、拐到相邻一侧回来的小钩。
- 布局:`geo` / `relax` / `force` / `layered`(Sugiyama-lite),去重叠、钉住注释框。
- 视图:无限画布、平移/缩放、铺满(`f` / `F`)、聚焦分组、图层、邻居高亮(`E`)、
  区块选择、真光标与绘制光标两种画法。

### 编辑

- 建框(`i` / `:new`)、改文字(`t` / `:text`)、两段式连边(`e` / `:link`)、
  剪贴板(`d` / `x` / `p`)、样式与颜色菜单(`s` / `c`)、撤销/重做、`:w` / `:wq` 存盘。
- 鼠标:单击落光标,空白处双击新建节点,拖动挪框或拉选区,Ctrl+拖/中键平移,滚轮缩放。
- 底行输入里 `Ctrl+G` 把内容交给 `$VISUAL` / `$EDITOR`(没有就用 `vi`),退出后读回。

### 格式与工具

- `.gtui` 存档格式 **v6**(JSON,整数单元格坐标;读 v1..=v6)。
- `.dot` / `.canvas` 只作导入;`.dot` 可 best-effort 导出(有损),含 `label` 与多行转义。
- `--dump` 出纯文本或真彩 ANSI 图;`--export` 写 `.svg` / `.png`(`rsvg-convert`)/ `.canvas` / `.dot`。
- `headless` 子命令:JSON 进/JSON 出,行协议与 `--batch`,覆盖建图、改属性、布局、渲染与导出。
- 没有 per-graph `.cfg` 调整层:启动视图走命令行(`--layout` / `--legend` / `--at` …)
  或直接存进 `.gtui` 的 `view`。

[0.10.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.10.0
[0.9.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.9.0
[0.8.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.8.0
[0.7.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.7.0
[0.6.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.6.0
[0.5.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.5.0
[0.4.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.4.0
[0.3.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.3.0
[0.2.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.2.0
[0.1.2]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.2
[0.1.1]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.1
[0.1.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.0
