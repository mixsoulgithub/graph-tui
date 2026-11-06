English | [中文](limits.zh.md)

# Known trade-offs

- `.png` export relies on `rsvg-convert` installed on the system (a terminal cannot rasterise
  fonts); without it you get a clear error.
- `.dot` export is lossy: same-name nodes merge, `via`/`layer`/`terms` mean different things in
  Graphviz, and camera state cannot be expressed.
- `--ascii` trades shape information for portability: every corner, tee and crossing is `+`,
  and double/heavy frames collapse to single lines. Use it when the terminal cannot draw
  box-drawing characters, not as a style choice.
- Commands and typing still require `1:1` (they switch back anchored on the cursor). Mouse dragging of
  boxes works at any zoom now; because documents are integer cells, a 2x view needs two mouse cells
  per cell and a 0.5x view jumps two cells per mouse cell.
- Zoom scales **geometry only**; glyphs are always one cell per character. `f` / `F` fit at most to
  `1:1`; going further just spreads boxes apart with the same glyph size, and below one cell per
  glyph characters get clipped — that is only a navigation overview.
- Opening a document resets the view exactly like pressing `0`: `1:1`, centred on the picture. The
  `zoom` / `center` stored in the `.gtui` file are still written and are used by `--dump` and image
  export, but an interactive open does not restore them; press `F` to fit everything instead.
- `cargo clippy --all-targets` is warning-free; the two routing primitives take many arguments and
  explicitly `allow(too_many_arguments)` with the reason written down.
- `via` points are **canvas coordinates**: after a layout change everything else moves and the
  waypoints stay put, so they need resetting.
- Dashed/dotted styles only break straight runs; corners and crossings are always solid (otherwise
  you cannot see where a line turns); a cell already taken by a solid line prefers solid.
- Two edges never merge into a `┼` / `├`: one cell shows a single edge's own stroke and the other
  edge simply ends there (so a crossing reads as `─│─`, not as a T-junction). The edge drawn first
  stays on top; selecting an edge (clicking a cell it covers with `n`/`N`-style cycling, or
  `Ctrl-e`) raises it, so the covered one comes forward instead. `E` (neighbour view) raises the
  lit edges the same way, so a highlighted edge does not get chopped up by the edges crossing it.
- Arrowheads are topmost: they survive a foreign edge crossing their cell (with their own colour)
  instead of being dropped, and a cell still holds only one arrow. Only nodes are drawn over them.
- There is no legend panel (removed in v9). A key has to be drawn as content — `none` or framed
  boxes, a sample edge, group colours — and it then takes part in layout, hit-testing and selection;
  use `geo` or place it after the layout if it must stay put. Old `legend` data is dropped on load,
  never migrated.
- Italic depends on terminal support (`Modifier::ITALIC`); some terminals render it as normal text
  or reverse video.
- Saving rewrites node ids to their labels: for graphs that used a separate `label=`, the old id
  moves into `terms` after one save.
- The star fan bundles spokes by *geometry*, not by explicit waypoints: the top and bottom leaf of a
  column can still leave through a vertical port (the port side is chosen by the closest pair of
  box borders, and that leaf is `dy` away from the hub's row), which draws one extra bracket around
  the column. `examples/limits/star40.gtui` has the hand-placed variant with `via` points and forced
  sides if you want the tidy version.
- Routing is orthogonal only: a 45° edge becomes an L on screen; there are no diagonal strokes
  (`╱ ╲ ╳`) because East Asian widths are uncertain.
- The `Ctrl+G` external editor is launched through `sh -c` and needs a Unix shell; the Windows build
  works, but that key reports it is unavailable.
- Glyphs such as `○ ◎ ─` are two cells wide in some CJK fonts, which shifts the picture.
- Nodes have no shape or size concept and labels have no leader lines; labels are dropped when
  positions collide.
- `layered` is Sugiyama-lite: layering, barycentre ordering and back-edge breaking are there. The
  layout records the row of every node, and edges between adjacent rows always leave from the lower
  side / arrive at the upper side, so a rank reads as one vertical channel; but there is no
  crossing-minimising orthogonal router and no explicit track assignment (the columns a channel uses
  come from the port alignment and the weighted cost, not from a channel plan), so a wide rank can
  still produce long horizontal runs.
- Routing is greedy, edge by edge: each edge picks the cheapest path given what is already drawn
  (length + bends + crossings + reusing another edge's stroke, with the reuse weighted highest).
  One planning pass runs before drawing, so every edge at least sees the planned route of the
  edges before it (a soft penalty, 18 per cell, lighter than the 40 of a stroke that is already on
  screen) and the drawing pass then follows those planned routes; there is no rip-up/reroute loop,
  so a later edge can still end up in a worse corridor than a joint plan would have given it, and
  an edge planned early keeps a corridor a later edge would have needed. A path that cannot be found
  at all (a wall taller than the search window and the frame) falls back to a clipped leg.
- Paths are recomputed for every frame (the planning pass plus the drawing pass, which reuses the
  planned polylines instead of routing again, so a frame costs one routing round); only the picture
  geometry and zoom matter (panning and the cursor do not change the result), but there is no cache
  kept between frames.
- `geo` requires every node to have coordinates; without them it falls back to `force` or `layered`.
- Scale: 6000 nodes with no coordinates take about 3.5 s in `force` (release); beyond that the
  iteration cap keeps it bounded.
