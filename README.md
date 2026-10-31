# graph-tui

[![crates.io](https://img.shields.io/crates/v/graph-tui.svg)](https://crates.io/crates/graph-tui)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mixsoulgithub/graph-tui/actions/workflows/ci.yml)

*Graph renderer and editor for the terminal: orthogonal box-drawing edges, one colour per group,
automatic crossings, pan/zoom, multi-line nodes and edge notes. Rust, `ratatui`.*

终端里的**通用图渲染引擎 + 小编辑器**:输入一张图(节点 + 边 + 边分组),输出字符格画面。
边是正交折线,拐角是圆角制表符 `╭╮╯╰`,两条边交叉时一条从另一条下面穿过(断一格),
一格只留一种颜色,每个分组一种颜色。线接到带框的节点时,起点画 `┬ ┴ ├ ┤` 连接点
(双线 `╦╩╠╣`、粗线 `┳┻┣┫`),终点把箭头**嵌进边框那一格**,框外不再画箭头;
无向边两端都是连接点。箭头字形默认 `^ v < >`,`--arrow ascii|arrows|triangles`
可换成 `↑↓←→` / `▲▼◀▶`(宽度不是一格的字符自动退回 ASCII)。

走线按**端口**对齐:两端按中心差挑面对的边,连接格按宽高奇偶取中,线只走到连接格外侧的
那一格,所以线、连接点、箭头永远在同一条正交线上。折弯从少到多试:能直就直,一个正交
就能连上就一个;连接格差一格时用一个**小 jog**,jog 优先贴着目标端(长直线保持干净);
两框之间放不下 jog 就沿源端口走完长线再错开。走线不穿过任何框,被挡住时绕到框外侧,
实在绕不开就停在最后一个可行格。

画布是**无限**的:平移没有边界,`1 份画布坐标 = 1 列`就是 1:1。
节点可以带框(圆角/单线/双线/粗线)并自己挑颜色,边可以有自己的颜色、线型(实线/虚线/点线)
和手工折点,标签支持 `**粗体**` 与 `/斜体/` —— 这些都能在界面里改。

存档是 native 的 `.gtui`(JSON,坐标就是整数单元格);`.dot` 和 `.canvas` 只当**导入**格式,
`.dot` 另外能 best-effort 导出成一张有损视图(不是源文档)。

```
                   North Gate ◎────────────╮
                              │            │
                              │            │
                              │────────────○────────────╮
                              │         Museum          │
                              │                         │  Harbour    分组
                              │                         │     ○       ── 1
                              │                         │     │       ── 2
 West End                     │                         │     │       ── 3
     ○────────────────────────◉─────────────────────────◎─────╯
                              │                     East Dock         ○ 节点
                              │                                       ◎ 枢纽
                              │                                       > 有向边
                              │
                              ○ South Park
节点 7  边 8  分组 3  真实坐标  光标 Central  1  2  3
```

(上面是 `examples/metro.dot` 的 `--dump` 输出,略去上下空行;`--dump` 打一帧真彩 ANSI 就走。)

## 安装

```bash
cargo install graph-tui                 # crates.io
brew install mixsoulgithub/tap/graph-tui
conda install -c https://mixsoulgithub.github.io/conda-channel graph-tui
```

预编译包(`graph-tui-<target>.tar.gz` + `.sha256`)在
[Releases](https://github.com/mixsoulgithub/graph-tui/releases);解包后
`install -m755 graph-tui ~/.local/bin/`。

### 从源码构建

```bash
cargo build --release
cargo install --path .          # 装到 ~/.cargo/bin
```

需要 Rust 1.88 及以上(`ratatui 0.30` 的 MSRV)。依赖只有 `ratatui` / `crossterm` /
`unicode-width`,运行时没有网络也没有数据库。

## 用法

```bash
graph-tui metro.gtui                     # 打开 native 文档(直接按文件里的坐标画)
graph-tui examples/pipeline.dot          # 导入 .dot;第一次 w 会写同目录的 pipeline.gtui
graph-tui                                # 什么都不给:空白缓冲(只在内存里,退出即弃)
graph-tui --dump --size 120x36 x.gtui    # 打一帧就退出,方便截图/塞进文档
```

没有文件、stdin 又是终端时会开一份**空白文档**:内存里的一页纸(一个分组、一个图层、
一个框 `a`),不写任何临时文件;退出就没了。存盘必须自己给路径(`:w 名字.gtui`),
不给就只回显"还没有文件名"。stdin 是管道时仍然从管道读。

```
--file <path>      读文档(`-` 表示 stdin;也可以直接把路径当参数给)
--layout <name>    geo | relax | force | layered,默认按图自己挑
--dump             打一帧真彩 ANSI 到 stdout 后退出
                   输入按扩展名认格式:`.gtui` native、`.canvas` JSON Canvas、其余当 .dot
--size <WxH|auto>  --dump / --export 的画布尺寸(默认 auto:内容 + 留白;小了就裁,不缩放)
--labels           开局就把节点名都标出来(等于按 b)
--no-labels        不画点节点的名字(--dump / --export)
--format <ansi|text>  --dump 的输出:真彩 ANSI(默认)或纯文本
--legend           强制打开右侧图例(不给就听文档的 view.legend)
--no-legend        强制关掉(和 --legend 互斥)
--no-notes         不画边注释(默认跟着文档的 view.notes)
--drawn-cursor     光标画在画面里(默认用终端真光标;--dump/--export 总是画进去)
--history <N>      撤销步数上限(默认 200)
--arrow <style>    箭头字形 ascii|arrows|triangles(默认 ascii)
--at <name>        开局把光标停在这个节点上
--find <text>      开局先搜一遍(名字 / 别名 / 分组号)
--lang <zh|en>     界面语言(默认 zh;命令行帮助与报错固定英文)
--export <path>    写出去就退出:.gtui(native 存档)/ .svg / .png(rsvg-convert 栅格化)/
                   .canvas(JSON Canvas)/ .dot(best-effort 有损视图,警告打到 stderr)
-v, --version      版本
-h, --help         帮助
```

### 出图:`--dump` / `--export`

只画图,不画界面:没有状态行、没有光标和选区。图例是画面的一部分,默认跟着文档走。

- **1:1**,永远不缩放;图居中,四周留白(左右各 2 列、上下各 1 行)。
- **尺寸自动贴合内容**:不给 `--size` 时,先按 1:1 探测一帧,量出真正有笔画的矩形
  (框、文字、边、T、箭头、点节点标签都算),再把那一块原样裁出来加留白。
  不重画第二次,所以自动摆放的标签不会因为"再画一遍"换位置。
- `--size WxH` 精确尺寸,仍然 1:1、仍然居中;**放不下就裁**,不缩小,提示写 stderr。
  `--size auto` 等于不给。
- `--format text` 输出纯文本(没有转义序列),`--format ansi`(默认)是真彩色。
  stdout 只有画面,末尾一个换行。
- `--no-labels` 不画点节点的名字;dump 默认是画的。
- `--export x.svg` / `x.png` 用同一套自动尺寸;PNG 仍然走 SVG + `rsvg-convert`。
- 图例是画面的一部分:默认跟着文档的 `view.legend` 画在图的右边(至少隔 2 列,竖直居中),
  `--no-legend` 才去掉;宽度上限是 `min(终端宽/3, 44)`,超宽的行截断补 `...`。
- 边注释也算画面内容:默认跟 `view.notes` 走,`--no-notes` 关掉(关掉后它不算障碍、也不占尺寸)。
- headless 的 `render` 也一样:`size` 可以省略或写 `"auto"`;`"legend": false` 关掉,
  返回的 json 里有 `legend.rect`。

```bash
graph-tui --dump graph.gtui > graph.ansi
graph-tui --dump --format text graph.gtui > graph.txt
graph-tui --export graph.svg graph.gtui
```

## 布局

| 名字 | 做什么 | 要求 |
| --- | --- | --- |
| `geo` | 不动现有位置(只把重叠的框挪开) | 任意文档 |
| `relax` | 从现有位置出发做弹簧放松,框距拉匀、走向吸附到 `- \| / \` 四种笔画 | 任意文档 |
| `force` | 力导布局(Fruchterman-Reingold),结束时同样吸附成横竖 45 度 | 任意文档 |
| `layered` | 分层布局(Sugiyama 那一套),适合有向依赖图/流程图 | 任意文档(有环也能画) |

方向(`view.orientation`):`auto` 时 `force` / `layered` 按窗口挑——把图转 90 度后需要的缩放更小就转
(差距不到 5% 不转,免得临界点抖动);`geo` 与 `relax` 保持输入方向,因为坐标本身带方向含义。
打开 `.gtui` 不重算坐标,只记住这个值。`:rotate` 显式转时改的是坐标(要求 1:1),框的宽高不交换。

四套布局都产出**整数单元格**坐标,写回 `box.at` 并把绑定的文字同量平移;任意两个框之间至少隔一格。
`r` 键在原地循环这四套。**同一张图换布局不会动光标**,搜索与聚焦都按框保留。

四套布局都是**确定性**的:同一张图跑两次结果逐点相同(没有随机数)。

## 键位

完整键位、鼠标动作与全部 `:` 动词见 **[docs/keybindings.md](docs/keybindings.md)**;这里只放最常用的:

| 键 | 做什么 |
| --- | --- |
| `h j k l` / 方向键 | 光标移动一格(`5l` = 走五格);格下是框就选框,是边就选边 |
| `i` `t` `p` | 建框或改选中框的文字 / 改文字 / 粘贴(压到框上会错开) |
| `s` `c` | 样式菜单(框型/颜色/对齐…)/ 颜色菜单(框=边框色,边=边颜色) |
| `d` `x` | 带边剪切 / 只剪框和文字(两者都把关联边从文档里删掉) |
| `e` / `Ctrl-e` | 两段式连边(先源,再目标;源就是目标即自环)/ 在光标节点的相邻边之间轮选 |
| `E` | 邻居高亮开关:只亮光标节点、它的邻居和相连的边,其余压暗;换到别的节点自动跟着走;再按 `E` 或 `Esc` 退出 |
| `Ctrl-q` / `v` | 区块选择(连续的灰边标范围),`Enter` 确认后命令作用于整块 |
| `Ctrl-hjkl` | 把选中的框(没选区就是光标框)挪一格,支持数字前缀(`10`+`Ctrl-l` = 十格,一步撤销);光标跟着走,挪到画面边距上视图会跟过去 |
| `+` `-` `0` `f` `F` `z` | 缩放与铺满、居中 |
| `/` `n` `N` `L` `b` `r` | 搜索(=聚焦分组)、切图层、标名、换布局 |
| `u` / `Ctrl-r` | 撤销 / 重做 |
| `:` `?` | 命令行(`Tab` 补全动词与取值,`:w` 存盘)/ 速查 |
| 鼠标 | 左键点:光标落到那一格,点重合的边会依次轮(点选区外面取消选区);**空白处双击 = `i` 新建节点**;左键拖:**按在节点上挪框**,按在空处拉区块选择(松开即选完,再拖换新的);`Ctrl`+左键/中键拖平移;滚轮缩放;右键动作菜单 |
| `:q` `:wq` | 退出 / 存盘后退出(想存就用 `:wq`) |

光标用终端真光标;选中的元素用红白呼吸标出来(框闪边框,没框的闪字,不铺底色);
`--drawn-cursor` 才退回"把光标画进画面"的老画法。
底部只有一行,左边是当前元素,右边是命令行 / 回显,空闲时不占位置。

## 存档格式 `.gtui`

native 文档是一份 JSON,`type` 用来自校验,`unit` 固定 `cell`:

```json
{
  "type": "graph-tui",
  "version": 6,
  "source": "graph-tui 0.1.0",
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
    {"id": "t1", "text": "Central\n第二行", "at": [11, 5], "box": "b1", "color": null,
     "bg": null, "align": "center", "layer": ""}
  ],
  "edges": [
    {"id": "e1", "from": "b1", "to": "b2", "dir": "both", "group": "g0",
     "style": "solid", "color": null, "bg": null, "corner": "sharp",
     "via": [[12, 5], [12, 9]], "layer": ""}
  ],
  "view": {"center": [20, 10], "zoom": 1.0, "layout": "layered", "cursor": [11, 5],
           "focus": [], "labels": false, "lang": "zh", "layer": null,
           "side": 2.0, "cycle": null, "orientation": "auto", "legend": true,
           "notes": true},
  "legend": {
    "title": "转移",
    "entries": [
      {"text": "CLOSED -> LISTEN      被动打开", "group": "g0"},
      {"text": "SYN-SENT -> CLOSED    收到 RST", "color": "#ef4b4b", "glyph": null},
      {"text": ""}
    ]
  }
}
```

要点:

- 框、文字、边是三种记录;框与文字**严格一对一**,文字永远属于某个框。
- 坐标是整数单元格,`at` 是**左上角**。`style` 六种:`single` / `rounded` / `double` / `heavy`
  画框;`point` 只占一格、画节点字形;`none` **什么都不画**,那块区域只有文字(宽随最长行、高随行数),
  边照样绕开它,但不往它上面画 T/箭头——箭头落在区域外面那一格。
  端口仍然按边框那套规则顺延:同一个节点上的多条边各占一个进出点(只是不画出来),
  不会全挤在同一格上。
  **`style` 可以省略,缺省就是 `none`**:什么都不说的节点就是一行字(`i` / `:new` /
  headless `add_box` 建出来的也是它)。
- 文字可以**多行**:`\n` 分行,`\r` / `\t` 不收。框按行数长高(`none`:高 = 行数、宽 = 最长行;
  带框:高 = 行数 + 2、宽 = 最长行 + 2,至少 3 行 4 列),`**粗体**` / `/斜体/` 与对齐逐行生效;
  点节点只有一个名字,换行不合法。规范形不保留末尾空行(编辑器读回时只去掉一个尾 `\n`,
  中间的连续空行保留);`"\n"` 就是两行。
- 加载时全量校验:未知键、重复键、重复 id、悬空引用、未知图层、非整数坐标、
  一个框挂两条文字、文字装不进框(`行数 > 高 - 2` 或 `最长行 > 宽 - 2`),一律拒绝并给出 JSON 路径。
- 颜色分成互不影响的几层:`box.color` 管**边框**,`box.fill` 管**框内底色**(点节点与 `none` 忽略),
  `text.color` 管**字**,`text.bg` 管**文字的底色**(只铺在字占的格子上,优先于 `fill`),
  `edge.color` 管**线**,`edge.bg` 管**线的底色**(只铺在这条边经过的格子上)。
  `text.bg` / `edge.bg` 是可选的(老文件没有这个键也能读),都接受 `null`。
  `text.align` 是 `left|center|right`,左右留白只在框比字宽时看得出来。
- `edge.dir` 是四态的 `none|forward|back|both`(不再是有向/无向两态);`edge.corner` 是
  `rounded|sharp`。缺省分别是 `forward`、`rounded`。
- 边可以**自己连自己**(`from` 与 `to` 相同):画成从节点一侧出去、拐到相邻一侧回来的小钩,
  箭头落在回来的那一端;它不进"线路"(不参与串联),布局也不拿它分层,
  但和其它边一样能点选、改 `:style` / `:text`、导出。
- `view` 只是视图状态(中心、缩放、光标格、聚焦、图层、语言、方向…),不影响几何;
  保存时**不会**把缩放乘进坐标。`view.cursor` 在 v2 里是 `[x, y]` 格子(老文件的框 id
  读进来会换算到该框文字起点,存一次就升成格子);顶层 `version` 是 6,
  读的时候 1 / 2 / 3 / 4 / 5 也认(v4 的 `edge.label` 读进来转成绑在边上的注释框,
  v5 的注释框与 v6 的多行文字本来就是同一套结构)。
- `legend` 是文档自带的图例:一个 `title`(可省)+ 任意行。每行 `text` 单行,
  `group`(借分组色)和 `color`(`#rrggbb`)二选一,`color` 赢;`glyph` 可选
  `solid|dashed|dotted|arrow`,画一个对应的前缀示意;空文本就是空行。
  `entries` 为空时退回按分组派生(标题 `分组` + 颜色 + 三种字形说明),老文件照此显示。
  开关不进这里——它是 `view.legend`(缺键按 `true` 读),内容是内容,视图是视图。
- 图例不是内容:不参与包围盒、布局、命中与选区,相机平移/缩放也不会把它一起缩放。
- 边的注释是**一个绑在边上的文字框**:`style` 为 `none` 的框加 `"edge": "<边 id>"`,
  文字就是注释,位置就是框自己的 `at`(不再每帧算贴边位置)。严格一对一:
  一条边最多一个注释框、一个框只绑一条边。删边时连注释框一起删(同一步撤销)。
  渲染不给它找位置;`i` 在选中边上新建(建议位置 = 两端中心的中点往右一格,避开别的框,
  只算一次),`:relabel [all]` 重新贴。文字色默认跟这条边。
  `view.notes` 是开关(缺键按 `true`):关掉后注释框不画、不算障碍、不占尺寸、也点不到。
- 保存先写 `.tmp` 再 rename,校验失败不覆盖原文件;同一份文档重复保存字节稳定。

### 导入 `.dot` / `.canvas`

- `.dot` 继续按 graphviz 子集解析,再转成 native:有框的节点按文字宽算框,点节点是个 `1x1` 的 `point`;
  米制 `pos` 在这里一次性投影成单元格(`geo` 只在这条路上有意义)。
  节点样式:`box=` 优先,其次认 graphviz 的 `shape=point`(点节点)与
  `shape=none|plaintext`(只有文字),都没有就是默认的 `none`。导出时点节点写成
  `shape="point"`,其余样式写自定义的 `box=`。
  `.dot` 的转义:`\n` / `\l` / `\r` 都是换行,`\\` / `\"` 照转,其它 `\x` 原样保留反斜杠
  (以前会被吞掉);导出时换行写成 `\n`。节点名进 `terms`,`label` 进文字。没有 `pos` 的 `.dot` 导入时跑一次布局(否则所有节点叠在原点)。
  样式类属性用自定义键读进来(graphviz 会忽略它们):`fill`、`align=left|center|right`、
  `corner=rounded|sharp`;边方向认 `->` `<-` `--` 与 `dir=none|forward|back|both`
  (`dir=<名字>` 优先级最高)。
- `.canvas`(Obsidian)按 8x16 像素/格换算成单元格;画布没有方向语义,一律按 `forward` 导入。
- 两种格式都只是**导入**:`.gtui` 才是可编辑的源。
- 导入后没有 native 源路径:**第一次 `w` 会写同目录的 `<stem>.gtui`**,原 `.dot` 不动。
- `.dot` 导出只通过显式 `--export x.dot` / `:export x.dot` 触发,文件头写明是有损视图,并逐条报失真项。
- 所有路径参数(`:w` / `:e` / `:export` 与 `--file` / `--export`)都会展开开头的 `~`,
  并且写之前会**自动建父目录**(`:w ~/图/a/b.gtui` 里没有的目录一并建出来)。

## 从零画一张图

不需要先写 `.dot`:直接 `graph-tui` 开空白缓冲,全部用 `:` 动词建。

```
graph-tui                 # 空白缓冲,开局一个节点 a(默认样式 none:只有一行字)
:new B                    # 在光标那一格新建一个节点,文字是 B,光标跟过去
:box single               # 想要框就显式给样式
:link a                   # 从光标节点(B)拉一条边到 a;目标可以写 id(b1)或文字
:box double               # 光标框换成双线
:fill #1b2440             # 框内底色
:textcolor #f59e0b        # 文字颜色
:text hello world         # 换文字(整行原样收下,**粗体** /斜体/ 照吃,框按文字自动改宽)
E                         # 选边;再 :dir both / :corner sharp / :style dashed
:w 名字.gtui              # 存盘;不给路径就不写(:w 只回显提示)
:w ~/图/名字.gtui         # 开头的 ~ 会展开成家目录(命令行参数也认)
:q                        # 退出(只有 :q 能退);没存 = 全丢,不会留下临时文件
:wq                       # 存成 .gtui 再退出;存失败就留在原地
```

`del` 删光标框(连它的文字与边);选了边就删那条边。最后一个框不让删。
新建的节点默认是 `none`(只有一行字,宽随文字),`:box` 才给它加框。

## 编辑(改属性,再写回 .gtui)

`Ctrl-e` 选边,`:` 动词改属性,`:w` 存盘;`e` 是两段式连边:先定源(选区或光标框),
再选目标,`Enter` 建边、`Esc` 取消。1->1 就是"光标在 A 上按 `e`,走到 B 按 `Enter`";
源和目标落在同一个框上就建自环(节点一侧出、相邻一侧回的小钩);
1->N 是"按 `e` 之后用 `v` 框住多个目标再 `Enter`";N->M 则是源也先用 `v` 框好。

**编辑只在 `1:1` 下进行**:放大或缩小时只能看(`VIEW x2.0`);直接敲修改命令会先把视图切回
`1:1` 并居中,然后再执行;底行标 `VIEW`(缩放后标成 `VIEW x2` 这种)。

底行输入(新建节点 / 改框文字)里按 **`Ctrl+G`** 把当前内容交给 `$VISUAL` / `$EDITOR`
(都没有就用 `vi`),退出后把文件读回来替换输入行:内容没变(`:q!`、直接退出、GUI 编辑器没阻塞)
就当取消,退出码非 0 原样保留并报错,写回的不是 UTF-8 或有回车/NUL 也拒绝。仍然要 `Enter` 才提交,
`Esc` 丢弃。编辑器必须**阻塞**(`code -w` / `vim`),命令行与搜索词不接这个键(按了只提示)。

选中边按 `i` 建/改它的注释框(空框 `Esc` 或空回车就撤掉);`a` 或 `:notes` 收起/展开全部注释,
`:relabel [all]` 把注释框重新贴回边旁边;注释收起来时它不画、不算障碍、也不占尺寸。

有选区时 `box` / `nodecolor` / `fill` / `textcolor` / `textalign` / `text` 作用于**整个选区**;
`L` 只看一层时,动词也只动这一层。全部动词见 [docs/keybindings.md](docs/keybindings.md#动词);
样式总表见 `examples/styles-all.gtui`(六种框型、粗斜体、文字色、填充、三种对齐、三种线型、
四种方向、两种拐角,一页看完),导入示例见 `examples/styles.dot`;
边注释示例见 `examples/edge-notes.gtui`(五条边各带一个注释框);
多行文字见 `examples/multiline.gtui`(带框的三行、只文字的两行、两行的边注释)。

撤销/重做是快照式的,默认 200 步(`--history N`);同值设置不占步,
拖拽整段算一步,只改视图的动作不进栈。细节见 [docs/keybindings.md](docs/keybindings.md#撤销与重做)。

图层用于压住重叠:框与边都能挂 `layer`,画的时候按层的先后叠、当前层在最上面,不在当前层的压暗。
挪框改的是 `box.at` 与它的 `text.at`(整数),存盘之后位置就定了;`r` 换布局才会重排。

## Headless(给脚本和 AI 用)

不启动 TUI、不要 TTY:一行一个 JSON 请求,一行一个 JSON 响应,stdout 只有协议数据,
诊断走 stderr,状态在会话里保留(文档、撤销栈、剪贴板)。

```bash
graph-tui headless --file a.gtui --op '{"op":"info"}'
graph-tui headless --file a.gtui < requests.ndjson > responses.ndjson
graph-tui headless --file a.gtui --batch ops.ndjson --save-on-exit [--partial]
```

- 请求:`{"id":1,"op":"add_box","label":"A","at":[10,4]}`(参数可以直接放顶层,也可以收在 `args`)。
- 响应:`{"id":1,"ok":true,"protocol":1,"result":{...}}`;失败是
  `{"ok":false,"error":{"code":"bad_ref","path":"targets[0]","message":"no such box: b9"}}`。
- 退出码:`0` 全成、`2` 请求不对、`3` 文档不合法、`4` IO 失败、`5` 事务回滚。
- 操作:`info` `list` `get` `inspect_cell` `add_box` `add_edges` `set` `move` `delete`/`cut`
  `paste` `layer` `text` `layout` `orient` `rotate` `view` `render` `export` `undo` `redo`
  `validate` `schema` `open` `save` `transaction`。全部按**稳定 id**,不认数组下标。
- `transaction`:整组成功才算,失败整组回滚,只占一步撤销;里面的 `add_edges` 可以直接用
  前面 `add_box` 返回的 id。
- `dry_run: true`:算结果、回 id,但不改文档。
- `render format=text|ansi|json`:text 是纯等宽格子(没有颜色),json 额外给每个框的屏幕矩形、
  每条边的可见格与注释(`note` / `note_rect`);`ansi` 与 `--dump` 用的是同一个渲染函数。
- 写入只认 `.gtui`(原子写);`.dot` / `.canvas` 只当导入格式。批量建边上限 4096 条,
  渲染尺寸上限 1000x1000。

## 与 metro-tui 的关系

渲染器(方向位合成字形、交叉下穿、一格一色、标签摆放)与 `relax` 的弹性松弛流程,
都搬自同一个作者的 [metro-tui](https://github.com/mixsoulgithub/metro-tui),
在这里把地铁语义(`线路` / `车站` / `换乘`)泛化成了 `分组` / `节点` / `枢纽`:
metro-tui 是这个引擎的一个特例。许可都是 MIT。

## 压力用例(examples/limits/)

这一批是故意为难引擎的图,拿来量边界在哪:

| 文件 | 试什么 |
| --- | --- |
| `star40.dot` | 一个枢纽挂 40 片叶子:一格只有四个方向,扇出到极限 |
| `k12.dot` | K12 完全图:66 条边挤在 12 个点上 |
| `grid20.dot` | 20x20 网格 400 点 760 边:密度与耗时 |
| `chain60.dot` | 60 长链、走分层:层高 3 格 x 60 层,窗口不够高就叠起来 |
| `ring.dot` | 有向环,分层布局必须拆一条回边 |
| `parallel.dot` | 同一对节点四条边四个分组:四条各占一条平行线(两端都没有可顺延的边框格,靠 lane 偏移) |
| `selfloop.dot` | 自环:从节点一侧出去、拐到相邻一侧回来 |
| `isolated.dot` | 孤立节点 + 一个连通块 |
| `components.dot` | 四个互不相连的连通块 |
| `inarrow.dot` | 12 条入边指向同一个点:箭头只有四个方向可放 |
| `collinear.dot` | 八个节点共线 |
| `samepos.dot` | 五个节点坐标完全重合 |
| `outlier.dot` | 一个远端离群点撑开包围盒 |
| `dupname.dot` | 两个同名节点(.dot 语义上就是一个节点) |
| `longlabel.dot` / `cjk.dot` | 超长标签与中文标签的占位 |

这些图就是拿来量边界的:投影之后节点叠在同一格、线没地方走、标签放不下,
都会照常画出来(叠了就叠着),状态行不报这些细节 —— 回显位留给命令与提示。

## 真实世界的图(examples/real/)

这几张不是编的:每一张都是手边的真实数据直接转出来的,文件头写着来源与重算办法。

| 文件 | 数据来源 | 规模 |
| --- | --- | --- |
| `modules.dot` | 本 crate 自己的模块依赖(源码里的 `crate::xxx` 引用) | 11 模块 18 边 |
| `cargo-deps.dot` | 本 crate 的 `Cargo.lock`,只展开一层 | 21 包 18 边 |
| `schema.dot` | 本机 Elisa 音乐播放器数据库的外键(`pragma foreign_key_list`) | 12 表 8 外键 |
| `systemd.dot` | 本机 `dbus.socket` 的反向依赖(`systemctl show` 的 Requires / Wants / After) | 21 单元 51 边 |
| `tcp.dot` | TCP 连接状态机(RFC 793) | 11 状态 26 条有向边 |
| `tcp.gtui` | 上面那张图的 native 版:坐标手摆,节点全是只有文字(`none`),`pair` 那条是虚线,带图例、三条边注释与七条自环 | 11 节点 26 边 |

这五张图(六份文件,`tcp` 有 `.dot` / `.gtui` 两份)都是 11~21 个节点 —— 不是巧合:终端画得下、人也读得下的图,差不多就是这个量级。
再往上(比如把 `Cargo.lock` 的传递依赖全展开,一百多个包)画出来只是一团麻,
GUI 也一样,那不是终端的问题。

`tcp.dot` 里那些"停在原地"的动作(RFC 那张图里的重传、超时、重发 FIN)
画成自环:从节点一侧出线、拐到相邻一侧回来,单独一组橙色虚线,图例里逐条列出。

## 已知取舍

- `.png` 导出靠系统里的 `rsvg-convert`(终端自己没法栅格化字体);没有这个命令就明确报错。
- `.dot` 导出有损:同名节点会合并、`via`/`layer`/`terms` 在 Graphviz 里语义不同、相机状态无法表达。
- 编辑必须在 `1:1`:想精细摆放就先缩放看清再按 `0` 回到 1:1。
- 缩放只缩放**几何**,字形永远是一格一个字:`f` / `F` 铺满最多放到 `1:1`,再大只会把框拉开、
  字还是那么大;缩到一格不到一个字时字会被裁掉,那只是导航用的缩略视图。
- 打开文档不自动铺满(缩放是文档状态,新文档就是 `1:1`),要看全图按 `F`。
- `cargo clippy --all-targets` 零警告;两个走线原语参数多,显式 `allow(too_many_arguments)` 并写明理由。
- `via` 折点是**画布坐标**:换布局之后位置全变了,折点还留在原地,要重设。
- 虚线/点线只断直笔,拐角与交叉点一定是实的(不然看不出线往哪拐);同一格被实线占过就优先实线。
- 斜体靠终端支持(`Modifier::ITALIC`),有的终端会画成普通字或反显。
- 存盘会把节点 id 改写成 label:原来 `label=` 分开写的那种图,存一次之后 id 会进 `terms`。
- 走线只有正交:45 度的边在屏幕上折成一条 L,没有 `╱ ╲ ╳` 这类斜笔画(东亚宽度不确定)。
- `Ctrl+G` 的外部编辑器靠 `sh -c` 启动,需要 Unix shell;Windows 构建能用,只是这个键会提示不可用。
- `○ ◎ ─` 这类字形在部分 CJK 字体下会被画成两格宽,图会错位。
- 节点没有形状与尺寸概念,标签也不画引线;位置冲突时标签会被挤掉。
- `layered` 是 Sugiyama-lite:分层、重心排序、回边拆解都有,但没有交叉最小化的正交布线器,
  层多的图会画得很"竖"。
- `geo` 要求每个节点都有坐标;拿不到坐标就用 `force` 或 `layered`。
- 规模:6000 节点无坐标的 `force` 约 3.5 秒(release),再往上靠迭代轮数封顶兜着。

## 版本号

两套版本别混:

- **crate 版本**(`Cargo.toml` 的 `version`,`graph-tui --version`)是程序本身的版本,现在 0.1.0。
- **`.gtui` 格式版本**(文件里的 `"version"`)是存档格式的版本,现在 6。格式只在结构变化时升
  (v1 光标存框 id → v2 光标存格 → v3 图例 → v4 边标签 → v5 边注释改成绑定框 → v6 多行文字),
  读的时候旧版本一律认。两者互不联动。

## 许可

MIT,见 [LICENSE](LICENSE)。
