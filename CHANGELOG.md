# 更新日志

格式照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。`.gtui` 的存档格式版本(`"version"`,现在 8)
和 crate 版本各自独立,见 README 的「版本号」。

## [0.2.0] - 2026-11-02

### Changed

- Overlapping edges no longer merge into junction glyphs (`┼`, `├`, `┬`): a cell shows one edge's
  own stroke and the edge underneath ends there, so a crossing reads as `─│─`. The first drawn edge
  stays on top and selecting an edge (click cycling or `Ctrl-e`) raises it so the covered one comes
  forward. Shape ownership is per edge now, not per group.

### Added

- Layout profiles: `auto` is the default for new documents and imports. It reads the structure
  (components, cycle rank, tree defect, density) and picks `chain` for a path or a pure cycle,
  `tree` for a tree/forest, `layered` otherwise, `relax` for clique-like graphs (with a
  `dense_graph` warning). Disconnected graphs are packed block by block (`components_packed`).
  New `tree` (tidy, parents centred over their subtree) and `chain` (straight main line, spurs
  below, deterministic cycle break) layouts; `r` now cycles geo → relax → force → layered →
  tree → chain. `LayoutReport` gained `profile` / `components`, exposed by the headless `layout`
  op; the status line shows `auto(tree)`.
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

[0.2.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.2.0
[0.1.2]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.2
[0.1.1]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.1
[0.1.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.0
