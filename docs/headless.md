English | [中文](headless.zh.md)

# Headless protocol

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
- Warnings are non-fatal and always structured: `render` adds `labels_skipped`
  (`[{"box":"b3","text":"Central","cell":[x,y]}]`) and `warnings`
  (`[{"code":"label_not_placed","box":"b3"}]`); `layout` adds `crossings`, `reversed` and
  `warnings` (`[{"code","message","id"}]`). `layout` also accepts `lane_gap` / `layer_gap` /
  `margin` (numbers, same meaning as the CLI flags). Codes are stable:
  `label_not_placed` (a point-node name had no free slot), `edges_reversed` (a back edge had to be
  treated as reversed to get layers), `dummy_cap` (too many long edges, so ordering fell back to
  no dummy nodes and is coarser). `--dump` prints one stderr line when labels were dropped.
- Writes only accept `.gtui` (atomic); `.dot` / `.canvas` are import-only. Batches are capped at
  4096 edges and renders at 1000x1000.
