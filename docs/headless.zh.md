[English](headless.md) | 中文

# Headless 协议

没有 TUI,也不要 TTY:一行一个 JSON 请求,一行一个 JSON 响应。stdout 只放协议数据,诊断信息
走 stderr;状态(文档、撤销栈、剪贴板)在同一个会话里持续保留。

```bash
graph-tui headless --file a.gtui --op '{"op":"info"}'
graph-tui headless --file a.gtui < requests.ndjson > responses.ndjson
graph-tui headless --file a.gtui --batch ops.ndjson --save-on-exit [--partial]
```

- 请求:`{"id":1,"op":"add_box","label":"A","at":[10,4]}`(参数可以放在顶层,也可以放在
  `args` 里)。
- 响应:`{"id":1,"ok":true,"protocol":1,"result":{...}}`;失败长这样
  `{"ok":false,"error":{"code":"bad_ref","path":"targets[0]","message":"no such box: b9"}}`。
- 退出码:`0` 全部成功,`2` 请求有误,`3` 文档非法,`4` IO 失败,`5` 事务回滚。
- `--limit <n>` 限制每个会话的请求数(默认 100000);超过就返回错误响应并结束会话。
- 操作:`info` `list` `get` `inspect_cell` `add_box` `add_edges` `set` `move` `delete`/`cut`
  `paste` `layer` `text` `layout` `orient` `rotate` `view` `render` `export` `undo` `redo`
  `validate` `schema` `open` `save` `transaction`。一切都用**稳定 id**,绝不用数组下标。
- `transaction`:只有整组都成功才提交,否则整体回滚,并消耗一步撤销;里面的 `add_edges` 可以
  用前面 `add_box` 返回的 id。
- `dry_run: true`:算出结果并返回 id,但不改文档。
- `render` 接受 `ascii: true` 来使用 ASCII 字形(等同 `--ascii`)。
- `render format=text|ansi|json`:text 是纯等宽格子(没有颜色);json 另外报每个框的屏幕矩形、
  每条边的可见格与注释(`note` / `note_rect`);`ansi` 用的渲染器和 `--dump` 相同。
- 警告都是非致命且结构化的:`render` 会加 `labels_skipped`
  (`[{"box":"b3","text":"Central","cell":[x,y]}]`)与 `warnings`
  (`[{"code":"label_not_placed","box":"b3"}]`);`layout` 会加 `crossings`、`reversed` 与
  `warnings`(`[{"code","message","id"}]`)。`info` 会报区域数量。`layout` 还接受
  `lane_gap` / `layer_gap` / `margin`(数字,含义同命令行),`layout: "auto"`(导入时的默认值;
  返回解析出的 `profile` 与 `components`),以及 `tree` / `chain` 两种 profile。错误码是稳定的:
  `label_not_placed`(点节点名字没地方放)、`edges_reversed`(回边只能按反向处理才能分层)、
  `dummy_cap`(长边太多,排序退回不用哑节点,因而更粗)。`--dump` 丢了标签时会在 stderr 打一行。
- 写入只接受 `.gtui`(原子写);`.dot` / `.canvas` 只作导入。批次上限 4096 条边,渲染上限
  1000x1000。
