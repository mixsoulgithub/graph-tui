[English](format.md) | 中文

# 文档格式 `.gtui`

原生文档是一个 JSON 对象;`type` 用于自校验,`unit` 永远是 `cell`:

```json
{
  "type": "graph-tui",
  "version": 10,
  "source": "graph-tui 0.11.0",
  "unit": "cell",
  "groups": [{"id": "g0", "label": "", "color": "#4c8ef8"}],
  "layers": [{"name": "", "order": 0}],
  "boxes": [
    {"id": "b1", "at": [10, 4], "size": [9, 3], "style": "rounded",
     "color": null, "fill": "#1b2440", "layer": "", "terms": ["central", "zx"],
     "edge": null},
    {"id": "b2", "at": [30, 4], "size": [8, 1], "style": "none",
     "color": null, "fill": null, "layer": "", "terms": [], "edge": "e1"}
  ],
  "texts": [
    {"id": "t1", "text": "Central\nsecond line", "at": [11, 5], "box": "b1", "color": null,
     "bg": null, "align": "center", "layer": ""}
  ],
  "edges": [
    {"id": "e1", "from": "b1", "to": "b2", "dir": "both", "group": "g0",
     "style": "solid", "color": null, "bg": null, "corner": "sharp",
     "from_side": "auto", "to_side": "auto",
     "via": [[12, 5], [12, 9]], "layer": ""}
  ],
  "view": {"center": [20, 10], "zoom": 1.0, "layout": "layered", "cursor": [11, 5],
           "focus": [], "labels": false, "lang": "zh", "layer": null,
           "side": 2.0, "cycle": null, "orientation": "auto",
           "notes": true},
  "regions": []
}
```

要点:

- 框、文字、边是三种记录;框与文字**严格一对一**,一个文字永远只属于一个框。
- 坐标是整数格,`at` 是**左上角**。`style` 有六个取值:`single` / `rounded` / `double` /
  `heavy` 画框;`point` 占一格、画一个节点字形;`none` **什么都不画**,那块地方只有文字
  (宽度跟最长行,高度跟行数)。边仍然绕着它走,但不会在它上面画 T 形接点或箭头,箭头落在
  紧挨着它外面的那一格。端口仍然按边框规则走:一个节点上的多条边各自取自己的进/出点
  (只是不画出来),所以永远不会堆到同一格。**`style` 可以省略,默认 `none`**:没有样式的
  节点就是一行文字(`i` / `:new` / headless 的 `add_box` 默认都建这个)。
- 文字可以**多行**:`\n` 分行;`\r` / `\t` 一律拒绝。框会跟着行数长(`none`:高 = 行数,
  宽 = 最长行;带框:高 = 行数 + 2,宽 = 最长行 + 2,至少 4 宽 3 高)。`**粗体**` / `/斜体/`
  与对齐逐行生效。点节点只有一个名字,不能换行。规范形式不带末尾空行(编辑器桥接只剥掉一个
  末尾 `\n`;中间的空行保留);`"\n"` 是两行。
- 读入时会校验一切:未知键、重复键、重复 id、悬空引用、未知图层、非整数坐标、一个框上两个
  文字,以及放不进框的文字(`lines > height - 2` 或 `longest line > width - 2`),全部连同
  JSON 路径一起拒绝。
- 颜色是互相独立的层:`box.color` 是**边框**,`box.fill` 是**内部填充**(对点节点和 `none`
  无效),`text.color` 是**字形**,`text.bg` 是**文字背景**(只铺在字形格上,优先于 `fill`),
  `edge.color` 是**线**,`edge.bg` 是**线的背景**(只铺在这条边经过的格子上)。`text.bg` /
  `edge.bg` 可省略(没有这个键的旧文件照样能读),也接受 `null`。`text.align` 是
  `left|center|right`;左右留白只有在框比文字宽时才看得见。
- `edge.dir` 是四态 `none|forward|back|both`(不再只是有向/无向);`edge.corner` 是
  `rounded|sharp`。默认值分别是 `forward` 与 `rounded`。
- 边可以**连接节点自身**(`from` 等于 `to`):画成一个小钩,从一侧出去、从相邻一侧回来,
  箭头在回来的那端。它不算一条"线"(不串成序列),布局也不会给它分层,但能选中、能用
  `:style` / `:text` 改样式、也能像别的边一样导出。
- `view` 只是视图状态(中心、缩放、光标格、聚焦、图层、语言、朝向…);它不影响几何,存盘时
  **缩放永远不会乘进坐标**。`view.cursor` 在 v2 里是 `[x, y]` 格子(旧文件里的框 id 会转成
  那个框的文字原点;存一次盘就升级)。顶层 `version` 是 10;1 到 9 版也能读
  (v4 的 `edge.label` 会转成绑在这条边上的注释框;v5 的注释框与 v6 的多行文字结构相同;
  v7 增加区域;v8 增加显式边端口;v9 移除图例;v10 增加每条边自己的箭头字形)。
- **v9 移除了图例。** `version: 8` 文件里的顶层 `legend` 对象和 `view.legend` 仍能读,
  但内容直接丢弃(不会转换成框:面板行到框没有可靠映射)。保存写 v10,两个键都不再出现。
  需要图例就把它画成内容:`none` 或带边框的框、一条示意边、分组颜色;位置固定用 `geo`。
- 边注释是**绑在边上的文本框**:一个 `style: none` 且带 `"edge": "<边 id>"` 的框。文字就是
  注释,框自己的 `at` 就是它的位置(不做逐帧摆放)。严格一对一:一条边最多一个注释框,一个框
  最多绑一条边。删边会连注释框一起删(一步撤销)。渲染从不会移动它;选中边时按 `i` 创建
  (建议位置 = 两个中心的中点右移一格,避开别的框,只算一次),`:relabel [all]` 重新锚定。
  它的文字颜色默认跟着边。开关是 `view.notes`(缺键按 `true` 读):关掉时不画、不算障碍、
  不占空间、也点不到。
- 存盘先写 `.tmp` 再改名;校验失败绝不会覆盖原文件,同一份文档存两次得到完全相同的字节。

### 导入 `.dot` / `.canvas`

- `.dot` 按 Graphviz 子集解析后转成原生格式:带框节点得到按文字定尺寸的框,点节点变成
  `1x1` 的点;带度量的 `pos` 在这条路径上一次性投影成格子(只有那里 `geo` 才有意义)。节点样式:
  `box=` 优先;否则识别 Graphviz 的 `shape=point`(点)与 `shape=none|plaintext`(纯文字);
  都没有就默认 `none`。导出时点节点写 `shape="point"`,其他样式写自定义的 `box=` 键。
  `.dot` 转义:`\n` / `\l` / `\r` 都表示换行,`\\` / `\"` 反转义,其他 `\x` 保留反斜杠
  (以前会被吞掉);导出把换行写成 `\n`。节点名进 `terms`,`label` 变成文字。没有 `pos` 的
  `.dot` 导入时排一次布局(否则所有节点都挤在原点)。样式属性用 Graphviz 会忽略的自定义键:
  `fill`、`align=left|center|right`、`corner=rounded|sharp`。边方向认 `->` `<-` `--` 与
  `dir=none|forward|back|both`(`dir=<name>` 优先)。
- `.canvas`(Obsidian)按每格 8x16 像素换算;canvas 没有方向语义,所以每条边导入后都是
  `forward`。
- 两种格式都**只作导入**:`.gtui` 才是可编辑的源。
- 导入之后没有原生源路径:**第一次 `w` 会在输入文件旁边写 `<stem>.gtui`**,原 `.dot` 不动。
- `.dot` 导出只走显式的 `--export x.dot` / `:export x.dot`;文件头会说明这是有损视图,
  每一项有损都会报告。
- 所有路径参数(`:w` / `:e` / `:export`,`--file` / `--export`)都展开开头的 `~`,
  缺少的父目录会**自动建出来**(`:w ~/figs/a/b.gtui` 会把目录一并建好)。

两个版本号,别混:

- **crate 版本**(`Cargo.toml` 里的 `version`、`graph-tui --version`)给程序本身定版本;
  现在是 0.11.0。
- **`.gtui` 格式版本**(文件里的 `"version"`)给文档格式定版本;现在是 10。只有结构性变化才升
  (v1 光标=框 id -> v2 光标=格子 -> v3 图例 -> v4 边标签 -> v5 注释=绑定的框 -> v6 多行文字
  -> v7 区域 -> v8 显式边端口 -> v9 移除图例 -> v10 每条边自己的箭头字形),旧版本永远能读。两者互不相关。

## 区域(Regions, v7)

`regions` 是顶层数组,每个区域是一个 cluster 风格的框。区域自己不带坐标:矩形由成员的包围盒
加 `padding` 得到(标题更长时向右加宽),所以成员移动时区域跟着走。

```json
"regions": [
  {"id": "r1", "label": "connection setup", "color": "#22b46a", "style": "rounded",
   "layer": "", "padding": 1, "members": ["b1", "b2"], "collapsed": false}
]
```

- `members` 是框 id 或其它区域 id;一个框最多属于一个区域,嵌套区域不能成环,两种错误都会在
  读取时拒绝。
- `style` 必须是带框的样式(`single` / `rounded` / `double` / `heavy`)。
- `collapsed` 只影响显示:只画标题条,成员框和碰到成员的边不画,数据保留。
- 区域画在边和框的下面,不会盖住文字;边可以穿过区域边框。v7 里边框和标题不参与光标命中
  (命令通过光标下的框或显式 id 找区域)。
- v7 之前的文件没有 `regions` 键,读进来是空表;保存一律写 v8。

## 边端口(Edge ports, v8)

边可以钉住从框的哪一侧出去、从哪一侧进来:

```json
{"id": "e1", "from": "b1", "to": "b2", "from_side": "left", "to_side": "up"}
```

- 取值:`auto`(默认,写盘时显式写出)/ `left` / `right` / `up` / `down`。`auto` 保持旧行为:
  按两个中心的差选边。
- 强制只影响这一端,另一端仍可以是 `auto`。
- 点节点没有边框,强制侧会被忽略(在 TUI 里设置时会有提示);`none` 节点有虚拟边框,遵循强制侧。
- 多条边钉在同一侧时,按现有 `border_slot` 规则依次占用空位;箭头仍然单独占一格。
- 在 TUI 里:选中边后用 `:fromside` / `:toside` / `:port from=… to=…`,或者打开样式菜单
  (`s` 或右键) -> **端口** -> 起点 / 终点 -> 侧;菜单实时预览,应用只算一步撤销。
- v8 之前的文件每条边读成 `auto`;保存一律写 v8。
