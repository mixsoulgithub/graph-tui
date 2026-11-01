English | [中文](limits.zh.md)

# Known trade-offs

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
