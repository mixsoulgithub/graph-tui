# 更新日志

格式照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。`.gtui` 的存档格式版本(`"version"`,现在 6)
和 crate 版本各自独立,见 README 的「版本号」。

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

[0.1.2]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.2
[0.1.1]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.1
[0.1.0]: https://github.com/mixsoulgithub/graph-tui/releases/tag/v0.1.0
